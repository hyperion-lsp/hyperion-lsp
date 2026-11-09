// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Hover Provider
//!
//! This module provides hover functionality for Kotlin files in the Hyperion LSP.

use crate::navigation::NavigationProvider;
use crate::position_utils::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;
use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase};
use hyperion_workspace::{KotlinTypeAliasArguments, QueryEngine, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::{
    Hover, HoverContents, MarkupContent, MarkupKind, Position, SymbolKind, Url,
};
use tracing::{debug, trace};

/// Check if a Kotlin token is an identifier or a soft keyword that can be used as an identifier.
///
/// In Kotlin, soft keywords like `data`, `sealed`, `catch`, `finally`, etc. can be used as
/// identifiers in most contexts (e.g., `val data = 5` is valid Kotlin).
pub(crate) const fn is_kotlin_identifier_like(kind: hyperion_kotlin::SyntaxKind) -> bool {
    use hyperion_kotlin::SyntaxKind as KtKind;
    matches!(
        kind,
        KtKind::IDENT
            | KtKind::DATA_KW
            | KtKind::SEALED_KW
            | KtKind::CATCH_KW
            | KtKind::FINALLY_KW
            | KtKind::CONSTRUCTOR_KW
            | KtKind::INIT_KW
            | KtKind::BY_KW
            | KtKind::EXPECT_KW
            | KtKind::ACTUAL_KW
            | KtKind::COMPANION_KW
            | KtKind::SUSPEND_KW
            | KtKind::INLINE_KW
            | KtKind::OPERATOR_KW
            | KtKind::INFIX_KW
            | KtKind::OPEN_KW
            | KtKind::ABSTRACT_KW
            | KtKind::OVERRIDE_KW
    )
}

/// Helper for Kotlin hover functionality
/// Split a type-argument list at its top-level commas: those not inside a nested `<...>`, so
/// `Pair<A, B>, Int` gives `Pair<A, B>` and `Int`.
fn split_top_level_commas(arguments: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in arguments.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&arguments[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&arguments[start..]);
    parts
}

pub(crate) struct KotlinHoverHelper<'a> {
    txn: &'a ReadTxn,
    workspace_index: &'a Arc<WorkspaceIndex>,
    navigation_provider: NavigationProvider<'a>,
}

/// Initializer forms whose type `TypeExtractor` cannot compute.
///
/// `TypeExtractor::infer_val_type` scans the initializer as a flat token stream
/// and returns the first literal/constructor it recognises. For `when`, `try`
/// and `if` that first literal is a branch *condition* (`when (x) { 1 -> "a" }`
/// yields `Int`), not the branch *result*. Those forms need the real
/// least-upper-bound computation in `KotlinTypeInferenceEngine`.
const fn is_lub_expression_kind(kind: hyperion_kotlin::SyntaxKind) -> bool {
    use hyperion_kotlin::SyntaxKind as KtKind;
    matches!(kind, KtKind::WHEN_EXPR | KtKind::TRY_EXPR | KtKind::IF_EXPR)
}

/// The initializer expression node of a `PROPERTY_DECL`, i.e. the first node
/// child that follows the `=` token. Returns `None` for declarations without an
/// initializer or whose initializer is a bare token run.
fn property_initializer_node(
    prop_decl: &hyperion_kotlin::SyntaxNode,
) -> Option<hyperion_kotlin::SyntaxNode> {
    use hyperion_kotlin::SyntaxKind as KtKind;

    let mut seen_eq = false;
    for child in prop_decl.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(t) if t.kind() == KtKind::EQ => seen_eq = true,
            rowan::NodeOrToken::Node(n) if seen_eq => return Some(n),
            _ => {}
        }
    }
    None
}

/// Does this property declaration carry an explicit type annotation?
fn property_has_explicit_type(prop_decl: &hyperion_kotlin::SyntaxNode) -> bool {
    hyperion_kotlin::TypeExtractor::extract_from_property(prop_decl).is_some()
}

impl<'a> KotlinHoverHelper<'a> {
    pub fn new(
        txn: &'a ReadTxn,
        workspace_index: &'a Arc<WorkspaceIndex>,
        source_providers: &'a Arc<crate::source_provider::SourceProviderChain>,
    ) -> Self {
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(workspace_index),
            Arc::new(QueryEngine::new(workspace_index.as_ref().clone())),
            Arc::clone(source_providers),
        );
        Self {
            txn,
            workspace_index,
            navigation_provider,
        }
    }

    /// Build the Kotlin type-inference engine plus a resolver seeded with this
    /// file's Salsa-cached symbol table.
    ///
    /// Mirrors how `navigation::kotlin` and `compute_kotlin_file_semantics`
    /// obtain the engine: a snapshot under `try_lock_for` (degrade to "no
    /// inference" rather than freeze behind `did_change`), the shared symbol
    /// table handed to `SymbolResolver::with_cached_table` to avoid the clone
    /// tax, and a fresh engine per request so its DSL-receiver memoisation is
    /// scoped to the request.
    fn inference_context(
        &self,
        file_id: FileId,
    ) -> Option<(
        hyperion_semantics::KotlinTypeInferenceEngine,
        hyperion_semantics::SymbolResolver,
    )> {
        let symbol_table = {
            let snapshot = self.txn.snapshot();
            snapshot.file_symbol_table(file_id)
        };

        Some((
            hyperion_semantics::KotlinTypeInferenceEngine::new(
                Arc::clone(self.workspace_index),
                file_id,
            ),
            hyperion_semantics::SymbolResolver::with_cached_table(symbol_table),
        ))
    }

    /// Infer the type of an expression node with the inference engine.
    ///
    /// `None` when the engine cannot type the expression, so callers can chain
    /// this with the scope-graph path using plain `Option` combinators.
    fn infer_node_type(
        &self,
        file_id: FileId,
        node: &hyperion_kotlin::SyntaxNode,
    ) -> Option<hyperion_kotlin::KotlinType> {
        let (engine, mut resolver) = self.inference_context(file_id)?;
        let inferred = engine.infer_expression(node, &mut resolver);
        Self::displayable_type(&inferred)
    }

    /// Infer the type of the identifier-like token under the cursor.
    fn infer_token_type(
        &self,
        file_id: FileId,
        token: &hyperion_kotlin::SyntaxToken,
    ) -> Option<hyperion_kotlin::KotlinType> {
        let (engine, mut resolver) = self.inference_context(file_id)?;
        let inferred = engine.infer_token_expression(token, &mut resolver);
        Self::displayable_type(&inferred)
    }

    /// Convert an inferred semantic type to the Kotlin surface type used in
    /// hover output, dropping results that carry no information.
    fn displayable_type(ty: &hyperion_semantics::Type) -> Option<hyperion_kotlin::KotlinType> {
        if matches!(ty, hyperion_semantics::Type::Unknown) {
            return None;
        }
        let kotlin_type = hyperion_semantics::semantics_type_to_kotlin(ty);
        if matches!(kotlin_type, hyperion_kotlin::KotlinType::Unknown) {
            return None;
        }
        Some(kotlin_type)
    }

    /// Type of a `val`/`var` whose initializer is a `when`/`try`/`if`
    /// expression, which only the inference engine can compute.
    ///
    /// Returns `None` for every other declaration so the (better tested)
    /// scope-graph path keeps precedence there.
    fn infer_lub_initializer_type(
        &self,
        file_id: FileId,
        prop_decl: &hyperion_kotlin::SyntaxNode,
    ) -> Option<hyperion_kotlin::KotlinType> {
        if property_has_explicit_type(prop_decl) {
            return None;
        }
        let initializer = property_initializer_node(prop_decl)?;
        if !is_lub_expression_kind(initializer.kind()) {
            return None;
        }
        self.infer_node_type(file_id, &initializer)
    }

    /// Walk up from a token to the enclosing `PROPERTY_DECL`, if any.
    fn enclosing_property_decl(
        token: &hyperion_kotlin::SyntaxToken,
    ) -> Option<hyperion_kotlin::SyntaxNode> {
        use hyperion_kotlin::SyntaxKind as KtKind;

        let mut current = token.parent();
        while let Some(node) = current {
            if node.kind() == KtKind::PROPERTY_DECL {
                return Some(node);
            }
            current = node.parent();
        }
        None
    }

    /// Type of the declaration a scope-graph symbol points at, when that
    /// declaration is a `when`/`try`/`if` initializer.
    ///
    /// Used at *use* sites (`println(r)`): the scope graph stores the type it
    /// got from the token-stream heuristic, which is wrong for these forms.
    fn infer_lub_type_of_declaration(
        &self,
        file_id: FileId,
        root: &hyperion_kotlin::SyntaxNode,
        definition_range: rowan::TextRange,
    ) -> Option<hyperion_kotlin::KotlinType> {
        let token = root
            .token_at_offset(definition_range.start())
            .right_biased()?;
        let prop_decl = Self::enclosing_property_decl(&token)?;
        self.infer_lub_initializer_type(file_id, &prop_decl)
    }

    /// Creates a Markdown hover response.
    const fn create_hover(&self, content: String) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }
    }

    /// Infer method return type for Kotlin using user-defined generic type information
    ///
    /// This handles user-defined generic classes by:
    /// 1. Looking up the receiver class in the workspace
    /// 2. Getting the class's type parameters
    /// 3. Finding the method's return type
    /// 4. Substituting type variables with concrete types from the receiver
    ///
    /// Example:
    /// ```kotlin
    /// class Repository<T> {
    ///     fun findById(id: Int): T = TODO()
    /// }
    /// val repo = Repository<User>()
    /// val user = repo.findById(1)  // user: User (inferred via substitution)
    /// ```
    pub fn infer_kotlin_method_return_type_with_generics(
        &self,
        file_id: hyperion_semantics::db::FileId,
        receiver_type: &hyperion_kotlin::KotlinType,
        method_name: &str,
    ) -> Option<hyperion_kotlin::KotlinType> {
        debug!(
            "HOVER: infer_kotlin_method_return_type_with_generics receiver={:?}, method={}",
            receiver_type, method_name
        );

        // Extract class FQN and type arguments from receiver
        let (class_fqn, written_type_args) = if let hyperion_kotlin::KotlinType::Class {
            fqn,
            type_arguments,
            ..
        } = receiver_type
        {
            (fqn.as_str(), type_arguments)
        } else {
            debug!("HOVER: receiver is not a Class type");
            return None;
        };

        debug!(
            "HOVER: class_fqn={}, type_args={:?}",
            class_fqn, written_type_args
        );

        // Skip stdlib types (handled by stdlib registry)
        if class_fqn.starts_with("kotlin.") || class_fqn.starts_with("java.") {
            debug!("HOVER: skipping stdlib type");
            return None;
        }

        // Look up the class in workspace with deterministic ranking:
        // workspace symbols first, then lexicographic fallback.
        let class_symbol = self.resolve_class_symbol_by_name(file_id, class_fqn)?;

        debug!(
            "HOVER: found class with {} type_params: {:?}",
            class_symbol.type_parameters.len(),
            class_symbol
                .type_parameters
                .iter()
                .map(|p| &p.name)
                .collect::<Vec<_>>()
        );

        // Check if class has type parameters
        if class_symbol.type_parameters.is_empty() {
            return None;
        }

        // A typealias gives the class the arguments its expansion does, with
        // the written arguments bound to the alias's own type parameters; a
        // use whose arguments do not match them is not guessed.
        let alias_type_args;
        let type_args = match self.workspace_index.resolve_kotlin_type_alias_arguments(
            file_id,
            &[],
            class_fqn,
            &written_type_args
                .iter()
                .map(Self::type_argument_spelling)
                .collect::<Vec<_>>(),
        ) {
            KotlinTypeAliasArguments::NotAnAlias => written_type_args,
            KotlinTypeAliasArguments::Expanded(arguments) => {
                alias_type_args = arguments
                    .iter()
                    .map(|spelling| Self::alias_type_argument(spelling))
                    .collect::<Vec<_>>();
                &alias_type_args
            }
            KotlinTypeAliasArguments::Unmatched => return None,
        };

        // Build substitution map: type param name -> concrete type
        let mut substitutions: std::collections::HashMap<String, hyperion_kotlin::KotlinType> =
            std::collections::HashMap::new();
        for (i, param) in class_symbol.type_parameters.iter().enumerate() {
            if let Some(hyperion_kotlin::TypeArgument::Type(concrete_type)) = type_args.get(i) {
                substitutions.insert(param.name.to_string(), concrete_type.clone());
            }
        }

        // Find the method in the class
        // Note: Kotlin methods don't include parameter signatures in FQN, so we need
        // to use a different lookup strategy than Java's find_methods_by_name
        let full_class_fqn = &class_symbol.fqn;

        // First try the standard lookup (works for Java methods with signatures)
        let mut methods = self
            .workspace_index
            .find_methods_by_name(full_class_fqn, method_name);

        // Kotlin method FQNs omit Java-style parameter descriptors. Query the
        // exact owner's effective members instead of scanning every same-named
        // declaration in the workspace and filtering after allocation.
        if methods.is_empty() {
            methods.extend(
                self.workspace_index
                    .get_class_members_with_inheritance(full_class_fqn)
                    .into_iter()
                    .filter(|symbol| {
                        symbol.simple_name.as_ref() == method_name
                            && matches!(
                                symbol.kind,
                                SymbolKind::FUNCTION | SymbolKind::METHOD | SymbolKind::CONSTRUCTOR
                            )
                    }),
            );
        }
        methods.sort_by(|left, right| left.fqn.cmp(&right.fqn));
        methods.dedup_by(|left, right| left.fqn == right.fqn);

        let mut inferred = methods
            .into_iter()
            .filter_map(|method| method.return_type.clone())
            .filter_map(|return_type| {
                Self::substitute_kotlin_type_string(&return_type, &substitutions)
            });
        let first = inferred.next()?;
        inferred
            .all(|candidate| candidate == first)
            .then_some(first)
    }

    /// One written type argument as a Kotlin spelling that keeps each class's
    /// name as the receiver type carries it (`com.example.User`, `out
    /// Number`, `*`). Empty for a type that has no spelling, which a
    /// typealias then refuses as a missing argument.
    fn type_argument_spelling(argument: &hyperion_kotlin::TypeArgument) -> String {
        fn argument_spelling(argument: &hyperion_kotlin::TypeArgument) -> Option<String> {
            match argument {
                hyperion_kotlin::TypeArgument::Type(ty) => type_spelling(ty),
                hyperion_kotlin::TypeArgument::Star => Some("*".to_owned()),
                hyperion_kotlin::TypeArgument::Projected { variance, bound } => {
                    let variance = match variance {
                        hyperion_kotlin::Variance::Covariant => "out ",
                        hyperion_kotlin::Variance::Contravariant => "in ",
                        hyperion_kotlin::Variance::Invariant => "",
                    };
                    Some(format!("{variance}{}", type_spelling(bound)?))
                }
            }
        }
        fn type_spelling(ty: &hyperion_kotlin::KotlinType) -> Option<String> {
            match ty {
                hyperion_kotlin::KotlinType::Class {
                    fqn,
                    type_arguments,
                    nullability,
                } => {
                    let mut spelling = fqn.clone();
                    if !type_arguments.is_empty() {
                        let arguments = type_arguments
                            .iter()
                            .map(argument_spelling)
                            .collect::<Option<Vec<_>>>()?;
                        spelling.push('<');
                        spelling.push_str(&arguments.join(", "));
                        spelling.push('>');
                    }
                    if *nullability == hyperion_kotlin::Nullability::Nullable {
                        spelling.push('?');
                    }
                    Some(spelling)
                }
                hyperion_kotlin::KotlinType::Unknown | hyperion_kotlin::KotlinType::Star => None,
                other => Some(other.to_string()),
            }
        }
        argument_spelling(argument).unwrap_or_default()
    }

    /// One type argument a typealias's expansion gives its class, from the
    /// alias's Kotlin spelling of it (`kotlin.String`, `out Number`, `*`).
    fn alias_type_argument(spelling: &str) -> hyperion_kotlin::TypeArgument {
        let spelling = spelling.trim();
        if spelling == "*" {
            return hyperion_kotlin::TypeArgument::Star;
        }
        let parse = |written: &str| {
            Self::substitute_kotlin_type_string(written.trim(), &std::collections::HashMap::new())
        };
        for (prefix, variance) in [
            ("out ", hyperion_kotlin::Variance::Covariant),
            ("in ", hyperion_kotlin::Variance::Contravariant),
        ] {
            if let Some(bound) = spelling.strip_prefix(prefix) {
                return parse(bound).map_or(hyperion_kotlin::TypeArgument::Star, |bound| {
                    hyperion_kotlin::TypeArgument::Projected { variance, bound }
                });
            }
        }
        parse(spelling).map_or(
            hyperion_kotlin::TypeArgument::Star,
            hyperion_kotlin::TypeArgument::Type,
        )
    }

    /// Substitute type variables in a type string with concrete types
    fn substitute_kotlin_type_string(
        type_str: &str,
        substitutions: &std::collections::HashMap<String, hyperion_kotlin::KotlinType>,
    ) -> Option<hyperion_kotlin::KotlinType> {
        let type_str = type_str.trim();

        // Check for nullable type
        if let Some(inner) = type_str.strip_suffix('?') {
            return Self::substitute_kotlin_type_string(inner, substitutions)
                .map(hyperion_kotlin::KotlinType::nullable);
        }

        // Check for direct type variable substitution (e.g., "T" -> "User")
        if let Some(concrete) = substitutions.get(type_str) {
            return Some(concrete.clone());
        }

        // Check for generic type like List<T>
        if let Some(lt_pos) = type_str.find('<') {
            let base_name = &type_str[..lt_pos];
            let args_str = &type_str[lt_pos + 1..type_str.len() - 1]; // Remove < and >

            // Split the arguments at the commas of this list, not at those inside a nested argument.
            let type_args: Vec<hyperion_kotlin::KotlinType> = split_top_level_commas(args_str)
                .into_iter()
                .filter_map(|arg| Self::substitute_kotlin_type_string(arg.trim(), substitutions))
                .collect();

            return Some(hyperion_kotlin::KotlinType::generic(base_name, type_args));
        }

        // Return as-is (concrete type name)
        Some(hyperion_kotlin::KotlinType::simple(type_str))
    }

    /// Hover implementation for Kotlin files
    pub fn hover_kotlin(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        debug!("HOVER: Kotlin hover for {:?} at {:?}", file_id, position);

        // Parse the Kotlin file. try_lock_for: degrade to no hover rather
        // than freeze if did_change is currently holding the DB mutex.
        let parse_tree = {
            let snapshot = self.txn.snapshot();
            snapshot.parse_file(file_id)
        };

        // Convert position to offset
        let offset = position_to_offset(source_text, position)?;

        // Check if offset is within u32 bounds (parser limitation)
        let offset_u32 = if let Ok(o) = u32::try_from(offset) {
            o
        } else {
            tracing::warn!(
                "Kotlin hover offset {} exceeds u32::MAX, file too large",
                offset
            );
            return Ok(None);
        };

        // Create Kotlin SyntaxNode from the green tree
        let kotlin_root: hyperion_kotlin::SyntaxNode =
            hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

        // Check if cursor is in comment or string - return None early
        if crate::navigation::ast_queries::is_in_comment_or_string_kotlin(&kotlin_root, offset_u32)
        {
            debug!("HOVER: Cursor in comment/string, skipping Kotlin hover");
            return Ok(None);
        }

        // Find token at position
        use rowan::TextSize;
        let token_offset = kotlin_root.token_at_offset(TextSize::from(offset_u32));

        use hyperion_kotlin::SyntaxKind as KtKind;

        // Try right-biased first, fallback to left-biased
        let token = match token_offset.clone().right_biased() {
            Some(t) if is_kotlin_identifier_like(t.kind()) => t,
            _ => {
                // Right token is not an identifier-like, try left
                token_offset
                    .clone()
                    .left_biased()
                    .filter(|t| is_kotlin_identifier_like(t.kind()))
                    .or_else(|| token_offset.right_biased()) // fallback to right if no identifier found
                    .ok_or("No token at position")?
            }
        };

        debug!(
            "HOVER: Kotlin token: {:?} (kind: {:?})",
            token.text(),
            token.kind()
        );

        // Handle declaration keywords (class, object, fun, etc.) by finding the parent declaration
        if token.kind() == KtKind::OBJECT_KW {
            // Find the OBJECT_DECL parent
            let mut current = token.parent();
            while let Some(node) = current {
                if node.kind() == KtKind::OBJECT_DECL {
                    if let Some(hover_info) = self.extract_kotlin_object_hover(&node) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                    break;
                }
                current = node.parent();
            }
        }

        // `this` is not an identifier token, but inside a scope function
        // (`with(sb) { ... }`, `apply`, `run`, a DSL builder) it has a concrete
        // receiver type that only the inference engine knows.
        if token.kind() == KtKind::THIS_KW {
            if let Some(receiver_type) = self.infer_token_type(file_id, &token) {
                return Ok(Some(self.create_hover(format!(
                    "```kotlin\nthis: {}\n```\n\n**Receiver**",
                    receiver_type.format()
                ))));
            }
        }

        // Only handle identifiers (including soft keywords used as identifiers) beyond this point
        if !is_kotlin_identifier_like(token.kind()) {
            debug!("HOVER: Not an identifier-like token, skipping");
            return Ok(None);
        }

        let identifier = token.text().to_string();

        // Check if this is a declaration name FIRST (before scope resolution)
        // This ensures hovering on "class User" shows "class User", not just "User" from scope
        if let Some(parent) = token.parent() {
            let parent_kind = parent.kind();

            match parent_kind {
                KtKind::CLASS_DECL => {
                    if let Some(hover_info) = self.extract_kotlin_class_hover(&parent) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                }
                KtKind::FUNCTION_DECL => {
                    if let Some(hover_info) = self.extract_kotlin_function_hover(&parent) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                }
                KtKind::PROPERTY_DECL => {
                    if let Some(hover_info) = self.extract_kotlin_property_hover(&parent, file_id) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                }
                KtKind::OBJECT_DECL => {
                    if let Some(hover_info) = self.extract_kotlin_object_hover(&parent) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                }
                KtKind::INTERFACE_DECL => {
                    if let Some(hover_info) = self.extract_kotlin_interface_hover(&parent) {
                        return Ok(Some(self.create_hover(hover_info)));
                    }
                }
                _ => {}
            }
        }

        // Build scope graph for local symbol resolution
        let scope_graph = hyperion_kotlin::build_scope_graph(&kotlin_root);

        // Try to resolve in local scope (variables, parameters, local functions)
        let resolved_symbol = scope_graph.resolve_symbol(&identifier, offset);
        if let Some(symbol) = resolved_symbol {
            debug!("HOVER: Found in local scope: {:?}", symbol);

            // Format hover based on symbol kind
            let hover_text = match symbol.kind {
                hyperion_kotlin::ScopeSymbolKind::Variable => {
                    // Try to determine the type to display (using KotlinType)
                    let display_type: Option<hyperion_kotlin::KotlinType> =
                        if let Some(method_info) = &symbol.method_call_info {
                            // This variable is assigned from a method call - try to resolve return type
                            let resolved = hyperion_kotlin::resolve_method_return_type(
                                method_info,
                                &scope_graph,
                                offset,
                            );

                            // Check if we got a valid type (not Unknown)
                            let resolved_type = resolved.as_ref().and_then(|t| {
                                if matches!(t, hyperion_kotlin::type_extractor::Type::Unknown) {
                                    None
                                } else {
                                    let kt = hyperion_kotlin::KotlinType::from(t.clone());
                                    Some(kt)
                                }
                            });

                            // If stdlib resolution failed, try user-defined generic registry
                            let user_defined_type = if resolved_type.is_none() {
                                // Get FULL receiver type (with type arguments) for generic substitution
                                let full_receiver_type: Option<hyperion_kotlin::KotlinType> =
                                    if let Some(ref recv_type) = method_info.receiver_type {
                                        // Use pre-resolved receiver type (convert from old Type to KotlinType)
                                        let kt_type =
                                            hyperion_kotlin::KotlinType::from(recv_type.clone());
                                        Some(kt_type)
                                    } else if let Some(ref recv_name) = method_info.receiver_name {
                                        // Look up receiver variable's type in scope
                                        let recv_symbol =
                                            scope_graph.resolve_symbol(recv_name, offset);
                                        recv_symbol.and_then(|s| s.type_annotation.clone())
                                    } else {
                                        None
                                    };

                                if let Some(ref recv_type) = full_receiver_type {
                                    // Try user-defined generic inference (substitutes type variables)
                                    self.infer_kotlin_method_return_type_with_generics(
                                        file_id,
                                        recv_type,
                                        &method_info.method_name,
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            // If user registry failed, try simple workspace lookup
                            let workspace_type = if user_defined_type.is_none()
                                && resolved_type.is_none()
                            {
                                // Get receiver type name for workspace lookup (fallback)
                                let receiver_type_name: Option<String> =
                                    if let Some(ref recv_type) = method_info.receiver_type {
                                        Some(recv_type.simple_name().to_string())
                                    } else if let Some(ref recv_name) = method_info.receiver_name {
                                        scope_graph
                                            .resolve_symbol(recv_name, offset)
                                            .and_then(|s| s.type_annotation.as_ref())
                                            .map(|t| t.simple_name().to_string())
                                    } else {
                                        None
                                    };

                                if let Some(type_name) = receiver_type_name {
                                    self.resolve_kotlin_method_from_workspace(
                                        &type_name,
                                        &method_info.method_name,
                                        &scope_graph,
                                    )
                                    .map(hyperion_kotlin::KotlinType::simple)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            // Use resolved type, user-defined generic type, workspace type, or fall back to annotation
                            resolved_type
                                .or(user_defined_type)
                                .or(workspace_type)
                                .or_else(|| symbol.type_annotation.clone())
                        } else {
                            // Normal variable with explicit or inferred type
                            symbol.type_annotation.clone()
                        };

                    // Check for smart cast narrowing (e.g., x is String)
                    let final_type = if let Some(smart_cast) =
                        scope_graph.resolve_smart_cast(&symbol.name, offset)
                    {
                        // Convert smart cast Type to KotlinType
                        let narrowed =
                            hyperion_kotlin::KotlinType::from(smart_cast.narrowed_type.clone());
                        Some(narrowed)
                    } else {
                        // At a *use* site the scope graph reports the type it
                        // derived at the declaration; for `when`/`try`/`if`
                        // initializers that type is a branch condition, so the
                        // engine's LUB takes precedence. Otherwise the engine
                        // only fills gaps.
                        self.infer_lub_type_of_declaration(
                            file_id,
                            &kotlin_root,
                            symbol.definition_range,
                        )
                        .or(display_type)
                        .or_else(|| self.infer_token_type(file_id, &token))
                    };

                    if let Some(type_ann) = final_type {
                        format!("```kotlin\nval {}: {}\n```", symbol.name, type_ann.format())
                    } else {
                        format!("```kotlin\nval {}\n```", symbol.name)
                    }
                }
                hyperion_kotlin::ScopeSymbolKind::Parameter => {
                    // Check for smart cast narrowing on parameters
                    let param_type = if let Some(smart_cast) =
                        scope_graph.resolve_smart_cast(&symbol.name, offset)
                    {
                        let narrowed =
                            hyperion_kotlin::KotlinType::from(smart_cast.narrowed_type.clone());
                        Some(narrowed)
                    } else {
                        symbol
                            .type_annotation
                            .clone()
                            .or_else(|| self.infer_token_type(file_id, &token))
                    };

                    if let Some(ref type_ann) = param_type {
                        format!(
                            "```kotlin\n{}: {}\n```\n\n**Parameter**",
                            symbol.name,
                            type_ann.format()
                        )
                    } else {
                        format!("```kotlin\n{}\n```\n\n**Parameter**", symbol.name)
                    }
                }
                hyperion_kotlin::ScopeSymbolKind::Function => {
                    format!("```kotlin\nfun {}\n```\n\n**Local function**", symbol.name)
                }
                hyperion_kotlin::ScopeSymbolKind::LambdaParameter => {
                    // Check for smart cast narrowing on lambda parameters
                    let lambda_type = if let Some(smart_cast) =
                        scope_graph.resolve_smart_cast(&symbol.name, offset)
                    {
                        let narrowed =
                            hyperion_kotlin::KotlinType::from(smart_cast.narrowed_type.clone());
                        Some(narrowed)
                    } else {
                        symbol
                            .type_annotation
                            .clone()
                            .or_else(|| self.infer_token_type(file_id, &token))
                    };

                    if let Some(ref type_ann) = lambda_type {
                        format!(
                            "```kotlin\n{}: {}\n```\n\n**Lambda parameter**",
                            symbol.name,
                            type_ann.format()
                        )
                    } else {
                        format!("```kotlin\n{}\n```\n\n**Lambda parameter**", symbol.name)
                    }
                }
                hyperion_kotlin::ScopeSymbolKind::LoopVariable => {
                    // The scope graph never records a type for loop variables;
                    // the engine can often derive it from the iterated sequence.
                    self.infer_token_type(file_id, &token).map_or_else(
                        || format!("```kotlin\n{}\n```\n\n**Loop variable**", symbol.name),
                        |ty| {
                            format!(
                                "```kotlin\n{}: {}\n```\n\n**Loop variable**",
                                symbol.name,
                                ty.format()
                            )
                        },
                    )
                }
                _ => {
                    format!("```kotlin\n{}\n```", symbol.name)
                }
            };

            return Ok(Some(self.create_hover(hover_text)));
        }

        // Workspace declarations use the same exact, revision-bound identity
        // as definition, references, rename, and call hierarchy. In
        // particular, an unresolved member must not borrow a globally unique
        // declaration with the same name.
        if let Some(path) = self.workspace_index.get_file_path(file_id) {
            if let Ok(uri) = Url::from_file_path(path) {
                if let Some(target) = self
                    .navigation_provider
                    .resolve_workspace_target_at_position(
                        file_id,
                        position,
                        source_text,
                        &uri,
                        None,
                    )?
                {
                    if let Some(hover_info) =
                        self.format_kotlin_symbol_hover(&target.definition.location)
                    {
                        if self
                            .navigation_provider
                            .workspace_target_is_current(&target)
                        {
                            return Ok(Some(self.create_hover(hover_info)));
                        }
                        return Ok(None);
                    }
                }
            }
        }

        // Nothing in the scope graph or the workspace index: last chance is the
        // inference engine, which knows about implicit `it`, lambda receivers
        // and members reached through them.
        if let Some(inferred) = self.infer_token_type(file_id, &token) {
            debug!("HOVER: Typed '{}' via the inference engine", identifier);
            return Ok(Some(self.create_hover(format!(
                "```kotlin\n{}: {}\n```",
                identifier,
                inferred.format()
            ))));
        }

        debug!("HOVER: No hover info found for '{}'", identifier);
        Ok(None)
    }

    /// Extract modifiers from a class declaration at a given location
    fn extract_modifiers_from_location(&self, file_contents: &str, offset: u32) -> String {
        use hyperion_kotlin::SyntaxKind as KtKind;

        // Parse the file and find the node at the offset
        let parse_tree = hyperion_kotlin::parse(file_contents);
        let syntax_node = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green_node);

        // Find the token at the offset
        let token = syntax_node
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased();
        if let Some(token) = token {
            if let Some(parent) = token.parent() {
                if parent.kind() == KtKind::CLASS_DECL {
                    // Look for MODIFIERS sibling
                    if let Some(grandparent) = parent.parent() {
                        for child in grandparent.children() {
                            if child.kind() == KtKind::MODIFIERS {
                                let mut mods = Vec::new();
                                for tok in child.children_with_tokens() {
                                    if let rowan::NodeOrToken::Token(t) = tok {
                                        match t.kind() {
                                            KtKind::DATA_KW => mods.push("data"),
                                            KtKind::SEALED_KW => mods.push("sealed"),
                                            KtKind::OPEN_KW => mods.push("open"),
                                            KtKind::ABSTRACT_KW => mods.push("abstract"),
                                            _ => {}
                                        }
                                    }
                                }
                                if !mods.is_empty() {
                                    return format!("{} ", mods.join(" "));
                                }
                            }
                        }
                    }
                }
            }
        }

        String::new()
    }

    /// Extract hover information from a Kotlin class declaration
    fn extract_kotlin_class_hover(&self, node: &hyperion_kotlin::SyntaxNode) -> Option<String> {
        use hyperion_kotlin::SyntaxKind as KtKind;

        let mut modifiers = Vec::new();
        let mut class_name = None;

        // First, check for modifiers in previous sibling (MODIFIERS node)
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if sibling.kind() == KtKind::MODIFIERS {
                    // Extract modifiers from MODIFIERS node
                    for child in sibling.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = child {
                            match token.kind() {
                                // Visibility modifiers
                                hyperion_kotlin::SyntaxKind::PUBLIC_KW => modifiers.push("public"),
                                hyperion_kotlin::SyntaxKind::PRIVATE_KW => {
                                    modifiers.push("private");
                                }
                                hyperion_kotlin::SyntaxKind::PROTECTED_KW => {
                                    modifiers.push("protected");
                                }
                                hyperion_kotlin::SyntaxKind::INTERNAL_KW => {
                                    modifiers.push("internal");
                                }
                                // Class modifiers
                                hyperion_kotlin::SyntaxKind::DATA_KW => modifiers.push("data"),
                                hyperion_kotlin::SyntaxKind::SEALED_KW => modifiers.push("sealed"),
                                hyperion_kotlin::SyntaxKind::OPEN_KW => modifiers.push("open"),
                                hyperion_kotlin::SyntaxKind::ABSTRACT_KW => {
                                    modifiers.push("abstract");
                                }
                                hyperion_kotlin::SyntaxKind::INLINE_KW => modifiers.push("inline"),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Extract class name from CLASS_DECL node
        for child in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind() == hyperion_kotlin::SyntaxKind::IDENT && class_name.is_none() {
                    class_name = Some(token.text().to_string());
                    break;
                }
            }
        }

        let class_name = class_name?;
        let modifier_str = if modifiers.is_empty() {
            String::new()
        } else {
            format!("{} ", modifiers.join(" "))
        };

        Some(format!("```kotlin\n{modifier_str}class {class_name}\n```"))
    }

    /// Extract hover information from a Kotlin function declaration
    fn extract_kotlin_function_hover(&self, node: &hyperion_kotlin::SyntaxNode) -> Option<String> {
        use hyperion_kotlin::SyntaxKind as KtKind;

        let mut modifiers = Vec::new();
        let mut function_name = None;
        let mut return_type = None;
        let mut past_param_list = false;

        // Extract modifiers from previous sibling (MODIFIERS node)
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if sibling.kind() == KtKind::MODIFIERS {
                    for child in sibling.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = child {
                            match token.kind() {
                                // Visibility modifiers
                                hyperion_kotlin::SyntaxKind::PUBLIC_KW => modifiers.push("public"),
                                hyperion_kotlin::SyntaxKind::PRIVATE_KW => {
                                    modifiers.push("private");
                                }
                                hyperion_kotlin::SyntaxKind::PROTECTED_KW => {
                                    modifiers.push("protected");
                                }
                                hyperion_kotlin::SyntaxKind::INTERNAL_KW => {
                                    modifiers.push("internal");
                                }
                                // Inheritance modifiers
                                hyperion_kotlin::SyntaxKind::OVERRIDE_KW => {
                                    modifiers.push("override");
                                }
                                hyperion_kotlin::SyntaxKind::OPEN_KW => modifiers.push("open"),
                                hyperion_kotlin::SyntaxKind::ABSTRACT_KW => {
                                    modifiers.push("abstract");
                                }
                                // Function modifiers
                                hyperion_kotlin::SyntaxKind::SUSPEND_KW => {
                                    modifiers.push("suspend");
                                }
                                hyperion_kotlin::SyntaxKind::INLINE_KW => modifiers.push("inline"),
                                hyperion_kotlin::SyntaxKind::NOINLINE_KW => {
                                    modifiers.push("noinline");
                                }
                                hyperion_kotlin::SyntaxKind::CROSSINLINE_KW => {
                                    modifiers.push("crossinline");
                                }
                                hyperion_kotlin::SyntaxKind::INFIX_KW => modifiers.push("infix"),
                                hyperion_kotlin::SyntaxKind::OPERATOR_KW => {
                                    modifiers.push("operator");
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == KtKind::IDENT && function_name.is_none() {
                        function_name = Some(token.text().to_string());
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if n.kind() == KtKind::PARAMETER_LIST {
                        past_param_list = true;
                    } else if past_param_list
                        && (n.kind() == KtKind::TYPE_REF || n.kind() == KtKind::NULLABLE_TYPE)
                    {
                        return_type = Some(hyperion_kotlin::TypeExtractor::extract(&n).format());
                    }
                }
            }
        }

        let function_name = function_name?;
        let modifier_str = if modifiers.is_empty() {
            String::new()
        } else {
            format!("{} ", modifiers.join(" "))
        };

        let return_type_str = return_type.map(|t| format!(": {t}")).unwrap_or_default();

        Some(format!(
            "```kotlin\n{modifier_str}fun {function_name}(){return_type_str}\n```"
        ))
    }

    /// Extract method call info directly from a CALL_EXPR node in a property initializer
    fn extract_method_call_from_property_initializer(
        &self,
        node: &hyperion_kotlin::SyntaxNode,
    ) -> Option<(String, String)> {
        // (receiver_name, method_name)
        use hyperion_kotlin::SyntaxKind as KtKind;

        // BUG FIX: Converted debug eprintln to trace! (only visible with RUST_LOG=trace)
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => trace!("  child node: {:?}", n.kind()),
                rowan::NodeOrToken::Token(t) => {
                    trace!("  child token: {:?} = '{}'", t.kind(), t.text());
                }
            }
        }

        // The property structure in Kotlin is:
        // PROPERTY_DECL
        //   VAL_KW
        //   IDENT (property name)
        //   EQ
        //   ... initializer expression ...
        //
        // We need to look at the expression after EQ

        let mut found_eq = false;
        let mut receiver_name = None;
        let mut method_name = None;

        // Scan source-ordered descendant tokens to find the method call pattern:
        // IDENT DOT IDENT L_PAREN. Initializers used to be emitted as direct
        // PROPERTY_DECL tokens, but the expression parser now nests them under
        // MEMBER_ACCESS_EXPR/CALL_EXPR nodes.
        let mut tokens: Vec<(KtKind, String)> = Vec::new();
        for token in node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
        {
            let kind = token.kind();
            if kind == KtKind::EQ {
                found_eq = true;
                continue;
            }
            if found_eq && !kind.is_trivia() {
                tokens.push((kind, token.text().to_string()));
            }
        }

        // Look for pattern: IDENT DOT IDENT L_PAREN
        for i in 0..tokens.len() {
            if tokens.len() > i + 3
                && tokens[i].0 == KtKind::IDENT
                && tokens[i + 1].0 == KtKind::DOT
                && tokens[i + 2].0 == KtKind::IDENT
                && tokens[i + 3].0 == KtKind::L_PAREN
            {
                receiver_name = Some(tokens[i].1.clone());
                method_name = Some(tokens[i + 2].1.clone());
                break;
            }
        }

        if let (Some(recv), Some(method)) = (receiver_name, method_name) {
            return Some((recv, method));
        }

        None
    }

    /// Extract hover information from a Kotlin property declaration
    fn extract_kotlin_property_hover(
        &self,
        node: &hyperion_kotlin::SyntaxNode,
        file_id: FileId,
    ) -> Option<String> {
        let mut is_val = false;
        let mut is_var = false;
        let mut property_name = None;
        let mut property_offset: Option<u32> = None;

        // Recursively search for val/var keywords
        fn find_val_var(node: &hyperion_kotlin::SyntaxNode, is_val: &mut bool, is_var: &mut bool) {
            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(token) => match token.kind() {
                        hyperion_kotlin::SyntaxKind::VAL_KW => *is_val = true,
                        hyperion_kotlin::SyntaxKind::VAR_KW => *is_var = true,
                        _ => {}
                    },
                    rowan::NodeOrToken::Node(n) => {
                        find_val_var(&n, is_val, is_var);
                    }
                }
            }
        }

        find_val_var(node, &mut is_val, &mut is_var);

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    // Accept soft keywords as property names (e.g., val data: Int)
                    if is_kotlin_identifier_like(token.kind()) && property_name.is_none() {
                        property_name = Some(token.text().to_string());
                        property_offset = Some(u32::from(token.text_range().start()));
                    }
                }
                rowan::NodeOrToken::Node(_) => {}
            }
        }

        let property_name = property_name?;
        let val_or_var = if is_val {
            "val"
        } else if is_var {
            "var"
        } else {
            "val"
        };

        // Highest precedence for un-annotated `when`/`try`/`if` initializers:
        // the token-stream heuristic below reports a branch condition instead of
        // the branch result there, so the inference engine's LUB wins outright.
        if let Some(inferred) = self.infer_lub_initializer_type(file_id, node) {
            return Some(format!(
                "```kotlin\n{} {}: {}\n```",
                val_or_var,
                property_name,
                inferred.format()
            ));
        }

        // First try: Detect method call directly from AST and use user-defined generic inference
        // This handles cases like: val user = repo.findById(1) where repo: Repository<User>
        if let Some((receiver_name, method_name)) =
            self.extract_method_call_from_property_initializer(node)
        {
            // Find root and build scope graph to look up receiver type
            if let Some(offset) = property_offset {
                let mut current = Some(node.clone());
                while let Some(n) = current {
                    if n.parent().is_none() {
                        let scope_graph = hyperion_kotlin::build_scope_graph(&n);

                        // Look up the receiver's type in scope
                        if let Some(recv_symbol) =
                            scope_graph.resolve_symbol(&receiver_name, offset as usize)
                        {
                            if let Some(ref recv_type) = recv_symbol.type_annotation {
                                // Try user-defined generic inference
                                if let Some(inferred) = self
                                    .infer_kotlin_method_return_type_with_generics(
                                        file_id,
                                        recv_type,
                                        &method_name,
                                    )
                                {
                                    return Some(format!(
                                        "```kotlin\n{} {}: {}\n```",
                                        val_or_var,
                                        property_name,
                                        inferred.format()
                                    ));
                                }
                            }
                        }
                        break;
                    }
                    current = n.parent();
                }
            }
        }

        // Second try: Use scope graph's method_call_info (for cases where scope builder detected it)
        if let Some(offset) = property_offset {
            let mut current = Some(node.clone());
            while let Some(n) = current {
                if n.parent().is_none() {
                    let scope_graph = hyperion_kotlin::build_scope_graph(&n);
                    if let Some(symbol) =
                        scope_graph.resolve_symbol(&property_name, offset as usize)
                    {
                        if let Some(method_info) = &symbol.method_call_info {
                            // Get FULL receiver type (with type arguments) for generic substitution
                            let full_receiver_type: Option<hyperion_kotlin::KotlinType> =
                                if let Some(ref recv_type) = method_info.receiver_type {
                                    Some(hyperion_kotlin::KotlinType::from(recv_type.clone()))
                                } else if let Some(ref recv_name) = method_info.receiver_name {
                                    scope_graph
                                        .resolve_symbol(recv_name, offset as usize)
                                        .and_then(|s| s.type_annotation.clone())
                                } else {
                                    None
                                };

                            // Try user-defined generic inference first
                            if let Some(ref recv_type) = full_receiver_type {
                                if let Some(inferred) = self
                                    .infer_kotlin_method_return_type_with_generics(
                                        file_id,
                                        recv_type,
                                        &method_info.method_name,
                                    )
                                {
                                    return Some(format!(
                                        "```kotlin\n{} {}: {}\n```",
                                        val_or_var,
                                        property_name,
                                        inferred.format()
                                    ));
                                }
                            }

                            // Fall back to stdlib resolution
                            if let Some(resolved_type) = hyperion_kotlin::resolve_method_return_type(
                                method_info,
                                &scope_graph,
                                offset as usize,
                            ) {
                                // Skip Unknown types
                                if !matches!(
                                    resolved_type,
                                    hyperion_kotlin::type_extractor::Type::Unknown
                                ) {
                                    let kotlin_type =
                                        hyperion_kotlin::KotlinType::from(resolved_type);
                                    return Some(format!(
                                        "```kotlin\n{} {}: {}\n```",
                                        val_or_var,
                                        property_name,
                                        kotlin_type.format()
                                    ));
                                }
                            }
                        }
                    }
                    break;
                }
                current = n.parent();
            }
        }

        // Second try: Use TypeExtractor for explicit types and simple inferred types (literals, constructors)
        if let Some(inferred_type) = hyperion_kotlin::TypeExtractor::infer_val_type(node) {
            return Some(format!(
                "```kotlin\n{} {}: {}\n```",
                val_or_var,
                property_name,
                inferred_type.format()
            ));
        }

        // Last resort before giving up on a type: ask the inference engine
        // about the initializer expression. This is the gap-filler for forms the
        // scope-graph/TypeExtractor path does not model at all.
        if let Some(initializer) = property_initializer_node(node) {
            if let Some(inferred) = self.infer_node_type(file_id, &initializer) {
                return Some(format!(
                    "```kotlin\n{} {}: {}\n```",
                    val_or_var,
                    property_name,
                    inferred.format()
                ));
            }
        }

        // Fallback: No type information available
        Some(format!("```kotlin\n{val_or_var} {property_name}\n```"))
    }

    /// Extract hover information from a Kotlin object declaration
    fn extract_kotlin_object_hover(&self, node: &hyperion_kotlin::SyntaxNode) -> Option<String> {
        use hyperion_kotlin::SyntaxKind as KtKind;

        let mut is_companion = false;
        let mut object_name = None;

        // Check for companion modifier in MODIFIERS sibling
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if sibling.kind() == KtKind::MODIFIERS {
                    // Check for COMPANION_KW in modifiers
                    for child in sibling.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = child {
                            if token.kind() == hyperion_kotlin::SyntaxKind::COMPANION_KW {
                                is_companion = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Extract object name from OBJECT_DECL node
        for child in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind() == hyperion_kotlin::SyntaxKind::IDENT && object_name.is_none() {
                    object_name = Some(token.text().to_string());
                    break;
                }
            }
        }

        if is_companion {
            let name_str = object_name.map(|n| format!(" {n}")).unwrap_or_default();
            Some(format!("```kotlin\ncompanion object{name_str}\n```"))
        } else {
            // Regular object declaration - must have a name
            let object_name = object_name?;
            Some(format!("```kotlin\nobject {object_name}\n```"))
        }
    }

    /// Extract hover information from a Kotlin interface declaration
    fn extract_kotlin_interface_hover(&self, node: &hyperion_kotlin::SyntaxNode) -> Option<String> {
        let mut interface_name = None;

        // Extract interface name recursively
        fn extract_name(node: &hyperion_kotlin::SyntaxNode, interface_name: &mut Option<String>) {
            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(token) => {
                        if token.kind() == hyperion_kotlin::SyntaxKind::IDENT
                            && interface_name.is_none()
                        {
                            *interface_name = Some(token.text().to_string());
                        }
                    }
                    rowan::NodeOrToken::Node(n) => {
                        extract_name(&n, interface_name);
                    }
                }
            }
        }

        extract_name(node, &mut interface_name);

        let interface_name = interface_name?;
        Some(format!("```kotlin\ninterface {interface_name}\n```"))
    }

    /// Format hover information from a workspace SymbolLocation
    fn format_kotlin_symbol_hover(
        &self,
        symbol: &hyperion_workspace::SymbolLocation,
    ) -> Option<String> {
        use tower_lsp::lsp_types::SymbolKind;

        let mut result = String::new();

        match symbol.kind {
            SymbolKind::CLASS => {
                // Try to get modifiers from the symbol's source
                let modifiers_str = if let Some(validated) = self
                    .workspace_index
                    .get_validated_file_contents(symbol.file_id)
                {
                    self.extract_modifiers_from_location(&validated.content, symbol.range.start)
                } else {
                    String::new()
                };
                result.push_str(&format!(
                    "```kotlin\n{}class {}\n```\n\n",
                    modifiers_str, symbol.name
                ));
            }
            SymbolKind::FUNCTION | SymbolKind::METHOD => {
                result.push_str(&format!("```kotlin\nfun {}\n```\n\n", symbol.name));
            }
            SymbolKind::PROPERTY | SymbolKind::FIELD => {
                result.push_str(&format!("```kotlin\nval {}\n```\n\n", symbol.name));
            }
            SymbolKind::OBJECT => {
                result.push_str(&format!("```kotlin\nobject {}\n```\n\n", symbol.name));
            }
            SymbolKind::INTERFACE => {
                result.push_str(&format!("```kotlin\ninterface {}\n```\n\n", symbol.name));
            }
            _ => {
                result.push_str(&format!("```kotlin\n{}\n```\n\n", symbol.name));
            }
        }

        // Add kind info with user-friendly labels
        let kind_label = match symbol.kind {
            lsp_types::SymbolKind::CLASS => "Class",
            lsp_types::SymbolKind::INTERFACE => "Interface",
            lsp_types::SymbolKind::ENUM => "Enum",
            lsp_types::SymbolKind::FUNCTION => "Function",
            lsp_types::SymbolKind::METHOD => "Method",
            lsp_types::SymbolKind::PROPERTY => "Property",
            lsp_types::SymbolKind::FIELD => "Field",
            lsp_types::SymbolKind::VARIABLE => "Variable",
            lsp_types::SymbolKind::CONSTANT => "Constant",
            lsp_types::SymbolKind::CONSTRUCTOR => "Constructor",
            lsp_types::SymbolKind::OBJECT => "Object",
            lsp_types::SymbolKind::TYPE_PARAMETER => "Type parameter",
            _ => "Symbol",
        };
        result.push_str(&format!("**{kind_label}**"));

        // Add container info if available
        if let Some(container) = &symbol.container_name {
            result.push_str(&format!(" in `{container}`"));
        }

        // Add documentation if available
        if let Some(doc) = &symbol.documentation {
            result.push_str(&format!("\n\n{doc}"));
        }

        // Add location info
        if let Some(file_path) = self.workspace_index.get_file_path(symbol.file_id) {
            let path_str = file_path.to_string_lossy();
            if let Some(validated) = self
                .workspace_index
                .get_validated_file_contents(symbol.file_id)
            {
                let offset: usize = symbol.range.start as usize;
                let position = offset_to_position(&validated.content, offset);
                let line_number = position.line + 1;
                result.push_str(&format!("\n\nDefined in `{path_str}:{line_number}`"));
            }
        }

        Some(result)
    }

    /// Resolve a Kotlin method return type using the workspace index
    ///
    /// This is used as a fallback when the stdlib-based type resolution doesn't find a match.
    /// It looks up the method in workspace-indexed classes (including JDK and user code).
    fn resolve_kotlin_method_from_workspace(
        &self,
        receiver_type_name: &str,
        method_name: &str,
        scope_graph: &hyperion_kotlin::ScopeGraph,
    ) -> Option<String> {
        // Build FQN candidates from imports
        let mut fqn_candidates = Vec::new();

        // Check explicit imports first
        if let Some(fqn) = scope_graph.resolve_import(receiver_type_name) {
            fqn_candidates.push(fqn.to_string());
        }

        // Check wildcard imports
        for wildcard_pkg in scope_graph.wildcard_imports() {
            fqn_candidates.push(format!("{wildcard_pkg}.{receiver_type_name}"));
        }

        // Add kotlin.* implicit imports for common types
        let kotlin_stdlib = [
            "kotlin",
            "kotlin.collections",
            "kotlin.io",
            "kotlin.text",
            "kotlin.ranges",
            "kotlin.sequences",
            "kotlin.comparisons",
        ];
        for pkg in kotlin_stdlib {
            fqn_candidates.push(format!("{pkg}.{receiver_type_name}"));
        }

        // Try java.lang for common types
        fqn_candidates.push(format!("java.lang.{receiver_type_name}"));

        // Also try the type name as-is (might already be FQN)
        if receiver_type_name.contains('.') {
            fqn_candidates.push(receiver_type_name.to_string());
        }

        debug!(
            "HOVER: Resolving method {}.{} with {} FQN candidates",
            receiver_type_name,
            method_name,
            fqn_candidates.len()
        );

        fqn_candidates.sort();
        fqn_candidates.dedup();
        let mut return_types = Vec::new();
        for fqn in fqn_candidates {
            let methods = self.workspace_index.find_methods_by_name(&fqn, method_name);
            for return_type in methods
                .into_iter()
                .filter_map(|method| method.return_type.clone())
            {
                debug!(
                    "HOVER: Found method {}.{}() -> {} via workspace",
                    fqn, method_name, return_type
                );
                return_types.push(return_type.to_string());
            }
        }
        return_types.sort();
        return_types.dedup();
        let [return_type] = return_types.as_slice() else {
            return None;
        };
        Some(return_type.clone())
    }

    fn resolve_class_symbol_by_name(
        &self,
        file_id: hyperion_semantics::db::FileId,
        class_name: &str,
    ) -> Option<Arc<hyperion_workspace::Symbol>> {
        if let Some(sym) = self.workspace_index.get_symbol(class_name) {
            if hyperion_workspace::is_type_declaration_kind(sym.kind) {
                return Some(sym);
            }
        }

        let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
            .workspace_index
            .resolve_type_name(file_id, &[], class_name)
        else {
            return None;
        };
        self.workspace_index
            .get_symbol(&fqn)
            .filter(|symbol| hyperion_workspace::is_type_declaration_kind(symbol.kind))
    }
}
