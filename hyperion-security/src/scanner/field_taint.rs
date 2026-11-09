// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Field taint collection and interprocedural taint propagation.
//!
//! This module handles Pass 0 (parameter map building), Pass 1 (field taint collection),
//! Pass 1.5 (fixpoint taint propagation), and Pass 2 (analysis with propagated taint)
//! of the SecurityVisitor's multi-pass analysis.

use super::utilities::{is_expr_kind, is_stmt_kind};
use super::visitor::SecurityVisitor;

use crate::taint::{TaintState, VulnTypes};

use hyperion_parser::syntax::{
    AstNode, ConstructorDecl, FieldDecl, MethodDecl, SyntaxKind, SyntaxNode,
};
use tracing::debug;

impl SecurityVisitor<'_> {
    /// Model Java serialization's `defaultReadObject`: inside a Serializable
    /// `readObject` hook it restores every non-static, non-transient instance
    /// field from untrusted serialized input.
    pub(super) fn collect_default_deserialized_fields(
        &mut self,
        type_decl: &SyntaxNode,
        methods: &[SyntaxNode],
    ) {
        let mut in_implements_clause = false;
        let mut is_serializable = false;
        for element in type_decl.children_with_tokens() {
            if element.kind() == SyntaxKind::L_BRACE {
                break;
            }
            if element.kind() == SyntaxKind::IMPLEMENTS_KW {
                in_implements_clause = true;
                continue;
            }
            if !in_implements_clause {
                continue;
            }
            is_serializable = match element {
                rowan::NodeOrToken::Node(node) => node
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .any(|token| token.kind().is_identifier() && token.text() == "Serializable"),
                rowan::NodeOrToken::Token(token) => {
                    token.kind().is_identifier() && token.text() == "Serializable"
                }
            };
            if is_serializable {
                break;
            }
        }
        if !is_serializable {
            return;
        }

        let default_read = methods.iter().find_map(|method_node| {
            let method = MethodDecl::cast(method_node.clone())?;
            if method.name()?.text() != "readObject" {
                return None;
            }
            method_node.descendants().find(|descendant| {
                descendant.kind() == SyntaxKind::METHOD_CALL_EXPR
                    && Self::extract_call_method_name(descendant).as_deref()
                        == Some("defaultReadObject")
            })
        });
        let Some(default_read) = default_read else {
            return;
        };

        let source_location = self.node_location(&default_read);
        let source_code = Self::get_code_snippet(&default_read);
        for field_node in type_decl
            .children()
            .filter(|child| child.kind() == SyntaxKind::FIELD_DECL)
        {
            let mut adjacent_modifiers = None;
            for sibling in type_decl.children() {
                if sibling.text_range() == field_node.text_range() {
                    break;
                }
                if sibling.kind() == SyntaxKind::MODIFIER_LIST {
                    adjacent_modifiers = Some(sibling);
                } else {
                    adjacent_modifiers = None;
                }
            }
            let is_not_serialized = adjacent_modifiers.is_some_and(|modifiers| {
                modifiers.descendants_with_tokens().any(|element| {
                    matches!(
                        element.kind(),
                        SyntaxKind::STATIC_KW | SyntaxKind::TRANSIENT_KW
                    )
                })
            });
            if is_not_serialized {
                continue;
            }

            let Some(field) = FieldDecl::cast(field_node) else {
                continue;
            };
            for field_name in field.names() {
                let taint = TaintState::tainted_with_kind(
                    "ObjectInputStream.defaultReadObject".to_string(),
                    source_location.clone(),
                    source_code.clone(),
                    VulnTypes::all_vulns(),
                    crate::taint::TrustLevel::Untrusted,
                    crate::sources::SourceKind::Deserialization,
                );
                self.taint_tracker
                    .mark_field_tainted(field_name.text(), taint);
            }
        }
    }

    pub(super) fn build_method_param_map(
        methods: &[SyntaxNode],
    ) -> std::collections::HashMap<String, Vec<String>> {
        let mut result = std::collections::HashMap::new();

        for method_node in methods {
            let Some(method) = MethodDecl::cast(method_node.clone()) else {
                continue;
            };

            let method_name = method
                .name()
                .map(|n| n.text().to_string())
                .unwrap_or_default();

            if method_name.is_empty() {
                continue;
            }

            let mut params = Vec::new();
            if let Some(param_list) = method.params() {
                for param in param_list.params() {
                    if let Some(name) = param.name() {
                        params.push(name.text().to_string());
                    }
                }
            }

            result.insert(method_name, params);
        }

        result
    }

    /// First pass: Visit method to collect field taint sources AND method call edges
    /// Returns list of (callee_method_name, param_index) for tainted arguments
    pub(super) fn visit_method_for_field_taint_collection_with_calls(
        &mut self,
        node: &SyntaxNode,
    ) -> Vec<(String, usize)> {
        // First collect field taint using the existing method
        self.visit_method_for_field_taint_collection(node);

        // Now collect method call edges with tainted arguments
        let mut tainted_calls = Vec::new();

        // Walk the method body looking for method calls
        for desc in node.descendants() {
            if desc.kind() != SyntaxKind::METHOD_CALL_EXPR {
                continue;
            }

            // Get the method name being called
            let method_name = Self::extract_call_method_name(&desc);
            let Some(method_name) = method_name else {
                continue;
            };

            // Check each argument for taint
            for child in desc.children() {
                if child.kind() != SyntaxKind::ARG_LIST {
                    continue;
                }

                let mut arg_index = 0;
                for arg in child.children() {
                    if !is_expr_kind(arg.kind()) {
                        continue;
                    }

                    // Check if this argument expression is tainted
                    if self.is_expression_tainted(&arg) {
                        tainted_calls.push((method_name.clone(), arg_index));
                    }
                    arg_index += 1;
                }
            }
        }

        tainted_calls
    }

    /// Extract method name from a METHOD_CALL_EXPR node.
    ///
    /// With nested trees, METHOD_CALL_EXPR contains all parts directly:
    /// - Simple call `foo(x)`:      METHOD_CALL_EXPR -> [NAME_EXPR("foo"), ARG_LIST]
    /// - Receiver call `obj.foo(x)`: METHOD_CALL_EXPR -> [NAME_EXPR("obj"), DOT, IDENT("foo"), ARG_LIST]
    fn extract_call_method_name(call: &SyntaxNode) -> Option<String> {
        let mut last_ident = None;
        for child in call.children_with_tokens() {
            match &child {
                rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                    last_ident = Some(t.text().to_string());
                }
                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                    break;
                }
                rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                    // For simple calls, NAME_EXPR holds the method name
                    for elem in n.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = elem {
                            if t.kind() == SyntaxKind::IDENT {
                                last_ident = Some(t.text().to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        last_ident
    }

    /// Check if an expression is tainted based on current taint tracker state
    fn is_expression_tainted(&self, expr: &SyntaxNode) -> bool {
        // Simple name expression - check if variable is tainted
        if expr.kind() == SyntaxKind::NAME_EXPR {
            let text = expr.text().to_string().trim().to_string();
            return self.taint_tracker.is_tainted(&text).is_some();
        }

        // For other expression types, check descendants
        for desc in expr.descendants() {
            if desc.kind() == SyntaxKind::NAME_EXPR {
                let text = desc.text().to_string().trim().to_string();
                if self.taint_tracker.is_tainted(&text).is_some() {
                    return true;
                }
            }
        }

        false
    }

    /// Propagate taint across method calls using fixpoint iteration
    pub(super) fn propagate_method_taint(
        &mut self,
        methods: &[SyntaxNode],
        method_params: &std::collections::HashMap<String, Vec<String>>,
        initial_taint: &std::collections::HashSet<(String, usize)>,
    ) -> std::collections::HashSet<(String, usize)> {
        // Start with initial taint from direct sources
        let mut tainted: std::collections::HashSet<(String, usize)> = initial_taint.clone();
        let mut changed = true;
        let mut iteration = 0;
        let max_iterations = 10;

        while changed && iteration < max_iterations {
            changed = false;
            iteration += 1;

            // For each method with tainted parameters, check what it calls
            for method_node in methods {
                let Some(method) = MethodDecl::cast(method_node.clone()) else {
                    continue;
                };

                let method_name = method
                    .name()
                    .map(|n| n.text().to_string())
                    .unwrap_or_default();

                if method_name.is_empty() {
                    continue;
                }

                // Get parameter names for this method
                let Some(param_names) = method_params.get(&method_name) else {
                    continue;
                };

                // Check if any parameter is tainted
                let mut tainted_params: std::collections::HashSet<usize> =
                    std::collections::HashSet::new();
                for (tainted_method, tainted_idx) in &tainted {
                    if tainted_method == &method_name {
                        tainted_params.insert(*tainted_idx);
                    }
                }

                if tainted_params.is_empty() {
                    continue;
                }

                // Simulate taint flow through this method
                // Reset tracker and mark tainted params
                self.taint_tracker.reset();
                for &param_idx in &tainted_params {
                    if param_idx < param_names.len() {
                        let param_name = &param_names[param_idx];
                        let location = self.node_location(method_node);
                        let taint = TaintState::tainted(
                            format!("parameter {param_name}"),
                            location,
                            param_name.clone(),
                            VulnTypes::all_vulns(),
                        );
                        self.taint_tracker.mark_tainted(param_name.clone(), taint);
                    }
                }

                // Walk method body and collect new call edges
                for desc in method_node.descendants() {
                    // Track variable assignments
                    if desc.kind() == SyntaxKind::LOCAL_VAR_DECL {
                        self.process_var_decl_for_propagation(&desc);
                    }

                    // Track method calls
                    if desc.kind() != SyntaxKind::METHOD_CALL_EXPR {
                        continue;
                    }

                    let Some(callee_name) = Self::extract_call_method_name(&desc) else {
                        continue;
                    };

                    // Check each argument for taint
                    for child in desc.children() {
                        if child.kind() != SyntaxKind::ARG_LIST {
                            continue;
                        }

                        let mut arg_index = 0;
                        for arg in child.children() {
                            if !is_expr_kind(arg.kind()) {
                                continue;
                            }

                            if self.is_expression_tainted(&arg) {
                                let key = (callee_name.clone(), arg_index);
                                if tainted.insert(key) {
                                    changed = true;
                                }
                            }
                            arg_index += 1;
                        }
                    }
                }
            }
        }

        tainted
    }

    /// Process variable declaration for taint propagation
    fn process_var_decl_for_propagation(&mut self, node: &SyntaxNode) {
        // Find the variable name (IDENT) and initializer expression
        let mut var_name = None;
        let mut initializer = None;

        // The name is a token and the initializer a node, so both are read from children_with_tokens.
        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT && var_name.is_none() {
                    var_name = Some(token.text().to_string());
                }
            } else if let Some(expr) = child.as_node() {
                if is_expr_kind(expr.kind()) {
                    initializer = Some(expr.clone());
                }
            }
        }

        let (Some(name), Some(init)) = (var_name, initializer) else {
            return;
        };

        // If initializer is tainted, mark variable as tainted
        if self.is_expression_tainted(&init) {
            let location = self.node_location(node);
            let taint = TaintState::tainted(
                format!("variable {name}"),
                location,
                name.clone(),
                VulnTypes::all_vulns(),
            );
            self.taint_tracker.mark_tainted(name, taint);
        }
    }

    /// Visit method in pass 2 with propagated taint information
    pub(super) fn visit_method_with_propagated_taint(
        &mut self,
        node: &SyntaxNode,
        method_params: &std::collections::HashMap<String, Vec<String>>,
        propagated_taint: &std::collections::HashSet<(String, usize)>,
    ) {
        // Reset variable/param taint but preserve field taint across methods
        self.taint_tracker.reset();

        let Some(method) = MethodDecl::cast(node.clone()) else {
            return;
        };

        let method_name = method
            .name()
            .map(|n| n.text().to_string())
            .unwrap_or_default();

        #[cfg(test)]
        eprintln!(
            "=== Visiting method '{}' with propagated taint ===",
            method_name
        );

        // Check if method has a message broker listener annotation
        let has_listener_annotation = Self::method_has_listener_annotation(&method);
        let has_http_endpoint_annotation = Self::method_has_http_endpoint_annotation(&method);

        // Pre-taint parameters from interprocedural analysis AND from annotations
        if let Some(params) = method.params() {
            let param_names_list = method_params.get(&method_name);

            for (param_idx, param) in params.params().enumerate() {
                let mut is_tainted = false;
                let mut param_type: Option<String> = None;

                // Extract parameter type
                for desc in param.syntax().descendants() {
                    if desc.kind() == SyntaxKind::CLASS_TYPE {
                        param_type = desc
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());
                        break;
                    }
                    if desc.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        param_type = desc
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|t| t.kind() == SyntaxKind::IDENT)
                            .map(|t| t.text().to_string());
                        break;
                    }
                }

                // Check parameter-level annotations
                if Self::param_is_tainted(&param) {
                    is_tainted = true;
                }

                // If method has a listener annotation, all parameters are tainted
                if has_listener_annotation {
                    is_tainted = true;
                }

                // Also check if parameter type is a known source type (e.g., HttpServletRequest)
                if let Some(ref type_name) = param_type {
                    if Self::is_source_type(type_name)
                        || (has_http_endpoint_annotation
                            && Self::is_raw_request_body_type(type_name))
                    {
                        is_tainted = true;
                    }
                }

                // Check if this parameter is tainted from interprocedural analysis
                if param_names_list.is_some() {
                    for (tainted_method, tainted_idx) in propagated_taint {
                        if tainted_method == &method_name && *tainted_idx == param_idx {
                            is_tainted = true;
                            #[cfg(test)]
                            eprintln!(
                                "Pass 2: Pre-tainting param idx {} in method '{}' from interprocedural",
                                param_idx, method_name
                            );
                            break;
                        }
                    }
                }

                // Store parameter type and mark as tainted if needed
                if let Some(name) = param.name() {
                    let param_name = name.text().to_string();
                    if let Some(type_name) = param_type.clone().filter(|t| t != "var") {
                        self.taint_tracker
                            .set_var_type(param_name.clone(), type_name);
                    }

                    if is_tainted {
                        debug!(
                            param = %param_name,
                            "Marking parameter as tainted (source annotation, listener, or interprocedural)"
                        );

                        let location = self.node_location(param.syntax());
                        let code = Self::get_code_snippet(param.syntax());
                        let taint = TaintState::tainted(
                            format!("parameter @{param_name}"),
                            location,
                            code,
                            VulnTypes::all_vulns(),
                        );
                        self.taint_tracker.mark_tainted(param_name, taint);
                    }
                }
            }
        }

        // Visit method body
        if let Some(body) = method.body() {
            self.visit_block(body.syntax());
        }
    }

    /// First pass: Visit method to collect field taint sources only
    /// This does not report findings - just tracks field assignments from tainted sources
    fn visit_method_for_field_taint_collection(&mut self, node: &SyntaxNode) {
        // Reset variable/param taint but preserve field taint across methods
        self.taint_tracker.reset();

        let Some(method) = MethodDecl::cast(node.clone()) else {
            return;
        };

        #[cfg(test)]
        eprintln!("=== Pass 1: Collecting field taint from method ===");

        // Check if method has a message broker listener annotation
        let has_listener_annotation = Self::method_has_listener_annotation(&method);
        let has_http_endpoint_annotation = Self::method_has_http_endpoint_annotation(&method);

        // Check for tainted parameters (Spring/JAX-RS annotations)
        if let Some(params) = method.params() {
            for param in params.params() {
                let mut is_tainted = false;
                let mut param_type: Option<String> = None;

                for desc in param.syntax().descendants() {
                    if desc.kind() == SyntaxKind::CLASS_TYPE {
                        param_type = desc
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());
                        break;
                    }
                    if desc.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        param_type = desc
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|t| t.kind() == SyntaxKind::IDENT)
                            .map(|t| t.text().to_string());
                        break;
                    }
                }

                if Self::param_is_tainted(&param) {
                    is_tainted = true;
                }

                if has_listener_annotation {
                    is_tainted = true;
                }

                // Also check if parameter type is a known source type (e.g., HttpServletRequest)
                if let Some(ref type_name) = param_type {
                    if Self::is_source_type(type_name)
                        || (has_http_endpoint_annotation
                            && Self::is_raw_request_body_type(type_name))
                    {
                        is_tainted = true;
                    }
                }

                if is_tainted {
                    if let Some(name) = param.name() {
                        let param_name = name.text().to_string();
                        if let Some(type_name) = param_type.clone().filter(|t| t != "var") {
                            self.taint_tracker
                                .set_var_type(param_name.clone(), type_name);
                        }
                        let location = self.node_location(param.syntax());
                        let code = Self::get_code_snippet(param.syntax());
                        let taint = TaintState::tainted(
                            format!("parameter @{param_name}"),
                            location,
                            code,
                            VulnTypes::all_vulns(),
                        );
                        self.taint_tracker.mark_tainted(param_name, taint);
                    }
                }
            }
        }

        // Visit method body to track field assignments (but don't report findings)
        if let Some(body) = method.body() {
            self.visit_block_for_field_taint(body.syntax());
        }
    }

    /// First pass: Visit constructor to collect field taint sources only
    /// This does not report findings - just tracks field assignments from tainted sources
    pub(super) fn visit_constructor_for_field_taint_collection(&mut self, node: &SyntaxNode) {
        // Reset variable/param taint but preserve field taint across methods
        self.taint_tracker.reset();

        let Some(constructor) = ConstructorDecl::cast(node.clone()) else {
            return;
        };

        #[cfg(test)]
        eprintln!("=== Pass 1: Collecting field taint from constructor ===");

        // Check for tainted parameters (Spring/JAX-RS annotations)
        if let Some(params) = constructor.params() {
            for param in params.params() {
                let mut is_tainted = false;
                let mut param_type: Option<String> = None;

                for desc in param.syntax().descendants() {
                    if desc.kind() == SyntaxKind::CLASS_TYPE {
                        param_type = desc
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());
                        break;
                    }
                    if desc.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        param_type = desc
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|t| t.kind() == SyntaxKind::IDENT)
                            .map(|t| t.text().to_string());
                        break;
                    }
                }

                if Self::param_is_tainted(&param) {
                    is_tainted = true;
                }

                if is_tainted {
                    if let Some(name) = param.name() {
                        let param_name = name.text().to_string();
                        if let Some(type_name) = param_type.clone().filter(|t| t != "var") {
                            self.taint_tracker
                                .set_var_type(param_name.clone(), type_name);
                        }
                        let location = self.node_location(param.syntax());
                        let code = Self::get_code_snippet(param.syntax());
                        let taint = TaintState::tainted(
                            format!("parameter @{param_name}"),
                            location,
                            code,
                            VulnTypes::all_vulns(),
                        );
                        self.taint_tracker.mark_tainted(param_name, taint);
                    }
                }
            }
        }

        // Visit constructor body to track field assignments (but don't report findings)
        if let Some(body) = constructor.body() {
            self.visit_block_for_field_taint(body.syntax());
        }
    }

    /// Visit a block during field taint collection pass
    /// Only tracks variable assignments and field assignments, does not check sinks
    fn visit_block_for_field_taint(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            self.visit_stmt_for_field_taint(&child);
        }
    }

    /// Visit a statement during field taint collection pass
    fn visit_stmt_for_field_taint(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::BLOCK_STMT => {
                self.visit_block_for_field_taint(node);
            }
            SyntaxKind::EXPR_STMT => {
                for child in node.children() {
                    self.visit_expr_for_field_taint(&child);
                }
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                // Track variable declarations (needed for taint flow to fields)
                self.visit_local_var_decl_for_field_taint(node);
            }
            SyntaxKind::IF_STMT | SyntaxKind::WHILE_STMT | SyntaxKind::FOR_STMT => {
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLOCK_STMT {
                        self.visit_block_for_field_taint(&child);
                    }
                }
            }
            SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => {
                for child in node.children() {
                    if child.kind() == SyntaxKind::RESOURCE_SPEC {
                        // Try-with-resources: resources live in RESOURCE_SPEC, not the try-body block.
                        // The parser wraps resource declarations in LOCAL_VAR_DECL for analysis.
                        for resource in child.children() {
                            if resource.kind() == SyntaxKind::LOCAL_VAR_DECL {
                                self.visit_local_var_decl_for_field_taint(&resource);
                            } else if is_expr_kind(resource.kind()) {
                                self.visit_expr_for_field_taint(&resource);
                            }
                        }
                    } else if child.kind() == SyntaxKind::BLOCK_STMT
                        || child.kind() == SyntaxKind::CATCH_CLAUSE
                        || child.kind() == SyntaxKind::FINALLY_CLAUSE
                    {
                        self.visit_block_for_field_taint(&child);
                    }
                }
            }
            _ => {
                for child in node.children() {
                    if is_stmt_kind(child.kind()) {
                        self.visit_stmt_for_field_taint(&child);
                    }
                }
            }
        }
    }

    /// Visit expression during field taint collection pass - only track assignments
    fn visit_expr_for_field_taint(&mut self, node: &SyntaxNode) {
        if node.kind() == SyntaxKind::ASSIGNMENT_EXPR {
            self.visit_assignment_for_field_taint(node);
        } else {
            // Visit children for nested assignments
            for child in node.children() {
                if is_expr_kind(child.kind()) {
                    self.visit_expr_for_field_taint(&child);
                }
            }
        }
    }

    /// Visit local variable declaration during field taint collection
    fn visit_local_var_decl_for_field_taint(&mut self, node: &SyntaxNode) {
        // Simplified version of visit_local_var_decl that only tracks taint flow
        let mut var_name: Option<String> = None;

        let children: Vec<_> = node.children_with_tokens().collect();
        let mut past_type = false;

        for child in &children {
            if let Some(child_node) = child.as_node() {
                if child_node.kind() == SyntaxKind::CLASS_TYPE
                    || child_node.kind() == SyntaxKind::PRIMITIVE_TYPE
                    || child_node.kind() == SyntaxKind::ARRAY_TYPE
                {
                    past_type = true;
                }
            } else if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::EQ {
                    break;
                }
                if past_type && var_name.is_none() {
                    let is_identifier_like =
                        token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::TO_KW;
                    if is_identifier_like {
                        var_name = Some(token.text().to_string());
                    }
                }
            }
        }

        // Check if initialization is tainted
        let initializer_tainted = self
            .taint_tracker
            .check_method_call_pattern_taint_in_children(node, &|n| self.node_location(n), &|n| {
                Self::get_code_snippet(n)
            });

        // Also check child expressions
        let mut final_taint = initializer_tainted;
        if final_taint.is_none() {
            for child in node.children() {
                if is_expr_kind(child.kind()) {
                    // Skip numeric conversions
                    if self.type_analyzer.is_sql_safe_type(&child) {
                        continue;
                    }
                    if let Some(taint) =
                        self.taint_tracker
                            .expr_taint(&child, &|n| self.node_location(n), &|n| {
                                Self::get_code_snippet(n)
                            })
                    {
                        final_taint = Some(taint);
                        break;
                    }
                }
            }
        }

        if let (Some(name), Some(taint_state)) = (var_name, final_taint) {
            let location = self.node_location(node);
            let code = Self::get_code_snippet(node);
            let propagated =
                taint_state.propagate(location, format!("Assigned to variable '{name}'"), code);
            self.taint_tracker.mark_tainted(name, propagated);
        }
    }

    /// Visit assignment expression during field taint collection
    fn visit_assignment_for_field_taint(&mut self, node: &SyntaxNode) {
        // ASSIGNMENT_EXPR only contains EQ + RHS; LHS is a preceding sibling.
        // Parent (EXPR_STMT) structure:
        //   FIELD_ACCESS_EXPR (or NAME_EXPR) ← LHS sibling
        //   ASSIGNMENT_EXPR                  ← this node (contains EQ + RHS)
        let children: Vec<SyntaxNode> = node.children().collect();
        let rhs = children.first();

        // Get parent to find LHS sibling
        let Some(parent) = node.parent() else {
            return;
        };

        // Find LHS: the expression node immediately before ASSIGNMENT_EXPR
        let mut field_name: Option<String> = None;
        let mut var_name: Option<String> = None;

        let parent_children: Vec<_> = parent.children().collect();
        let assignment_pos = parent_children.iter().position(|n| n == node);
        if let Some(pos) = assignment_pos {
            if pos > 0 {
                let lhs = &parent_children[pos - 1];
                if lhs.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                    // With nested trees, FIELD_ACCESS_EXPR contains THIS_EXPR + DOT + IDENT
                    field_name = lhs
                        .children_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .last()
                        .map(|t| t.text().to_string());
                } else if lhs.kind() == SyntaxKind::NAME_EXPR {
                    var_name = Self::get_var_name(lhs);
                    if let Some(ref name) = var_name {
                        field_name = Some(name.clone());
                    }
                }
            }
        }

        // Check if RHS is tainted
        let mut rhs_taint = self
            .taint_tracker
            .check_method_call_pattern_taint_in_children(node, &|n| self.node_location(n), &|n| {
                Self::get_code_snippet(n)
            });

        if rhs_taint.is_none() {
            if let Some(rhs_node) = rhs {
                rhs_taint =
                    self.taint_tracker
                        .expr_taint(rhs_node, &|n| self.node_location(n), &|n| {
                            Self::get_code_snippet(n)
                        });
            }
        }

        if let Some(taint) = rhs_taint {
            let location = self.node_location(node);
            let code = Self::get_code_snippet(node);

            if let Some(field) = field_name {
                #[cfg(test)]
                eprintln!("  Pass 1: Marking field '{}' as tainted", field);

                let propagated = taint.propagate(
                    location.clone(),
                    format!("Assigned to field '{field}'"),
                    code.clone(),
                );
                self.taint_tracker.mark_field_tainted(&field, propagated);
            }

            if let Some(name) = var_name {
                let propagated =
                    taint.propagate(location, format!("Assigned to variable '{name}'"), code);
                self.taint_tracker.mark_tainted(name, propagated);
            }
        }
    }

    pub(super) fn visit_constructor(&mut self, node: &SyntaxNode) {
        // Reset taint tracking for new constructor
        self.taint_tracker.reset();

        let Some(constructor) = ConstructorDecl::cast(node.clone()) else {
            return;
        };

        #[cfg(test)]
        eprintln!("=== Visiting constructor ===");

        // Check for tainted parameters (Spring/JAX-RS annotations, or HttpServletRequest type)
        if let Some(params) = constructor.params() {
            for param in params.params() {
                let mut is_tainted = false;
                let mut param_type: Option<String> = None;

                for desc in param.syntax().descendants() {
                    if desc.kind() == SyntaxKind::CLASS_TYPE {
                        param_type = desc
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());
                        break;
                    }
                    if desc.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        param_type = desc
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|t| t.kind() == SyntaxKind::IDENT)
                            .map(|t| t.text().to_string());
                        break;
                    }
                }

                // Check parameter-level annotations
                if Self::param_is_tainted(&param) {
                    is_tainted = true;
                }

                // Also check if parameter type is a known source type (e.g., HttpServletRequest)
                if let Some(ref type_name) = param_type {
                    if Self::is_source_type(type_name) {
                        is_tainted = true;
                    }
                }

                if is_tainted {
                    if let Some(name) = param.name() {
                        let param_name = name.text().to_string();

                        debug!(
                            param = %param_name,
                            "Marking constructor parameter as tainted"
                        );

                        // Store the type for the parameter
                        if let Some(type_name) = param_type.clone().filter(|t| t != "var") {
                            self.taint_tracker
                                .set_var_type(param_name.clone(), type_name);
                        }

                        // Create taint state for the parameter
                        let location = self.node_location(param.syntax());
                        let code = Self::get_code_snippet(param.syntax());
                        let taint = TaintState::tainted(
                            format!("parameter @{param_name}"),
                            location,
                            code,
                            VulnTypes::all_vulns(),
                        );
                        self.taint_tracker.mark_tainted(param_name, taint);
                    }
                } else if let Some(name) = param.name() {
                    let param_name = name.text().to_string();
                    if let Some(type_name) = param_type.clone().filter(|t| t != "var") {
                        self.taint_tracker.set_var_type(param_name, type_name);
                    }
                }
            }
        }

        // Visit constructor body
        if let Some(body) = constructor.body() {
            self.visit_block(body.syntax());
        }
    }
}
