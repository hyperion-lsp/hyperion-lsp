// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Context analysis methods for SecurityVisitor.
//!
//! This module contains methods that analyze the surrounding context of AST nodes
//! to reduce false positives: JDBC connection detection, exception logging patterns,
//! file output context detection, type inference from method names, and method containment.

use crate::finding::VulnType;
use crate::taint::{TaintState, VulnTypes};

use hyperion_parser::syntax::{AstNode, MethodCallExpr, SyntaxKind, SyntaxNode};

use super::method_call_pattern;
use super::utilities::is_expr_kind;
use super::visitor::SecurityVisitor;

impl SecurityVisitor<'_> {
    pub(super) fn is_connection_like_receiver(&self, receiver: &str) -> bool {
        if receiver.ends_with("Connection") {
            return true;
        }

        if let Some(ty) = self.taint_tracker.get_var_type(receiver) {
            if ty.ends_with("Connection") {
                return true;
            }
        }

        // Only use name-based fallbacks for common, unambiguous aliases.
        matches!(receiver, "conn" | "con" | "connection")
    }

    /// If this node contains a JDBC `Connection.prepareStatement/prepareCall(sql)` call where `sql` is tainted
    /// for SQL injection, return a propagated taint state representing the SQL-template taint.
    pub(super) fn tainted_sql_template_from_jdbc_prepare_call(
        &self,
        node: &SyntaxNode,
    ) -> Option<TaintState> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            if !matches!(
                pattern.method_name.as_str(),
                "prepareStatement" | "prepareCall"
            ) {
                continue;
            }

            let Some(ref receiver_name) = pattern.receiver_name else {
                continue;
            };

            if !self.is_connection_like_receiver(receiver_name) {
                continue;
            }

            let Some(arg_list) = pattern
                .call_node
                .children()
                .find(|c| c.kind() == SyntaxKind::ARG_LIST)
            else {
                continue;
            };

            let Some(sql_arg) = arg_list.children().find(|c| is_expr_kind(c.kind())) else {
                continue;
            };

            let Some(taint) =
                self.taint_tracker
                    .expr_taint(&sql_arg, &|n| self.node_location(n), &|n| {
                        Self::get_code_snippet(n)
                    })
            else {
                continue;
            };

            if !taint.is_dangerous_for(VulnType::SqlInjection) {
                continue;
            }

            let sql_only = taint.sanitize(VulnTypes::all_vulns() & !VulnTypes::SQL_INJECTION);

            let location = self.node_location(node);
            let code = Self::get_code_snippet(node);
            return Some(sql_only.propagate(
                location,
                format!(
                    "Tainted SQL passed to {}.{}",
                    receiver_name, pattern.method_name
                ),
                code,
            ));
        }

        None
    }

    /// Infer type from common factory method names for `var` declarations.
    ///
    /// This handles patterns like:
    /// - `var con = getConnection()` -> Connection
    /// - `var con = ds.getConnection()` -> Connection
    /// - `var stmt = con.createStatement()` -> Statement
    pub(super) fn infer_type_from_method_name_in_node(node: &SyntaxNode) -> Option<String> {
        // With nested trees, find all METHOD_CALL_EXPR descendants and check method names
        for call_node in method_call_pattern::find_method_calls(node) {
            if let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) {
                match pattern.method_name.as_str() {
                    "getConnection" => return Some("Connection".to_string()),
                    "createStatement" | "getStatement" => {
                        return Some("Statement".to_string());
                    }
                    "getResultSet" | "executeQuery" => {
                        return Some("ResultSet".to_string());
                    }
                    "getMetaData" => return Some("ResultSetMetaData".to_string()),
                    _ => {}
                }
            }
        }

        None
    }

    /// Best-effort inference for JDBC prepare* return types inside a node.
    ///
    /// This is primarily used to support `var ps = conn.prepareStatement(...)` so we can still
    /// select the correct sink patterns later.
    pub(super) fn jdbc_prepare_return_type_in_node(&self, node: &SyntaxNode) -> Option<String> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            let Some(ref receiver_name) = pattern.receiver_name else {
                continue;
            };

            if !self.is_connection_like_receiver(receiver_name) {
                continue;
            }

            match pattern.method_name.as_str() {
                "prepareStatement" => return Some("PreparedStatement".to_string()),
                "prepareCall" => return Some("CallableStatement".to_string()),
                _ => {}
            }
        }

        None
    }

    /// Check if an expression represents exception-derived data that is safe for logging.
    ///
    /// Exception logging is generally safe because:
    /// 1. Exception messages are rarely attacker-controlled
    /// 2. Even if exception messages contain user data, they're typically from internal operations
    /// 3. Logging exceptions is a critical debugging practice that we shouldn't discourage
    ///
    /// This function detects:
    /// - Variables named e, ex, exception, error, err, throwable, cause, etc.
    /// - Method calls like .getMessage(), .toString(), .getCause(), .getStackTrace()
    /// - Exception objects passed directly to logging methods
    pub(super) fn is_exception_logging_pattern(node: &SyntaxNode) -> bool {
        // Check for common exception variable name patterns
        let exception_var_names = [
            "e",
            "ex",
            "exception",
            "error",
            "err",
            "throwable",
            "cause",
            "exc",
            "t",
            "th",
            "failure",
            "fault",
        ];

        // Check for exception-related method calls
        let exception_methods = [
            "getMessage",
            "toString",
            "getCause",
            "getStackTrace",
            "getLocalizedMessage",
            "printStackTrace",
            "exception",
            "getSuppressed",
            "fillInStackTrace",
        ];

        // Check the expression structure
        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                // Check if variable name matches exception patterns
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            let name = token.text().to_lowercase();
                            // Check exact matches
                            if exception_var_names.contains(&name.as_str()) {
                                return true;
                            }
                            // Check if name contains exception-related keywords
                            if name.contains("exception")
                                || name.contains("error")
                                || name.contains("throwable")
                                || name.contains("cause")
                            {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            SyntaxKind::METHOD_CALL_EXPR => {
                // Check if this is a call to an exception method like .getMessage()
                if let Some(call) = MethodCallExpr::cast(node.clone()) {
                    if let Some(method_name) = call.name() {
                        let method_str = method_name.text();
                        if exception_methods.contains(&method_str) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => {
                // Check if the text of the node contains exception patterns
                let text = node.text().to_string().to_lowercase();

                // Check for method calls on exception variables in flat structure
                // Pattern: exception.getMessage() or e.toString()
                for var_name in &exception_var_names {
                    for method in &exception_methods {
                        let pattern = format!("{}.{}", var_name, method.to_lowercase());
                        if text.contains(&pattern) {
                            return true;
                        }
                    }
                }

                // Also check for direct exception reference
                for var_name in &exception_var_names {
                    if text == *var_name {
                        return true;
                    }
                }

                false
            }
        }
    }

    /// Check if an argument in a log call appears to be exception-derived.
    /// This handles the common patterns:
    /// - log.error("message", exception);  // exception object as last arg
    /// - log.error("Error: " + e.getMessage());  // getMessage() call
    /// - log.debug("Failed", future.exception());  // exception() method call
    pub(super) fn is_exception_logging_argument(arg_node: &SyntaxNode) -> bool {
        // Direct check on the argument node
        if Self::is_exception_logging_pattern(arg_node) {
            return true;
        }

        // Check all descendants for exception patterns
        for desc in arg_node.descendants() {
            if Self::is_exception_logging_pattern(&desc) {
                return true;
            }
        }

        // Check the parent structure for patterns like `receiver.exception()`
        // This handles cases where the method name itself is "exception"
        if let Some(parent) = arg_node.parent() {
            for call_node in method_call_pattern::find_method_calls(&parent) {
                if let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) {
                    let method_lower = pattern.method_name.to_lowercase();
                    if method_lower == "exception"
                        || method_lower == "getmessage"
                        || method_lower == "getcause"
                        || method_lower == "getstacktrace"
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check if the context suggests file output rather than HTTP output.
    /// Used to filter out XSS false positives when output goes to files instead of HTTP responses.
    ///
    /// Returns true if:
    /// - The receiver variable name contains file-related patterns (file, output, stream, writer but not servlet)
    /// - The containing class name contains "File" (checked via file path)
    /// - The class name suggests file operations (FileStream, FileWriter, etc.)
    pub(super) fn is_file_output_context(&self, receiver_name: Option<&str>) -> bool {
        // Check receiver variable name for file-related patterns
        if let Some(recv) = receiver_name {
            let recv_lower = recv.to_lowercase();

            // Explicit HTTP patterns - these are NOT file outputs
            if recv_lower.contains("servlet")
                || recv_lower.contains("response")
                || recv_lower.contains("http")
            {
                return false;
            }

            // File-related receiver names
            if recv_lower.contains("file")
                || recv_lower.contains("fileoutput")
                || recv_lower.contains("filewriter")
                || recv_lower.contains("filestream")
            {
                return true;
            }
        }

        // Check file path for file-related class names
        // This catches classes like "FileStreamSinkTask", "FileWriter", etc.
        let file_name = self.file.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        let file_lower = file_name.to_lowercase();

        // HTTP-related class names are NOT file outputs
        if file_lower.contains("servlet")
            || file_lower.contains("controller")
            || file_lower.contains("handler")
            || file_lower.contains("endpoint")
            || file_lower.contains("resource")
        {
            return false;
        }

        // File-related class names
        if file_lower.contains("file")
            || file_lower.contains("stream")
                && !file_lower.contains("servlet")
                && !file_lower.contains("http")
        {
            return true;
        }

        false
    }

    /// Find the containing method declaration for a given node
    pub(super) fn find_containing_method(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut current = node.parent()?;

        loop {
            if current.kind() == SyntaxKind::METHOD_DECL {
                return Some(current);
            }

            current = current.parent()?;
        }
    }
}
