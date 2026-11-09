// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Signature matching and method call inference for find-references.
//!
//! General-purpose utilities for:
//! - Extracting parameter signatures from FQN strings and method declarations
//! - Inferring call-site signatures from AST argument lists
//! - Wildcard-aware signature comparison (accounting for `_` and `null`)
//! - Scope-aware variable type resolution for enriching signatures
//!
//! These are used by constructor references and the lexical same-file path.

use crate::navigation::NavigationProvider;
use hyperion_parser::syntax::{AstNode, SyntaxKind};
use hyperion_workspace::usage_extractor::signature_inference::build_type_signature_from_arg_list_with_resolver;
use rowan::TextSize;

impl NavigationProvider<'_> {
    pub(crate) fn extract_signature_from_fqn(fqn: &str) -> Option<&str> {
        let start = fqn.find('(')?;
        let end = fqn.rfind(')')?;
        if start > end {
            return None;
        }
        Some(&fqn[start + 1..end])
    }

    pub(crate) fn extract_method_decl_signature(
        &self,
        method_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<String> {
        use hyperion_parser::syntax::{AstNode, ParamList};

        let param_list_node = method_node
            .children()
            .find(|n| n.kind() == SyntaxKind::PARAM_LIST);
        let Some(param_list_node) = param_list_node else {
            return Some(String::new());
        };
        let param_list = ParamList::cast(param_list_node)?;

        let mut params = Vec::new();
        for param in param_list.params() {
            let raw_ty = param.ty()?.syntax().text().to_string();
            let ty = self.normalize_signature_type(&raw_ty, param.is_varargs());
            params.push(ty);
        }

        Some(params.join(","))
    }

    fn normalize_signature_type(&self, raw: &str, is_varargs: bool) -> String {
        let trimmed = raw.replace(char::is_whitespace, "");
        let (base, suffix) = if let Some(angle_pos) = trimmed.find('<') {
            let base = &trimmed[..angle_pos];
            let suffix = if trimmed.ends_with("...") || is_varargs {
                "..."
            } else if trimmed.contains("[]") {
                let count = trimmed.matches("[]").count();
                return format!(
                    "{}{}",
                    base.rsplit('.').next().unwrap_or(base),
                    "[]".repeat(count)
                );
            } else {
                ""
            };
            (base, suffix)
        } else {
            let suffix = if trimmed.ends_with("...") || is_varargs {
                "..."
            } else if trimmed.contains("[]") {
                let count = trimmed.matches("[]").count();
                return format!(
                    "{}{}",
                    trimmed.rsplit('.').next().unwrap_or(trimmed.as_str()),
                    "[]".repeat(count)
                );
            } else {
                ""
            };
            (trimmed.as_str(), suffix)
        };

        format!("{}{}", base.rsplit('.').next().unwrap_or(base), suffix)
    }

    pub(crate) fn infer_method_call_signature(
        &self,
        call_expr: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<String> {
        for child in call_expr.descendants() {
            if child.kind() == SyntaxKind::ARG_LIST {
                return Some(build_type_signature_from_arg_list_with_resolver(
                    &child,
                    |name_node| {
                        let var_name = name_node.text().to_string().trim().to_string();
                        Self::resolve_variable_type_in_scope(name_node, &var_name)
                    },
                ));
            }
        }
        None
    }

    pub(crate) fn same_file_method_ref_matches_signature(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
        expected_sig: &str,
    ) -> bool {
        let token = root.token_at_offset(TextSize::from(offset)).right_biased();
        let Some(token) = token else {
            return false;
        };
        if !token.kind().is_identifier() {
            return false;
        }

        let method_decl = token.parent().and_then(|p| {
            let mut in_body = false;
            for ancestor in p.ancestors() {
                if ancestor.kind() == SyntaxKind::BLOCK_STMT {
                    in_body = true;
                }
                if ancestor.kind() == SyntaxKind::METHOD_DECL {
                    return if in_body { None } else { Some(ancestor) };
                }
            }
            None
        });
        if let Some(method_node) = method_decl {
            if let Some(signature) = self.extract_method_decl_signature(&method_node) {
                return self.signatures_match(&signature, expected_sig);
            }
        }

        let mut current = token.parent();
        for _ in 0..3 {
            let Some(node) = current else { break };
            if let Some(call_expr) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::METHOD_CALL_EXPR)
            {
                if let Some(sig) = self.infer_method_call_signature(&call_expr) {
                    return self.signatures_match(expected_sig, &sig);
                }
                return true;
            }
            if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                if let Some(sig) = self.infer_method_call_signature(&node) {
                    return self.signatures_match(expected_sig, &sig);
                }
                return true;
            }
            current = node.parent();
        }

        false
    }

    /// Resolve a variable's type by walking up to the enclosing method and scanning
    /// parameters and local variable declarations.
    pub(crate) fn resolve_variable_type_in_scope(
        node: &hyperion_parser::syntax::SyntaxNode,
        var_name: &str,
    ) -> Option<String> {
        use hyperion_parser::syntax::{LocalVarDecl, Param, ParamList};

        // Walk up to find enclosing METHOD_DECL (or CONSTRUCTOR_DECL)
        let method_decl = node.ancestors().find(|n| {
            n.kind() == SyntaxKind::METHOD_DECL || n.kind() == SyntaxKind::CONSTRUCTOR_DECL
        })?;

        // Check method parameters
        if let Some(param_list) = method_decl.children().find_map(ParamList::cast) {
            for param_node in param_list.syntax().children() {
                if let Some(param) = Param::cast(param_node) {
                    if let Some(name_token) = param.name() {
                        if name_token.text() == var_name {
                            return param.ty().map(|ty| Self::simple_type_name(&ty));
                        }
                    }
                }
            }
        }

        // Check local variable declarations preceding the call site
        let call_offset = node.text_range().start();
        for descendant in method_decl.descendants() {
            if descendant.kind() == SyntaxKind::LOCAL_VAR_DECL
                && descendant.text_range().start() < call_offset
            {
                if let Some(local_var) = LocalVarDecl::cast(descendant) {
                    for name_token in local_var.names() {
                        if name_token.text() == var_name {
                            return local_var.ty().map(|ty| Self::simple_type_name(&ty));
                        }
                    }
                }
            }
        }

        None
    }

    /// Extract the simple type name from a Type AST node for signature matching.
    fn simple_type_name(ty: &hyperion_parser::syntax::Type) -> String {
        use hyperion_parser::syntax::Type;

        match ty {
            Type::Primitive(p) => p.syntax().text().to_string().trim().to_string(),
            Type::Class(c) => {
                // Get the last identifier (simple name, ignoring package qualifiers)
                let mut last_ident = String::new();
                for child in c.syntax().descendants_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = child {
                        if t.kind().is_identifier() {
                            last_ident = t.text().to_string();
                        }
                    }
                }
                if last_ident.is_empty() {
                    "_".to_string()
                } else {
                    last_ident
                }
            }
            Type::Array(_) => {
                // For arrays, get the element type text representation
                ty.syntax().text().to_string().trim().to_string()
            }
            _ => "_".to_string(),
        }
    }

    /// Check if two signatures match, accounting for wildcards (_)
    ///
    /// A wildcard (_) in either signature matches any type in the other signature.
    /// This allows matching variable arguments against literal arguments.
    ///
    /// Examples:
    /// - "String,int" matches "String,int" (exact match)
    /// - "String,_" matches "String,int" (wildcard in expected)
    /// - "String,int" matches "String,_" (wildcard in call)
    /// - "_,_" matches "String,int" (all wildcards)
    /// - "null" matches "String" (null is assignable to reference types)
    /// - "String,int" does NOT match "int,String" (order matters)
    pub(crate) fn signatures_match(&self, expected: &str, actual: &str) -> bool {
        // Empty signatures match
        if expected.is_empty() || actual.is_empty() {
            return expected.is_empty() && actual.is_empty();
        }

        let expected_types: Vec<&str> = expected.split(',').collect();
        let actual_types: Vec<&str> = actual.split(',').collect();

        // Different parameter counts don't match
        if expected_types.len() != actual_types.len() {
            return false;
        }

        // Check each parameter pair
        for (exp, act) in expected_types.iter().zip(actual_types.iter()) {
            let exp = Self::normalize_signature_component(exp);
            let act = Self::normalize_signature_component(act);

            // Wildcard matches anything
            if exp == "_" || act == "_" {
                continue;
            }

            // null matches any reference type (not primitives)
            if exp == "null" || act == "null" {
                // Allow null to match reference types
                // Primitives: int, long, float, double, boolean, char, byte, short
                let is_primitive = matches!(
                    exp.as_str(),
                    "int" | "long" | "float" | "double" | "boolean" | "char" | "byte" | "short"
                ) || matches!(
                    act.as_str(),
                    "int" | "long" | "float" | "double" | "boolean" | "char" | "byte" | "short"
                );

                if !is_primitive {
                    continue; // null matches reference types
                }
            }

            // Exact match required
            if exp != act {
                return false;
            }
        }

        true
    }

    fn normalize_signature_component(sig: &str) -> String {
        if sig == "_" || sig == "null" {
            return sig.to_string();
        }

        let mut base = sig;
        let mut suffix = String::new();

        if let Some(stripped) = base.strip_suffix("...") {
            base = stripped;
            suffix.push_str("...");
        }

        let mut array_count = 0;
        while let Some(stripped) = base.strip_suffix("[]") {
            base = stripped;
            array_count += 1;
        }
        if array_count > 0 {
            suffix.push_str(&"[]".repeat(array_count));
        }

        let simple = base.rsplit('.').next().unwrap_or(base);
        format!("{simple}{suffix}")
    }
}
