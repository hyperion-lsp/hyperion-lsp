// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Signature Help Provider
//!
//! Provides signature help (parameter hints) when typing method calls.
//! Shows method signatures with parameter names and types as the user types.

use crate::semantic_host::ReadTxn;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, ParseDatabase};
use hyperion_semantics::inference::TypeInferenceEngine;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::Type;
use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};
use tracing::debug;

use std::sync::LazyLock;

use crate::navigation_util::position_to_offset;

static PACKAGE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"package\s+([\w.]+)\s*;").expect("valid package regex"));
static CLASS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"class\s+(\w+)").expect("valid class regex"));

/// Provides signature help for method calls
pub struct SignatureHelpProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use hyperion_semantics::db::{Database, Language, SourceDatabase};
    use tempfile::tempdir;

    #[test]
    fn signature_candidates_render_concrete_receiver_projection_with_names() {
        let temp = tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Text {} class Base<T> { ",
                    "T convert(T value, T... rest) { return value; } } ",
                    "class Child<U> extends Base<U> {}"
                ),
            )
            .expect("generic signature fixture should index");
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = SignatureHelpProvider::new(&txn, Arc::clone(&index));

        let candidates =
            provider.get_method_signature_candidates("p.Child", &["p.Text".to_string()], "convert");
        let [(_, signature)] = candidates.as_slice() else {
            panic!("one projected signature expected: {candidates:#?}")
        };
        assert_eq!(
            signature.label,
            "p.Text convert(p.Text value, p.Text... rest)"
        );
        assert_eq!(signature.parameters.as_ref().map(Vec::len), Some(2));

        let self_type = provider
            .infer_current_class("package p; class Child<U> { void use() { convert(); } }")
            .expect("generic self type");
        let Type::Class(self_class) = self_type else {
            panic!("generic self must be a class type")
        };
        assert_eq!(self_class.type_arguments.len(), 1);
        assert_eq!(self_class.type_arguments[0].to_string(), "U");

        let nested_source = Arc::new(
            "package p; class Outer<T> { class Inner<V> { void use() { convert(); } } }"
                .to_string(),
        );
        let nested_path = temp.path().join("p/Outer.java");
        index
            .index_file_with_source(&nested_path, nested_source.as_ref())
            .expect("nested generic fixture should index");
        let file_id = index.get_file_id(&nested_path).expect("nested file id");
        let mut db = Database::default();
        db.set_file_text_tracked(file_id, Arc::clone(&nested_source));
        db.set_file_language(file_id, Language::Java);
        let root = db.parse_file(file_id).syntax();
        let call = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .expect("nested call expression");
        let nested_receiver = provider
            .infer_enclosing_class(&call, nested_source.as_ref())
            .expect("nearest nested receiver");
        let Type::Class(nested_class) = nested_receiver else {
            panic!("nested receiver must be a class")
        };
        assert_eq!(nested_class.qualified_name(), "p.Outer.Inner");
        assert_eq!(nested_class.type_arguments.len(), 2);
        assert_eq!(nested_class.type_arguments[0].to_string(), "T");
        assert_eq!(nested_class.type_arguments[1].to_string(), "V");
    }
}

impl<'txn> SignatureHelpProvider<'txn> {
    /// Create a new SignatureHelpProvider
    pub const fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            txn,
            workspace_index,
        }
    }

    /// Get signature help at the given position
    ///
    /// # Arguments
    /// * `file_id` - The file ID
    /// * `position` - The cursor position
    /// * `source_text` - The source code
    ///
    /// # Returns
    /// SignatureHelp with signatures and active parameter, or None if not in a method call
    pub fn signature_help(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<SignatureHelp>, String> {
        debug!(
            "Signature help request at position {:?} in file {:?}",
            position, file_id
        );

        // Convert position to offset
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;

        // Reuse the incremental parse registered by didOpen/didChange. Besides
        // avoiding a full reparse per keystroke, this keeps signature help on
        // the same syntax snapshot as the other semantic features.
        let root = self.txn.parse_file(file_id).syntax();

        // Try to find constructor invocation first, then method call
        if let Some((class_name, paren_offset)) =
            self.find_constructor_context(&root, offset, source_text)
        {
            debug!(
                "Found constructor context: class={}, paren_offset={}",
                class_name, paren_offset
            );

            // Resolve class name to FQN
            use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
            let class_fqn =
                resolve_class_fqn_via_imports(&self.workspace_index, file_id, &class_name)
                    .unwrap_or_else(|_| {
                        // Try with current package
                        self.resolve_type_name(&class_name, source_text)
                    });

            debug!("Resolved constructor class FQN: {}", class_fqn);

            // Get constructor signatures
            let sigs = self.get_constructor_signatures(&class_fqn);

            // Determine active parameter
            let active_parameter = self.count_parameters(source_text, paren_offset, offset);
            let active_signature = self.select_best_signature(&sigs, active_parameter);

            debug!(
                "Found {} constructor signature(s), active parameter: {}, active signature: {}",
                sigs.len(),
                active_parameter,
                active_signature
            );

            if sigs.is_empty() {
                return Ok(None);
            }

            Ok(Some(SignatureHelp {
                signatures: sigs,
                active_signature: Some(active_signature as u32),
                active_parameter: Some(active_parameter),
            }))
        } else if let Some((method_name, receiver_type, paren_offset, call_node)) =
            self.find_method_call_context(&root, offset, source_text, file_id)
        {
            debug!(
                "Found method call context: method={}, receiver={:?}",
                method_name, receiver_type
            );

            // Resolve the receiver type to get the class FQN
            let (class_fqn, receiver_type) = if let Some(receiver_type) = receiver_type {
                let Some(type_name) = self.type_to_fqn(&receiver_type) else {
                    return Ok(None);
                };
                // Try to resolve via imports
                use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                let class_fqn =
                    resolve_class_fqn_via_imports(&self.workspace_index, file_id, &type_name)
                        .unwrap_or(type_name);
                (class_fqn, receiver_type)
            } else {
                debug!("No receiver type - might be local method call");
                return Ok(None);
            };

            debug!("Resolved class FQN: {}", class_fqn);

            // Determine active parameter
            let active_parameter = self.count_parameters(source_text, paren_offset, offset);
            let receiver_arguments = match &receiver_type {
                Type::Class(class) => class
                    .type_arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            let candidates =
                self.get_method_signature_candidates(&class_fqn, &receiver_arguments, &method_name);
            let sigs: Vec<_> = candidates
                .iter()
                .map(|(_, information)| information.clone())
                .collect();

            let symbol_table =
                ResolverBuilder::with_workspace(file_id, Arc::clone(&self.workspace_index))
                    .build_symbol_table(&root);
            let inference = ResolverBuilder::for_var_inference(
                file_id,
                symbol_table,
                Arc::clone(&self.workspace_index),
            );
            let mut argument_types = inference.infer_method_call_argument_types(&call_node);
            if active_parameter > 0 && active_parameter as usize >= argument_types.len() {
                argument_types.resize(active_parameter as usize + 1, Type::Unknown);
            }
            let active_signature = hyperion_semantics::call_resolution::resolve_call(
                &self.workspace_index,
                &class_fqn,
                Some(&receiver_type),
                &method_name,
                &argument_types,
            )
            .and_then(|resolved| {
                candidates
                    .iter()
                    .position(|(declaration, _)| declaration.fqn == resolved.declaration.fqn)
            })
            .unwrap_or_else(|| self.select_best_signature(&sigs, active_parameter));

            debug!(
                "Found {} signature(s), active parameter: {}, active signature: {}",
                sigs.len(),
                active_parameter,
                active_signature
            );

            if sigs.is_empty() {
                return Ok(None);
            }

            Ok(Some(SignatureHelp {
                signatures: sigs,
                active_signature: Some(active_signature as u32),
                active_parameter: Some(active_parameter),
            }))
        } else {
            debug!(
                "No method call or constructor found at position (offset {})",
                offset
            );
            Ok(None)
        }
    }

    /// Select the best matching signature based on parameter count
    ///
    /// Chooses the signature whose parameter count best matches the current argument index
    fn select_best_signature(
        &self,
        signatures: &[SignatureInformation],
        active_param: u32,
    ) -> usize {
        if signatures.is_empty() {
            return 0;
        }

        // Find signature that best matches current parameter count
        let mut best_idx = 0;
        let mut best_diff = usize::MAX;

        for (idx, sig) in signatures.iter().enumerate() {
            let param_count = sig.parameters.as_ref().map_or(0, std::vec::Vec::len);

            // Calculate difference between signature's param count and current position
            let diff = if active_param as usize <= param_count {
                // Still within params - prefer exact match or signature with more params
                param_count.abs_diff(active_param as usize)
            } else {
                // Beyond params - less relevant
                (active_param as usize - param_count) + 1000
            };

            if diff < best_diff {
                best_diff = diff;
                best_idx = idx;
            }
        }

        best_idx
    }

    /// Find constructor invocation context at the given offset
    ///
    /// Returns: (class_name, opening_paren_offset)
    fn find_constructor_context(
        &self,
        root: &SyntaxNode,
        offset: u32,
        _source_text: &str,
    ) -> Option<(String, usize)> {
        // Find the token at the cursor position
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;

        debug!("Token at cursor (constructor check): '{}'", token.text());

        // Walk up the tree to find NEW_EXPR node
        let mut node = token.parent()?;
        let mut depth = 0;

        loop {
            match node.kind() {
                SyntaxKind::NEW_EXPR => {
                    // Found a constructor invocation - extract details
                    return self.extract_constructor_info(&node);
                }
                _ => {
                    // Keep walking up
                    match node.parent() {
                        Some(parent) => {
                            node = parent;
                            depth += 1;
                            if depth > 20 {
                                return None;
                            }
                        }
                        None => {
                            return None;
                        }
                    }
                }
            }
        }
    }

    /// Extract class name and paren offset from a NEW_EXPR node
    ///
    /// NEW_EXPR structure: NEW_KW, CLASS_TYPE, METHOD_CALL_EXPR (with parens)
    fn extract_constructor_info(&self, node: &SyntaxNode) -> Option<(String, usize)> {
        debug!("Extracting constructor info from: '{}'", node.text());

        // Find the class name from CLASS_TYPE
        let class_name = node
            .children()
            .find(|n| n.kind() == SyntaxKind::CLASS_TYPE)?
            .text()
            .to_string()
            .trim()
            .to_string();

        debug!("Found class name: {}", class_name);

        // Find the opening parenthesis - it's in the METHOD_CALL_EXPR or ARG_LIST child
        let paren_offset = node.descendants_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(t) = child {
                if t.kind() == SyntaxKind::L_PAREN {
                    return Some(t.text_range().start().into());
                }
            }
            None
        })?;

        debug!("Constructor paren offset: {}", paren_offset);

        Some((class_name, paren_offset))
    }

    /// Find the method call context at the given offset
    ///
    /// Returns: (method_name, receiver_type, opening_paren_offset)
    fn find_method_call_context(
        &self,
        root: &SyntaxNode,
        offset: u32,
        source_text: &str,
        file_id: FileId,
    ) -> Option<(String, Option<Type>, usize, SyntaxNode)> {
        // Find the token at the cursor position
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;

        debug!("Token at cursor: '{}'", token.text());

        // Walk up the tree to find METHOD_CALL_EXPR or METHOD_INVOCATION
        let mut node = token.parent()?;

        let mut depth = 0;
        loop {
            debug!("Checking node: {:?}", node.kind());

            match node.kind() {
                SyntaxKind::METHOD_CALL_EXPR => {
                    // Found a method call - extract details
                    return self.extract_method_call_info(
                        &node,
                        root,
                        offset,
                        source_text,
                        file_id,
                    );
                }
                _ => {
                    // Keep walking up
                    match node.parent() {
                        Some(parent) => {
                            node = parent;
                            depth += 1;
                            if depth > 20 {
                                return None;
                            }
                        }
                        None => {
                            return None;
                        }
                    }
                }
            }
        }
    }

    /// Extract method name, receiver type, and paren offset from a method call node
    fn extract_method_call_info(
        &self,
        node: &SyntaxNode,
        root: &SyntaxNode,
        offset: u32,
        source_text: &str,
        file_id: FileId,
    ) -> Option<(String, Option<Type>, usize, SyntaxNode)> {
        let text = node.text().to_string();
        debug!("Extracting from method call node: '{}'", text);

        // Find the opening parenthesis - it's in the ARG_LIST child
        let arg_list = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST)?;
        let paren_offset = arg_list
            .children_with_tokens()
            .find(|child| {
                child
                    .as_token()
                    .is_some_and(|t| t.kind() == SyntaxKind::L_PAREN)
            })?
            .text_range()
            .start()
            .into();

        // METHOD_CALL_EXPR is self-contained:
        // 1) receiver.method(args) => [receiver_expr, DOT, IDENT, ARG_LIST]
        // 2) method(args)          => [NAME_EXPR, ARG_LIST]
        let found_dot = node
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::DOT);

        let mut receiver_expr = None;
        let mut receiver_text = None;
        if found_dot {
            for child in node.children_with_tokens() {
                if child.kind() == SyntaxKind::DOT {
                    break;
                }
                match child {
                    rowan::NodeOrToken::Node(n) if n.kind() != SyntaxKind::ARG_LIST => {
                        receiver_text = Some(n.text().to_string());
                        receiver_expr = Some(n);
                        break;
                    }
                    rowan::NodeOrToken::Token(t)
                        if t.kind() == SyntaxKind::THIS_KW || t.kind() == SyntaxKind::SUPER_KW =>
                    {
                        receiver_text = Some(t.text().to_string());
                        break;
                    }
                    _ => {}
                }
            }
        }

        let method_name = if found_dot {
            let mut saw_dot = false;
            let mut method_name = None;

            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::DOT => {
                        saw_dot = true;
                    }
                    rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                        break;
                    }
                    rowan::NodeOrToken::Token(t) if saw_dot && t.kind().is_identifier() => {
                        method_name = Some(t.text().to_string());
                    }
                    _ => {}
                }
            }

            method_name
        } else {
            node.children()
                .find(|n| n.kind() == SyntaxKind::NAME_EXPR)
                .and_then(|name_expr| Self::extract_name_expr_identifier(&name_expr))
        };

        let method_name = method_name?;

        // Try to resolve receiver type
        let receiver_type = if let Some(receiver_expr) = receiver_expr {
            self.infer_receiver_expression_type(&receiver_expr, root, file_id)
                .or_else(|| {
                    receiver_text.as_deref().and_then(|receiver_text| {
                        self.infer_receiver_type_with_resolver(
                            receiver_text,
                            root,
                            offset,
                            source_text,
                            file_id,
                        )
                    })
                })
        } else if let Some(receiver_text) = receiver_text {
            self.infer_receiver_type_with_resolver(
                &receiver_text,
                root,
                offset,
                source_text,
                file_id,
            )
        } else {
            // Local method call: syntax establishes the nearest enclosing
            // nested/secondary type. Regex recovery is only for incomplete
            // trees that do not retain a declaration ancestor.
            self.infer_enclosing_class(node, source_text)
                .or_else(|| self.infer_current_class(source_text))
        };

        debug!(
            "Extracted: method='{}', receiver_type={:?}, paren={}",
            method_name, receiver_type, paren_offset
        );

        Some((method_name, receiver_type, paren_offset, node.clone()))
    }

    /// Infer complex receiver expressions such as fluent method-call chains.
    fn infer_receiver_expression_type(
        &self,
        receiver: &SyntaxNode,
        root: &SyntaxNode,
        file_id: FileId,
    ) -> Option<Type> {
        let builder = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone());
        let resolver = builder.build_from_syntax(root);
        let engine = TypeInferenceEngine::new(self.workspace_index.clone(), file_id);
        let inferred = engine.infer_expression(receiver, &resolver, None);
        (!matches!(inferred, Type::Unknown)).then_some(inferred)
    }

    fn extract_name_expr_identifier(node: &SyntaxNode) -> Option<String> {
        node.children_with_tokens().find_map(|child| match child {
            rowan::NodeOrToken::Token(t) if t.kind().is_identifier() => Some(t.text().to_string()),
            _ => None,
        })
    }

    /// Infer the type of a receiver expression using SymbolResolver
    fn infer_receiver_type_with_resolver(
        &self,
        receiver: &str,
        root: &SyntaxNode,
        offset: u32,
        source_text: &str,
        file_id: FileId,
    ) -> Option<Type> {
        // Build resolver
        let builder = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone());
        let mut resolver = builder.build_from_syntax(root);

        // Find scope at cursor position
        let scope = resolver.find_scope_at_offset(offset)?;

        // Try to resolve the receiver as a variable
        if let Some(resolved) = resolver.resolve_in_scope(receiver, scope) {
            return Some(resolved.ty);
        }

        // Fallback to simple heuristic
        let pattern = format!(r"(\w+)\s+{}\s*=", regex::escape(receiver));
        if let Ok(re) = regex::Regex::new(&pattern) {
            if let Some(caps) = re.captures(source_text) {
                if let Some(type_name) = caps.get(1).map(|m| m.as_str().to_string()) {
                    // Try to resolve type name to FQN
                    return hyperion_semantics::type_parser::parse_type_string(
                        &self.resolve_type_name(&type_name, source_text),
                    );
                }
            }
        }

        // A receiver can itself be an imported class in a static invocation,
        // e.g. `Collections.singletonList(...)`.
        crate::navigation::import_resolver::resolve_class_fqn_via_imports(
            &self.workspace_index,
            file_id,
            receiver,
        )
        .ok()
        .and_then(|fqn| hyperion_semantics::type_parser::parse_type_string(&fqn))
    }

    /// Convert a Type to FQN
    fn type_to_fqn(&self, typ: &Type) -> Option<String> {
        match typ {
            Type::Class(class_type) => {
                if let Some(ref pkg) = class_type.package {
                    Some(format!("{}.{}", pkg, class_type.name))
                } else {
                    Some(class_type.name.to_string())
                }
            }
            _ => None,
        }
    }

    /// Resolve a simple type name to FQN using package context
    fn resolve_type_name(&self, type_name: &str, source_text: &str) -> String {
        // Extract package from source
        if let Some(caps) = PACKAGE_RE.captures(source_text) {
            if let Some(pkg) = caps.get(1) {
                // Try workspace lookup with package prefix
                let fqn_candidate = format!("{}.{}", pkg.as_str(), type_name);
                if self.workspace_index.get_symbol(&fqn_candidate).is_some() {
                    return fqn_candidate;
                }
            }
        }

        // Fallback: return simple name
        type_name.to_string()
    }

    /// Infer the current class (for local method calls)
    fn infer_current_class(&self, source_text: &str) -> Option<Type> {
        // Extract package and class name to build FQN
        let package = PACKAGE_RE
            .captures(source_text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str());

        let class_name = CLASS_RE
            .captures(source_text)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str())?;

        // Build FQN
        let fqn = match package {
            Some(pkg) => Some(format!("{pkg}.{class_name}")),
            None => Some(class_name.to_string()),
        }?;
        self.receiver_type_for_fqn(&fqn)
    }

    fn infer_enclosing_class(&self, node: &SyntaxNode, source_text: &str) -> Option<Type> {
        let mut names = node
            .ancestors()
            .filter(|ancestor| {
                matches!(
                    ancestor.kind(),
                    SyntaxKind::CLASS_DECL
                        | SyntaxKind::INTERFACE_DECL
                        | SyntaxKind::ENUM_DECL
                        | SyntaxKind::RECORD_DECL
                )
            })
            .filter_map(|declaration| {
                declaration.children_with_tokens().find_map(|child| {
                    child
                        .as_token()
                        .filter(|token| token.kind() == SyntaxKind::IDENT)
                        .map(|token| token.text().to_string())
                })
            })
            .collect::<Vec<_>>();
        names.reverse();
        let class_name = names.join(".");
        if class_name.is_empty() {
            return None;
        }
        let package = PACKAGE_RE
            .captures(source_text)
            .and_then(|captures| captures.get(1))
            .map(|package| package.as_str());
        let fqn = package.map_or(class_name.clone(), |package| {
            format!("{package}.{class_name}")
        });
        self.receiver_type_for_fqn(&fqn)
    }

    fn receiver_type_for_fqn(&self, fqn: &str) -> Option<Type> {
        let arguments: Vec<String> = self
            .workspace_index
            .project_owner_type_arguments(fqn, &[], fqn)
            .map(|parameters| {
                parameters
                    .into_iter()
                    .map(|(parameter, _)| parameter.name.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let receiver = if arguments.is_empty() {
            fqn.to_string()
        } else {
            format!("{fqn}<{}>", arguments.join(", "))
        };
        hyperion_semantics::type_parser::parse_type_string(&receiver)
    }

    /// Count the number of parameters before the cursor (by counting commas)
    ///
    /// Properly handles:
    /// - Nested parentheses (only counts commas at depth 1)
    /// - String literals (skips commas inside "...")
    /// - Char literals (skips commas inside '...')
    /// - Escape sequences (handles \" and \' correctly)
    fn count_parameters(&self, source_text: &str, paren_offset: usize, cursor_offset: u32) -> u32 {
        let cursor_offset = cursor_offset as usize;

        if cursor_offset <= paren_offset {
            return 0;
        }

        // Count commas between opening paren and cursor
        let substring = &source_text[paren_offset..cursor_offset.min(source_text.len())];

        let mut comma_count = 0;
        let mut paren_depth = 0;
        let mut in_string = false;
        let mut in_char = false;
        let mut escape_next = false;

        for ch in substring.chars() {
            if escape_next {
                escape_next = false;
                continue;
            }

            match ch {
                '\\' if in_string || in_char => {
                    escape_next = true;
                }
                '"' if !in_char => {
                    in_string = !in_string;
                }
                '\'' if !in_string => {
                    in_char = !in_char;
                }
                '(' if !in_string && !in_char => {
                    paren_depth += 1;
                }
                ')' if !in_string && !in_char => {
                    paren_depth -= 1;
                }
                ',' if !in_string && !in_char && paren_depth == 1 => {
                    // Only count commas at depth 1 (our method's params)
                    comma_count += 1;
                }
                _ => {}
            }
        }

        comma_count
    }

    /// Render every canonical overload candidate for signature help.
    fn get_method_signature_candidates(
        &self,
        class_fqn: &str,
        receiver_arguments: &[String],
        method_name: &str,
    ) -> Vec<(Arc<hyperion_workspace::Symbol>, SignatureInformation)> {
        self.workspace_index
            .find_projected_effective_methods_by_name(class_fqn, receiver_arguments, method_name)
            .into_iter()
            .filter_map(|method| {
                let signature = method.projection?;
                let symbol = method.definition.symbol;
                let return_type = signature.return_type.as_deref().unwrap_or("void");
                let parameter_declarations = signature.parameter_declarations();
                let params_str = parameter_declarations.join(", ");
                let type_params_str = if signature.type_parameters.is_empty() {
                    String::new()
                } else {
                    let parameters: Vec<_> = signature
                        .type_parameters
                        .iter()
                        .map(|parameter| {
                            if parameter.bounds.is_empty() {
                                parameter.name.to_string()
                            } else {
                                format!(
                                    "{} extends {}",
                                    parameter.name,
                                    parameter.bounds.join(" & ")
                                )
                            }
                        })
                        .collect();
                    format!("<{}> ", parameters.join(", "))
                };
                let label = format!("{type_params_str}{return_type} {method_name}({params_str})");
                let parameters = if signature.parameters.is_empty() {
                    Vec::new()
                } else {
                    self.parse_parameters(&params_str, &label)
                };
                let information = SignatureInformation {
                    label,
                    documentation: symbol.documentation.as_ref().map(|documentation| {
                        tower_lsp::lsp_types::Documentation::String(documentation.to_string())
                    }),
                    parameters: Some(parameters),
                    active_parameter: None,
                };
                Some((symbol, information))
            })
            .collect()
    }

    /// Get all constructor signatures for a class (handles overloads)
    fn get_constructor_signatures(&self, class_fqn: &str) -> Vec<SignatureInformation> {
        debug!("Getting constructor signatures for {}", class_fqn);

        let mut signatures = Vec::new();

        // Use get_constructors_for_class for O(1) lookup instead of O(N) search()
        let constructors = self.workspace_index.get_constructors_for_class(class_fqn);

        // Extract simple class name for display
        let simple_class_name = class_fqn
            .rsplit_once('.')
            .map_or(class_fqn, |(_, simple)| simple);

        for symbol in constructors {
            // Build constructor signature: ClassName(params) or <T> ClassName(params)
            let params_str = symbol.parameters.join(", ");

            // Include type parameters for generic constructors
            let type_params_str = if symbol.type_parameters.is_empty() {
                String::new()
            } else {
                let params: Vec<_> = symbol
                    .type_parameters
                    .iter()
                    .map(|tp| {
                        if tp.bounds.is_empty() {
                            tp.name.to_string()
                        } else {
                            format!("{} extends {}", tp.name, tp.bounds.join(" & "))
                        }
                    })
                    .collect();
                format!("<{}> ", params.join(", "))
            };

            let label = format!("{type_params_str}{simple_class_name}({params_str})");

            // Parse parameters
            let parameters = if symbol.parameters.is_empty() {
                vec![]
            } else {
                self.parse_parameters(&params_str, &label)
            };

            signatures.push(SignatureInformation {
                label: label.clone(),
                documentation: symbol
                    .documentation
                    .as_ref()
                    .map(|doc| tower_lsp::lsp_types::Documentation::String(doc.to_string())),
                parameters: Some(parameters),
                active_parameter: None,
            });
        }

        // If no constructors found, create a default no-arg constructor signature
        if signatures.is_empty() {
            debug!("No constructors found, creating default no-arg constructor");
            signatures.push(SignatureInformation {
                label: format!("{simple_class_name}()"),
                documentation: Some(tower_lsp::lsp_types::Documentation::String(
                    "Default constructor".to_string(),
                )),
                parameters: Some(vec![]),
                active_parameter: None,
            });
        }

        debug!(
            "Found {} constructor signature(s) for {}",
            signatures.len(),
            class_fqn
        );
        signatures
    }

    /// Parse parameter string like "String name, int age" into ParameterInformation
    ///
    /// BUG FIX: Now properly handles:
    /// 1. Sequential parameter searching (avoids duplicate offsets for same-type params)
    /// 2. Generic type parameters with commas (e.g., Map<String, Integer>)
    fn parse_parameters(&self, params_str: &str, full_label: &str) -> Vec<ParameterInformation> {
        let mut parameters = Vec::new();

        // Split by comma, respecting generic angle brackets
        let params = Self::split_parameters_respecting_generics(params_str);

        // Track search position to find each parameter sequentially
        // This fixes the bug where "int a" and "int b" both found at same offset
        let mut search_start = 0;

        for param in params {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }

            // Find where this parameter appears in the full label, starting from search_start
            // This ensures sequential parameters get correct, non-overlapping offsets
            if let Some(relative_start) = full_label[search_start..].find(param) {
                let start = search_start + relative_start;
                let end = start + param.len();
                parameters.push(ParameterInformation {
                    label: ParameterLabel::LabelOffsets([start as u32, end as u32]),
                    documentation: None,
                });
                // Move search_start past this parameter for next iteration
                search_start = end;
            } else {
                // Fallback: use string label
                parameters.push(ParameterInformation {
                    label: ParameterLabel::Simple(param.to_string()),
                    documentation: None,
                });
            }
        }

        parameters
    }

    /// Split parameter string by commas, respecting generic angle brackets.
    ///
    /// Handles cases like "List<String> items, Map<String, Integer> config"
    /// where naive split(',') would break on the comma inside Map<String, Integer>.
    fn split_parameters_respecting_generics(params_str: &str) -> Vec<&str> {
        let mut result = Vec::new();
        let mut start = 0;
        let mut angle_depth: i32 = 0;

        for (i, ch) in params_str.char_indices() {
            match ch {
                '<' => angle_depth += 1,
                '>' => angle_depth = angle_depth.saturating_sub(1),
                ',' if angle_depth == 0 => {
                    result.push(&params_str[start..i]);
                    start = i + 1; // Skip the comma
                }
                _ => {}
            }
        }

        // Add the last parameter (everything after last comma or entire string)
        if start < params_str.len() {
            result.push(&params_str[start..]);
        }

        result
    }
}
