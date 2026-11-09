// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method Resolution
//!
//! Handles resolution of method calls, return types, and type parameter substitution.
//! Extracted from type_inference.rs for maintainability.

use super::builder::ResolverBuilder;
use crate::symbol::{ClassType, Type};
use crate::utils::build_imports_from_syntax;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::sync::Arc;
use tracing::{debug, info};

impl ResolverBuilder {
    fn mentions_declared_type_parameter(
        type_name: &str,
        parameters: &[hyperion_workspace::TypeParameterInfo],
    ) -> bool {
        type_name
            .split(|character: char| {
                !character.is_alphanumeric() && character != '_' && character != '$'
            })
            .any(|part| {
                parameters
                    .iter()
                    .any(|parameter| parameter.name.as_ref() == part)
            })
    }

    /// Score method candidates by information quality.
    ///
    /// Higher score means "prefer this overload when multiple are compatible".
    /// This helps prefer workspace stubs with generic information over erased
    /// external/JDK signatures.
    fn score_method_candidate(&self, method: &hyperion_workspace::Symbol) -> i32 {
        let mut score = 0;

        // Prefer workspace symbols over external symbols when both exist.
        if let Some(loc) = self
            .workspace_index
            .as_ref()
            .and_then(|ws| ws.find_symbol(&method.fqn))
        {
            if self
                .workspace_index
                .as_ref()
                .is_some_and(|ws| ws.is_workspace_file(loc.file_id))
            {
                score += 100;
            }
        }

        if !method.type_parameters.is_empty() {
            score += 30;
        }

        if let Some(return_type) = method.return_type.as_ref() {
            let rt = return_type.as_ref();
            if rt.contains('<')
                || Self::mentions_declared_type_parameter(rt, &method.type_parameters)
            {
                score += 20;
            }
        }

        // Prefer generic parameterized methods over erased Object signatures.
        if method.parameters.iter().any(|param| {
            let ty = param.split_whitespace().next().unwrap_or("");
            let ty = ty.trim_end_matches("...").trim_end_matches("[]");
            ty.contains('<') || Self::mentions_declared_type_parameter(ty, &method.type_parameters)
        }) {
            score += 10;
        }

        if method.parameters.iter().any(|param| {
            let ty = param.split_whitespace().next().unwrap_or("");
            ty == "Object" || ty == "java.lang.Object"
        }) {
            score -= 5;
        }

        score
    }

    fn select_best_method_candidate(
        &self,
        candidates: impl Iterator<Item = Arc<hyperion_workspace::Symbol>>,
    ) -> Option<Arc<hyperion_workspace::Symbol>> {
        candidates.max_by(|a, b| {
            let score_a = self.score_method_candidate(a.as_ref());
            let score_b = self.score_method_candidate(b.as_ref());
            score_a
                .cmp(&score_b)
                // Stable tie-breaker: prefer lexicographically smaller FQN.
                .then_with(|| b.fqn.as_ref().cmp(a.fqn.as_ref()))
        })
    }

    pub(crate) fn resolve_qualified_method_call(&self, field_access_node: &SyntaxNode) -> Type {
        // FIELD_ACCESS_EXPR structure for method calls:
        // - First child: receiver expression
        // - Tokens in between: DOT, method name IDENT

        let children: Vec<_> = field_access_node.children().collect();
        if children.is_empty() {
            return Type::Unknown;
        }

        // First child is the receiver
        let receiver_expr = &children[0];
        let receiver_type = self.infer_expression_type(receiver_expr);

        // Extract method name from tokens
        let method_name = self.extract_method_name_from_field_access(field_access_node);
        let method_name = match method_name {
            Some(name) => name,
            None => return Type::Unknown,
        };

        // Resolve method return type
        // Note: We don't have the METHOD_CALL_EXPR node here, so can't infer type parameters
        // This is okay - this path is for general expression type inference, not var inference
        let result = self.resolve_method_return_type(&receiver_type, &method_name, None);
        if result.is_none() {
            debug!(
                "Type::Unknown: method return type unresolved for {}.{}()",
                receiver_type, method_name,
            );
        }
        result.unwrap_or(Type::Unknown)
    }

    /// Extract method name from FIELD_ACCESS_EXPR
    /// For "receiver.methodName", returns "methodName"
    #[allow(clippy::unused_self)]
    pub(crate) fn extract_method_name_from_field_access(
        &self,
        node: &SyntaxNode,
    ) -> Option<String> {
        use rowan::NodeOrToken;

        // Look for IDENT token after DOT
        // Allow contextual keywords as method/field names (e.g., foo.record())
        let mut found_dot = false;
        for item in node.children_with_tokens() {
            match item {
                NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::DOT {
                        found_dot = true;
                    } else if found_dot && token.kind().is_identifier() {
                        return Some(token.text().to_string());
                    }
                }
                NodeOrToken::Node(_) => {}
            }
        }
        None
    }

    /// Extract declaring class/interface FQN from a method symbol FQN.
    ///
    /// Supports both source method format (`Class.method(Type)`) and
    /// JDK format (`Class.method`).
    fn extract_declaring_type_fqn(method_fqn: &str, method_name: &str) -> Option<String> {
        let with_params = format!(".{method_name}(");
        if let Some(idx) = method_fqn.find(&with_params) {
            return Some(method_fqn[..idx].to_string());
        }

        let without_params = format!(".{method_name}");
        method_fqn
            .strip_suffix(&without_params)
            .map(std::string::ToString::to_string)
    }

    /// Rebuild a class Type from an FQN while preserving generic metadata from `template`.
    fn class_type_from_fqn_with_template(&self, fqn: &str, template: &ClassType) -> Type {
        let (package, name) = if let Some(last_dot) = fqn.rfind('.') {
            (
                Some(fqn[..last_dot].to_string()),
                fqn[last_dot + 1..].to_string(),
            )
        } else {
            (None, fqn.to_string())
        };

        Type::Class(ClassType {
            name,
            package,
            type_arguments: template.type_arguments.clone(),
            is_diamond: template.is_diamond,
        })
    }

    /// Extract `receiver.method` from a METHOD_CALL_EXPR in current parser shape.
    fn extract_simple_method_call_parts(method_call: &SyntaxNode) -> Option<(String, String)> {
        use rowan::NodeOrToken;

        let mut receiver_text: Option<String> = None;
        let mut method_name: Option<String> = None;
        let mut saw_dot = false;

        for item in method_call.children_with_tokens() {
            match item {
                NodeOrToken::Node(node) => {
                    if receiver_text.is_none()
                        && matches!(
                            node.kind(),
                            SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
                        )
                    {
                        receiver_text = Some(node.text().to_string().trim().to_string());
                    }
                }
                NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::DOT {
                        saw_dot = true;
                    } else if saw_dot && token.kind().is_identifier() {
                        method_name = Some(token.text().to_string());
                        break;
                    }
                }
            }
        }

        Some((receiver_text?, method_name?))
    }

    /// Resolve method return type using workspace index
    /// Returns the return type of a method on the given receiver type
    ///
    /// If `method_call_node` is provided, uses it to infer type parameters
    /// from the method call arguments.
    pub(crate) fn resolve_method_return_type(
        &self,
        receiver_type: &Type,
        method_name: &str,
        method_call_node: Option<&SyntaxNode>,
    ) -> Option<Type> {
        debug!(
            "Resolving method return type: {:?}.{}",
            receiver_type, method_name
        );

        // Special handling for Stream API methods (simplified heuristic approach)
        if let Some(stream_result) =
            self.try_resolve_stream_method(receiver_type, method_name, method_call_node)
        {
            return Some(stream_result);
        }

        let Some(workspace) = self.workspace_index.as_ref() else {
            tracing::warn!(
                "Cannot resolve method return type for {:?}.{}: no workspace index available",
                receiver_type,
                method_name
            );
            return None;
        };

        // Convert semantics Type to FQN string for workspace lookup
        let Some(receiver_fqn) = self.type_to_fqn(receiver_type) else {
            tracing::warn!(
                "Cannot resolve method return type for {:?}.{}: failed to convert receiver type to FQN",
                receiver_type,
                method_name
            );
            return None;
        };
        debug!("Receiver FQN: {}", receiver_fqn);

        // Helper function to try looking up a method
        let try_lookup = |fqn: &str| -> Option<Type> {
            // Priority 1: Find the effective source method set for the receiver.
            // This walks the hierarchy while collapsing overridden declarations.
            // Source files store methods as "Class.method(param1,param2)" with generic type info
            let source_methods = workspace.find_effective_methods_by_name(fqn, method_name);
            debug!(
                "find_effective_methods_by_name({}, {}) returned {} methods",
                fqn,
                method_name,
                source_methods.len()
            );

            // Priority 2: Fall back to exact FQN lookups (JDK format without parens)
            let method_fqn = format!("{fqn}.{method_name}");
            let method_fqn_with_empty_parens = format!("{method_fqn}()");

            let (symbol_opt, canonical_return_type) = if !source_methods.is_empty() {
                if let Some(call_node) = method_call_node {
                    let argument_types = self.infer_method_call_argument_types(call_node);
                    if let Some(resolved) = crate::call_resolution::resolve_call(
                        workspace,
                        fqn,
                        Some(receiver_type),
                        method_name,
                        &argument_types,
                    ) {
                        debug!(
                            declaration = %resolved.declaration.fqn,
                            confidence = ?resolved.confidence,
                            "legacy inference delegated overload selection"
                        );
                        (Some(resolved.declaration), Some(resolved.return_type))
                    } else {
                        // Keep a visible degraded path so incomplete editing states still
                        // produce a useful type. Compatibility decisions stay in resolve_call.
                        let fallback =
                            self.select_best_method_candidate(source_methods.iter().cloned());
                        if let Some(ref method) = fallback {
                            tracing::warn!(
                                "Canonical overload resolution found no match for '{}' \
                                 with {} args among {} candidates. Using metadata-quality fallback {}",
                                method_name,
                                argument_types.len(),
                                source_methods.len(),
                                method.fqn
                            );
                        }
                        (fallback, None)
                    }
                } else {
                    // The caller supplied no argument syntax, so overload applicability
                    // cannot be established. Choose only by metadata quality.
                    (
                        self.select_best_method_candidate(source_methods.iter().cloned()),
                        None,
                    )
                }
            } else if let Some(s) = workspace.get_symbol(&method_fqn_with_empty_parens) {
                debug!(
                    "Found method with empty parens: {}",
                    method_fqn_with_empty_parens
                );
                (Some(s), None)
            } else if let Some(s) = workspace.get_symbol(&method_fqn) {
                // JDK methods are stored without parentheses
                debug!("Found JDK method: {}", method_fqn);
                (Some(s), None)
            } else {
                (None, None)
            };

            if let Some(symbol) = symbol_opt {
                if let Some(return_type_str) = &symbol.return_type {
                    debug!("Symbol return_type string: '{}'", return_type_str);
                    let declaring_fqn = symbol
                        .container
                        .as_deref()
                        .map(str::to_owned)
                        .or_else(|| Self::extract_declaring_type_fqn(&symbol.fqn, method_name))
                        .unwrap_or_else(|| fqn.to_string());
                    // void return is meaningless when the result is being assigned (e.g. `var x = m();`),
                    // so collapse it to Unknown rather than propagating Type::Void here.
                    let resolved_by_contract = canonical_return_type.is_some();
                    let return_type = if let Some(canonical) = canonical_return_type {
                        canonical
                    } else {
                        // This fallback does not have resolve_call's owner substitution map.
                        // Project the declaration's spelling through the actual receiver
                        // hierarchy before parsing it, while leaving method-level type
                        // parameters untouched when they shadow owner parameters.
                        let receiver_arguments = match receiver_type {
                            Type::Class(receiver) => receiver
                                .type_arguments
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>(),
                            _ => Vec::new(),
                        };
                        let projected = workspace.project_member_type_from_receiver(
                            fqn,
                            &receiver_arguments,
                            &declaring_fqn,
                            return_type_str,
                            &symbol.type_parameters,
                        )?;
                        self.parse_type_from_string(&projected)?
                    };
                    let mut parsed = match return_type {
                        Type::Void => Type::Unknown,
                        other => other,
                    };
                    debug!("Parsed return type: {:?}", parsed);

                    // Normalize unqualified/ambiguous return class names using the actual
                    // declaring class FQN from the resolved symbol (important for nested classes).
                    if let Type::Class(ref ct) = parsed {
                        let declaring_simple = declaring_fqn.rsplit('.').next().unwrap_or_default();

                        // Fluent pattern: method returns declaring type itself.
                        if ct.name == declaring_simple {
                            parsed = self.class_type_from_fqn_with_template(&declaring_fqn, ct);
                        } else {
                            let parsed_fqn = ct.package.as_ref().map_or_else(
                                || ct.name.clone(),
                                |pkg| format!("{pkg}.{}", ct.name),
                            );

                            // Nested-class fallback:
                            // if parsed class FQN doesn't exist, try declaringFqn.ReturnType.
                            if workspace.get_symbol(&parsed_fqn).is_none() {
                                let nested_fqn = format!("{}.{}", declaring_fqn, ct.name);
                                if workspace.get_symbol(&nested_fqn).is_some() {
                                    parsed =
                                        self.class_type_from_fqn_with_template(&nested_fqn, ct);
                                }
                            }
                        }
                    }

                    // Type parameter inference from method call arguments (Floor 6-7)
                    // If we have a method call node and the return type contains type variables,
                    // try to infer the type parameters from the actual arguments
                    if !resolved_by_contract {
                        if let Some(call_node) = method_call_node {
                            parsed = self.infer_and_substitute_type_parameters(
                                parsed,
                                call_node,
                                symbol.as_ref(),
                            );
                        }
                    }

                    return Some(parsed);
                }
            }
            None
        };

        // 1. Try direct lookup with the receiver FQN as-is
        if let Some(result) = try_lookup(&receiver_fqn) {
            info!(
                "Method return type resolved: {:?}.{} -> {:?}",
                receiver_type, method_name, result
            );
            return Some(result);
        }

        // 2. If receiver has no package, try common package patterns
        if let Type::Class(ct) = receiver_type {
            if ct.package.is_none() {
                // Try looking up the class symbol to get its full FQN
                // Look up just the class name - the workspace might have it
                if let Some(class_symbol) = workspace.get_symbol(&ct.name) {
                    if let Some(result) = try_lookup(&class_symbol.fqn) {
                        return Some(result);
                    }
                }

                // Try common package prefixes (including standard Java packages)
                // Reuse ct.name reference to avoid cloning
                for prefix in &[
                    "java.util.function",
                    "java.util",
                    "java.lang",
                    "java.io",
                    "com.example",
                    "org.example",
                    "",
                ] {
                    let fqn = if prefix.is_empty() {
                        &ct.name
                    } else {
                        // Only allocate for qualified names
                        &format!("{}.{}", prefix, ct.name)
                    };
                    if let Some(result) = try_lookup(fqn) {
                        return Some(result);
                    }
                }
            }
        }

        // 3. If receiver has a package but lookup failed, it might be a nested class
        // with incorrect FQN (e.g., "com.example.Builder" should be "com.example.User.Builder")
        // Try alternative FQNs for classes with the same simple name
        if let Type::Class(ct) = receiver_type {
            // Extract simple name from the receiver type
            let simple_name = &ct.name;

            // Get all FQNs with this simple name
            let alternative_fqns = workspace.get_fqns_by_simple_name(simple_name);
            debug!(
                "Trying {} alternative FQNs for {}",
                alternative_fqns.len(),
                simple_name
            );

            for alt_fqn in alternative_fqns {
                // Skip the one we already tried
                if alt_fqn == receiver_fqn {
                    continue;
                }
                if let Some(result) = try_lookup(&alt_fqn) {
                    debug!(
                        "Found method via alternative FQN: {}.{}",
                        alt_fqn, method_name
                    );
                    return Some(result);
                }
            }
        }

        tracing::warn!(
            "Failed to resolve method return type for {:?}.{}: method not found in workspace after trying all FQN alternatives",
            receiver_type, method_name
        );
        None
    }

    /// Infer type parameters from method call arguments and substitute in return type (Floor 6-7)
    /// For generic methods like Arrays.asList("a", "b"), infers T=String from arguments
    pub(crate) fn infer_and_substitute_type_parameters(
        &self,
        return_type: Type,
        method_call_node: &SyntaxNode,
        declaration: &hyperion_workspace::Symbol,
    ) -> Type {
        // Extract argument types from the METHOD_CALL_EXPR (Floor 4 logic)
        let arg_types = self.infer_method_call_argument_types(method_call_node);

        debug!(
            "Type parameter inference: return_type={:?}, arg_types={:?}",
            return_type, arg_types
        );

        let substitutions =
            crate::call_resolution::infer_method_type_substitutions(declaration, &arg_types);
        super::type_parameter_mapping::apply_type_parameter_substitutions(
            return_type,
            &substitutions,
        )
    }

    /// Extract and infer types of all arguments in a method call (Floor 4 logic)
    ///
    /// Uses comma-based boundaries to correctly count arguments even when
    /// expressions contain nested sub-expressions (e.g., `method(a.b())` is 1 arg, not 2).
    /// Infer the argument types present in a parsed Java method call.
    ///
    /// Callers can pass the result directly to [`crate::call_resolution::resolve_call`].
    pub fn infer_method_call_argument_types(&self, method_call: &SyntaxNode) -> Vec<Type> {
        let mut arg_types = Vec::new();

        // Find ARG_LIST child
        if let Some(arg_list) = method_call
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        {
            // Use comma-based argument extraction to correctly handle nested expressions
            let mut current_arg_first_expr: Option<SyntaxNode> = None;

            for child in arg_list.children_with_tokens() {
                match child {
                    // Record only the first expression node for this argument.
                    rowan::NodeOrToken::Node(node)
                        if current_arg_first_expr.is_none()
                            && Self::is_argument_expression_kind(node.kind()) =>
                    {
                        current_arg_first_expr = Some(node.clone());
                    }
                    rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                        // Comma marks end of current argument - process it
                        if let Some(arg) = current_arg_first_expr.take() {
                            let arg_type = self.infer_single_argument_type(&arg);
                            arg_types.push(arg_type);
                        }
                        // Reset for next argument
                        current_arg_first_expr = None;
                    }
                    _ => {
                        // Skip other tokens (parens, whitespace, etc.)
                    }
                }
            }

            // Don't forget the last argument (no trailing comma)
            if let Some(arg) = current_arg_first_expr.take() {
                let arg_type = self.infer_single_argument_type(&arg);
                arg_types.push(arg_type);
            }
        }

        arg_types
    }

    /// Check if a SyntaxKind represents an argument expression node
    const fn is_argument_expression_kind(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::LITERAL_EXPR
                | SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::BINARY_EXPR
                | SyntaxKind::UNARY_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::TERNARY_EXPR
                | SyntaxKind::THIS_EXPR
                | SyntaxKind::SUPER_EXPR
                | SyntaxKind::LAMBDA_EXPR
                | SyntaxKind::ARRAY_ACCESS_EXPR
                | SyntaxKind::ARRAY_INIT_EXPR
                | SyntaxKind::CLASS_LITERAL_EXPR
                | SyntaxKind::METHOD_REF_EXPR
        )
    }

    /// Infer type for a single argument expression
    fn infer_single_argument_type(&self, arg: &SyntaxNode) -> Type {
        // Delegate to the full expression type inference system
        // This handles:
        // - NAME_EXPR: variable/parameter lookup from scope
        // - FIELD_ACCESS_EXPR: field type resolution
        // - METHOD_CALL_EXPR: recursive method return type inference
        // - LITERAL_EXPR, NEW_EXPR, and all other expression types
        self.infer_expression_type(arg)
    }

    /// Try to resolve Stream API methods using heuristic approach
    ///
    /// This function handles common Stream API patterns:
    /// - `Collection<T>.stream()` → `Stream<T>`
    /// - `Stream<T>.map(fn)` → `Stream<R>` where R is fn's return type
    /// - `Stream<T>.collect(Collectors.toList())` → `List<T>`
    ///
    /// Returns None if this isn't a Stream API method or can't be resolved.
    fn try_resolve_stream_method(
        &self,
        receiver_type: &Type,
        method_name: &str,
        method_call_node: Option<&SyntaxNode>,
    ) -> Option<Type> {
        match method_name {
            "stream" => {
                // Collection<T>.stream() → Stream<T>
                if let Type::Class(ct) = receiver_type {
                    // Extract element type from Collection, List, Set, etc.
                    // Clone once, reuse for vec construction
                    let element_type = if ct.type_arguments.is_empty() {
                        Type::Unknown
                    } else {
                        ct.type_arguments[0].clone()
                    };

                    debug!("Stream() on collection, element type: {:?}", element_type);
                    return Some(Type::Class(ClassType {
                        name: "Stream".into(),
                        package: Some("java.util.stream".into()),
                        type_arguments: vec![element_type],
                        is_diamond: false,
                    }));
                }
            }
            "map" => {
                // Stream<T>.map(function) → Stream<R>
                // Need to extract R from the function argument
                if let Type::Class(ct) = receiver_type {
                    if ct.name == "Stream" {
                        // Extract the function argument to get its return type
                        if let Some(call_node) = method_call_node {
                            // Look for METHOD_REF_EXPR or LAMBDA_EXPR in arguments
                            for child in call_node.descendants() {
                                if child.kind() == SyntaxKind::METHOD_REF_EXPR {
                                    // Extract method reference (e.g., User::getName)
                                    if let Some(method_ref) =
                                        crate::resolver::method_reference::extract_method_reference(
                                            &child,
                                        )
                                    {
                                        debug!(
                                            "Found method reference in map(): {}::{}",
                                            method_ref.receiver_type, method_ref.method_name
                                        );

                                        // Try to resolve the method reference return type
                                        // For User::getName, this should resolve to String
                                        let resolve_type_fn = |type_name: &str| {
                                            // Simple resolution: try to find the class in workspace
                                            let workspace = self.workspace_index.as_ref()?;

                                            // Try common package prefixes
                                            let candidates = vec![
                                                type_name.to_string(), // Already qualified
                                                format!("com.example.{}", type_name),
                                                format!("java.lang.{}", type_name),
                                            ];

                                            for fqn in candidates {
                                                if workspace.get_symbol(&fqn).is_some() {
                                                    return Some(Type::Class(ClassType {
                                                        name: type_name.to_string(),
                                                        package: None,
                                                        type_arguments: vec![],
                                                        is_diamond: false,
                                                    }));
                                                }
                                            }
                                            None
                                        };

                                        let lookup_method_fn =
                                            |receiver_ty: &Type, method_name: &str| {
                                                // Use existing method resolution
                                                self.resolve_method_return_type(
                                                    receiver_ty,
                                                    method_name,
                                                    None,
                                                )
                                            };

                                        if let Some(return_type) = crate::resolver::method_reference::resolve_method_reference_return_type(
                                            &method_ref,
                                            resolve_type_fn,
                                            lookup_method_fn,
                                        ) {
                                            debug!("Method reference returns: {:?}", return_type);
                                            return Some(Type::Class(ClassType {
                                                name: "Stream".into(),
                                                package: Some("java.util.stream".into()),
                                                type_arguments: vec![return_type],
                                                is_diamond: false,
                                            }));
                                        }
                                    }
                                }
                            }
                        }

                        // Fallback: return Stream<Unknown>
                        debug!("Couldn't infer map() result type, returning Stream<Unknown>");
                        return Some(Type::Class(ClassType {
                            name: "Stream".into(),
                            package: Some("java.util.stream".into()),
                            type_arguments: vec![Type::Unknown],
                            is_diamond: false,
                        }));
                    }
                }
            }
            "collect" => {
                // Stream<T>.collect(Collectors.toList()) → List<T>
                if let Type::Class(ct) = receiver_type {
                    if ct.name == "Stream" {
                        // Clone element type once upfront
                        let element_type = if ct.type_arguments.is_empty() {
                            Type::Unknown
                        } else {
                            ct.type_arguments[0].clone()
                        };

                        // Try to infer the return type from the collector argument
                        if let Some(call_node) = method_call_node {
                            if let Some(inferred_type) =
                                self.infer_collect_return_type(call_node, &element_type)
                            {
                                return Some(inferred_type);
                            }
                        }

                        // Unknown collector - most Collectors return some form of Collection
                        // Use Collection<T> as a reasonable fallback
                        debug!(
                            "Unknown collector, returning Collection<{:?}> as fallback",
                            element_type
                        );
                        return Some(Type::Class(ClassType {
                            name: "Collection".into(),
                            package: Some("java.util".into()),
                            type_arguments: vec![element_type],
                            is_diamond: false,
                        }));
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Infer the return type of Stream.collect() based on the collector argument
    ///
    /// Properly resolves the collector method call instead of using string matching.
    /// Returns None if the collector type cannot be determined.
    fn infer_collect_return_type(
        &self,
        method_call_node: &SyntaxNode,
        element_type: &Type,
    ) -> Option<Type> {
        // Find the ARG_LIST and extract the first argument (the collector)
        let arg_list = method_call_node
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)?;

        // Extract the first argument expression
        let collector_arg = arg_list
            .children()
            .find(|n| Self::is_argument_expression_kind(n.kind()))?;

        debug!(
            "Found collector argument of kind: {:?}, text: {}",
            collector_arg.kind(),
            collector_arg.text()
        );

        // Check if the collector is a method call (e.g., Collectors.toList())
        if collector_arg.kind() == SyntaxKind::METHOD_CALL_EXPR {
            return self.resolve_collectors_method(&collector_arg, element_type);
        }

        // Check if it's a field access that might be a static method reference
        // e.g., Collectors.toList() might be parsed as FIELD_ACCESS_EXPR + METHOD_CALL_EXPR
        if collector_arg.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
            return self.resolve_collectors_method(&collector_arg, element_type);
        }

        debug!("Unknown collector type, kind: {:?}", collector_arg.kind());
        // Unknown collector type
        None
    }

    /// Resolve a Collectors.method() call to determine the collection type
    ///
    /// Handles:
    /// - Collectors.toList() → List<T>
    /// - Collectors.toSet() → Set<T>
    /// - Collectors.toCollection(TreeSet::new) → Collection<T>
    /// - Other collectors → Unknown
    fn resolve_collectors_method(
        &self,
        collector_expr: &SyntaxNode,
        element_type: &Type,
    ) -> Option<Type> {
        // For METHOD_CALL_EXPR, check if it's calling a method on Collectors
        if collector_expr.kind() == SyntaxKind::METHOD_CALL_EXPR {
            // Current parser shape: METHOD_CALL_EXPR(NAME_EXPR, DOT, IDENT, ARG_LIST)
            if let Some((receiver_name, method_name)) =
                Self::extract_simple_method_call_parts(collector_expr)
            {
                if Self::is_collectors_type_name(&receiver_name) {
                    return Self::collectors_result_type(&method_name, element_type);
                }
            }

            if let Some(method_name) = Self::extract_unqualified_call_name(collector_expr) {
                if Self::is_statically_imported_collectors_method(collector_expr, &method_name) {
                    return Self::collectors_result_type(&method_name, element_type);
                }
            }

            // Legacy parser shape: METHOD_CALL_EXPR(FIELD_ACCESS_EXPR, ARG_LIST)
            if let Some(field_access) = collector_expr
                .children()
                .find(|n| n.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
            {
                return self.resolve_collectors_field_access(&field_access, element_type);
            }
        }

        // For FIELD_ACCESS_EXPR, check if receiver is Collectors
        if collector_expr.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
            return self.resolve_collectors_field_access(collector_expr, element_type);
        }

        None
    }

    /// Check if a FIELD_ACCESS_EXPR is Collectors.method and return the appropriate type
    fn resolve_collectors_field_access(
        &self,
        field_access: &SyntaxNode,
        element_type: &Type,
    ) -> Option<Type> {
        // Extract the receiver and method name
        let children: Vec<_> = field_access.children().collect();
        if children.is_empty() {
            return None;
        }

        // First child is the receiver
        let receiver = &children[0];

        // Check if receiver is a NAME_EXPR with text "Collectors"
        if receiver.kind() == SyntaxKind::NAME_EXPR {
            let receiver_text = receiver.text().to_string();
            if !Self::is_collectors_type_name(receiver_text.trim()) {
                return None;
            }
        } else {
            return None;
        }

        // Extract the method name
        let method_name = self.extract_method_name_from_field_access(field_access)?;

        Self::collectors_result_type(&method_name, element_type)
    }

    /// Recognize the conventional imported spelling and the canonical JDK FQN.
    /// Static-imported factory methods are validated separately against the
    /// source file's import map.
    fn is_collectors_type_name(name: &str) -> bool {
        matches!(name, "Collectors" | "java.util.stream.Collectors")
    }

    fn extract_unqualified_call_name(method_call: &SyntaxNode) -> Option<String> {
        if method_call.children_with_tokens().any(|item| {
            item.as_token()
                .is_some_and(|token| token.kind() == SyntaxKind::DOT)
        }) {
            return None;
        }

        method_call
            .children()
            .find(|node| node.kind() == SyntaxKind::NAME_EXPR)
            .map(|node| node.text().to_string().trim().to_string())
            .filter(|name| !name.is_empty())
    }

    fn is_statically_imported_collectors_method(
        method_call: &SyntaxNode,
        method_name: &str,
    ) -> bool {
        let Some(root) = method_call.ancestors().last() else {
            return false;
        };
        let imports = build_imports_from_syntax(&root);
        imports.static_imports_named(method_name).any(|imported| {
            imported
                .rsplit_once('.')
                .is_some_and(|(owner, _)| owner == "java.util.stream.Collectors")
        }) || imports
            .static_wildcards
            .iter()
            .any(|owner| owner == "java.util.stream.Collectors")
    }

    fn collectors_result_type(method_name: &str, element_type: &Type) -> Option<Type> {
        match method_name {
            "toList" => {
                debug!(
                    "Resolved Collectors.toList(), returning List<{:?}>",
                    element_type
                );
                Some(Type::Class(ClassType {
                    name: "List".into(),
                    package: Some("java.util".into()),
                    type_arguments: vec![element_type.clone()],
                    is_diamond: false,
                }))
            }
            "toSet" => {
                debug!(
                    "Resolved Collectors.toSet(), returning Set<{:?}>",
                    element_type
                );
                Some(Type::Class(ClassType {
                    name: "Set".into(),
                    package: Some("java.util".into()),
                    type_arguments: vec![element_type.clone()],
                    is_diamond: false,
                }))
            }
            "toCollection" => {
                debug!(
                    "Resolved Collectors.toCollection(), returning Collection<{:?}>",
                    element_type
                );
                Some(Type::Class(ClassType {
                    name: "Collection".into(),
                    package: Some("java.util".into()),
                    type_arguments: vec![element_type.clone()],
                    is_diamond: false,
                }))
            }
            _ => {
                // Unknown Collectors method, return Unknown
                debug!(
                    "Unknown Collectors method: {}, returning Unknown",
                    method_name
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileId;
    use hyperion_parser::{Lexer, Parser};
    use std::sync::Arc;

    fn method_call(source: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "test source should parse: {errors:?}");
        SyntaxNode::new_root(green)
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .expect("method call should exist")
    }

    #[test]
    fn legacy_return_inference_uses_canonical_invocation_phases() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        let service_source = r#"
            package com.example;
            class Service {
                Object choose(Integer value) { return value; }
                String choose(long value) { return ""; }
            }
        "#;
        workspace
            .index_file_with_source(
                &temp.path().join("com/example/Service.java"),
                service_source,
            )
            .expect("service should index");

        let call = method_call("class Main { void test(Service service) { service.choose(1); } }");
        let receiver = Type::Class(ClassType {
            name: "Service".to_string(),
            package: Some("com.example".to_string()),
            type_arguments: Vec::new(),
            is_diamond: false,
        });
        let builder = ResolverBuilder::with_workspace(FileId::new(0), workspace);

        let resolved = builder
            .resolve_method_return_type(&receiver, "choose", Some(&call))
            .expect("overload should resolve");

        assert!(matches!(
            resolved,
            Type::Class(class_type) if class_type.name == "String"
        ));
    }

    #[test]
    fn degraded_return_inference_composes_inherited_owner_arguments() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        workspace
            .index_file_with_source(
                &temp.path().join("test/Hierarchy.java"),
                r#"
                    package test;
                    class Base<T> { T value() { return null; } }
                    class Middle<U> extends Base<java.util.List<U>> {}
                    class Child extends Middle<String> {}
                "#,
            )
            .expect("generic hierarchy should index");
        let receiver = Type::Class(ClassType {
            name: "Child".to_string(),
            package: Some("test".to_string()),
            type_arguments: Vec::new(),
            is_diamond: false,
        });
        let builder = ResolverBuilder::with_workspace(FileId::new(0), workspace);

        let resolved = builder
            .resolve_method_return_type(&receiver, "value", None)
            .expect("inherited return should project without argument syntax");

        assert!(matches!(
            resolved,
            Type::Class(class_type)
                if class_type.name == "List"
                    && matches!(class_type.type_arguments.as_slice(),
                        [Type::Class(element)] if element.name == "String")
        ));
    }

    #[test]
    fn degraded_return_inference_preserves_method_parameter_shadowing() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(hyperion_workspace::WorkspaceIndex::new(
            temp.path().to_path_buf(),
        ));
        workspace
            .index_file_with_source(
                &temp.path().join("test/Hierarchy.java"),
                r#"
                    package test;
                    class Base<T> { <T> T value() { return null; } }
                    class Child extends Base<String> {}
                "#,
            )
            .expect("generic hierarchy should index");
        let receiver = Type::Class(ClassType {
            name: "Child".to_string(),
            package: Some("test".to_string()),
            type_arguments: Vec::new(),
            is_diamond: false,
        });
        let builder = ResolverBuilder::with_workspace(FileId::new(0), workspace);

        let resolved = builder
            .resolve_method_return_type(&receiver, "value", None)
            .expect("shadowed method parameter should remain unresolved");

        assert!(matches!(
            resolved,
            Type::Class(class_type)
                if class_type.package.is_none() && class_type.name == "T"
        ));
    }

    #[test]
    fn qualified_collectors_factory_preserves_stream_element_type() {
        let call =
            method_call("class Main { void test() { java.util.stream.Collectors.toList(); } }");
        let element_type = Type::Class(ClassType {
            name: "String".to_string(),
            package: Some("java.lang".to_string()),
            type_arguments: Vec::new(),
            is_diamond: false,
        });
        let builder = ResolverBuilder::new(FileId::new(0));

        let resolved = builder.resolve_collectors_method(&call, &element_type);

        assert!(
            matches!(
                resolved,
                Some(Type::Class(class_type))
                    if class_type.name == "List"
                        && class_type.type_arguments == vec![element_type]
            ),
            "qualified collector call shape was not recognized: {call:#?}"
        );
    }
}
