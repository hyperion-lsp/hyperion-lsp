// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbolic taint tracker for method summary generation.
//!
//! Tracks how taint flows through a method symbolically, determining
//! which parameters influence the return value without concrete taint values.

use crate::constant_eval::{ConstantEvaluator, ConstantValue};
use crate::symbolic_taint::{SymbolicTaint, TaintOrigins};
use crate::taint::VulnTypes;
use hyperion_parser::{SyntaxKind, SyntaxNode};
use std::collections::HashMap;

/// Tracks symbolic taint during method summary generation.
///
/// Unlike the concrete `TaintTracker`, this tracks which parameters
/// *could* influence values, enabling generation of method summaries
/// that can be applied across call sites.
pub struct SymbolicTracker {
    /// Variable name -> symbolic taint state.
    vars: HashMap<String, SymbolicTaint>,
    /// Field name -> symbolic taint state (for `this.field`).
    fields: HashMap<String, SymbolicTaint>,
    /// Evaluator for constant expressions.
    constant_eval: ConstantEvaluator,
    /// Parameter names in order.
    param_names: Vec<String>,
    /// Default vulnerability types for parameter taint.
    default_vuln_types: VulnTypes,
}

impl SymbolicTracker {
    /// Create a new symbolic tracker with parameter names.
    #[must_use]
    pub fn new(param_names: Vec<String>) -> Self {
        Self::with_vuln_types(param_names, VulnTypes::all_vulns())
    }

    /// Create a new symbolic tracker with specific vulnerability types.
    #[must_use]
    pub fn with_vuln_types(param_names: Vec<String>, default_vuln_types: VulnTypes) -> Self {
        let mut vars = HashMap::new();

        // Initialize parameters as tainted with their index
        for (idx, name) in param_names.iter().enumerate() {
            vars.insert(
                name.clone(),
                SymbolicTaint::from_param(idx, default_vuln_types),
            );
        }

        Self {
            vars,
            fields: HashMap::new(),
            constant_eval: ConstantEvaluator::new(),
            param_names,
            default_vuln_types,
        }
    }

    /// Create a symbolic tracker with pre-defined constants.
    #[must_use]
    pub fn with_constants(
        param_names: Vec<String>,
        constants: HashMap<String, ConstantValue>,
    ) -> Self {
        let mut tracker = Self::new(param_names);
        tracker.constant_eval = ConstantEvaluator::with_constants(constants);
        tracker
    }

    /// Get the parameter names.
    #[must_use]
    pub fn param_names(&self) -> &[String] {
        &self.param_names
    }

    /// Track an assignment, checking for constants first.
    pub fn track_assignment(&mut self, var_name: &str, expr: &SyntaxNode) {
        // First, check if the expression is a constant
        if self.constant_eval.evaluate(expr).is_some() {
            self.vars
                .insert(var_name.to_string(), SymbolicTaint::constant());
            return;
        }

        // Otherwise, evaluate the expression's taint symbolically
        let taint = self.evaluate_expr_taint(expr);
        self.vars.insert(var_name.to_string(), taint);
    }

    /// Track a field assignment.
    pub fn track_field_assignment(&mut self, field_name: &str, expr: &SyntaxNode) {
        if self.constant_eval.evaluate(expr).is_some() {
            self.fields
                .insert(field_name.to_string(), SymbolicTaint::constant());
            return;
        }

        let taint = self.evaluate_expr_taint(expr);
        self.fields.insert(field_name.to_string(), taint);
    }

    /// Get symbolic taint for a return expression.
    #[must_use]
    pub fn get_return_taint(&self, return_expr: &SyntaxNode) -> SymbolicTaint {
        // Check if return value is constant
        if self.constant_eval.evaluate(return_expr).is_some() {
            return SymbolicTaint::constant();
        }

        self.evaluate_expr_taint(return_expr)
    }

    /// Get taint for a variable.
    #[must_use]
    pub fn get_var_taint(&self, var_name: &str) -> Option<&SymbolicTaint> {
        self.vars.get(var_name)
    }

    /// Get taint for a field.
    #[must_use]
    pub fn get_field_taint(&self, field_name: &str) -> Option<&SymbolicTaint> {
        self.fields.get(field_name)
    }

    /// Set taint for a variable directly.
    pub fn set_var_taint(&mut self, var_name: &str, taint: SymbolicTaint) {
        self.vars.insert(var_name.to_string(), taint);
    }

    /// Evaluate expression taint symbolically.
    fn evaluate_expr_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        match expr.kind() {
            // Literals are clean (constants handled separately)
            SyntaxKind::LITERAL_EXPR
            | SyntaxKind::STRING_LITERAL
            | SyntaxKind::INT_LITERAL
            | SyntaxKind::LONG_LITERAL
            | SyntaxKind::FLOAT_LITERAL
            | SyntaxKind::DOUBLE_LITERAL
            | SyntaxKind::CHAR_LITERAL
            | SyntaxKind::BOOLEAN_LITERAL
            | SyntaxKind::NULL_LITERAL
            | SyntaxKind::TEXT_BLOCK_LITERAL => SymbolicTaint::constant(),

            // Name expression - look up variable
            SyntaxKind::NAME_EXPR => {
                extract_identifier(expr).map_or_else(SymbolicTaint::clean, |name| {
                    self.vars
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(SymbolicTaint::clean)
                })
            }

            // Field access - could be this.field or other.field
            SyntaxKind::FIELD_ACCESS_EXPR => self.evaluate_field_access_taint(expr),

            // Binary expression - merge operand taints
            SyntaxKind::BINARY_EXPR => self.evaluate_binary_expr_taint(expr),

            // Unary, parenthesized, and cast expressions - propagate operand taint
            SyntaxKind::UNARY_EXPR | SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR => {
                for child in expr.children() {
                    if is_expression_kind(child.kind()) {
                        return self.evaluate_expr_taint(&child);
                    }
                }
                SymbolicTaint::clean()
            }

            // Ternary expression - merge both branches
            SyntaxKind::TERNARY_EXPR => self.evaluate_ternary_expr_taint(expr),

            // Method call - for now, assume it propagates taint from receiver and args
            SyntaxKind::METHOD_CALL_EXPR => self.evaluate_method_call_taint(expr),

            // New expression - generally clean unless tainted args
            SyntaxKind::NEW_EXPR => self.evaluate_new_expr_taint(expr),

            // Array access - merge array and index taint
            SyntaxKind::ARRAY_ACCESS_EXPR => self.evaluate_array_access_taint(expr),

            // Assignment expression - return the assigned value's taint
            SyntaxKind::ASSIGNMENT_EXPR => self.evaluate_assignment_expr_taint(expr),

            // Lambda/method reference, this/super, and all other kinds - treat as clean
            _ => SymbolicTaint::clean(),
        }
    }

    /// Evaluate taint for field access expressions.
    fn evaluate_field_access_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut receiver_taint = SymbolicTaint::clean();
        let mut field_name = None;

        for child in expr.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::IDENT {
                        field_name = Some(token.text().to_string());
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    if node.kind() == SyntaxKind::THIS_EXPR {
                        // this.field - look up in fields map
                        if let Some(ref name) = field_name {
                            if let Some(taint) = self.fields.get(name) {
                                return taint.clone();
                            }
                        }
                    } else if is_expression_kind(node.kind()) {
                        receiver_taint = self.evaluate_expr_taint(&node);
                    }
                }
            }
        }

        // If receiver is tainted, the field access propagates taint
        if receiver_taint.is_tainted() {
            receiver_taint.propagate()
        } else {
            // Check if field name matches a tracked variable
            field_name.map_or_else(SymbolicTaint::clean, |name| {
                self.vars
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(SymbolicTaint::clean)
            })
        }
    }

    /// Evaluate taint for binary expressions.
    fn evaluate_binary_expr_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut result = SymbolicTaint::clean();

        for child in expr.children() {
            if is_expression_kind(child.kind()) {
                let child_taint = self.evaluate_expr_taint(&child);
                result = result.merge(&child_taint);
            }
        }

        result
    }

    /// Evaluate taint for ternary expressions.
    fn evaluate_ternary_expr_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut exprs = Vec::new();

        for child in expr.children() {
            if is_expression_kind(child.kind()) {
                exprs.push(child);
            }
        }

        // Ternary has 3 parts: condition, then, else
        // Return taint is merge of then and else branches
        if exprs.len() >= 3 {
            let then_taint = self.evaluate_expr_taint(&exprs[1]);
            let else_taint = self.evaluate_expr_taint(&exprs[2]);
            then_taint.merge(&else_taint)
        } else if exprs.len() == 2 {
            // Partial ternary

            self.evaluate_expr_taint(&exprs[1])
        } else {
            SymbolicTaint::clean()
        }
    }

    /// Evaluate taint for method call expressions.
    fn evaluate_method_call_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut result = SymbolicTaint::clean();

        // Get receiver taint
        for child in expr.children() {
            match child.kind() {
                SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR => {
                    let receiver_taint = self.evaluate_expr_taint(&child);
                    result = result.merge(&receiver_taint);
                }
                SyntaxKind::ARG_LIST => {
                    // Merge taint from all arguments
                    for arg in child.children() {
                        if is_expression_kind(arg.kind()) {
                            let arg_taint = self.evaluate_expr_taint(&arg);
                            result = result.merge(&arg_taint);
                        }
                    }
                }
                _ => {}
            }
        }

        // If everything is constant, the call might still be constant
        // (e.g., "foo".toLowerCase())
        if !result.is_tainted() && self.constant_eval.evaluate(expr).is_some() {
            return SymbolicTaint::constant();
        }

        result
    }

    /// Evaluate taint for new expressions.
    fn evaluate_new_expr_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut result = SymbolicTaint::clean();

        // Check constructor arguments
        for child in expr.children() {
            if child.kind() == SyntaxKind::ARG_LIST {
                for arg in child.children() {
                    if is_expression_kind(arg.kind()) {
                        let arg_taint = self.evaluate_expr_taint(&arg);
                        result = result.merge(&arg_taint);
                    }
                }
            }
        }

        result
    }

    /// Evaluate taint for array access expressions.
    fn evaluate_array_access_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        let mut result = SymbolicTaint::clean();

        for child in expr.children() {
            if is_expression_kind(child.kind()) {
                let child_taint = self.evaluate_expr_taint(&child);
                result = result.merge(&child_taint);
            }
        }

        result
    }

    /// Evaluate taint for assignment expressions.
    fn evaluate_assignment_expr_taint(&self, expr: &SyntaxNode) -> SymbolicTaint {
        // Find the right-hand side expression (after the operator)
        let mut found_operator = false;

        for child in expr.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    // Assignment operators
                    if matches!(
                        token.kind(),
                        SyntaxKind::EQ
                            | SyntaxKind::PLUS_EQ
                            | SyntaxKind::MINUS_EQ
                            | SyntaxKind::STAR_EQ
                            | SyntaxKind::SLASH_EQ
                    ) {
                        found_operator = true;
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    if found_operator && is_expression_kind(node.kind()) {
                        return self.evaluate_expr_taint(&node);
                    }
                }
            }
        }

        SymbolicTaint::clean()
    }

    /// Add a source origin to a variable.
    pub fn add_source_origin(&mut self, var_name: &str, source: TaintOrigins) {
        let taint = self
            .vars
            .entry(var_name.to_string())
            .or_insert_with(SymbolicTaint::clean);

        *taint = SymbolicTaint {
            origins: taint.origins | source,
            dangerous_for: taint.dangerous_for | self.default_vuln_types,
            is_constant: false,
        };
    }
}

// Helper functions

fn extract_identifier(expr: &SyntaxNode) -> Option<String> {
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

const fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_method_body(code: &str) -> SyntaxNode {
        let wrapped = format!("class Test {{ void test() {{ {} }} }}", code);
        let tokens = Lexer::new(&wrapped).collect();
        let (green, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green)
    }

    #[test]
    fn test_param_taint_propagation() {
        let tracker = SymbolicTracker::new(vec!["input".to_string()]);

        // Parameter should be tainted
        let taint = tracker.get_var_taint("input").unwrap();
        assert!(taint.is_tainted());
        assert!(taint.origins.contains(TaintOrigins::PARAM_0));
    }

    #[test]
    fn test_constant_does_not_taint() {
        let root = parse_method_body(r#"String x = "constant"; return x;"#);
        let mut tracker = SymbolicTracker::new(vec![]);

        // Find the assignment and track it
        for node in root.descendants() {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                let mut var_name = None;
                let mut init_expr = None;

                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Token(token) => {
                            if token.kind() == SyntaxKind::IDENT && var_name.is_none() {
                                var_name = Some(token.text().to_string());
                            }
                        }
                        rowan::NodeOrToken::Node(n) => {
                            if is_expression_kind(n.kind()) {
                                init_expr = Some(n);
                            }
                        }
                    }
                }

                if let (Some(name), Some(expr)) = (var_name, init_expr) {
                    tracker.track_assignment(&name, &expr);
                }
            }
        }

        // x should be constant/clean
        let taint = tracker.get_var_taint("x");
        assert!(taint.is_some());
        assert!(taint.unwrap().is_constant);
    }

    #[test]
    fn test_taint_through_assignment() {
        let root = parse_method_body("String x = input; return x;");
        let mut tracker = SymbolicTracker::new(vec!["input".to_string()]);

        // Find and track assignments
        for node in root.descendants() {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                let mut var_name = None;
                let mut init_expr = None;

                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Token(token) => {
                            if token.kind() == SyntaxKind::IDENT && var_name.is_none() {
                                var_name = Some(token.text().to_string());
                            }
                        }
                        rowan::NodeOrToken::Node(n) => {
                            if is_expression_kind(n.kind()) {
                                init_expr = Some(n);
                            }
                        }
                    }
                }

                if let (Some(name), Some(expr)) = (var_name, init_expr) {
                    tracker.track_assignment(&name, &expr);
                }
            }
        }

        // x should be tainted (from input)
        let taint = tracker.get_var_taint("x");
        assert!(taint.is_some());
        assert!(taint.unwrap().is_tainted());
    }

    #[test]
    fn test_taint_merge_in_concat() {
        let tracker = SymbolicTracker::new(vec!["a".to_string(), "b".to_string()]);

        // Both a and b should be tainted with different param indices
        let taint_a = tracker.get_var_taint("a").unwrap();
        let taint_b = tracker.get_var_taint("b").unwrap();

        assert!(taint_a.origins.contains(TaintOrigins::PARAM_0));
        assert!(taint_b.origins.contains(TaintOrigins::PARAM_1));

        // Merge should combine both
        let merged = taint_a.merge(taint_b);
        assert!(merged.origins.contains(TaintOrigins::PARAM_0));
        assert!(merged.origins.contains(TaintOrigins::PARAM_1));
    }

    #[test]
    fn test_source_detection() {
        let mut tracker = SymbolicTracker::new(vec![]);

        // Add HTTP source to a variable
        tracker.add_source_origin("request", TaintOrigins::SOURCE_HTTP);

        let taint = tracker.get_var_taint("request").unwrap();
        assert!(taint.is_tainted());
        assert!(taint.origins.contains(TaintOrigins::SOURCE_HTTP));
    }
}
