// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Sink detection and finding creation
//!
//! This module handles detection of security sinks and creation of findings
//! when tainted data flows into those sinks.

use hyperion_parser::syntax::{AstNode, MethodCallExpr, SyntaxKind, SyntaxNode};
use tracing::debug;

use crate::finding::{Finding, Location, Severity, VulnType};
use crate::logging_context::LoggingSafety;
use crate::response_context::ResponseContextAnalyzer;
use crate::sinks::{SinkPattern, SinkRegistry};
use crate::taint::{TaintState, TaintTracker, TrustLevel};
use crate::type_resolver::TypeAwareAnalyzer;

use super::method_call_pattern::MethodCallPattern;

/// Information about a detected sink and its taint status
#[derive(Debug)]
pub(super) struct SinkContext {
    pub tainted_args: Vec<usize>,
    pub taint_state: Option<TaintState>,
    pub receiver_taint: Option<TaintState>,
    pub arg_list_node: Option<SyntaxNode>,
}

/// Checks if a sink should be skipped based on context
///
/// Returns true if the sink should NOT be reported.
pub(super) fn should_skip_sink(
    pattern: &MethodCallPattern,
    sink_pattern: &SinkPattern,
    type_analyzer: TypeAwareAnalyzer,
    _taint_tracker: &TaintTracker,
    find_containing_method_fn: &impl Fn(&SyntaxNode) -> Option<SyntaxNode>,
    is_file_output_context_fn: &impl Fn(Option<&str>) -> bool,
    file_path: &std::path::Path,
) -> bool {
    // Check if this is a PreparedStatement setter (sanitization point)
    if let Some(call) = MethodCallExpr::cast(pattern.call_node.clone()) {
        if type_analyzer.is_prepared_statement_setter(&call) {
            #[cfg(test)]
            eprintln!("  -> PreparedStatement setter - safe, not reporting");
            return true;
        }
    }

    // Check if receiver is a PreparedStatement - those are safe
    //
    // NOTE: Do NOT blanket-skip PreparedStatement receivers.
    //
    // `PreparedStatement` inherits `Statement.execute*(String sql)` overloads, and calling those
    // with attacker-controlled SQL is still SQL injection. Even for the no-arg executeQuery(),
    // safety depends on how the PreparedStatement was created (e.g., tainted SQL passed to
    // Connection.prepareStatement should still be flagged at creation time).

    let vuln_type = sink_pattern.vuln_type;

    // Phase 2.1: Skip XSS for file output contexts
    // XSS only applies to HTTP responses, not file output
    if vuln_type == VulnType::Xss && is_file_output_context_fn(pattern.receiver_name.as_deref()) {
        debug!(
            receiver = ?pattern.receiver_name,
            file = ?file_path,
            "Skipping XSS for file output context"
        );
        #[cfg(test)]
        eprintln!("  -> File output context - skipping XSS detection");
        return true;
    }

    // Phase 1: Check response context for XSS sinks that require validation
    // Skip XSS checks in REST controller contexts (JSON responses)
    if sink_pattern.requires_context_check {
        if let Some(ref validator) = sink_pattern.context_validator {
            if *validator == crate::sinks::ContextValidator::RestControllerContext {
                let context_analyzer = ResponseContextAnalyzer::new();
                let method_node = find_containing_method_fn(pattern.call_node);

                if let Some(ref method) = method_node {
                    let response_context = context_analyzer.analyze_response_context(method);

                    if context_analyzer.should_skip_xss(&response_context) {
                        debug!(
                            receiver = ?pattern.receiver_name,
                            method = %pattern.method_name,
                            context = ?response_context,
                            "Skipping XSS for REST controller JSON response"
                        );
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Collects tainted arguments from a sink call
pub(super) fn collect_tainted_arguments(
    call_node: &SyntaxNode,
    vuln_type: VulnType,
    taint_tracker: &TaintTracker<'_>,
    is_expr_kind_fn: &impl Fn(SyntaxKind) -> bool,
    is_exception_logging_arg_fn: &impl Fn(&SyntaxNode) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> (Vec<usize>, Option<TaintState>, Option<SyntaxNode>) {
    let mut tainted_args = Vec::new();
    let mut taint_state_opt: Option<TaintState> = None;
    let mut arg_list_node: Option<SyntaxNode> = None;

    let Some(arg_list) = call_node
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
    else {
        return (tainted_args, taint_state_opt, arg_list_node);
    };

    arg_list_node = Some(arg_list.clone());

    for (idx, arg) in arg_list.children().enumerate() {
        if !is_expr_kind_fn(arg.kind()) {
            continue;
        }

        let Some(taint) = taint_tracker.expr_taint(&arg, node_location_fn, get_code_snippet_fn)
        else {
            continue;
        };

        if !taint.is_dangerous_for(vuln_type) {
            continue;
        }

        // Phase 1.2: Skip exception-derived data for log injection
        // Exception messages are rarely attacker-controlled and logging
        // exceptions is essential for debugging
        if vuln_type == VulnType::LogInjection && is_exception_logging_arg_fn(&arg) {
            debug!(
                arg_idx = idx,
                "Skipping exception-derived argument for log injection"
            );
            #[cfg(test)]
            eprintln!(
                "  Argument {} is exception-derived, skipping for log injection",
                idx
            );
            continue;
        }

        #[cfg(test)]
        eprintln!("  Argument {} is tainted: {:?}", idx, arg.text());

        tainted_args.push(idx);
        if taint_state_opt.is_none() {
            taint_state_opt = Some(taint);
        }
    }

    (tainted_args, taint_state_opt, arg_list_node)
}

/// Checks if receiver is tainted (for sinks that check receiver taint)
pub(super) fn check_receiver_taint(
    sink_pattern: &SinkPattern,
    receiver_node: Option<&SyntaxNode>,
    taint_tracker: &TaintTracker<'_>,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> Option<TaintState> {
    if !sink_pattern.tainted_receiver {
        return None;
    }

    let recv_node = receiver_node?;
    let taint = taint_tracker.expr_taint(recv_node, node_location_fn, get_code_snippet_fn)?;
    if taint.is_dangerous_for(sink_pattern.vuln_type) {
        Some(taint)
    } else {
        None
    }
}

/// Creates a finding for a detected sink with tainted data
#[allow(clippy::too_many_arguments)]
pub(super) fn create_sink_finding(
    pattern: &MethodCallPattern,
    sink_pattern: &SinkPattern,
    sink_context: &SinkContext,
    logging_safety: LoggingSafety,
    is_parameterized_log_fn: &impl Fn(&SyntaxNode, &[usize]) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> Finding {
    let vuln_type = sink_pattern.vuln_type;
    let location = node_location_fn(pattern.call_node);
    let code = get_code_snippet_fn(pattern.call_node);

    let detail = if sink_context.receiver_taint.is_some() {
        "tainted receiver".to_string()
    } else {
        format!(
            "tainted data in argument(s) {:?}",
            sink_context.tainted_args
        )
    };

    let message = format!(
        "Potential {} in call to {}.{}() - {}",
        vuln_type.description(),
        pattern.receiver_name.as_deref().unwrap_or("?"),
        pattern.method_name,
        detail
    );

    let mut finding = Finding::new(vuln_type, message, location.clone(), code)
        .with_rule_id(format!("taint:{}", vuln_type.as_str()));

    // Add data flow path if we have taint state
    let taint_state_opt = sink_context
        .taint_state
        .as_ref()
        .or(sink_context.receiver_taint.as_ref());

    if let Some(taint_state) = taint_state_opt {
        if let Some(path) = taint_state.path() {
            finding.dataflow = path
                .iter()
                .map(super::super::taint::state::FlowStep::to_finding_step)
                .collect();
        }
        finding.sink_location = Some(location);
        finding.confidence = confidence_for_taint(taint_state);
        finding.set_source_kind(taint_state.source_kind().map(|kind| format!("{kind:?}")));
    }

    finding.set_sink_kind(Some(format!(
        "{}.{}",
        pattern.receiver_name.as_deref().unwrap_or("?"),
        pattern.method_name
    )));

    // Phase 9: Adjust severity based on trust level
    adjust_severity_for_trust_level(&mut finding, vuln_type, taint_state_opt);

    // Additional log injection severity adjustments
    if vuln_type == VulnType::LogInjection {
        adjust_log_injection_severity(
            &mut finding,
            sink_context,
            logging_safety,
            is_parameterized_log_fn,
        );
    }

    finding
}

/// Adjusts finding severity based on taint trust level
fn adjust_severity_for_trust_level(
    finding: &mut Finding,
    vuln_type: VulnType,
    taint_state_opt: Option<&TaintState>,
) {
    let Some(taint_state) = taint_state_opt else {
        return;
    };

    let Some(trust_level) = taint_state.trust_level() else {
        return;
    };

    match trust_level {
        TrustLevel::Derived => {
            if vuln_type == VulnType::LogInjection {
                finding.severity = Severity::Low;
            } else {
                // Conservative taint propagation - lower confidence
                finding.severity = Severity::Medium;
            }
        }
        TrustLevel::Untrusted => {
            // Direct user input - moderate risk for log injection
            if vuln_type == VulnType::LogInjection {
                finding.severity = Severity::Medium;
            }
            // Other vulns keep default severity
        }
        TrustLevel::SemiTrusted => {
            // Internal data - lower risk for log injection
            if vuln_type == VulnType::LogInjection {
                finding.severity = Severity::Low;
            }
            // Other vulns keep default severity
        }
    }
}

/// Adjusts log injection finding severity based on logging context
fn adjust_log_injection_severity(
    finding: &mut Finding,
    sink_context: &SinkContext,
    logging_safety: LoggingSafety,
    is_parameterized_log_fn: &impl Fn(&SyntaxNode, &[usize]) -> bool,
) {
    if let Some(ref args) = sink_context.arg_list_node {
        if is_parameterized_log_fn(args, &sink_context.tainted_args) {
            finding.severity = Severity::Info;
        }
    }

    match logging_safety {
        LoggingSafety::StructuredSafe => {
            finding.severity = Severity::Info;
        }
        LoggingSafety::StructuredUnknown => {
            if finding.severity > Severity::Low {
                finding.severity = Severity::Low;
            }
        }
        LoggingSafety::PlainText | LoggingSafety::Unknown => {}
    }
}

/// Determines confidence score based on taint trust level
const fn confidence_for_taint(taint_state: &TaintState) -> f32 {
    match taint_state.trust_level() {
        Some(TrustLevel::Untrusted) => 0.9,
        Some(TrustLevel::SemiTrusted) => 0.7,
        Some(TrustLevel::Derived) => 0.5,
        None => 0.6,
    }
}

/// Attempts to find a sink pattern for the given receiver and method
pub(super) fn find_sink_pattern<'a>(
    pattern: &MethodCallPattern,
    sink_registry: &'a SinkRegistry,
    taint_tracker: &TaintTracker,
) -> Option<&'a SinkPattern> {
    // Prefer inferred/static receiver type over receiver name aliases. This avoids cases where
    // a variable named "stmt" is actually a PreparedStatement (or other subtype), and a generic
    // alias-based sink pattern would win and hide a more specific sink rule.
    let recv = pattern.receiver_name.as_deref()?;

    if let Some(recv_type) = taint_tracker.get_var_type(recv) {
        if let Some(pattern) = sink_registry.get_sink_pattern(Some(recv_type), &pattern.method_name)
        {
            return Some(pattern);
        }
    }

    sink_registry.get_sink_pattern(Some(recv), &pattern.method_name)
}
