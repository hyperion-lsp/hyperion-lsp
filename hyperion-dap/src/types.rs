// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! DAP (Debug Adapter Protocol) type definitions.
//!
//! Hand-rolled types covering the ~20 requests + ~10 events needed for JVM debugging.
//! Avoids external dependency drift from using `dap-rs`.

use serde::{Deserialize, Serialize};

// ---- Protocol envelope ----

#[derive(Debug, Deserialize)]
pub struct DapRequest {
    pub seq: u64,
    pub command: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// Outgoing DAP response (to client).
#[derive(Debug, Serialize)]
pub struct DapResponse {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub request_seq: u64,
    pub success: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl DapResponse {
    pub fn success(seq: u64, request: &DapRequest, body: impl Serialize) -> Self {
        Self {
            seq,
            message_type: "response",
            request_seq: request.seq,
            success: true,
            command: request.command.clone(),
            message: None,
            body: Some(serde_json::to_value(body).expect("DAP response bodies serialize to JSON")),
        }
    }

    pub fn success_empty(seq: u64, request: &DapRequest) -> Self {
        Self {
            seq,
            message_type: "response",
            request_seq: request.seq,
            success: true,
            command: request.command.clone(),
            message: None,
            body: None,
        }
    }

    pub fn error(seq: u64, request: &DapRequest, message: String) -> Self {
        Self {
            seq,
            message_type: "response",
            request_seq: request.seq,
            success: false,
            command: request.command.clone(),
            message: Some(message),
            body: None,
        }
    }
}

/// Outgoing DAP event (to client).
#[derive(Debug, Serialize)]
pub struct DapEvent {
    pub seq: u64,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl DapEvent {
    pub fn new(seq: u64, event: &str, body: impl Serialize) -> Self {
        Self {
            seq,
            message_type: "event",
            event: event.to_owned(),
            body: Some(serde_json::to_value(body).expect("DAP response bodies serialize to JSON")),
        }
    }

    pub fn simple(seq: u64, event: &str) -> Self {
        Self {
            seq,
            message_type: "event",
            event: event.to_owned(),
            body: None,
        }
    }
}

// ---- Request arguments ----

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeArguments {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_name: Option<String>,
    #[serde(default)]
    pub lines_start_at1: Option<bool>,
    #[serde(default)]
    pub columns_start_at1: Option<bool>,
    #[serde(default)]
    pub supports_variable_type: Option<bool>,
    #[serde(default)]
    pub supports_run_in_terminal_request: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArguments {
    pub main_class: String,
    #[serde(default)]
    pub classpath: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub jvm_args: Option<Vec<String>>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    /// Extra environment variables for the debuggee.
    #[serde(default)]
    pub env: Option<std::collections::HashMap<String, String>>,
    /// If true, don't launch a JVM — assume one is running and attach to it.
    #[serde(default)]
    pub no_debug: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachArguments {
    #[serde(default = "default_host")]
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub project_root: Option<String>,
}

fn default_host() -> String {
    "127.0.0.1".to_owned()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArguments {
    pub source: Source,
    #[serde(default)]
    pub breakpoints: Vec<SourceBreakpoint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u64,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub hit_condition: Option<String>,
    #[serde(default)]
    pub log_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueArguments {
    pub thread_id: u64,
    #[serde(default)]
    pub single_thread: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepArguments {
    pub thread_id: u64,
    #[serde(default)]
    pub single_thread: Option<bool>,
    #[serde(default)]
    pub granularity: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceArguments {
    pub thread_id: u64,
    #[serde(default)]
    pub start_frame: Option<u64>,
    #[serde(default)]
    pub levels: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesArguments {
    pub frame_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArguments {
    pub variables_reference: u64,
    #[serde(default)]
    pub start: Option<u64>,
    #[serde(default)]
    pub count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExceptionBreakpointsArguments {
    #[serde(default)]
    pub filters: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionInfoArguments {
    pub thread_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateArguments {
    pub expression: String,
    #[serde(default)]
    pub frame_id: Option<u64>,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectArguments {
    #[serde(default)]
    pub restart: Option<bool>,
    #[serde(default)]
    pub terminate_debuggee: Option<bool>,
}

// ---- Response bodies ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_set_variable: bool,
    pub supports_conditional_breakpoints: bool,
    pub supports_terminate_request: bool,
    pub supports_single_thread_execution_requests: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_log_points: bool,
    pub supports_hit_conditional_breakpoints: bool,
    /// Advertises the "caught"/"uncaught" checkboxes DAP clients show in the
    /// breakpoints panel; ticking one sends a `setExceptionBreakpoints`
    /// request with the corresponding filter id.
    pub exception_breakpoint_filters: Vec<ExceptionBreakpointsFilter>,
    pub supports_exception_info_request: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            supports_configuration_done_request: true,
            supports_set_variable: false,
            supports_conditional_breakpoints: true,
            supports_terminate_request: true,
            supports_single_thread_execution_requests: true,
            supports_evaluate_for_hovers: true,
            supports_log_points: true,
            supports_hit_conditional_breakpoints: true,
            exception_breakpoint_filters: vec![
                ExceptionBreakpointsFilter {
                    filter: "caught".to_owned(),
                    label: "Caught Exceptions".to_owned(),
                    default: Some(false),
                },
                ExceptionBreakpointsFilter {
                    filter: "uncaught".to_owned(),
                    label: "Uncaught Exceptions".to_owned(),
                    default: Some(true),
                },
            ],
            supports_exception_info_request: true,
        }
    }
}

/// One entry of `Capabilities.exceptionBreakpointFilters` — a checkbox the
/// client shows for "break on this category of exception".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpointsFilter {
    pub filter: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsResponseBody {
    pub breakpoints: Vec<Breakpoint>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    pub id: Option<u64>,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueResponseBody {
    pub all_threads_continued: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadsResponseBody {
    pub threads: Vec<Thread>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceResponseBody {
    pub stack_frames: Vec<StackFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_frames: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub line: u64,
    pub column: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesResponseBody {
    pub scopes: Vec<Scope>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub name: String,
    pub variables_reference: u64,
    pub expensive: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesResponseBody {
    pub variables: Vec<Variable>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    pub variables_reference: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResponseBody {
    pub result: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    pub variables_reference: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionInfoResponseBody {
    /// DAP calls this an "id" but it's conventionally the exception's type
    /// name — JDWP has no separate short error code to report here.
    pub exception_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub break_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ExceptionDetails>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(rename = "typeName", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(rename = "fullTypeName", skip_serializing_if = "Option::is_none")]
    pub full_type_name: Option<String>,
}

// ---- Event bodies ----

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedEventBody {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_threads_stopped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuedEventBody {
    pub thread_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputEventBody {
    pub category: String, // "console", "stdout", "stderr"
    pub output: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitedEventBody {
    pub exit_code: i32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventBody {
    pub reason: String, // "started", "exited"
    pub thread_id: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointEventBody {
    pub reason: String, // "changed", "new", "removed"
    pub breakpoint: Breakpoint,
}
