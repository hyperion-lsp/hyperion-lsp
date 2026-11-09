// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Dangerous API usage checks

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use crate::scanner::method_call_pattern;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Check for Thread.stop(), suspend(), resume() usage
pub struct ThreadStopCheck;

impl Check for ThreadStopCheck {
    fn name(&self) -> &'static str {
        "thread-stop"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            let dangerous_methods = ["stop", "suspend", "resume", "destroy"];
            if !dangerous_methods.contains(&pattern.method_name.as_str()) {
                continue;
            }

            // Only flag if receiver looks like a Thread
            let is_thread_receiver = pattern.receiver_name.as_ref().is_some_and(|name| {
                let lower = name.to_lowercase();
                lower == "thread"
                    || lower.contains("thread")
                    || lower == "t"  // Common thread variable name
                    || lower == "worker"
                    || lower == "executor"
            });

            if is_thread_receiver {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = format!(
                    "Dangerous method Thread.{}() is deprecated and unsafe. It can corrupt object state and cause deadlocks. Use interrupt() and check interruption status instead.",
                    pattern.method_name
                );

                let mut finding = Finding::new(VulnType::DangerousMethod, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation = "Use thread.interrupt() and check Thread.interrupted() or Thread.currentThread().isInterrupted() to safely stop threads".to_string();
                return vec![finding];
            }
        }

        vec![]
    }
}

/// Check for System.exit() in library code
pub struct SystemExitCheck;

impl Check for SystemExitCheck {
    fn name(&self) -> &'static str {
        "system-exit"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            if pattern.receiver_name.as_deref() == Some("System") && pattern.method_name == "exit" {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "System.exit() terminates the entire JVM process. This is dangerous in library code, web applications, and any shared environment. Use exceptions for error handling instead.".to_string();

                let mut finding = Finding::new(VulnType::DangerousMethod, message, location, code);
                finding.severity = Severity::Medium;
                finding.remediation =
                    "Throw an exception instead of calling System.exit(). Let the application decide how to handle fatal errors."
                        .to_string();
                return vec![finding];
            }
        }

        vec![]
    }
}

/// Check for Runtime.halt()
pub struct RuntimeHaltCheck;

impl Check for RuntimeHaltCheck {
    fn name(&self) -> &'static str {
        "runtime-halt"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        &[
            SyntaxKind::LOCAL_VAR_DECL,
            SyntaxKind::EXPR_STMT,
            SyntaxKind::RETURN_STMT,
        ]
    }

    fn check(&self, node: &SyntaxNode, context: &CheckContext) -> Vec<Finding> {
        let node_text = node.text().to_string();

        // Quick filter: must contain Runtime or runtime
        if !node_text.contains("Runtime") && !node_text.contains("runtime") {
            return vec![];
        }

        for call_node in method_call_pattern::find_method_calls(node) {
            let Some(pattern) = method_call_pattern::extract_from_nested(&call_node) else {
                continue;
            };

            if pattern.method_name == "halt" {
                let location = context.node_location(node);
                let code = context.get_code_snippet(node);
                let message = "Runtime.halt() forcibly terminates the JVM without running shutdown hooks or finalizers. This is even more abrupt than System.exit() and should only be used in extreme circumstances.".to_string();

                let mut finding = Finding::new(VulnType::DangerousMethod, message, location, code);
                finding.severity = Severity::High;
                finding.remediation = "Use System.exit() if JVM termination is necessary, or preferably throw an exception".to_string();
                return vec![finding];
            }
        }

        vec![]
    }
}
