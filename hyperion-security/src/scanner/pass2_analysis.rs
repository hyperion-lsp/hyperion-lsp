// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Pass 2 analysis methods for SecurityVisitor.
//!
//! This module contains the main AST-walking methods used during the second pass
//! of taint analysis: visiting blocks, statements, expressions, local variable
//! declarations, assignments, lambda expressions, and sink/constructor checks.

use crate::finding::{Finding, VulnType};
use crate::taint::{TaintState, TaintTracker};

use hyperion_parser::syntax::{AstNode, MethodCallExpr, SyntaxKind, SyntaxNode};
use tracing::{debug, info};

use super::collection_taint;
use super::method_call_pattern;
use super::sink_detection;
use super::utilities::{
    code_snippet_from_node, confidence_for_taint, is_expr_kind, is_stmt_kind,
    lambda_parameter_names, location_from_node, looks_like_identifier_expr,
};
use super::visitor::SecurityVisitor;

impl SecurityVisitor<'_> {
    pub(super) fn visit_block(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            self.visit_stmt(&child);
        }
    }

    fn visit_stmt(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::BLOCK_STMT => {
                self.visit_block(node);
            }
            SyntaxKind::EXPR_STMT => {
                // Check for method call sink pattern in flat structure
                self.check_sink_pattern_in_children(node);

                // Also visit child expressions
                for child in node.children() {
                    self.visit_expr(&child);
                }
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                self.visit_local_var_decl(node);
            }
            SyntaxKind::IF_STMT | SyntaxKind::WHILE_STMT | SyntaxKind::FOR_STMT => {
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLOCK_STMT {
                        self.visit_block(&child);
                    } else if is_expr_kind(child.kind()) {
                        self.visit_expr(&child);
                    }
                }
            }
            SyntaxKind::RETURN_STMT => {
                // Check for sink patterns in return statement
                self.check_sink_pattern_in_children(node);

                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        self.visit_expr(&child);
                    }
                }
            }
            SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => {
                for child in node.children() {
                    if child.kind() == SyntaxKind::RESOURCE_SPEC {
                        // Try-with-resources: resource initializers can contain sinks (e.g., JDBC, URL.openStream()).
                        // The parser represents resource declarations as LOCAL_VAR_DECL nodes.
                        for resource in child.children() {
                            if resource.kind() == SyntaxKind::LOCAL_VAR_DECL {
                                self.visit_local_var_decl(&resource);
                            } else if is_expr_kind(resource.kind()) {
                                self.visit_expr(&resource);
                            }
                        }
                    } else if child.kind() == SyntaxKind::BLOCK_STMT
                        || child.kind() == SyntaxKind::CATCH_CLAUSE
                        || child.kind() == SyntaxKind::FINALLY_CLAUSE
                    {
                        self.visit_block(&child);
                    }
                }
            }
            _ => {
                // Visit children recursively
                for child in node.children() {
                    if is_stmt_kind(child.kind()) {
                        self.visit_stmt(&child);
                    } else if is_expr_kind(child.kind()) {
                        self.visit_expr(&child);
                    }
                }
            }
        }
    }

    fn visit_local_var_decl(&mut self, node: &SyntaxNode) {
        // Track if the initializer is tainted
        // The structure for `String id = request.getParameter("id");` is:
        //   LOCAL_VAR_DECL
        //     CLASS_TYPE (String)
        //     IDENT (id)          <- var name
        //     EQ
        //     NAME_EXPR (request) <- receiver
        //     DOT
        //     IDENT (getParameter) <- method name
        //     METHOD_CALL_EXPR    <- just the args
        //     SEMICOLON

        let mut var_name: Option<String> = None;
        let mut is_array_type = false;
        let mut declared_type: Option<String> = None;

        // Collect children in order
        let children: Vec<_> = node.children_with_tokens().collect();

        // Find variable name (first IDENT or keyword-as-identifier after type)
        let mut past_type = false;
        for child in &children {
            if let Some(node) = child.as_node() {
                if node.kind() == SyntaxKind::VAR_TYPE {
                    past_type = true;
                    if declared_type.is_none() {
                        declared_type = Some("var".to_string());
                    }
                }
                if node.kind() == SyntaxKind::CLASS_TYPE
                    || node.kind() == SyntaxKind::PRIMITIVE_TYPE
                {
                    past_type = true;
                    if declared_type.is_none() && node.kind() == SyntaxKind::CLASS_TYPE {
                        declared_type = node
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());
                    } else if declared_type.is_none() && node.kind() == SyntaxKind::PRIMITIVE_TYPE {
                        declared_type = node
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|t| t.kind() == SyntaxKind::IDENT)
                            .map(|t| t.text().to_string());
                    }
                }
                // Check if this is an array type (has DIMS node or [] in the type)
                if node.kind() == SyntaxKind::ARRAY_TYPE {
                    is_array_type = true;
                    past_type = true;
                }
            } else if let Some(token) = child.as_token() {
                // Check for array brackets
                if token.kind() == SyntaxKind::L_BRACKET {
                    is_array_type = true;
                }

                // Accept IDENT or keywords that can be used as identifiers (like "to", "from", etc.)
                // Stop at EQ token
                if token.kind() == SyntaxKind::EQ {
                    break;
                }
                if past_type && var_name.is_none() {
                    // Check if it's an identifier-like token (IDENT or certain keywords)
                    // Note: Some Java keywords can be used as variable names in certain contexts
                    let is_identifier_like =
                        token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::TO_KW;

                    if is_identifier_like {
                        var_name = Some(token.text().to_string());
                    }
                }
            }
        }

        // Check for sinks in the initialization expression (e.g., path.resolve(tainted))
        self.check_sink_pattern_in_children(node);

        // Check if the initialization expression is tainted
        // We need to look for method call patterns in the siblings
        let mut initializer_tainted: Option<TaintState> = self
            .taint_tracker
            .check_method_call_pattern_taint_in_children(node, &|n| self.node_location(n), &|n| {
                Self::get_code_snippet(n)
            });
        let mut is_prepared_statement = false;
        let mut is_numeric_conversion = false;
        let mut initializer_type_hint: Option<String> = None;

        // Also check individual expression nodes
        for child in node.children() {
            if is_expr_kind(child.kind()) {
                // Check if this creates a PreparedStatement
                if let Some(method_call) = MethodCallExpr::cast(child.clone()) {
                    if initializer_type_hint.is_none() {
                        if let Some(receiver_name) = Self::method_call_receiver_name(&method_call) {
                            if let Some(method_name) =
                                method_call.name().map(|t| t.text().to_string())
                            {
                                if matches!(
                                    method_name.as_str(),
                                    "newInstance" | "builder" | "create"
                                ) {
                                    initializer_type_hint = Some(receiver_name);
                                }
                            }
                        }
                    }
                    if self.type_analyzer.creates_prepared_statement(&method_call) {
                        is_prepared_statement = true;
                    } else if self.type_analyzer.is_sql_safe_type(&child) {
                        // Numeric conversions are safe - don't taint the variable
                        // This handles: int id = Integer.parseInt(tainted);
                        is_numeric_conversion = true;
                        initializer_tainted = None;
                    } else if let Some(taint) =
                        self.taint_tracker
                            .expr_taint(&child, &|n| self.node_location(n), &|n| {
                                Self::get_code_snippet(n)
                            })
                    {
                        initializer_tainted = Some(taint);
                    }
                } else if let Some(taint) =
                    self.taint_tracker
                        .expr_taint(&child, &|n| self.node_location(n), &|n| {
                            Self::get_code_snippet(n)
                        })
                {
                    initializer_tainted = Some(taint);
                } else if initializer_type_hint.is_none() && child.kind() == SyntaxKind::NEW_EXPR {
                    initializer_type_hint = Self::new_expr_class_name(&child);
                }
                self.visit_expr(&child);
            }
        }

        // JDBC: infer `var ps = conn.prepareStatement(...)` return type and propagate SQL-template taint.
        if initializer_type_hint.is_none() {
            if let Some(ty) = self.jdbc_prepare_return_type_in_node(node) {
                initializer_type_hint = Some(ty);
                is_prepared_statement = true;
            }
        }

        // Infer type from common factory method names when type is `var`
        // e.g., var con = getConnection(); -> Connection
        if initializer_type_hint.is_none() {
            if let Some(ty) = Self::infer_type_from_method_name_in_node(node) {
                initializer_type_hint = Some(ty);
            }
        }

        if let Some(sql_template_taint) = self.tainted_sql_template_from_jdbc_prepare_call(node) {
            initializer_tainted = Some(sql_template_taint);
        }

        // Also check for static method calls like Integer.parseInt() in siblings
        // For patterns like: int id = Integer.parseInt(tainted);
        // The structure is: CLASS_TYPE IDENT EQ NAME_EXPR DOT IDENT METHOD_CALL_EXPR
        if !is_numeric_conversion
            && initializer_tainted.is_none()
            && TaintTracker::is_numeric_type_conversion_in_siblings(node)
        {
            // Numeric conversion sanitizes the taint - no action needed
        }

        if let Some(name) = var_name {
            let inferred_type = declared_type
                .filter(|t| t != "var")
                .or(initializer_type_hint);
            if let Some(type_name) = inferred_type {
                self.taint_tracker.set_var_type(name.clone(), type_name);
            }
            if is_prepared_statement {
                #[cfg(test)]
                eprintln!("Marking {} as PreparedStatement", name);
                self.taint_tracker.mark_prepared_statement(name.clone());
            }
            if let Some(taint_state) = initializer_tainted {
                #[cfg(test)]
                eprintln!("Marking {} as tainted (array: {})", name, is_array_type);

                // For arrays, mark the entire array as tainted
                if is_array_type {
                    // Propagate taint through array initialization
                    let location = self.node_location(node);
                    let code = Self::get_code_snippet(node);
                    let propagated = taint_state.propagate(
                        location,
                        format!("Array '{name}' initialized with tainted data"),
                        code,
                    );
                    self.taint_tracker
                        .mark_whole_array_tainted(&name, propagated);
                } else {
                    // Regular variable assignment
                    let location = self.node_location(node);
                    let code = Self::get_code_snippet(node);
                    let propagated = taint_state.propagate(
                        location,
                        format!("Assigned to variable '{name}'"),
                        code,
                    );
                    self.taint_tracker.mark_tainted(name, propagated);
                }
            }
        }
    }

    /// Check for sink patterns like `receiver.method(args)` in nested METHOD_CALL_EXPR nodes.
    /// Also handles StringBuilder/StringBuffer append() tracking.
    fn check_sink_pattern_in_children(&mut self, node: &SyntaxNode) {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };
            #[cfg(test)]
            eprintln!(
                "Checking sink pattern: {:?}.{}",
                pattern.receiver_name, pattern.method_name
            );

            // Handle collection mutation tracking (not sinks, just taint propagation)
            if self.handle_collection_mutation(&pattern, node) {
                continue; // Tracked, but not a sink - move to next window
            }

            // Skip utility method calls that aren't sinks
            if Self::should_skip_method_call(&pattern) {
                continue;
            }

            // Try to find a sink pattern for this method call
            let Some(sink_pattern) = sink_detection::find_sink_pattern(
                &pattern,
                &self.scanner.sink_registry,
                &self.taint_tracker,
            ) else {
                continue; // Not a sink
            };

            let vuln_type = sink_pattern.vuln_type;
            debug!(
                receiver = ?pattern.receiver_name,
                method = %pattern.method_name,
                vuln_type = ?vuln_type,
                "Detected sink"
            );

            #[cfg(test)]
            eprintln!("  -> This is a sink for {:?}", vuln_type);

            // Check if we should skip this sink based on context
            if sink_detection::should_skip_sink(
                &pattern,
                sink_pattern,
                self.type_analyzer,
                &self.taint_tracker,
                &|n| Self::find_containing_method(n),
                &|r| self.is_file_output_context(r),
                &self.file,
            ) {
                continue;
            }

            // Collect tainted arguments
            let (tainted_args, taint_state_opt, arg_list_node) =
                sink_detection::collect_tainted_arguments(
                    pattern.call_node,
                    vuln_type,
                    &self.taint_tracker,
                    &is_expr_kind,
                    &Self::is_exception_logging_argument,
                    &|n| self.node_location(n),
                    &|n| Self::get_code_snippet(n),
                );
            // Check receiver taint if needed
            let receiver_taint = sink_detection::check_receiver_taint(
                sink_pattern,
                pattern.receiver_node.as_ref(),
                &self.taint_tracker,
                &|n| self.node_location(n),
                &|n| Self::get_code_snippet(n),
            );

            // Only create a finding if we have tainted data
            if tainted_args.is_empty() && receiver_taint.is_none() {
                continue;
            }

            info!(
                receiver = ?pattern.receiver_name,
                method = %pattern.method_name,
                vuln_type = ?vuln_type,
                tainted_args = ?tainted_args,
                "Found vulnerability: tainted data flows to sink"
            );

            // Create the finding
            let sink_context = sink_detection::SinkContext {
                tainted_args,
                taint_state: taint_state_opt,
                receiver_taint,
                arg_list_node,
            };

            let finding = sink_detection::create_sink_finding(
                &pattern,
                sink_pattern,
                &sink_context,
                self.scanner.logging_safety(),
                &|args, tainted| Self::is_parameterized_log_with_id_args(args, tainted),
                &|n| self.node_location(n),
                &|n| Self::get_code_snippet(n),
            );

            self.findings.push(finding);
        }
    }

    /// Handles collection mutation methods that propagate taint but aren't sinks
    ///
    /// Returns true if the method was handled (and should be skipped for sink detection)
    fn handle_collection_mutation(
        &mut self,
        pattern: &method_call_pattern::MethodCallPattern,
        parent_node: &SyntaxNode,
    ) -> bool {
        let Some(ref receiver_name) = pattern.receiver_name else {
            return false;
        };

        let handled = match pattern.method_name.as_str() {
            "append" => self.track_append_taint(receiver_name, pattern.call_node, parent_node),
            "add" => self.track_add_taint(receiver_name, pattern.call_node, parent_node),
            "set" => self.track_set_taint(receiver_name, pattern.call_node, parent_node),
            "put" => self.track_put_taint(receiver_name, pattern.call_node, parent_node),
            _ => false,
        };
        handled
    }

    fn track_append_taint(
        &mut self,
        receiver_name: &str,
        call_node: &SyntaxNode,
        parent_node: &SyntaxNode,
    ) -> bool {
        // Extract immutable data before mutable borrow
        let source = self.source;
        let file = &self.file;

        collection_taint::track_append_taint(
            receiver_name,
            call_node,
            parent_node,
            &mut self.taint_tracker,
            &is_expr_kind,
            &|n| location_from_node(n, source, file),
            &|n| code_snippet_from_node(n, source),
        )
    }

    fn track_add_taint(
        &mut self,
        receiver_name: &str,
        call_node: &SyntaxNode,
        parent_node: &SyntaxNode,
    ) -> bool {
        let source = self.source;
        let file = &self.file;

        collection_taint::track_add_taint(
            receiver_name,
            call_node,
            parent_node,
            &mut self.taint_tracker,
            &is_expr_kind,
            &|n| location_from_node(n, source, file),
            &|n| code_snippet_from_node(n, source),
        )
    }

    fn track_set_taint(
        &mut self,
        receiver_name: &str,
        call_node: &SyntaxNode,
        parent_node: &SyntaxNode,
    ) -> bool {
        let source = self.source;
        let file = &self.file;

        collection_taint::track_set_taint(
            receiver_name,
            call_node,
            parent_node,
            &mut self.taint_tracker,
            &is_expr_kind,
            &|n| location_from_node(n, source, file),
            &|n| code_snippet_from_node(n, source),
        )
    }

    fn track_put_taint(
        &mut self,
        receiver_name: &str,
        call_node: &SyntaxNode,
        parent_node: &SyntaxNode,
    ) -> bool {
        let source = self.source;
        let file = &self.file;

        collection_taint::track_put_taint(
            receiver_name,
            call_node,
            parent_node,
            &mut self.taint_tracker,
            &is_expr_kind,
            &|n| location_from_node(n, source, file),
            &|n| code_snippet_from_node(n, source),
        )
    }

    /// Checks if a method call should be skipped (utility methods, etc.)
    fn should_skip_method_call(pattern: &method_call_pattern::MethodCallPattern) -> bool {
        // Skip split() on StringUtils and Splitter - these are utility methods
        if pattern.method_name == "split" {
            if let Some(ref recv_name) = pattern.receiver_name {
                if recv_name.ends_with("StringUtils") || recv_name.ends_with("Splitter") {
                    return true;
                }
            }
        }
        false
    }

    fn new_expr_class_name(node: &SyntaxNode) -> Option<String> {
        if node.kind() != SyntaxKind::NEW_EXPR {
            return None;
        }

        for child in node.children() {
            if child.kind() == SyntaxKind::CLASS_TYPE {
                return child
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|t| t.kind() == SyntaxKind::IDENT)
                    .last()
                    .map(|t| t.text().to_string());
            }
        }

        None
    }

    fn method_call_receiver_name(call: &MethodCallExpr) -> Option<String> {
        call.receiver()
            .and_then(|receiver| Self::receiver_name_from_expr(receiver.syntax()))
    }

    fn receiver_name_from_expr(node: &SyntaxNode) -> Option<String> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => node
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string()),
            SyntaxKind::FIELD_ACCESS_EXPR => node
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .last()
                .map(|t| t.text().to_string()),
            _ => None,
        }
    }

    fn visit_expr(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::METHOD_CALL_EXPR => {
                self.check_method_call(node);
            }
            SyntaxKind::NEW_EXPR => {
                self.check_constructor_call(node);
            }
            SyntaxKind::ASSIGNMENT_EXPR => {
                self.visit_assignment(node);
            }
            SyntaxKind::BINARY_EXPR => {
                // Check for string concatenation that might flow to sinks
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        self.visit_expr(&child);
                    }
                }
            }
            SyntaxKind::LAMBDA_EXPR => {
                // Lambda expression: x -> expr or (x, y) -> expr
                // Phase 4.2: Visit lambda body for potential sinks
                self.visit_lambda_expr(node);
            }
            SyntaxKind::METHOD_REF_EXPR => {
                // Method references don't have bodies to visit, but they may appear as arguments
                // The taint tracking is handled in TaintTracker::expr_taint
            }
            _ => {
                // Visit children
                for child in node.children() {
                    if is_expr_kind(child.kind()) {
                        self.visit_expr(&child);
                    }
                }
            }
        }
    }

    fn is_parameterized_log_with_id_args(arg_list: &SyntaxNode, tainted_args: &[usize]) -> bool {
        if tainted_args.is_empty() {
            return false;
        }

        let args: Vec<SyntaxNode> = arg_list
            .children()
            .filter(|child| is_expr_kind(child.kind()))
            .collect();

        let first = match args.first() {
            Some(node) => node,
            None => return false,
        };

        if first.kind() != SyntaxKind::LITERAL_EXPR {
            return false;
        }

        let first_text = first.text().to_string();
        if !first_text.contains("{}") {
            return false;
        }

        for idx in tainted_args {
            let Some(arg) = args.get(*idx) else {
                return false;
            };
            if !looks_like_identifier_expr(arg.text().to_string().as_str()) {
                return false;
            }
        }

        true
    }

    fn visit_assignment(&mut self, node: &SyntaxNode) {
        // ASSIGNMENT_EXPR only contains EQ + RHS; LHS is a preceding sibling.
        // Parent (EXPR_STMT) structure:
        //   FIELD_ACCESS_EXPR (or NAME_EXPR) ← LHS sibling
        //   ASSIGNMENT_EXPR                  ← this node (contains EQ + RHS)

        tracing::debug!("visit_assignment called");

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
                    tracing::debug!("Found FIELD_ACCESS_EXPR as LHS");
                    // With nested trees, FIELD_ACCESS_EXPR contains THIS_EXPR + DOT + IDENT
                    field_name = lhs
                        .children_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .last()
                        .map(|t| t.text().to_string());
                    tracing::debug!(field = ?field_name, "Extracted field name");
                } else if lhs.kind() == SyntaxKind::NAME_EXPR {
                    tracing::debug!("Found NAME_EXPR as LHS");
                    var_name = Self::get_var_name(lhs);
                    tracing::debug!(var_name = ?var_name, "Variable name from NAME_EXPR");
                    if let Some(ref name) = var_name {
                        field_name = Some(name.clone());
                        tracing::debug!(field = %name, "Setting field_name");
                    }
                }
            }
        }

        // Check if RHS is tainted
        tracing::debug!("Checking if RHS is tainted");

        let mut rhs_taint = self
            .taint_tracker
            .check_method_call_pattern_taint_in_children(node, &|n| self.node_location(n), &|n| {
                Self::get_code_snippet(n)
            });

        if rhs_taint.is_none() {
            if let Some(rhs_node) = rhs {
                tracing::debug!("No method call pattern found, checking child expression");
                rhs_taint =
                    self.taint_tracker
                        .expr_taint(rhs_node, &|n| self.node_location(n), &|n| {
                            Self::get_code_snippet(n)
                        });
            }
        }

        // JDBC: infer `var stmt; stmt = conn.prepareStatement(...)` type and propagate SQL-template taint.
        if let Some(jdbc_ty) = self.jdbc_prepare_return_type_in_node(node) {
            if let Some(ref name) = var_name {
                if self.taint_tracker.get_var_type(name).is_none() {
                    self.taint_tracker.set_var_type(name.clone(), jdbc_ty);
                }
            }
        }

        if let Some(sql_template_taint) = self.tainted_sql_template_from_jdbc_prepare_call(node) {
            rhs_taint = Some(sql_template_taint);
        }

        if let Some(taint) = rhs_taint {
            tracing::debug!(field_name = ?field_name, var_name = ?var_name, "RHS is tainted");
            let location = self.node_location(node);
            let code = Self::get_code_snippet(node);

            if let Some(field) = field_name {
                tracing::debug!(field = %field, "Marking field as tainted");
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

        // Visit RHS
        if let Some(rhs_node) = rhs {
            self.visit_expr(rhs_node);
        }
    }

    pub(super) fn get_var_name(node: &SyntaxNode) -> Option<String> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            return Some(token.text().to_string());
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn visit_lambda_expr(&mut self, node: &SyntaxNode) {
        // Lambda expression: x -> expr or (x, y) -> expr
        // Phase 4.2: Visit lambda body for sinks
        //
        // Strategy:
        // 1. Lambda parameters shadow outer scope variables (handled by parser)
        // 2. Lambda can capture tainted variables from outer scope
        // 3. Visit lambda body for sinks (e.g., lambda that calls a sink with captured tainted data)

        #[cfg(test)]
        eprintln!("=== Visiting lambda expression ===");
        #[cfg(test)]
        eprintln!("Lambda text: {}", node.text());

        // A stream terminal lambda receives the elements produced by its
        // receiver pipeline. If a preceding map/flatMap captured tainted data,
        // bind that taint to the terminal lambda parameters while its body is
        // analyzed. The binding is lexical and restored afterwards.
        let parameter_names = lambda_parameter_names(node);
        let input_taint = self.stream_terminal_input_taint(node);
        let previous_bindings = input_taint.as_ref().map(|taint| {
            parameter_names
                .iter()
                .map(|name| {
                    let propagated = taint.propagate(
                        self.node_location(node),
                        format!("Stream pipeline element bound to lambda parameter '{name}'"),
                        Self::get_code_snippet(node),
                    );
                    let previous = self.taint_tracker.replace_taint(name, Some(propagated));
                    (name.clone(), previous)
                })
                .collect::<Vec<_>>()
        });

        // Visit all child expressions and statements in the lambda body
        // The lambda body can be:
        // - A single expression: x -> x + 1
        // - A block with statements: x -> { return x + 1; }
        for child in node.descendants() {
            if is_stmt_kind(child.kind()) {
                self.visit_stmt(&child);
            } else if is_expr_kind(child.kind()) && child != *node {
                // Don't re-visit the lambda expr itself
                self.visit_expr(&child);
            }
        }

        if let Some(previous_bindings) = previous_bindings {
            for (name, previous) in previous_bindings {
                self.taint_tracker.replace_taint(&name, previous);
            }
        }
    }

    fn stream_terminal_input_taint(&self, lambda: &SyntaxNode) -> Option<TaintState> {
        let arg_list = lambda.parent()?;
        if arg_list.kind() != SyntaxKind::ARG_LIST {
            return None;
        }
        let terminal_call = arg_list.parent()?;
        let pattern = method_call_pattern::extract_from_nested(&terminal_call)?;
        if !matches!(pattern.method_name.as_str(), "forEach" | "forEachOrdered") {
            return None;
        }

        let receiver = pattern.receiver_node?;
        let source = self.source;
        let file = &self.file;
        self.taint_tracker.expr_taint(
            &receiver,
            &|node| location_from_node(node, source, file),
            &|node| code_snippet_from_node(node, source),
        )
    }

    fn check_method_call(&mut self, node: &SyntaxNode) {
        let Some(call) = MethodCallExpr::cast(node.clone()) else {
            return;
        };

        // Check if this method call itself is a sink by examining the parent's sibling structure
        // This handles nested sinks like: consume(stmt.executeQuery(tainted))
        // where executeQuery is inside consume's ARG_LIST
        if let Some(parent) = node.parent() {
            // The parent has the sibling structure containing: receiver, DOT, method_name, METHOD_CALL_EXPR
            self.check_sink_pattern_in_children(&parent);
        }

        // Then, visit arguments (they might be sinks themselves)
        if let Some(args) = call.args() {
            for arg in args.args() {
                self.visit_expr(arg.syntax());
            }
        }
    }

    fn check_constructor_call(&mut self, node: &SyntaxNode) {
        // Extract constructor class name from NEW_EXPR
        // NEW_EXPR structure: new ClassName(args) or new pkg.ClassName(args)
        let mut class_name: Option<String> = None;
        let mut arg_list: Option<SyntaxNode> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::CLASS_TYPE => {
                    // Extract the simple class name from the type
                    // CLASS_TYPE contains QUALIFIED_NAME which contains IDENT tokens
                    class_name = child
                        .descendants_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .last() // Get the last IDENT (the simple class name, not the package)
                        .map(|t| t.text().to_string());
                }
                SyntaxKind::ARG_LIST => {
                    arg_list = Some(child.clone());
                }
                _ => {}
            }
        }

        // First, visit arguments (they might be sinks themselves)
        if let Some(ref args) = arg_list {
            for arg in args.children() {
                if is_expr_kind(arg.kind()) {
                    self.visit_expr(&arg);
                }
            }
        }

        let Some(class) = class_name else {
            return;
        };

        // Determine what vulnerability type this constructor might be a sink for
        let vuln_type = if matches!(
            class.as_str(),
            "File"
                | "FileInputStream"
                | "FileOutputStream"
                | "FileReader"
                | "FileWriter"
                | "RandomAccessFile"
        ) {
            Some(VulnType::PathTraversal)
        } else if matches!(class.as_str(), "RedirectView") {
            // new RedirectView(url) - Open Redirect
            Some(VulnType::OpenRedirect)
        } else if class.as_str() == "ModelAndView" {
            // new ModelAndView("redirect:" + url) - need to check if arg contains "redirect:"
            // We'll check for tainted args, but the user must use string concat with "redirect:"
            Some(VulnType::OpenRedirect)
        } else if matches!(class.as_str(), "URL" | "URI") {
            // new URL(taintedString) or new URI(taintedString) - SSRF sink
            // The URL/URI constructor creates a URL object that can be used to make requests
            Some(VulnType::Ssrf)
        } else {
            None
        };

        let Some(vuln) = vuln_type else {
            return;
        };

        // Check if any argument is tainted
        let mut tainted_args = Vec::new();
        let mut taint_state_opt: Option<TaintState> = None;

        if let Some(args) = arg_list {
            for (idx, arg) in args.children().enumerate() {
                if is_expr_kind(arg.kind()) {
                    if let Some(taint) =
                        self.taint_tracker
                            .expr_taint(&arg, &|n| self.node_location(n), &|n| {
                                Self::get_code_snippet(n)
                            })
                    {
                        if taint.is_dangerous_for(vuln) {
                            tainted_args.push(idx);
                            if taint_state_opt.is_none() {
                                taint_state_opt = Some(taint);
                            }
                        }
                    }
                }
            }
        }

        if !tainted_args.is_empty() {
            let location = self.node_location(node);
            let code = Self::get_code_snippet(node);
            let message = format!(
                "Potential {} in constructor new {}() - tainted data in argument(s) {:?}",
                vuln.description(),
                class,
                tainted_args
            );

            info!(
                class = %class,
                vuln_type = ?vuln,
                tainted_args = ?tainted_args,
                "Found vulnerability in constructor"
            );

            let mut finding = Finding::new(vuln, message, location.clone(), code)
                .with_rule_id(format!("taint:{}", vuln.as_str()));
            finding.set_sink_kind(Some(format!("new {class}")));

            // Add data flow path if we have taint state
            if let Some(taint_state) = taint_state_opt {
                if let Some(path) = taint_state.path() {
                    finding.dataflow = path
                        .iter()
                        .map(super::super::taint::state::FlowStep::to_finding_step)
                        .collect();
                }
                finding.sink_location = Some(location);
                finding.confidence = confidence_for_taint(&taint_state);
                finding.set_source_kind(taint_state.source_kind().map(|kind| format!("{kind:?}")));
            }

            self.findings.push(finding);
        }
    }
}
