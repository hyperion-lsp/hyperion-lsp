// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Debug session state machine.
//!
//! Translates DAP requests into JDWP commands and maintains all debug state:
//! breakpoint mappings, class cache, line tables, and variable reference counters.

mod breakpoints;
mod evaluate;
mod expr;
mod output;
mod presentation;
mod state;
mod values;

use std::path::PathBuf;
use std::process::Stdio;

use tokio::sync::mpsc;

use hyperion_jdwp::commands::{self, Command, EventModifier, IdSizes, SUSPEND_ALL};
use hyperion_jdwp::events::EventSet;
use hyperion_jdwp::replies::{self, FieldInfo, LineTableEntry, Location, MethodInfo, VariableInfo};
use hyperion_jdwp::JdwpConnection;

use self::expr::is_truthy;
pub use self::output::OutputChunk;
use self::output::{drain_pipe, OutputCapture, OUTPUT_CHANNEL_CAPACITY};
use self::presentation::{jni_sig_to_display, sig_to_tag};
use self::state::{
    BreakpointState, ExceptionState, ScopeInfo, StepIntent, TypeCache, VariableState,
};

use crate::types::{
    AttachArguments, Capabilities, ContinueArguments, ContinueResponseBody, DisconnectArguments,
    EvaluateArguments, ExceptionDetails, ExceptionInfoArguments, ExceptionInfoResponseBody,
    InitializeArguments, LaunchArguments, Scope, ScopesArguments, ScopesResponseBody,
    SetExceptionBreakpointsArguments, Source, StackFrame, StackTraceArguments,
    StackTraceResponseBody, StepArguments, Thread, ThreadsResponseBody, Variable,
    VariablesArguments, VariablesResponseBody,
};

/// Upper bound on re-issued steps for one user step (filtered frames).
const MAX_STEP_HOPS: u32 = 4000;

/// Class signatures no step stops in (JNI form).
const STEP_FILTERED_PREFIXES: &[&str] = &[
    "Ljava/",
    "Ljavax/",
    "Ljdk/",
    "Lsun/",
    "Lcom/sun/",
    "Lkotlin/jvm/internal/",
    "Lkotlin/coroutines/jvm/internal/",
];

fn step_filtered_signature(signature: &str) -> bool {
    STEP_FILTERED_PREFIXES
        .iter()
        .any(|prefix| signature.starts_with(prefix))
}

const JDWP_CONNECT_ATTEMPTS: usize = 20;
const JDWP_CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(-1)
}

fn describe_exit(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    status.code().map_or_else(
        || "an unknown status".to_owned(),
        |code| format!("code {code}"),
    )
}

async fn connect_jdwp_with_retry(host: &str, port: u16) -> Result<JdwpConnection, String> {
    let mut last_error = String::new();

    for attempt in 1..=JDWP_CONNECT_ATTEMPTS {
        match JdwpConnection::connect((host, port)).await {
            Ok(connection) => return Ok(connection),
            Err(error) => {
                last_error = error.to_string();
                tracing::debug!(
                    "JDWP connect attempt {attempt}/{JDWP_CONNECT_ATTEMPTS} to {host}:{port} failed: {error}"
                );
            }
        }

        if attempt < JDWP_CONNECT_ATTEMPTS {
            tokio::time::sleep(JDWP_CONNECT_RETRY_DELAY).await;
        }
    }

    Err(format!(
        "failed to connect to JVM at {host}:{port} after {JDWP_CONNECT_ATTEMPTS} attempts: {last_error}"
    ))
}

/// Core debug session holding the JDWP connection and all state.
pub struct DebugSession {
    /// JDWP connection to the debuggee JVM.
    connection: Option<JdwpConnection>,
    /// Spawned JVM process (for launch mode).
    child: Option<tokio::process::Child>,
    /// Project root for source path resolution.
    project_root: Option<PathBuf>,
    /// Whether lines are 1-based (DAP default).
    lines_start_at1: bool,

    breakpoints: BreakpointState,
    types: TypeCache,
    variables: VariableState,
    exceptions: ExceptionState,

    // ---- Debuggee output forwarding ----
    /// Receiver for stdout/stderr chunks drained from the launched JVM, taken
    /// by the server loop once after `launch` (mirrors `take_event_receiver`).
    output_rx: Option<mpsc::Receiver<OutputChunk>>,
    /// Everything the launched JVM printed, kept for launch-failure reports.
    output_capture: OutputCapture,
    /// Tasks copying the JVM's stdout/stderr; finished when both pipes hit EOF.
    drain_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// Exit code of the launched JVM once known.
    exit_code: Option<i32>,
    /// Source roots learned from breakpoint files, for resolving frame paths.
    source_roots: Vec<PathBuf>,
    /// Set when a method call in the debuggee overran its time limit; further
    /// calls are refused because the thread may still be busy.
    invoke_disabled: bool,
    /// Module source roots found by scanning the project (lazily).
    discovered_roots: Option<Vec<PathBuf>>,
    /// Depth of `suspend_stopping_events` calls in flight.
    stop_events_suspended: u32,
    /// False while evaluating a hover: accessor fallbacks must not run code.
    allow_invoke: bool,
    /// Logpoint messages waiting to be sent to the client as `output` events.
    pending_logs: Vec<String>,
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugSession {
    pub fn new() -> Self {
        Self {
            connection: None,
            child: None,
            project_root: None,
            lines_start_at1: true,
            breakpoints: BreakpointState::default(),
            types: TypeCache::default(),
            variables: VariableState::default(),
            exceptions: ExceptionState::default(),
            output_rx: None,
            output_capture: OutputCapture::default(),
            drain_tasks: Vec::new(),
            exit_code: None,
            source_roots: Vec::new(),
            invoke_disabled: false,
            discovered_roots: None,
            stop_events_suspended: 0,
            allow_invoke: true,
            pending_logs: Vec::new(),
        }
    }

    pub fn conn(&self) -> Result<&JdwpConnection, String> {
        self.connection
            .as_ref()
            .ok_or_else(|| "not connected to debuggee".to_owned())
    }

    fn id_sizes(&self) -> Result<&IdSizes, String> {
        Ok(self.conn()?.id_sizes())
    }

    fn alloc_var_ref(&self) -> u64 {
        self.variables.allocate_reference()
    }

    // ---- Handler: initialize ----

    pub fn handle_initialize(&mut self, args: InitializeArguments) -> Capabilities {
        self.lines_start_at1 = args.lines_start_at1.unwrap_or(true);
        Capabilities::default()
    }

    // ---- Handler: launch ----

    pub async fn handle_launch(&mut self, args: LaunchArguments) -> Result<(), String> {
        self.project_root = args.project_root.map(PathBuf::from);
        self.exit_code = None;

        // Build the classpath.
        let classpath = args.classpath.clone().unwrap_or_default();

        // Find a free port for JDWP.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("failed to find free port: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        drop(listener);

        let cwd = args.cwd.as_deref().unwrap_or(".");

        let build_command = |java: &std::ffi::OsStr| {
            let mut cmd = tokio::process::Command::new(java);
            cmd.current_dir(cwd);
            // Kill the debuggee if the `Child` is ever dropped without an
            // explicit `disconnect` (client crash, task abort, etc.) — otherwise
            // it's orphaned and keeps running forever since tokio does not kill
            // child processes on drop by default.
            cmd.kill_on_drop(true);

            if let Some(env) = &args.env {
                cmd.envs(env);
            }

            // JVM args (before main class).
            if let Some(jvm_args) = &args.jvm_args {
                cmd.args(jvm_args);
            }

            // JDWP agent.
            cmd.arg(format!(
                "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"
            ));

            // Classpath.
            if !classpath.is_empty() {
                cmd.arg("-cp").arg(&classpath);
            }

            // Main class.
            cmd.arg(&args.main_class);

            // Program args.
            if let Some(program_args) = &args.args {
                cmd.args(program_args);
            }

            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            cmd
        };

        tracing::info!(
            "launching JVM: java ... -agentlib:jdwp=...address=127.0.0.1:{port} {} {} (cwd {cwd})",
            if classpath.is_empty() {
                ""
            } else {
                "-cp <classpath>"
            },
            args.main_class
        );

        let mut child = match build_command(std::ffi::OsStr::new("java")).spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // No `java` on PATH: fall back to JAVA_HOME before giving up.
                let from_java_home = std::env::var_os("JAVA_HOME")
                    .map(|home| PathBuf::from(home).join("bin").join("java"))
                    .filter(|path| path.is_file());
                let Some(java) = from_java_home else {
                    return Err(format!(
                        "failed to start the JVM: `java` was not found on PATH and JAVA_HOME is \
                         not set to a JDK ({error})"
                    ));
                };
                build_command(java.as_os_str())
                    .spawn()
                    .map_err(|e| format!("failed to start the JVM {}: {e}", java.display()))?
            }
            Err(error) => {
                return Err(format!(
                    "failed to start the JVM (working directory {cwd}): {error}"
                ));
            }
        };

        // Drain the debuggee's stdout/stderr in the background. Without this,
        // once the OS pipe buffer fills up (a few tens of KB — trivial for a
        // program that logs a lot), the JVM blocks on its next `write` and the
        // whole debug session appears to hang, even though JDWP itself is
        // fine. Each chunk is both logged and forwarded on `output_tx` so the
        // server loop can turn it into a DAP `output` event (see
        // `take_output_receiver`). A bounded channel caps memory use, while
        // the drain tasks use `try_send` so a slow/absent client cannot apply
        // backpressure to the JVM through a full OS pipe. The same text is kept
        // in `capture` so a JVM that dies before the debugger connects can be
        // reported with what it actually printed.
        let (output_tx, output_rx) = mpsc::channel::<OutputChunk>(OUTPUT_CHANNEL_CAPACITY);
        self.output_rx = Some(output_rx);
        self.output_capture = OutputCapture::default();
        self.drain_tasks.clear();
        if let Some(stdout) = child.stdout.take() {
            self.drain_tasks.push(tokio::spawn(drain_pipe(
                stdout,
                "stdout",
                output_tx.clone(),
                self.output_capture.clone(),
            )));
        }
        if let Some(stderr) = child.stderr.take() {
            self.drain_tasks.push(tokio::spawn(drain_pipe(
                stderr,
                "stderr",
                output_tx,
                self.output_capture.clone(),
            )));
        }

        self.child = Some(child);

        match self.connect_to_launched_jvm(port).await {
            Ok(connection) => {
                tracing::info!("connected to JVM JDWP agent on port {port}");
                self.connection = Some(connection);
                Ok(())
            }
            Err(message) => {
                tracing::error!("launch failed: {message}");
                // Nothing will consume the output channel after a failed launch.
                self.output_rx = None;
                if let Some(mut child) = self.child.take() {
                    let _ = child.kill().await;
                }
                Err(message)
            }
        }
    }

    /// Connect to the JDWP agent of the JVM this session just spawned. Unlike
    /// a plain retry loop this notices the JVM dying (bad main class flags,
    /// unsupported class file version, invalid options, ...) and reports what
    /// it printed instead of waiting out the retries.
    async fn connect_to_launched_jvm(&mut self, port: u16) -> Result<JdwpConnection, String> {
        let mut last_error = String::new();
        for attempt in 1..=JDWP_CONNECT_ATTEMPTS {
            match JdwpConnection::connect(("127.0.0.1", port)).await {
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    last_error = error.to_string();
                    tracing::debug!(
                        "JDWP connect attempt {attempt}/{JDWP_CONNECT_ATTEMPTS} to port {port} failed: {error}"
                    );
                }
            }
            if let Some(child) = self.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    self.exit_code = Some(exit_code_of(status));
                    self.finish_output().await;
                    let printed = self.output_capture.text();
                    let printed = printed.trim();
                    return Err(if printed.is_empty() {
                        format!(
                            "the JVM exited with {} before the debugger could connect, and printed nothing",
                            describe_exit(status)
                        )
                    } else {
                        format!(
                            "the JVM exited with {} before the debugger could connect:\n{printed}",
                            describe_exit(status)
                        )
                    });
                }
            }
            if attempt < JDWP_CONNECT_ATTEMPTS {
                tokio::time::sleep(JDWP_CONNECT_RETRY_DELAY).await;
            }
        }
        Err(format!(
            "failed to connect to the JVM's debug agent on port {port} after {JDWP_CONNECT_ATTEMPTS} attempts: {last_error}"
        ))
    }

    /// Wait (bounded) for the debuggee's output pipes to reach EOF so every
    /// line it printed has been forwarded or captured.
    pub async fn finish_output(&mut self) {
        for task in std::mem::take(&mut self.drain_tasks) {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), task).await;
        }
    }

    /// Exit code of the launched JVM, waiting briefly for it to finish.
    /// `None` for an attached session, where the process is not ours.
    pub async fn wait_exit_code(&mut self) -> Option<i32> {
        if let Some(code) = self.exit_code {
            return Some(code);
        }
        let child = self.child.as_mut()?;
        let status = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
            .await
            .ok()?
            .ok()?;
        let code = exit_code_of(status);
        self.exit_code = Some(code);
        Some(code)
    }

    // ---- Handler: terminate ----

    /// Ask the debuggee to stop. The resulting `exited`/`terminated` events
    /// arrive through the normal JDWP/process channels.
    pub async fn handle_terminate(&mut self) -> Result<(), String> {
        if let Some(child) = self.child.as_mut() {
            child
                .start_kill()
                .map_err(|e| format!("failed to terminate the debuggee: {e}"))?;
            return Ok(());
        }
        let conn = self.conn()?;
        // Attached: the process is not ours to kill, but the VM can be told to exit.
        let _ = conn.send_command(Command::vm_exit(143)).await;
        Ok(())
    }

    // ---- Handler: attach ----

    pub async fn handle_attach(&mut self, args: AttachArguments) -> Result<(), String> {
        self.project_root = args.project_root.map(PathBuf::from);

        let conn = connect_jdwp_with_retry(&args.host, args.port).await?;

        tracing::info!("attached to JVM at {}:{}", args.host, args.port);
        self.connection = Some(conn);
        Ok(())
    }

    // ---- Handler: configurationDone ----

    pub async fn handle_configuration_done(&self) -> Result<(), String> {
        // Resume all threads — the JVM was launched with suspend=y.
        self.conn()?
            .send_command(Command::vm_resume())
            .await
            .map_err(|e| format!("failed to resume VM: {e}"))?;
        Ok(())
    }

    // ---- Handler: continue ----

    pub async fn handle_continue(
        &mut self,
        _args: ContinueArguments,
    ) -> Result<ContinueResponseBody, String> {
        // A step that a breakpoint interrupted is still armed in the VM: left
        // there it would stop the program again at the end of that step.
        self.forget_step().await;
        self.conn()?
            .send_command(Command::vm_resume())
            .await
            .map_err(|e| format!("failed to resume: {e}"))?;
        Ok(ContinueResponseBody {
            all_threads_continued: true,
        })
    }

    /// Drop the step in progress and its JDWP request.
    pub async fn forget_step(&mut self) {
        self.breakpoints.step = None;
        if let Err(error) = self.clear_active_step_request().await {
            tracing::warn!("{error}");
        }
    }

    // ---- Handler: next (step over) ----

    pub async fn handle_next(&mut self, args: StepArguments) -> Result<(), String> {
        self.do_step(args.thread_id, commands::STEP_DEPTH_OVER)
            .await
    }

    // ---- Handler: stepIn ----

    pub async fn handle_step_in(&mut self, args: StepArguments) -> Result<(), String> {
        self.do_step(args.thread_id, commands::STEP_DEPTH_INTO)
            .await
    }

    // ---- Handler: stepOut ----

    pub async fn handle_step_out(&mut self, args: StepArguments) -> Result<(), String> {
        self.do_step(args.thread_id, commands::STEP_DEPTH_OUT).await
    }

    async fn do_step(&mut self, thread_id: u64, depth: i32) -> Result<(), String> {
        // The step starts in the frame the user is looking at; when that frame
        // is itself filtered (stopped inside the JDK) nothing is filtered.
        let filtering = match self.top_frame_location(thread_id).await {
            Some(location) => !self.frame_is_filtered(&location).await,
            None => true,
        };
        self.breakpoints.step = Some(StepIntent {
            thread_id,
            depth,
            hops: 0,
            filtering,
            leaving_filtered: false,
        });
        self.arm_step(thread_id, depth).await?;

        // Resume all threads to execute the step.
        // Using vm_resume() instead of thread_resume() avoids deadlocks when
        // other threads are suspended from SUSPEND_ALL breakpoints.
        self.conn()?
            .send_command(Command::vm_resume())
            .await
            .map_err(|e| format!("failed to resume VM: {e}"))?;

        Ok(())
    }

    /// Install the (single) JDWP step request of `thread_id`.
    async fn arm_step(&mut self, thread_id: u64, depth: i32) -> Result<(), String> {
        let id_sizes = *self.id_sizes()?;

        // Clear any previous step request — JDWP only allows one step per thread.
        self.clear_active_step_request().await?;

        let modifier = EventModifier::Step {
            thread_id,
            size: commands::STEP_SIZE_LINE,
            depth,
        };
        let cmd = Command::event_request_set(
            commands::EVENT_KIND_SINGLE_STEP,
            SUSPEND_ALL,
            &[modifier],
            &id_sizes,
        );
        let reply = self
            .conn()?
            .send_command(cmd)
            .await
            .map_err(|e| format!("failed to set step: {e}"))?;
        let mut data = reply.data;
        let request_id = replies::parse_event_request_set(&mut data).map_err(|e| e.to_string())?;
        self.breakpoints.active_step_request = Some(request_id);
        Ok(())
    }

    async fn top_frame_location(&self, thread_id: u64) -> Option<Location> {
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::thread_frames(
                thread_id,
                0,
                1,
                id_sizes.object_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        replies::parse_thread_frames(&mut data, &id_sizes)
            .ok()?
            .into_iter()
            .next()
            .map(|frame| frame.location)
    }

    /// Frames a step never stops in: JDK and Kotlin runtime internals, and
    /// frames without a source file or line (lambda proxies, generated code).
    async fn frame_is_filtered(&mut self, location: &Location) -> bool {
        if let Some(signature) = self.get_class_signature(location.class_id).await {
            if step_filtered_signature(&signature) {
                return true;
            }
        }
        if self.source_file_of(location.class_id).await.is_none() {
            return true;
        }
        let (_, _, line) = self.resolve_frame_info(location).await;
        line == 0
    }

    /// A step event arrived: if it landed in a filtered frame, keep going
    /// (arming the next step) and return true; false means the stop is real.
    ///
    /// Step-into leaves a filtered callee (`Double.valueOf`) with a step-out
    /// and then steps into again from the caller, so the next user call on
    /// the line is entered. Step-over/out that ends in a filtered caller (the
    /// JDK frame that invoked a lambda) steps into it until user code runs
    /// again, which is the next lambda call or the end of the loop.
    pub async fn step_continues(&mut self, thread_id: u64) -> bool {
        let Some(mut intent) = self.breakpoints.step.filter(|i| i.thread_id == thread_id) else {
            return false;
        };
        if !intent.filtering || intent.hops >= MAX_STEP_HOPS {
            self.breakpoints.step = None;
            return false;
        }
        let Some(location) = self.top_frame_location(thread_id).await else {
            self.breakpoints.step = None;
            return false;
        };
        let filtered = self.frame_is_filtered(&location).await;
        let next_depth = if filtered {
            if intent.depth == commands::STEP_DEPTH_INTO {
                intent.leaving_filtered = true;
                commands::STEP_DEPTH_OUT
            } else {
                commands::STEP_DEPTH_INTO
            }
        } else if intent.depth == commands::STEP_DEPTH_INTO && intent.leaving_filtered {
            intent.leaving_filtered = false;
            commands::STEP_DEPTH_INTO
        } else {
            self.breakpoints.step = None;
            return false;
        };
        intent.hops += 1;
        self.breakpoints.step = Some(intent);
        if let Err(error) = self.arm_step(thread_id, next_depth).await {
            tracing::warn!("{error}");
            self.breakpoints.step = None;
            return false;
        }
        true
    }

    /// Clear the active JDWP step request after its event, before execution resumes.
    /// JDWP step requests remain active until explicitly cleared.
    pub async fn clear_active_step_request(&mut self) -> Result<(), String> {
        let Some(request_id) = self.breakpoints.active_step_request else {
            return Ok(());
        };
        let command = Command::event_request_clear(commands::EVENT_KIND_SINGLE_STEP, request_id);
        self.conn()?
            .send_command(command)
            .await
            .map_err(|error| format!("failed to clear step request: {error}"))?;
        self.breakpoints.active_step_request = None;
        Ok(())
    }

    /// Decide whether a breakpoint hit stops the program: its condition must
    /// hold, then its hit condition must accept the hit count, and a logpoint
    /// logs instead of stopping. Returns `false` to auto-resume.
    pub async fn should_stop_for_breakpoint(&mut self, request_id: u32, thread_id: u64) -> bool {
        if let Some(condition) = self.breakpoints.conditions.get(&request_id).cloned() {
            match self.evaluate_in_top_frame(thread_id, &condition).await {
                Some(Ok(result)) => {
                    if !is_truthy(&result) {
                        return false;
                    }
                }
                // No frame to evaluate against, or the evaluation failed:
                // stop so the user can see what is wrong.
                Some(Err(e)) => {
                    tracing::debug!("condition evaluation failed: {e}, stopping anyway");
                    return true;
                }
                None => return true,
            }
        }

        let Some((hit_condition, log_message, hits)) = self.count_hit(request_id) else {
            return true;
        };
        if let Some(hit_condition) = hit_condition {
            if !hit_condition_met(&hit_condition, hits) {
                return false;
            }
        }
        if let Some(message) = log_message {
            let text = self.render_log_message(thread_id, &message).await;
            self.pending_logs.push(format!("{text}\n"));
            return false;
        }
        true
    }

    /// Count one hit of the breakpoint entry owning `request_id`; returns its
    /// hit condition, log message and the new count.
    fn count_hit(&mut self, request_id: u32) -> Option<(Option<String>, Option<String>, u64)> {
        let entry = self
            .breakpoints
            .sources
            .values_mut()
            .flat_map(|source| source.entries.iter_mut())
            .find(|entry| entry.request_ids.contains(&request_id))?;
        entry.hits += 1;
        if entry.hit_condition.is_none() && entry.log_message.is_none() {
            return None;
        }
        Some((
            entry.hit_condition.clone(),
            entry.log_message.clone(),
            entry.hits,
        ))
    }

    /// Logpoint messages produced since the last call.
    pub fn take_pending_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_logs)
    }

    /// Replace each `{expression}` of a logpoint message with its value in the
    /// top frame of the thread that hit the breakpoint.
    async fn render_log_message(&mut self, thread_id: u64, message: &str) -> String {
        let mut out = String::new();
        let mut rest = message;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let Some(close) = rest[open..].find('}') else {
                out.push_str(&rest[open..]);
                return out;
            };
            let expression = rest[open + 1..open + close].trim();
            match self.evaluate_in_top_frame(thread_id, expression).await {
                Some(Ok(value)) => out.push_str(&value),
                Some(Err(error)) => out.push_str(&format!("<{error}>")),
                None => out.push_str("<no frame>"),
            }
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Evaluate `expression` in the top frame of `thread_id`. `None` when the
    /// frame cannot be fetched.
    async fn evaluate_in_top_frame(
        &mut self,
        thread_id: u64,
        expression: &str,
    ) -> Option<Result<String, String>> {
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::thread_frames(
                thread_id,
                0,
                1,
                id_sizes.object_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let frame = replies::parse_thread_frames(&mut data, &id_sizes)
            .ok()?
            .into_iter()
            .next()?;

        // Register a temporary scope for this frame so evaluate can use it.
        let temp_frame_id = self.alloc_var_ref();
        self.variables.scopes.insert(
            temp_frame_id,
            ScopeInfo {
                thread_id,
                frame_id: frame.frame_id,
                class_id: frame.location.class_id,
                method_id: frame.location.method_id,
                code_index: frame.location.index,
                depth: 0,
            },
        );
        let result = self
            .handle_evaluate(EvaluateArguments {
                expression: expression.to_owned(),
                frame_id: Some(temp_frame_id),
                context: Some("watch".to_owned()),
            })
            .await;
        self.variables.scopes.remove(&temp_frame_id);
        Some(result.map(|body| body.result))
    }

    // ---- Handler: setExceptionBreakpoints ----

    pub async fn handle_set_exception_breakpoints(
        &mut self,
        args: SetExceptionBreakpointsArguments,
    ) -> Result<(), String> {
        // Clear any previously registered exception event request(s) before
        // installing new ones — JDWP has no "update filters" operation, and
        // draining into an owned Vec first avoids holding a borrow of
        // `self.exceptions.requests` across the `.await` below.
        let old_requests = std::mem::take(&mut self.exceptions.requests);
        for request_id in old_requests {
            let cmd = Command::event_request_clear(commands::EVENT_KIND_EXCEPTION, request_id);
            if let Err(e) = self.conn()?.send_command(cmd).await {
                tracing::warn!("failed to clear exception breakpoint: {e}");
            }
        }

        let caught = args.filters.iter().any(|f| f == "caught");
        let uncaught = args.filters.iter().any(|f| f == "uncaught");

        if !caught && !uncaught {
            // Both filters unticked — leave no exception request registered.
            self.exceptions.active = None;
            return Ok(());
        }
        self.exceptions.active = Some((caught, uncaught));
        self.install_exception_request(caught, uncaught).await
    }

    pub(super) async fn install_exception_request(
        &mut self,
        caught: bool,
        uncaught: bool,
    ) -> Result<(), String> {
        let id_sizes = *self.id_sizes()?;
        // `ref_type_id_or_null: 0` matches exceptions of any type — DAP's
        // standard "caught"/"uncaught" filters don't narrow by exception class.
        let modifier = EventModifier::ExceptionOnly {
            ref_type_id_or_null: 0,
            caught,
            uncaught,
        };
        let cmd = Command::event_request_set(
            commands::EVENT_KIND_EXCEPTION,
            SUSPEND_ALL,
            &[modifier],
            &id_sizes,
        );

        let reply = self
            .conn()?
            .send_command(cmd)
            .await
            .map_err(|e| format!("failed to set exception breakpoint: {e}"))?;

        let mut data = reply.data;
        let request_id = replies::parse_event_request_set(&mut data).map_err(|e| e.to_string())?;
        self.exceptions.requests.push(request_id);

        Ok(())
    }

    // ---- Handler: exceptionInfo ----

    /// Record which exception object a thread most recently stopped on, so a
    /// later `exceptionInfo` request (keyed only by `threadId`) can look it
    /// up. Called from the server's event loop when an `Exception` event
    /// arrives, before the `stopped` event is sent to the client.
    pub fn record_exception(&mut self, thread_id: u64, exception_id: u64, uncaught: bool) {
        self.exceptions
            .by_thread
            .insert(thread_id, (exception_id, uncaught));
    }

    /// Build a short "TypeName: message" description for a thrown exception
    /// object, for the `stopped` event's `description`/`text` fields. Best
    /// effort: returns `None` if any JDWP call along the way fails rather
    /// than propagating an error, since a missing description shouldn't stop
    /// the `stopped` event itself from being delivered.
    pub async fn describe_exception(&mut self, exception_id: u64) -> Option<String> {
        let type_id = self.exception_type(exception_id).await?;
        let signature = self.get_class_signature(type_id).await?;
        let type_name = jni_sig_to_display(&signature);
        let message = self.exception_message(exception_id, type_id).await;
        Some(match message {
            Some(msg) => format!("{type_name}: {msg}"),
            None => type_name,
        })
    }

    pub async fn handle_exception_info(
        &mut self,
        args: ExceptionInfoArguments,
    ) -> Result<ExceptionInfoResponseBody, String> {
        let (exception_id, uncaught) = *self
            .exceptions
            .by_thread
            .get(&args.thread_id)
            .ok_or("no exception recorded for this thread")?;

        let type_id = self
            .exception_type(exception_id)
            .await
            .ok_or("failed to resolve exception's runtime type")?;
        let signature = self
            .get_class_signature(type_id)
            .await
            .ok_or("failed to resolve exception's type signature")?;
        let type_name = jni_sig_to_display(&signature);
        let message = self.exception_message(exception_id, type_id).await;

        let description = Some(match &message {
            Some(msg) => format!("{type_name}: {msg}"),
            None => type_name.clone(),
        });

        Ok(ExceptionInfoResponseBody {
            exception_id: type_name.clone(),
            description,
            break_mode: if uncaught {
                "unhandled".to_owned()
            } else {
                "always".to_owned()
            },
            details: Some(ExceptionDetails {
                message,
                type_name: Some(type_name.clone()),
                full_type_name: Some(type_name),
            }),
        })
    }

    /// Get the runtime reference-type ID of an object (e.g. an exception
    /// instance) via `ObjectReference.ReferenceType`.
    async fn exception_type(&self, object_id: u64) -> Option<u64> {
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::object_reference_type(
                object_id,
                id_sizes.object_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let (_tag, type_id) = replies::parse_object_reference_type(&mut data, &id_sizes).ok()?;
        Some(type_id)
    }

    /// Resolve a `Throwable`'s message via its `detailMessage` field, walking
    /// up the superclass chain from the exception's concrete type since
    /// `detailMessage` is declared on `java.lang.Throwable` itself and
    /// `ReferenceType.Fields` only returns fields declared directly on the
    /// type it's asked about.
    async fn exception_message(&mut self, object_id: u64, type_id: u64) -> Option<String> {
        let field = self
            .find_field_in_hierarchy(type_id, "detailMessage")
            .await?;
        let id_sizes = *self.id_sizes().ok()?;
        let reply = self
            .conn()
            .ok()?
            .send_command(Command::object_get_values(
                object_id,
                &[field.field_id],
                id_sizes.object_id_size,
                id_sizes.field_id_size,
            ))
            .await
            .ok()?;
        let mut data = reply.data;
        let value = replies::parse_object_get_values(&mut data, &id_sizes)
            .ok()?
            .into_iter()
            .next()?;
        match value {
            replies::JdwpValue::Object { tag, object_id }
                if tag == replies::tag::STRING && object_id != 0 =>
            {
                self.resolve_string_value(object_id).await
            }
            _ => None,
        }
    }

    /// Find a field by name, walking up the superclass chain if it's not
    /// declared directly on `type_id`. Bounded to 32 hops to guard against a
    /// malformed/cyclic hierarchy.
    async fn find_field_in_hierarchy(
        &mut self,
        mut type_id: u64,
        field_name: &str,
    ) -> Option<FieldInfo> {
        for _ in 0..32 {
            let fields = self.get_fields(type_id).await.ok()?;
            if let Some(field) = fields.iter().find(|f| f.name == field_name) {
                return Some(field.clone());
            }

            let id_sizes = *self.id_sizes().ok()?;
            let reply = self
                .conn()
                .ok()?
                .send_command(Command::class_type_superclass(
                    type_id,
                    id_sizes.reference_type_id_size,
                ))
                .await
                .ok()?;
            let mut data = reply.data;
            let superclass_id = replies::parse_class_type_superclass(&mut data, &id_sizes).ok()?;
            if superclass_id == 0 {
                return None;
            }
            type_id = superclass_id;
        }
        None
    }

    // ---- Handler: threads ----

    pub async fn handle_threads(&self) -> Result<ThreadsResponseBody, String> {
        let conn = self.conn()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::vm_all_threads())
            .await
            .map_err(|e| format!("failed to get threads: {e}"))?;

        let mut data = reply.data;
        let thread_ids =
            replies::parse_all_threads(&mut data, &id_sizes).map_err(|e| e.to_string())?;

        let mut threads = Vec::with_capacity(thread_ids.len());
        for tid in thread_ids {
            let name_reply = conn
                .send_command(Command::thread_name(tid, id_sizes.object_id_size))
                .await;

            let name = match name_reply {
                Ok(r) => {
                    let mut d = r.data;
                    replies::parse_thread_name(&mut d).unwrap_or_else(|_| format!("Thread-{tid}"))
                }
                Err(_) => format!("Thread-{tid}"),
            };

            threads.push(Thread { id: tid, name });
        }

        Ok(ThreadsResponseBody { threads })
    }

    // ---- Handler: stackTrace ----

    pub async fn handle_stack_trace(
        &mut self,
        args: StackTraceArguments,
    ) -> Result<StackTraceResponseBody, String> {
        let conn = self.conn()?;
        let id_sizes = *conn.id_sizes();

        let start = args.start_frame.unwrap_or(0) as i32;
        let length = args.levels.map_or(-1, |l| l as i32);

        let reply = conn
            .send_command(Command::thread_frames(
                args.thread_id,
                start,
                length,
                id_sizes.object_id_size,
            ))
            .await
            .map_err(|e| format!("failed to get frames: {e}"))?;

        let mut data = reply.data;
        let frames =
            replies::parse_thread_frames(&mut data, &id_sizes).map_err(|e| e.to_string())?;

        // Clear previous scope refs, variable cache, and object refs for fresh allocation.
        self.variables.scopes.clear();
        self.variables.cached.clear();
        self.variables.objects.clear();

        let mut stack_frames = Vec::with_capacity(frames.len());
        for (offset, frame) in frames.iter().enumerate() {
            let (method_name, source, line) = self.resolve_frame_info(&frame.location).await;

            // Allocate a unique DAP frame ID from the same counter as var_refs
            // to avoid collisions between frame IDs and variable references.
            let dap_frame_id = self.alloc_var_ref();

            // Store ScopeInfo so handle_variables can use it directly
            // without re-fetching frames.
            self.variables.scopes.insert(
                dap_frame_id,
                ScopeInfo {
                    thread_id: args.thread_id,
                    frame_id: frame.frame_id,
                    class_id: frame.location.class_id,
                    method_id: frame.location.method_id,
                    code_index: frame.location.index,
                    depth: start + offset as i32,
                },
            );

            stack_frames.push(StackFrame {
                id: dap_frame_id,
                name: method_name,
                source,
                line,
                column: 0,
            });
        }

        let total = stack_frames.len() as u64;
        Ok(StackTraceResponseBody {
            stack_frames,
            total_frames: Some(total),
        })
    }

    // ---- Handler: scopes ----

    pub fn handle_scopes(&mut self, args: ScopesArguments) -> Result<ScopesResponseBody, String> {
        // Each frame gets a "Locals" scope. The variable_reference points to a
        // scope ref that we can use in the `variables` handler.
        let var_ref = self.alloc_var_ref();

        // Transfer the ScopeInfo mapping to the new var_ref.
        if let Some(&info) = self.variables.scopes.get(&args.frame_id) {
            self.variables.scopes.insert(var_ref, info);
        }

        Ok(ScopesResponseBody {
            scopes: vec![Scope {
                name: "Locals".to_owned(),
                variables_reference: var_ref,
                expensive: false,
            }],
        })
    }

    // ---- Handler: variables ----

    pub async fn handle_variables(
        &mut self,
        args: VariablesArguments,
    ) -> Result<VariablesResponseBody, String> {
        // Showing values runs `toString()` in the debuggee; a breakpoint or
        // exception stop inside that call would deadlock the request.
        self.suspend_stopping_events().await;
        let result = self.variables_inner(args).await;
        self.resume_stopping_events().await;
        result
    }

    async fn variables_inner(
        &mut self,
        args: VariablesArguments,
    ) -> Result<VariablesResponseBody, String> {
        // Check if this is a cached expansion.
        if let Some(cached) = self.variables.cached.get(&args.variables_reference) {
            return Ok(VariablesResponseBody {
                variables: cached.clone(),
            });
        }

        // Check if this is an object/array reference to expand.
        if let Some(obj_ref) = self
            .variables
            .objects
            .get(&args.variables_reference)
            .cloned()
        {
            let variables = self.expand(&obj_ref).await?;
            self.variables
                .cached
                .insert(args.variables_reference, variables.clone());
            return Ok(VariablesResponseBody { variables });
        }

        // Must be a scope reference — look up cached ScopeInfo directly.
        let &ScopeInfo {
            thread_id,
            frame_id,
            class_id,
            method_id,
            code_index,
            ..
        } = self
            .variables
            .scopes
            .get(&args.variables_reference)
            .ok_or("unknown variables reference")?;

        let id_sizes = *self.id_sizes()?;

        // Get the variable table for this method.
        let var_table_reply = self
            .conn()?
            .send_command(Command::method_variable_table_with_generic(
                class_id,
                method_id,
                id_sizes.reference_type_id_size,
                id_sizes.method_id_size,
            ))
            .await;

        let variables = match var_table_reply {
            Ok(reply) => {
                let mut data = reply.data;
                let (_arg_count, var_infos) = replies::parse_variable_table_with_generic(&mut data)
                    .map_err(|e| e.to_string())?;

                // Filter to variables visible at current code index.
                let visible: Vec<&VariableInfo> = var_infos
                    .iter()
                    .filter(|v| {
                        code_index >= v.code_index
                            && code_index < v.code_index + u64::from(v.length)
                    })
                    .collect();

                if visible.is_empty() {
                    return Ok(VariablesResponseBody {
                        variables: Vec::new(),
                    });
                }

                // Build slot requests.
                let slots: Vec<(i32, u8)> = visible
                    .iter()
                    .map(|v| (v.slot, sig_to_tag(&v.signature)))
                    .collect();

                let values_reply = self
                    .conn()?
                    .send_command(Command::stack_frame_get_values(
                        thread_id,
                        frame_id,
                        &slots,
                        id_sizes.object_id_size,
                        id_sizes.frame_id_size,
                    ))
                    .await
                    .map_err(|e| format!("failed to get values: {e}"))?;

                let mut values_data = values_reply.data;
                let values = replies::parse_stack_frame_values(&mut values_data, &id_sizes)
                    .map_err(|e| e.to_string())?;

                let mut result = Vec::with_capacity(visible.len());
                for (var_info, value) in visible.iter().zip(values.iter()) {
                    // Kotlin's inline-function bookkeeping locals (`$i$a$-forEach-...`).
                    if var_info.name.starts_with("$i$") {
                        continue;
                    }
                    let presented = self.present(value, thread_id).await;
                    result.push(Variable {
                        name: var_info.name.clone(),
                        value: presented.text,
                        // The runtime type when there is one, else the declared type.
                        type_: presented
                            .type_name
                            .or_else(|| Some(jni_sig_to_display(&var_info.signature))),
                        variables_reference: presented.variables_reference,
                    });
                }
                result
            }
            Err(e) => {
                tracing::debug!("variable table unavailable: {e}");
                Vec::new()
            }
        };

        Ok(VariablesResponseBody { variables })
    }

    /// Get fields for a class, using cache.
    async fn get_fields(&mut self, class_id: u64) -> Result<Vec<FieldInfo>, String> {
        if let Some(fields) = self.types.fields.get(&class_id) {
            return Ok(fields.clone());
        }

        let id_sizes = *self.id_sizes()?;
        let reply = self
            .conn()?
            .send_command(Command::reference_type_fields(
                class_id,
                id_sizes.reference_type_id_size,
            ))
            .await
            .map_err(|e| format!("failed to get fields: {e}"))?;

        let mut data = reply.data;
        let fields = replies::parse_fields(&mut data, &id_sizes).map_err(|e| e.to_string())?;
        self.types.fields.insert(class_id, fields.clone());
        Ok(fields)
    }

    /// Resolve a `String` object's actual contents via `StringReference.Value`.
    async fn resolve_string_value(&self, object_id: u64) -> Option<String> {
        let conn = self.conn().ok()?;
        let id_sizes = conn.id_sizes();
        let cmd = Command::string_value(object_id, id_sizes.object_id_size);
        let reply = conn.send_command(cmd).await.ok()?;
        let mut data = reply.data;
        replies::parse_string_value(&mut data).ok()
    }

    // ---- Handler: disconnect ----

    pub async fn handle_disconnect(&mut self, args: DisconnectArguments) {
        // Drop the JDWP connection.
        self.connection = None;

        // Kill the child process if we launched it.
        if let Some(mut child) = self.child.take() {
            if args.terminate_debuggee.unwrap_or(true) {
                let _ = child.kill().await;
            }
        }
    }

    // ---- Event handling ----

    /// Take ownership of the event receiver for use in `tokio::select!`.
    pub fn take_event_receiver(&mut self) -> Option<tokio::sync::mpsc::Receiver<EventSet>> {
        let conn = self.connection.as_mut()?;
        Some(conn.take_event_receiver())
    }

    /// Take ownership of the debuggee output receiver for use in
    /// `tokio::select!`. Only set after a `launch` (attach doesn't spawn a
    /// process, so there's nothing to drain).
    pub const fn take_output_receiver(&mut self) -> Option<mpsc::Receiver<OutputChunk>> {
        self.output_rx.take()
    }

    // ---- Internal helpers ----

    async fn resolve_class(&mut self, signature: &str) -> Option<u64> {
        if let Some(&id) = self.types.by_signature.get(signature) {
            return Some(id);
        }

        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::vm_classes_by_signature(signature))
            .await
            .ok()?;

        let mut data = reply.data;
        let classes = replies::parse_classes_by_signature(&mut data, &id_sizes).ok()?;

        // Take the first prepared class.
        let class = classes.into_iter().find(|c| c.status & 2 != 0)?; // 2 = prepared

        self.types
            .by_signature
            .insert(signature.to_owned(), class.type_id);
        self.types
            .signature_by_id
            .insert(class.type_id, signature.to_owned());
        Some(class.type_id)
    }

    async fn get_methods(&mut self, class_id: u64) -> Option<Vec<MethodInfo>> {
        if let Some(methods) = self.types.methods.get(&class_id) {
            return Some(methods.clone());
        }

        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::reference_type_methods(
                class_id,
                id_sizes.reference_type_id_size,
            ))
            .await
            .ok()?;

        let mut data = reply.data;
        let methods = replies::parse_methods(&mut data, &id_sizes).ok()?;
        self.types.methods.insert(class_id, methods.clone());
        Some(methods)
    }

    async fn get_line_table(
        &mut self,
        class_id: u64,
        method_id: u64,
    ) -> Option<Vec<LineTableEntry>> {
        let key = (class_id, method_id);
        if let Some(entries) = self.types.line_tables.get(&key) {
            return Some(entries.clone());
        }

        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::method_line_table(
                class_id,
                method_id,
                id_sizes.reference_type_id_size,
                id_sizes.method_id_size,
            ))
            .await
            .ok()?;

        let mut data = reply.data;
        let table = replies::parse_line_table(&mut data).ok()?;
        self.types.line_tables.insert(key, table.lines.clone());
        Some(table.lines)
    }

    async fn resolve_frame_info(&mut self, location: &Location) -> (String, Option<Source>, u64) {
        let methods = self.get_methods(location.class_id).await;
        let method_name = methods
            .as_ref()
            .and_then(|ms| ms.iter().find(|m| m.method_id == location.method_id))
            .map_or_else(|| "<unknown>".to_string(), |m| m.name.clone());

        let line_table = self
            .get_line_table(location.class_id, location.method_id)
            .await;

        let line = line_table
            .as_ref()
            .and_then(|entries| {
                // Find the line for this code index (largest index <= location.index).
                entries
                    .iter()
                    .filter(|e| e.line_code_index <= location.index)
                    .max_by_key(|e| e.line_code_index)
            })
            .map_or(0, |e| e.line_number as u64);

        // Try to resolve source file.
        let source = self.resolve_source_file(location.class_id).await;

        (method_name, source, line)
    }

    async fn resolve_source_file(&mut self, class_id: u64) -> Option<Source> {
        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::reference_type_source_file(
                class_id,
                id_sizes.reference_type_id_size,
            ))
            .await
            .ok()?;

        let mut data = reply.data;
        let file_name = replies::parse_source_file(&mut data).ok()?;

        // Try to resolve full path using class signature and project root.
        let full_path = self.resolve_source_path(class_id, &file_name).await;

        Some(Source {
            name: Some(file_name),
            path: full_path.map(|p| p.to_string_lossy().into_owned()),
            source_reference: None,
        })
    }

    /// Resolve a source file to an absolute path using the class signature
    /// to derive the package directory structure.
    async fn resolve_source_path(&mut self, class_id: u64, file_name: &str) -> Option<PathBuf> {
        // Get the JNI signature for this class to derive the package path.
        let signature = self.get_class_signature(class_id).await?;

        // Convert signature like "Lcom/example/Foo;" to package path "com/example/".
        let inner = signature.strip_prefix('L')?.strip_suffix(';')?;
        let package_path = match inner.rfind('/') {
            Some(idx) => &inner[..=idx], // "com/example/"
            None => "",                  // default package
        };

        // Roots learned from files the client set breakpoints in.
        for root in &self.source_roots {
            let candidate = root.join(package_path).join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        let project_root = self.project_root.clone()?;
        let mut roots: Vec<PathBuf> = ["main", "test"]
            .iter()
            .flat_map(|set| hyperion_layout::source_dirs(&project_root, set))
            .collect();
        roots.push(project_root.join("src"));
        roots.extend(self.discovered_source_roots(&project_root).clone());

        for root in &roots {
            let candidate = root.join(package_path).join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // Fallback: try just the file name in the project root.
        let candidate = project_root.join(file_name);
        candidate.is_file().then_some(candidate)
    }

    /// Source roots of a multi-module project (`<module>/src/<set>/<lang>` and
    /// `<module>/src`), found once by a bounded walk of the project.
    fn discovered_source_roots(&mut self, project_root: &std::path::Path) -> &Vec<PathBuf> {
        self.discovered_roots
            .get_or_insert_with(|| hyperion_layout::discover_source_roots(project_root))
    }

    /// Get or fetch the JNI signature for a class by its reference type ID.
    async fn get_class_signature(&mut self, class_id: u64) -> Option<String> {
        if let Some(sig) = self.types.signature_by_id.get(&class_id) {
            return Some(sig.clone());
        }

        // Fetch via JDWP ReferenceType.Signature command.
        let conn = self.connection.as_ref()?;
        let id_sizes = *conn.id_sizes();

        let reply = conn
            .send_command(Command::reference_type_signature(
                class_id,
                id_sizes.reference_type_id_size,
            ))
            .await
            .ok()?;

        let mut data = reply.data;
        let signature = replies::parse_reference_type_signature(&mut data).ok()?;
        self.types
            .signature_by_id
            .insert(class_id, signature.clone());
        Some(signature)
    }
}

/// Does the hit count satisfy a DAP `hitCondition`? `N` and `==N` stop on
/// exactly the Nth hit; `>N`, `>=N`, `<N`, `<=N` compare; `%N` stops on every
/// Nth hit. An unparsable condition never blocks the stop.
pub(crate) fn hit_condition_met(condition: &str, hits: u64) -> bool {
    let condition = condition.trim();
    let (operator, number) = ["==", ">=", "<=", ">", "<", "%", "="]
        .iter()
        .find_map(|op| condition.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("==", condition));
    let Ok(n) = number.trim().parse::<u64>() else {
        return true;
    };
    match operator {
        "==" | "=" => hits == n,
        ">=" => hits >= n,
        "<=" => hits <= n,
        ">" => hits > n,
        "<" => hits < n,
        "%" => n != 0 && hits.is_multiple_of(n),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_conditions_follow_the_dap_operators() {
        assert!(hit_condition_met("3", 3));
        assert!(!hit_condition_met("3", 4));
        assert!(hit_condition_met(">= 2", 5));
        assert!(!hit_condition_met("> 2", 2));
        assert!(hit_condition_met("<3", 2));
        assert!(hit_condition_met("%2", 4));
        assert!(!hit_condition_met("%2", 3));
        assert!(hit_condition_met("nonsense", 1));
    }

    #[test]
    fn jdk_and_kotlin_runtime_frames_are_step_filtered() {
        assert!(step_filtered_signature("Ljava/lang/Double;"));
        assert!(step_filtered_signature("Ljdk/internal/misc/Unsafe;"));
        assert!(step_filtered_signature("Lkotlin/jvm/internal/Intrinsics;"));
        assert!(!step_filtered_signature("Lcom/example/Main;"));
        assert!(!step_filtered_signature(
            "Lkotlin/collections/CollectionsKt;"
        ));
    }
}
