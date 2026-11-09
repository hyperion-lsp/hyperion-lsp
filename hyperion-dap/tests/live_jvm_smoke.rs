// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Live-JVM end-to-end smoke test for the DAP session.
//!
//! Drives `hyperion_dap::server::run` in-process over an in-memory duplex
//! pipe — the exact same request/response/event loop that the standalone
//! `hyperion-dap` binary (`hyperion-dap/src/main.rs`) and `hyperion-lsp`'s
//! `dap` subcommand (`hyperion-lsp/src/main.rs:416`) drive over real stdio.
//! Only the debuggee JVM launched by `handle_launch` is a real subprocess.
//!
//! This is, as far as this repo's history shows, the first test that ever
//! spawns a real JVM and steps through it via `hyperion-dap`/`hyperion-jdwp`.
//!
//! Self-skips (eprintln + return) if `java`/`javac` are not on `PATH`,
//! following the same pattern as `hyperion-eval/tests/class_mode.rs`.

use std::collections::VecDeque;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::time::timeout;

/// Every wait in this test is bounded so a regression makes the test fail
/// fast instead of hanging CI.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Line of `System.out.println(...)` inside `compute`, 1-based. See
/// `MAIN_SRC` below — keep in sync if the fixture changes.
const BREAKPOINT_LINE: u64 = 16;

fn tool_available(tool: &str) -> bool {
    StdCommand::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap_or_else(|e| panic!("write {name}: {e}"));
}

fn javac_compile(dir: &Path, sources: &[&str]) {
    let mut cmd = StdCommand::new("javac");
    // `-g` (full debug info) is required: javac's default is `-g:source,lines`,
    // which omits the LocalVariableTable attribute entirely. Without it,
    // `Method.VariableTableWithGeneric` comes back empty and `variables`
    // requests silently return no locals — a real trap for anyone testing
    // against a release-mode build.
    cmd.arg("-g").arg("-d").arg(dir);
    for s in sources {
        cmd.arg(dir.join(s));
    }
    let out = cmd.output().expect("failed to spawn javac");
    assert!(
        out.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A tiny program with a method (`compute`) that has three kinds of locals
/// exercised by the test: an `int` parameter, a `String` local, and an
/// object (`Box`) local with an `int` field — called in a 10-iteration loop
/// so both plain and conditional breakpoints have something to land on.
const MAIN_SRC: &str = r#"public class Main {
    static class Box {
        int value;
        Box(int value) { this.value = value; }
    }

    public static void main(String[] args) throws Exception {
        for (int i = 0; i < 10; i++) {
            compute(i);
        }
    }

    static void compute(int i) {
        String label = "item-" + i;
        Box box = new Box(i * 10);
        System.out.println(label + " " + box.value);
    }
}
"#;

/// Minimal in-process DAP client driving `hyperion_dap::server::run` over an
/// in-memory duplex pipe. Tracks out-of-order events the way a real DAP
/// client must, since `stopped`/`continued`/`breakpoint` events can arrive
/// interleaved with request/response pairs.
struct DapClient {
    writer: tokio::io::DuplexStream,
    reader: BufReader<tokio::io::DuplexStream>,
    seq: u64,
    pending_events: VecDeque<Value>,
    server: tokio::task::JoinHandle<()>,
}

impl DapClient {
    fn spawn() -> Self {
        // Two duplex pipes: one carries client -> server bytes (what the
        // server reads as "stdin"), the other server -> client bytes (what
        // the server writes as "stdout").
        let (client_to_server_w, server_stdin_r) = tokio::io::duplex(1 << 20);
        let (server_stdout_w, client_from_server_r) = tokio::io::duplex(1 << 20);

        let server = tokio::spawn(async move {
            let reader = BufReader::new(server_stdin_r);
            if let Err(e) = hyperion_dap::server::run(reader, server_stdout_w).await {
                eprintln!("DAP server exited with error: {e}");
            }
        });

        Self {
            writer: client_to_server_w,
            reader: BufReader::new(client_from_server_r),
            seq: 1,
            pending_events: VecDeque::new(),
            server,
        }
    }

    async fn read_one(&mut self, awaiting: &str) -> Value {
        let json = timeout(
            TIMEOUT,
            hyperion_dap::protocol::read_message(&mut self.reader),
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "timed out waiting for {awaiting}: {error}; queued events: {:?}; server finished: {}",
                self.pending_events,
                self.server.is_finished()
            )
        })
        .expect("io error reading DAP message")
        .expect("DAP server closed its output (EOF) unexpectedly");
        serde_json::from_str(&json).expect("DAP message was not valid JSON")
    }

    /// Send a request and wait for its matching response, queuing any events
    /// observed in the meantime for a later `wait_event`/`wait_event_any`
    /// call. Panics if the response reports failure.
    async fn request(&mut self, command: &str, arguments: Value) -> Value {
        let msg = self.try_request(command, arguments).await;
        assert!(
            msg["success"].as_bool().unwrap_or(false),
            "{command} request failed: {:?}",
            msg["message"]
        );
        msg
    }

    /// Like `request`, but returns the response whether or not it succeeded.
    async fn try_request(&mut self, command: &str, arguments: Value) -> Value {
        let seq = self.seq;
        self.seq += 1;
        let msg = json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": arguments,
        })
        .to_string();

        timeout(
            TIMEOUT,
            hyperion_dap::protocol::write_message(&mut self.writer, &msg),
        )
        .await
        .expect("timed out writing DAP request")
        .expect("io error writing DAP request");

        loop {
            let msg = self
                .read_one(&format!("response to {command} (seq={seq})"))
                .await;
            match msg["type"].as_str() {
                Some("event") => self.pending_events.push_back(msg),
                Some("response") if msg["request_seq"] == json!(seq) => return msg,
                _ => panic!("unexpected message while waiting for '{command}' response: {msg:?}"),
            }
        }
    }

    /// Wait for the next event named `name`.
    async fn wait_event(&mut self, name: &str) -> Value {
        self.wait_event_any(&[name]).await
    }

    /// Wait for the next event whose name is one of `names`.
    async fn wait_event_any(&mut self, names: &[&str]) -> Value {
        if let Some(pos) = self
            .pending_events
            .iter()
            .position(|e| names.iter().any(|n| e["event"] == json!(*n)))
        {
            return self.pending_events.remove(pos).unwrap();
        }
        loop {
            let msg = self.read_one(&format!("event in {names:?}")).await;
            match msg["type"].as_str() {
                Some("event") if names.iter().any(|n| msg["event"] == json!(*n)) => return msg,
                Some("event") => self.pending_events.push_back(msg),
                _ => panic!("unexpected message while waiting for one of {names:?}: {msg:?}"),
            }
        }
    }

    /// Wait for an `output` event whose `body.output` contains `needle`,
    /// queuing any other events observed along the way (e.g. `thread`,
    /// `continued`, other `output` lines) for later `wait_event` calls.
    async fn wait_output_containing(&mut self, needle: &str) -> Value {
        let matches = |e: &Value| {
            e["event"] == json!("output")
                && e["body"]["output"]
                    .as_str()
                    .is_some_and(|s| s.contains(needle))
        };

        if let Some(pos) = self.pending_events.iter().position(matches) {
            return self.pending_events.remove(pos).unwrap();
        }
        loop {
            let msg = self
                .read_one(&format!("output containing {needle:?}"))
                .await;
            match msg["type"].as_str() {
                Some("event") if matches(&msg) => return msg,
                Some("event") => self.pending_events.push_back(msg),
                _ => panic!(
                    "unexpected message while waiting for output containing {needle:?}: {msg:?}"
                ),
            }
        }
    }
}

impl Drop for DapClient {
    fn drop(&mut self) {
        // Belt-and-suspenders cleanup: aborting the server task drops its
        // `DebugSession`, which drops the `tokio::process::Child` for the
        // debuggee JVM. `handle_launch` sets `kill_on_drop(true)` on that
        // child, so this kills the JVM even if the test panics (e.g. on a
        // failed assertion) before reaching an explicit `disconnect` request.
        self.server.abort();
    }
}

fn setup_fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "Main.java", MAIN_SRC);
    javac_compile(&dir, &["Main.java"]);
    assert!(dir.join("Main.class").exists());
    assert!(dir.join("Main$Box.class").exists());
    dir
}

/// A program whose `thrower` method throws an uncaught `RuntimeException`,
/// used by the exception-breakpoint end-to-end test.
const MAIN_THROW_SRC: &str = r#"public class MainThrow {
    public static void main(String[] args) {
        thrower();
    }

    static void thrower() {
        throw new RuntimeException("boom");
    }
}
"#;

/// Line of the `throw` statement inside `thrower`, 1-based. Keep in sync with
/// `MAIN_THROW_SRC` above.
const THROW_LINE: u64 = 7;

fn setup_throw_fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_file(&dir, "MainThrow.java", MAIN_THROW_SRC);
    javac_compile(&dir, &["MainThrow.java"]);
    assert!(dir.join("MainThrow.class").exists());
    dir
}

async fn do_initialize_and_launch(client: &mut DapClient, dir: &Path) {
    do_initialize_and_launch_class(client, dir, "Main").await;
}

/// Same as `do_initialize_and_launch`, but for a main class other than the
/// default `Main` fixture (e.g. the exception-throwing fixture below).
async fn do_initialize_and_launch_class(client: &mut DapClient, dir: &Path, main_class: &str) {
    client
        .request(
            "initialize",
            json!({
                "clientID": "hyperion-dap-test",
                "linesStartAt1": true,
                "columnsStartAt1": true,
            }),
        )
        .await;
    client.wait_event("initialized").await;

    let dir_str = dir.to_string_lossy().to_string();
    client
        .request(
            "launch",
            json!({
                "mainClass": main_class,
                "classpath": dir_str,
                "cwd": dir_str,
                "projectRoot": dir_str,
            }),
        )
        .await;
}

/// Bind an ephemeral port and immediately release it, the same way
/// `handle_launch` picks a free JDWP port before spawning the JVM.
async fn find_free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn live_jvm_smoke_breakpoint_step_and_variables() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP live_jvm_smoke_breakpoint_step_and_variables: java/javac not on PATH");
        return;
    }

    let dir = setup_fixture("hyp_dap_smoke_basic");
    let mut client = DapClient::spawn();

    do_initialize_and_launch(&mut client, &dir).await;

    let source_path = dir.join("Main.java").to_string_lossy().to_string();
    let bp_resp = client
        .request(
            "setBreakpoints",
            json!({
                "source": { "path": source_path, "name": "Main.java" },
                "breakpoints": [{ "line": BREAKPOINT_LINE }],
            }),
        )
        .await;
    let breakpoints = bp_resp["body"]["breakpoints"].as_array().unwrap();
    assert_eq!(
        breakpoints.len(),
        1,
        "expected exactly one breakpoint result"
    );

    client.request("configurationDone", json!({})).await;

    // Wait for the stop — the breakpoint may first go through the deferred
    // path (class not loaded until the VM resumes past its initial suspend),
    // resolving via a ClassPrepare event before the real hit.
    let stopped = client.wait_event("stopped").await;
    assert_eq!(stopped["body"]["reason"], json!("breakpoint"));
    let thread_id = stopped["body"]["threadId"]
        .as_u64()
        .expect("threadId in stopped event");

    let threads_resp = client.request("threads", json!({})).await;
    let threads = threads_resp["body"]["threads"].as_array().unwrap();
    assert!(!threads.is_empty(), "expected at least one thread");

    let st_resp = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    assert!(!frames.is_empty(), "expected at least one stack frame");
    let top_frame = &frames[0];
    let frame_id = top_frame["id"].as_u64().unwrap();
    assert_eq!(top_frame["line"].as_u64(), Some(BREAKPOINT_LINE));
    assert_eq!(top_frame["name"].as_str(), Some("compute"));

    let scopes_resp = client
        .request("scopes", json!({ "frameId": frame_id }))
        .await;
    let scopes = scopes_resp["body"]["scopes"].as_array().unwrap();
    assert!(!scopes.is_empty(), "expected at least one scope");
    let locals_ref = scopes[0]["variablesReference"].as_u64().unwrap();

    let vars_resp = client
        .request("variables", json!({ "variablesReference": locals_ref }))
        .await;
    let vars = vars_resp["body"]["variables"].as_array().unwrap().clone();
    let find = |name: &str| -> Value {
        vars.iter()
            .find(|v| v["name"] == json!(name))
            .unwrap_or_else(|| panic!("variable '{name}' not found in {vars:?}"))
            .clone()
    };

    let i_var = find("i");
    assert_eq!(
        i_var["value"],
        json!("0"),
        "expected int local i == 0 on the first breakpoint hit"
    );

    let label_var = find("label");
    assert_eq!(
        label_var["value"],
        json!("\"item-0\""),
        "expected String local label == \"item-0\""
    );

    let box_var = find("box");
    let box_ref = box_var["variablesReference"]
        .as_u64()
        .expect("box (an object local) should have a non-zero variablesReference");
    assert_ne!(box_ref, 0);

    let box_fields_resp = client
        .request("variables", json!({ "variablesReference": box_ref }))
        .await;
    let box_fields = box_fields_resp["body"]["variables"].as_array().unwrap();
    let value_field = box_fields
        .iter()
        .find(|v| v["name"] == json!("value"))
        .expect("box.value field");
    assert_eq!(
        value_field["value"],
        json!("0"),
        "expected box.value == 0 (i * 10 for i == 0)"
    );

    // next (step over): the line must move off the breakpoint line.
    client
        .request("next", json!({ "threadId": thread_id }))
        .await;
    client.wait_event("continued").await;
    let stepped = client.wait_event("stopped").await;
    assert_eq!(stepped["body"]["reason"], json!("step"));

    let st_resp2 = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frames2 = st_resp2["body"]["stackFrames"].as_array().unwrap();
    let new_line = frames2[0]["line"].as_u64().unwrap();
    assert_ne!(
        new_line, BREAKPOINT_LINE,
        "expected the line to advance after a step-over"
    );

    // continue, then keep resuming past the remaining 9 breakpoint hits
    // (i = 1..9) until the program terminates.
    client
        .request("continue", json!({ "threadId": thread_id }))
        .await;
    client.wait_event("continued").await;

    let next_stop = client.wait_event("stopped").await;
    assert_eq!(
        next_stop["body"]["reason"], "breakpoint",
        "the completed step request remained active after continue: {next_stop}"
    );
    let next_thread = next_stop["body"]["threadId"].as_u64().unwrap();
    client
        .request("continue", json!({ "threadId": next_thread }))
        .await;
    client.wait_event("continued").await;

    loop {
        let ev = client
            .wait_event_any(&["stopped", "terminated", "exited"])
            .await;
        match ev["event"].as_str().unwrap() {
            "stopped" => {
                let tid = ev["body"]["threadId"].as_u64().unwrap();
                client.request("continue", json!({ "threadId": tid })).await;
                client.wait_event("continued").await;
            }
            _ => break,
        }
    }

    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn conditional_breakpoint_stops_only_when_condition_true() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP conditional_breakpoint_stops_only_when_condition_true: java/javac not on PATH"
        );
        return;
    }

    let dir = setup_fixture("hyp_dap_smoke_conditional");
    let mut client = DapClient::spawn();

    do_initialize_and_launch(&mut client, &dir).await;

    let source_path = dir.join("Main.java").to_string_lossy().to_string();
    client
        .request(
            "setBreakpoints",
            json!({
                "source": { "path": source_path, "name": "Main.java" },
                "breakpoints": [{ "line": BREAKPOINT_LINE, "condition": "i == 3" }],
            }),
        )
        .await;

    client.request("configurationDone", json!({})).await;

    // If conditional breakpoints didn't actually filter, this would stop on
    // the very first hit (i == 0) instead of waiting for i == 3.
    let stopped = client.wait_event("stopped").await;
    assert_eq!(stopped["body"]["reason"], json!("breakpoint"));
    let thread_id = stopped["body"]["threadId"].as_u64().unwrap();

    let st_resp = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    let frame_id = frames[0]["id"].as_u64().unwrap();

    let scopes_resp = client
        .request("scopes", json!({ "frameId": frame_id }))
        .await;
    let locals_ref = scopes_resp["body"]["scopes"][0]["variablesReference"]
        .as_u64()
        .unwrap();

    let vars_resp = client
        .request("variables", json!({ "variablesReference": locals_ref }))
        .await;
    let vars = vars_resp["body"]["variables"].as_array().unwrap();
    let i_var = vars
        .iter()
        .find(|v| v["name"] == json!("i"))
        .expect("local 'i' present at the conditional breakpoint");
    assert_eq!(
        i_var["value"],
        json!("3"),
        "conditional breakpoint 'i == 3' should only stop once i == 3, but got: {vars:?}"
    );

    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn replacing_deferred_breakpoints_removes_old_requests() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP replacing_deferred_breakpoints_removes_old_requests: java/javac not on PATH"
        );
        return;
    }

    let dir = setup_fixture("hyp_dap_smoke_replace_deferred");
    let mut client = DapClient::spawn();
    do_initialize_and_launch(&mut client, &dir).await;

    let source_path = dir.join("Main.java").to_string_lossy().to_string();
    let source = json!({ "path": source_path, "name": "Main.java" });
    let initial = client
        .request(
            "setBreakpoints",
            json!({
                "source": source,
                "breakpoints": [{ "line": BREAKPOINT_LINE }],
            }),
        )
        .await;
    assert_eq!(initial["body"]["breakpoints"][0]["verified"], false);

    let replacement = client
        .request(
            "setBreakpoints",
            json!({ "source": source, "breakpoints": [] }),
        )
        .await;
    assert_eq!(replacement["body"]["breakpoints"], json!([]));

    client.request("configurationDone", json!({})).await;
    let event = client.wait_event_any(&["stopped", "terminated"]).await;
    assert_eq!(
        event["event"], "terminated",
        "removed breakpoint still stopped the JVM: {event}"
    );

    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn output_events_forwarded_from_launched_jvm() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP output_events_forwarded_from_launched_jvm: java/javac not on PATH");
        return;
    }

    let dir = setup_fixture("hyp_dap_smoke_output");
    let mut client = DapClient::spawn();

    do_initialize_and_launch(&mut client, &dir).await;

    // No breakpoints set — just resume and let the program run to
    // completion, exercising the launch-path stdout drain -> DAP `output`
    // event forwarding described in `handle_launch`/`drain_pipe`.
    client.request("configurationDone", json!({})).await;

    // `compute(0)` prints exactly this line (see `MAIN_SRC` above); if
    // output events weren't wired up to the debuggee's stdout, this would
    // time out inside `read_one` instead of ever finding a match.
    let output = client.wait_output_containing("item-0 0").await;
    assert_eq!(output["body"]["category"], json!("stdout"));

    // The program finishes on its own in well under a second; `disconnect`
    // just confirms the session tears down cleanly afterward.
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn attach_path_end_to_end() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP attach_path_end_to_end: java/javac not on PATH");
        return;
    }

    let dir = setup_fixture("hyp_dap_smoke_attach");

    // Unlike `handle_launch` (which spawns the JVM itself and owns its
    // lifecycle), `handle_attach` connects to an independently managed JVM.
    // Start that JVM shortly after the attach request so this also pins the
    // bounded retry behavior needed when an editor and debuggee start at the
    // same time.
    let port = find_free_port().await;

    let mut client = DapClient::spawn();
    client
        .request(
            "initialize",
            json!({
                "clientID": "hyperion-dap-test",
                "linesStartAt1": true,
                "columnsStartAt1": true,
            }),
        )
        .await;
    client.wait_event("initialized").await;

    let child_dir = dir.clone();
    let child_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        tokio::process::Command::new("java")
            .current_dir(&child_dir)
            .kill_on_drop(true)
            .arg(format!(
                "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{port}"
            ))
            .arg("-cp")
            .arg(&child_dir)
            .arg("Main")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn attach-target JVM")
    });

    let dir_str = dir.to_string_lossy().to_string();
    client
        .request(
            "attach",
            json!({
                "host": "127.0.0.1",
                "port": port,
                "projectRoot": dir_str,
            }),
        )
        .await;
    let mut child = child_task.await.expect("attach-target spawn task panicked");

    let source_path = dir.join("Main.java").to_string_lossy().to_string();
    client
        .request(
            "setBreakpoints",
            json!({
                "source": { "path": source_path, "name": "Main.java" },
                "breakpoints": [{ "line": BREAKPOINT_LINE }],
            }),
        )
        .await;

    client.request("configurationDone", json!({})).await;

    // Same deferred-breakpoint dance as the launch tests: the JVM was
    // suspended before its main class was prepared, so this may resolve via
    // ClassPrepare before the real breakpoint hit.
    let stopped = client.wait_event("stopped").await;
    assert_eq!(stopped["body"]["reason"], json!("breakpoint"));
    let thread_id = stopped["body"]["threadId"]
        .as_u64()
        .expect("threadId in stopped event");

    let st_resp = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    assert!(
        !frames.is_empty(),
        "expected at least one stack frame over attach"
    );
    let top_frame = &frames[0];
    assert_eq!(top_frame["line"].as_u64(), Some(BREAKPOINT_LINE));
    let frame_id = top_frame["id"].as_u64().unwrap();

    let scopes_resp = client
        .request("scopes", json!({ "frameId": frame_id }))
        .await;
    let locals_ref = scopes_resp["body"]["scopes"][0]["variablesReference"]
        .as_u64()
        .unwrap();
    let vars_resp = client
        .request("variables", json!({ "variablesReference": locals_ref }))
        .await;
    let vars = vars_resp["body"]["variables"].as_array().unwrap();
    let i_var = vars
        .iter()
        .find(|v| v["name"] == json!("i"))
        .expect("local 'i' present over attach");
    assert_eq!(
        i_var["value"],
        json!("0"),
        "expected int local i == 0 on the first breakpoint hit over attach"
    );

    // `disconnect` on the attach path intentionally does NOT kill the
    // debuggee: `DebugSession::handle_disconnect` only kills `self.child`,
    // which stays `None` for `attach` (only `handle_launch` sets it). That's
    // correct DAP semantics — attach means "you don't own this process's
    // lifecycle" — but it means *this test* (which spawned the JVM above,
    // playing the role of "whoever started the debuggee") is responsible
    // for killing it afterward, exactly as a real caller of the attach path
    // would be.
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;

    child
        .kill()
        .await
        .expect("failed to kill attach-target JVM");
    // Reaping the process (not just sending the kill signal) is the actual
    // proof there's no orphan left behind: this only returns once the OS has
    // confirmed the process is gone.
    let status = timeout(TIMEOUT, child.wait())
        .await
        .expect("timed out waiting for attach-target JVM to exit")
        .expect("wait() on attach-target JVM failed");
    assert!(
        !status.success(),
        "expected the killed attach-target JVM to report a non-success exit status, got: {status:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uncaught_exception_stops_at_throw_site() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP uncaught_exception_stops_at_throw_site: java/javac not on PATH");
        return;
    }

    let dir = setup_throw_fixture("hyp_dap_smoke_exception");
    let mut client = DapClient::spawn();

    do_initialize_and_launch_class(&mut client, &dir, "MainThrow").await;

    // Ask only for uncaught exceptions — mirrors what a real DAP client
    // sends when just the "Uncaught Exceptions" checkbox is ticked in the
    // breakpoints panel (see `Capabilities::exception_breakpoint_filters`).
    client
        .request(
            "setExceptionBreakpoints",
            json!({ "filters": ["uncaught"] }),
        )
        .await;

    client.request("configurationDone", json!({})).await;

    let stopped = client.wait_event("stopped").await;
    assert_eq!(stopped["body"]["reason"], json!("exception"));
    let description = stopped["body"]["description"]
        .as_str()
        .expect("stopped event should carry an exception description");
    assert!(
        description.contains("RuntimeException"),
        "expected description to name the exception type, got: {description:?}"
    );
    assert!(
        description.contains("boom"),
        "expected description to include the exception message, got: {description:?}"
    );
    let thread_id = stopped["body"]["threadId"]
        .as_u64()
        .expect("threadId in stopped event");

    // The thread is suspended at the throw site itself (JDWP fires EXCEPTION
    // at the throw, not the eventual catch/crash point), so the top frame
    // must be `thrower` at the `throw` line.
    let st_resp = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frames = st_resp["body"]["stackFrames"].as_array().unwrap();
    assert!(
        !frames.is_empty(),
        "expected at least one stack frame at the exception stop"
    );
    assert_eq!(frames[0]["line"].as_u64(), Some(THROW_LINE));
    assert_eq!(frames[0]["name"].as_str(), Some("thrower"));

    let info_resp = client
        .request("exceptionInfo", json!({ "threadId": thread_id }))
        .await;
    assert_eq!(info_resp["body"]["breakMode"], json!("unhandled"));
    assert_eq!(
        info_resp["body"]["exceptionId"],
        json!("java.lang.RuntimeException")
    );
    assert_eq!(info_resp["body"]["details"]["message"], json!("boom"));

    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

// ---------------------------------------------------------------------------
// Breakpoints in nested/inner/anonymous/local classes
// and lambdas, exited/terminated events, launch failures, terminate, and
// method-invoking evaluate.
// ---------------------------------------------------------------------------

/// Compile `files` (name -> source) into a fresh directory with full debug info.
fn setup_project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, source) in files {
        write_file(&dir, file, source);
    }
    let names: Vec<&str> = files.iter().map(|(file, _)| *file).collect();
    javac_compile(&dir, &names);
    dir
}

async fn set_breakpoints(client: &mut DapClient, dir: &Path, file: &str, lines: &[u64]) -> Value {
    let path = dir.join(file).to_string_lossy().to_string();
    let breakpoints: Vec<Value> = lines.iter().map(|line| json!({ "line": line })).collect();
    client
        .request(
            "setBreakpoints",
            json!({ "source": { "path": path, "name": file }, "breakpoints": breakpoints }),
        )
        .await
}

/// Wait for the next `stopped` event and return (threadId, top frame).
async fn next_stop(client: &mut DapClient) -> (u64, Value) {
    let stopped = client.wait_event("stopped").await;
    let thread_id = stopped["body"]["threadId"].as_u64().expect("threadId");
    let stack = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    (thread_id, stack["body"]["stackFrames"][0].clone())
}

async fn resume(client: &mut DapClient, thread_id: u64) {
    client
        .request("continue", json!({ "threadId": thread_id }))
        .await;
    client.wait_event("continued").await;
}

/// Everything up to and including the (single) `terminated` event, and proof
/// that no second `terminated` follows.
async fn events_until_terminated(client: &mut DapClient) -> Vec<Value> {
    let mut events = Vec::new();
    loop {
        let event = client
            .wait_event_any(&["exited", "terminated", "output"])
            .await;
        let name = event["event"].as_str().unwrap().to_owned();
        events.push(event);
        if name == "terminated" {
            break;
        }
    }
    // The JDWP connection closing after VM death used to produce a second one.
    match timeout(
        Duration::from_millis(800),
        hyperion_dap::protocol::read_message(&mut client.reader),
    )
    .await
    {
        Err(_) | Ok(Ok(None)) => {}
        Ok(Ok(Some(extra))) => {
            let extra: Value = serde_json::from_str(&extra).unwrap();
            assert_ne!(extra["event"], "terminated", "duplicate terminated event");
        }
        Ok(Err(error)) => panic!("read error: {error}"),
    }
    events
}

const NEST_SRC: &str = r#"public class Nest {
    static class Inner {
        int twice(int x) {
            return x * 2;
        }
    }
    class Member {
        int thrice(int x) {
            return x * 3;
        }
    }
    public static void main(String[] args) {
        Nest nest = new Nest();
        Runnable anon = new Runnable() {
            public void run() {
                System.out.println("anon " + new Inner().twice(1));
            }
        };
        anon.run();
        java.util.function.IntUnaryOperator lambda = v -> {
            return v + 100;
        };
        System.out.println(lambda.applyAsInt(1));
        System.out.println(nest.new Member().thrice(2));
        class Local {
            int four() {
                return 4;
            }
        }
        System.out.println(new Local().four());
    }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn breakpoints_bind_in_nested_inner_anonymous_local_classes_and_lambdas() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP breakpoints_bind_in_nested_inner_anonymous_local_classes_and_lambdas: no JDK"
        );
        return;
    }
    let dir = setup_project("hyp_dap_nest", &[("Nest.java", NEST_SRC)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Nest").await;

    // Lines: Inner.twice, Member.thrice, anonymous run(), lambda body, Local.four.
    let lines = [4, 9, 16, 21, 27];
    let response = set_breakpoints(&mut client, &dir, "Nest.java", &lines).await;
    let breakpoints = response["body"]["breakpoints"].as_array().unwrap();
    assert_eq!(breakpoints.len(), lines.len());
    client.request("configurationDone", json!({})).await;

    // The classes load as the program runs; each breakpoint must bind on its
    // ClassPrepare and hit in program order: anonymous run() (16), Inner.twice
    // (4), lambda (21), Member.thrice (9), Local.four (27).
    let expected = [
        (16, "run"),
        (4, "twice"),
        (21, "lambda$main$0"),
        (9, "thrice"),
        (27, "four"),
    ];
    for (line, method) in expected {
        let (thread_id, frame) = next_stop(&mut client).await;
        assert_eq!(frame["line"].as_u64(), Some(line), "stopped at {frame}");
        assert_eq!(frame["name"].as_str(), Some(method), "stopped at {frame}");
        assert_eq!(frame["source"]["name"], "Nest.java");
        assert_eq!(
            frame["source"]["path"].as_str(),
            Some(dir.join("Nest.java").to_string_lossy().as_ref()),
            "frame source must resolve to the file the breakpoint was set in"
        );
        resume(&mut client, thread_id).await;
    }
    let events = events_until_terminated(&mut client).await;
    assert!(events.iter().any(|e| e["event"] == "terminated"));
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unloaded_class_breakpoints_are_reported_verified_when_the_class_loads() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP unloaded_class_breakpoints_are_reported_verified_when_the_class_loads: no JDK"
        );
        return;
    }
    let dir = setup_project("hyp_dap_nest_verified", &[("Nest.java", NEST_SRC)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Nest").await;
    let response = set_breakpoints(&mut client, &dir, "Nest.java", &[4]).await;
    assert_eq!(response["body"]["breakpoints"][0]["verified"], false);
    let id = response["body"]["breakpoints"][0]["id"].clone();
    client.request("configurationDone", json!({})).await;

    let changed = client.wait_event("breakpoint").await;
    assert_eq!(changed["body"]["breakpoint"]["verified"], true);
    assert_eq!(changed["body"]["breakpoint"]["id"], id);
    assert_eq!(changed["body"]["breakpoint"]["line"], 4);
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["name"], "twice");
    resume(&mut client, thread_id).await;
    events_until_terminated(&mut client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn breakpoint_on_a_line_without_code_slides_to_the_next_line_with_code() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP breakpoint_on_a_line_without_code_slides_to_the_next_line_with_code: no JDK"
        );
        return;
    }
    let dir = setup_fixture("hyp_dap_slide");
    let mut client = DapClient::spawn();
    do_initialize_and_launch(&mut client, &dir).await;
    // Line 13 of MAIN_SRC is the signature `static void compute(int i) {`
    // (no code); the first statement is on line 14.
    let response = set_breakpoints(&mut client, &dir, "Main.java", &[BREAKPOINT_LINE - 3]).await;
    assert_eq!(response["body"]["breakpoints"][0]["verified"], false);
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["name"], "compute");
    assert_eq!(frame["line"], 14);
    resume(&mut client, thread_id).await;
    client.request("disconnect", json!({})).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn exited_then_a_single_terminated_with_the_exit_code() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP exited_then_a_single_terminated_with_the_exit_code: no JDK");
        return;
    }
    let dir = setup_project(
        "hyp_dap_exit",
        &[(
            "Exit.java",
            "public class Exit { public static void main(String[] a) { System.out.println(\"bye\"); System.exit(3); } }\n",
        )],
    );
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Exit").await;
    client.request("configurationDone", json!({})).await;
    let events = events_until_terminated(&mut client).await;
    let names: Vec<&str> = events
        .iter()
        .filter(|e| e["event"] != "output")
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["exited", "terminated"], "events: {events:?}");
    let exited = events.iter().find(|e| e["event"] == "exited").unwrap();
    assert_eq!(exited["body"]["exitCode"], 3);
    assert!(
        events.iter().any(|e| e["event"] == "output"
            && e["body"]["output"]
                .as_str()
                .is_some_and(|o| o.contains("bye"))),
        "program output must arrive before terminated: {events:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_main_class_reports_the_jvm_error_and_exit_code() {
    if !tool_available("java") {
        eprintln!("SKIP missing_main_class_reports_the_jvm_error_and_exit_code: no JVM");
        return;
    }
    let dir = std::env::temp_dir().join("hyp_dap_no_main_class");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "does.not.Exist").await;
    client.request("configurationDone", json!({})).await;
    let events = events_until_terminated(&mut client).await;
    let stderr: String = events
        .iter()
        .filter(|e| e["event"] == "output" && e["body"]["category"] == "stderr")
        .map(|e| e["body"]["output"].as_str().unwrap())
        .collect();
    assert!(
        stderr.contains("Could not find or load main class"),
        "the JVM's own message must reach the client, got {stderr:?}"
    );
    let exited = events
        .iter()
        .find(|e| e["event"] == "exited")
        .expect("exited event");
    assert_eq!(exited["body"]["exitCode"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn jvm_that_dies_before_connecting_fails_the_launch_with_its_stderr() {
    if !tool_available("java") {
        eprintln!("SKIP jvm_that_dies_before_connecting_fails_the_launch_with_its_stderr: no JVM");
        return;
    }
    let dir = std::env::temp_dir().join("hyp_dap_bad_flag");
    std::fs::create_dir_all(&dir).unwrap();
    let mut client = DapClient::spawn();
    client
        .request("initialize", json!({ "linesStartAt1": true }))
        .await;
    client.wait_event("initialized").await;
    let started = std::time::Instant::now();
    let response = client
        .try_request(
            "launch",
            json!({
                "mainClass": "Main",
                "classpath": dir.to_string_lossy(),
                "cwd": dir.to_string_lossy(),
                "jvmArgs": ["-Xmx1x"],
            }),
        )
        .await;
    assert_eq!(response["success"], false, "{response}");
    let message = response["message"].as_str().unwrap();
    assert!(
        message.contains("Invalid maximum heap size") || message.contains("Xmx"),
        "the failed launch must carry the JVM's stderr, got {message:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "a dead JVM must be noticed immediately, not after the connect retries"
    );
    // ... and it is also shown as output.
    let output = client.wait_event("output").await;
    assert!(
        output["body"]["output"].as_str().unwrap().contains("Xmx")
            || output["body"]["output"].as_str().unwrap().contains("heap")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn terminate_request_ends_a_running_debuggee_with_exited_and_terminated() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!(
            "SKIP terminate_request_ends_a_running_debuggee_with_exited_and_terminated: no JDK"
        );
        return;
    }
    let dir = setup_project(
        "hyp_dap_terminate",
        &[(
            "Sleeper.java",
            "public class Sleeper { public static void main(String[] a) throws Exception { System.out.println(\"sleeping\"); Thread.sleep(60000); } }\n",
        )],
    );
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Sleeper").await;
    client.request("configurationDone", json!({})).await;
    client.wait_output_containing("sleeping").await;
    client.request("terminate", json!({})).await;
    let events = events_until_terminated(&mut client).await;
    assert!(events.iter().any(|e| e["event"] == "exited"));
}

const USERS_SRC: &str = r#"import java.util.*;

public class Users {
    record User(String name, int age) {}

    static int counter = 5;
    private final List<User> users = new ArrayList<>();
    private final Map<String, Integer> ages = new HashMap<>();

    String greet(String who) {
        return "hi " + who;
    }

    void run() {
        users.add(new User("Ann", 30));
        users.add(new User("Bob", 25));
        ages.put("Ann", 30);
        User first = users.get(0);
        Integer boxed = 42;
        String text = "hello";
        int[] nums = {1, 2, 3};
        System.out.println(first);
    }

    public static void main(String[] args) {
        new Users().run();
    }
}
"#;

async fn evaluate(client: &mut DapClient, frame_id: u64, expression: &str) -> Value {
    client
        .try_request(
            "evaluate",
            json!({ "expression": expression, "frameId": frame_id, "context": "repl" }),
        )
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn evaluate_invokes_methods_and_computes_expressions() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP evaluate_invokes_methods_and_computes_expressions: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_users", &[("Users.java", USERS_SRC)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Users").await;
    set_breakpoints(&mut client, &dir, "Users.java", &[23]).await;
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["name"], "run");
    let frame_id = frame["id"].as_u64().unwrap();

    let result = |response: &Value| {
        response["body"]["result"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    let cases: &[(&str, &str)] = &[
        // Method invocation on fields, locals and results of calls.
        ("users.size()", "2"),
        ("this.users.size()", "2"),
        ("first.name()", "\"Ann\""),
        ("users.get(1).age()", "25"),
        ("users.get(0).name().length()", "3"),
        ("text.length() * 2", "10"),
        // Property-style access falls back to accessors (Kotlin, records, beans).
        ("text.length + 1", "6"),
        ("text.substring(1, 3)", "\"el\""),
        ("text.equals(\"hello\")", "true"),
        ("users.contains(first)", "true"),
        ("ages.get(\"Ann\")", "30"),
        ("ages.containsKey(\"Zed\")", "false"),
        ("greet(\"you\")", "\"hi you\""),
        // Static methods and fields of loaded classes.
        ("Math.max(3, 7)", "7"),
        ("Integer.parseInt(\"12\") + 1", "13"),
        ("counter + 1", "6"),
        ("Users.counter", "5"),
        ("java.util.Collections.emptyList().size()", "0"),
        // Arithmetic, comparison, logic and concatenation.
        ("1 + 2 * 3", "7"),
        ("10 / 4", "2"),
        ("10 / 4.0", "2.5"),
        // Integer overflow and float promotion must agree with the debuggee JVM.
        ("Integer.MIN_VALUE / -1", "-2147483648"),
        ("Integer.MIN_VALUE % -1", "0"),
        ("Long.MIN_VALUE / -1L", "-9223372036854775808L"),
        ("Long.MIN_VALUE % -1L", "0L"),
        ("16777217 == 16777216f", "true"),
        ("16777217L > 16777216f", "false"),
        ("16777217 == 16777216.0", "false"),
        ("4611686293305294849L == 4611686568183201792f", "true"),
        ("4611686293305294849L + 0f == 4611686568183201792f", "true"),
        ("first.age() > 20 && boxed == 42", "true"),
        ("boxed + 1", "43"),
        ("text + \" \" + first.age()", "\"hello 30\""),
        ("\"a\" + 1 + 2", "\"a12\""),
        ("first.age() == 30 ? \"old\" : \"young\"", "\"old\""),
        ("nums[1] + nums.length", "5"),
        ("!(text == \"hello\")", "false"),
        ("text != null", "true"),
    ];
    for (expression, expected) in cases {
        let response = evaluate(&mut client, frame_id, expression).await;
        assert_eq!(
            response["success"], true,
            "`{expression}` failed: {response}"
        );
        assert_eq!(&result(&response), expected, "`{expression}`");
    }

    // Object results carry their runtime type and expand.
    let first = evaluate(&mut client, frame_id, "users.get(0)").await;
    assert_eq!(first["body"]["result"], "User[name=Ann, age=30]");
    assert_eq!(first["body"]["type"], "Users$User");
    assert_ne!(first["body"]["variablesReference"], 0);

    // Errors are reported, not swallowed or fabricated.
    for bad in [
        "1 / 0",
        "first.nope()",
        "missing + 1",
        "users.get(",
        "text.length(1)",
    ] {
        let response = evaluate(&mut client, frame_id, bad).await;
        assert_eq!(
            response["success"], false,
            "`{bad}` should fail: {response}"
        );
        assert_ne!(response["message"].as_str().unwrap(), "");
    }
    let exception = evaluate(&mut client, frame_id, "users.get(9)").await;
    assert_eq!(exception["success"], false);
    assert!(
        exception["message"]
            .as_str()
            .unwrap()
            .contains("IndexOutOfBounds"),
        "{exception}"
    );

    // The frame is still usable after all those invocations.
    let after = evaluate(&mut client, frame_id, "first.age()").await;
    assert_eq!(after["body"]["result"], "30");
    // Hover never runs code.
    let hover = client
        .try_request(
            "evaluate",
            json!({ "expression": "users.size()", "frameId": frame_id, "context": "hover" }),
        )
        .await;
    assert_eq!(hover["success"], false);

    resume(&mut client, thread_id).await;
    client.request("disconnect", json!({})).await;
}

/// Code run by an evaluation must not stop at breakpoints or exception
/// requests inside it: a stop during `InvokeMethod` never returns.
#[tokio::test(flavor = "multi_thread")]
async fn evaluation_is_not_interrupted_by_breakpoints_or_exception_stops() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP evaluation_is_not_interrupted_by_breakpoints_or_exception_stops: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_users_nostop", &[("Users.java", USERS_SRC)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Users").await;
    // Line 11 is inside `greet`, which the evaluation below calls.
    set_breakpoints(&mut client, &dir, "Users.java", &[11, 23]).await;
    client
        .request(
            "setExceptionBreakpoints",
            json!({ "filters": ["caught", "uncaught"] }),
        )
        .await;
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(
        frame["line"], 23,
        "the greet breakpoint must not fire before the evaluation"
    );
    let frame_id = frame["id"].as_u64().unwrap();

    let greeted = evaluate(&mut client, frame_id, "greet(\"z\")").await;
    assert_eq!(greeted["success"], true, "{greeted}");
    assert_eq!(greeted["body"]["result"], "\"hi z\"");
    // A thrown (and caught by the evaluator) exception is reported, not a stop.
    let thrown = evaluate(&mut client, frame_id, "Integer.parseInt(\"x\")").await;
    assert_eq!(thrown["success"], false);
    assert!(
        thrown["message"]
            .as_str()
            .unwrap()
            .contains("NumberFormatException"),
        "{thrown}"
    );
    // The frame survived and both kinds of stops are armed again afterwards.
    assert_eq!(
        evaluate(&mut client, frame_id, "text").await["body"]["result"],
        "\"hello\""
    );

    resume(&mut client, thread_id).await;
    events_until_terminated(&mut client).await;
    assert!(
        client
            .pending_events
            .iter()
            .all(|e| e["event"] != "stopped"),
        "nothing else should have stopped: {:?}",
        client.pending_events
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn variables_show_runtime_types_summaries_and_collection_contents() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP variables_show_runtime_types_summaries_and_collection_contents: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_users_vars", &[("Users.java", USERS_SRC)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Users").await;
    set_breakpoints(&mut client, &dir, "Users.java", &[23]).await;
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    let frame_id = frame["id"].as_u64().unwrap();

    let scopes = client
        .request("scopes", json!({ "frameId": frame_id }))
        .await;
    let locals = scopes["body"]["scopes"][0]["variablesReference"]
        .as_u64()
        .unwrap();
    let vars = client
        .request("variables", json!({ "variablesReference": locals }))
        .await;
    let vars = vars["body"]["variables"].as_array().unwrap().clone();
    let find = |name: &str| {
        vars.iter()
            .find(|v| v["name"] == name)
            .unwrap_or_else(|| panic!("no variable {name} in {vars:?}"))
            .clone()
    };

    let first = find("first");
    assert_eq!(first["value"], "User[name=Ann, age=30]");
    assert_eq!(first["type"], "Users$User");
    let boxed = find("boxed");
    assert_eq!(boxed["value"], "42");
    assert_eq!(boxed["type"], "java.lang.Integer");
    assert_eq!(boxed["variablesReference"], 0);
    let text = find("text");
    assert_eq!(text["value"], "\"hello\"");
    assert_eq!(text["type"], "java.lang.String");
    assert_eq!(find("nums")["value"], "int[3]");

    // `this` expands to its fields; the list field shows its size and elements.
    let this = find("this");
    let fields = client
        .request(
            "variables",
            json!({ "variablesReference": this["variablesReference"] }),
        )
        .await;
    let fields = fields["body"]["variables"].as_array().unwrap().clone();
    let users = fields
        .iter()
        .find(|v| v["name"] == "users")
        .expect("users field");
    assert_eq!(users["type"], "java.util.ArrayList");
    let users_value = users["value"].as_str().unwrap();
    assert!(users_value.starts_with("size=2"), "{users_value}");
    let elements = client
        .request(
            "variables",
            json!({ "variablesReference": users["variablesReference"] }),
        )
        .await;
    let elements = elements["body"]["variables"].as_array().unwrap();
    assert_eq!(elements.len(), 2, "{elements:?}");
    assert_eq!(elements[0]["name"], "[0]");
    assert_eq!(elements[1]["value"], "User[name=Bob, age=25]");

    let ages = fields
        .iter()
        .find(|v| v["name"] == "ages")
        .expect("ages field");
    assert!(ages["value"].as_str().unwrap().starts_with("size=1"));
    let entries = client
        .request(
            "variables",
            json!({ "variablesReference": ages["variablesReference"] }),
        )
        .await;
    let entries = entries["body"]["variables"].as_array().unwrap();
    assert_eq!(entries[0]["name"], "\"Ann\"");
    assert_eq!(entries[0]["value"], "30");

    // Stepping still works after all the invocations above.
    client
        .request("next", json!({ "threadId": thread_id }))
        .await;
    client.wait_event("continued").await;
    let stepped = client.wait_event("stopped").await;
    assert_eq!(stepped["body"]["reason"], "step");
    client.request("disconnect", json!({})).await;
}

/// A second top-level class shares its file with `Multi`, so its class name
/// never equals the file name: the shape of Kotlin facades, reproducible with
/// plain `javac`.
#[tokio::test(flavor = "multi_thread")]
async fn breakpoint_in_a_second_top_level_class_of_the_file_binds() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP breakpoint_in_a_second_top_level_class_of_the_file_binds: no JDK");
        return;
    }
    let source = "public class Multi {\n    public static void main(String[] args) {\n        Helper.help(2);\n    }\n}\n\nclass Helper {\n    static void help(int n) {\n        System.out.println(\"helping \" + n);\n    }\n}\n";
    let dir = setup_project("hyp_dap_multi", &[("Multi.java", source)]);
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, &dir, "Multi").await;
    set_breakpoints(&mut client, &dir, "Multi.java", &[9]).await;
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["name"], "help");
    assert_eq!(frame["line"], 9);
    resume(&mut client, thread_id).await;
    events_until_terminated(&mut client).await;
}

/// Kotlin: top-level functions live in `<File>Kt`, so the old path-to-class
/// rule never found them. Needs a Kotlin compiler; skipped without one.
#[tokio::test(flavor = "multi_thread")]
async fn kotlin_breakpoints_bind_in_facades_classes_and_lambdas() {
    if !tool_available("java") || !tool_available("kotlinc") {
        eprintln!("SKIP kotlin_breakpoints_bind_in_facades_classes_and_lambdas: no kotlinc");
        return;
    }
    let dir = std::env::temp_dir().join("hyp_dap_kotlin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    let source = "package demo\n\nclass Greeter {\n    fun greet(name: String): String {\n        return \"hi $name\"\n    }\n}\n\nfun shout(text: String): String {\n    return text.uppercase()\n}\n\nfun main() {\n    val greeter = Greeter()\n    val words = listOf(\"a\", \"b\")\n    words.forEach { w ->\n        println(greeter.greet(w))\n    }\n    println(shout(\"done\"))\n}\n";
    write_file(&dir, "Demo.kt", source);
    let compile = StdCommand::new("kotlinc")
        .arg(dir.join("Demo.kt"))
        .arg("-d")
        .arg(dir.join("out"))
        .output()
        .expect("kotlinc");
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let mut client = DapClient::spawn();
    client
        .request("initialize", json!({ "linesStartAt1": true }))
        .await;
    client.wait_event("initialized").await;
    let out = dir.join("out").to_string_lossy().to_string();
    client
        .request(
            "launch",
            json!({ "mainClass": "demo.DemoKt", "classpath": out, "cwd": dir.to_string_lossy(), "projectRoot": dir.to_string_lossy() }),
        )
        .await;
    // greet body (class), shout body (top-level function), lambda body.
    set_breakpoints(&mut client, &dir, "Demo.kt", &[5, 10, 17]).await;
    client.request("configurationDone", json!({})).await;
    let mut seen = Vec::new();
    for _ in 0..5 {
        let (thread_id, frame) = next_stop(&mut client).await;
        seen.push(frame["line"].as_u64().unwrap());
        assert_eq!(frame["source"]["name"], "Demo.kt");
        resume(&mut client, thread_id).await;
    }
    // Lambda body, Greeter.greet, lambda body, Greeter.greet, top-level shout.
    assert_eq!(seen, [17, 5, 17, 5, 10]);
    client.request("disconnect", json!({})).await;
}

// ---- Stepping and breakpoint behaviour ----

/// Launch `class` from `dir` with `lines` of `file` as breakpoints and finish
/// the configuration; returns the client ready for the first stop.
async fn launch_with_breakpoints(
    dir: &Path,
    class: &str,
    file: &str,
    breakpoints: Value,
) -> DapClient {
    let mut client = DapClient::spawn();
    do_initialize_and_launch_class(&mut client, dir, class).await;
    let path = dir.join(file).to_string_lossy().to_string();
    client
        .request(
            "setBreakpoints",
            json!({ "source": { "path": path, "name": file }, "breakpoints": breakpoints }),
        )
        .await;
    client.request("configurationDone", json!({})).await;
    client
}

async fn step(client: &mut DapClient, command: &str, thread_id: u64) -> Value {
    client
        .request(command, json!({ "threadId": thread_id }))
        .await;
    let (_, frame) = next_stop(client).await;
    frame
}

const STALE_SRC: &str = r#"public class Stale {
    static int f(int x) {
        return x * 2;
    }
    public static void main(String[] args) {
        int a = f(1);
        int b = 5;
        int c = 6;
        System.out.println(a + b + c);
    }
}
"#;

/// A step-over that a breakpoint interrupts must not fire after the next
/// Continue.
#[tokio::test(flavor = "multi_thread")]
async fn interrupted_step_over_does_not_stop_after_continue() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP interrupted_step_over_does_not_stop_after_continue: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_stale_step", &[("Stale.java", STALE_SRC)]);
    let mut client = launch_with_breakpoints(
        &dir,
        "Stale",
        "Stale.java",
        json!([{ "line": 6 }, { "line": 3 }]),
    )
    .await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["line"], 6);
    // The breakpoint inside `f` interrupts the step over `f(1)`.
    let stopped_in_f = step(&mut client, "next", thread_id).await;
    assert_eq!(stopped_in_f["name"], "f");
    client
        .request("continue", json!({ "threadId": thread_id }))
        .await;
    let next = client.wait_event_any(&["stopped", "terminated"]).await;
    assert_eq!(
        next["event"], "terminated",
        "a stale step request stopped the program again: {next:?}"
    );
}

const ENUM_SRC: &str = r#"public class Enm {
    enum Mode {
        FAST {
            int f() { return 1; }
        };
        abstract int f();
        static int g() { return 2; }
    }
    public static void main(String[] args) {
        System.out.println(Mode.FAST.f() + Mode.g());
    }
}
"#;

/// The line requested is reached in a class that loads after another class
/// made the breakpoint slide; the exact line must win (the Kotlin
/// inline-lambda case in miniature: `Mode$1` loads after `Mode`).
#[tokio::test(flavor = "multi_thread")]
async fn breakpoint_moves_back_to_the_requested_line_when_a_later_class_has_code_there() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP breakpoint_moves_back_to_the_requested_line: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_slide_back", &[("Enm.java", ENUM_SRC)]);
    let mut client = launch_with_breakpoints(&dir, "Enm", "Enm.java", json!([{ "line": 4 }])).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["line"], 4, "stopped on a slid line: {frame:?}");
    assert_eq!(frame["name"], "f");
    client
        .request("continue", json!({ "threadId": thread_id }))
        .await;
    let next = client.wait_event_any(&["stopped", "terminated"]).await;
    assert_eq!(
        next["event"], "terminated",
        "stale slid breakpoint: {next:?}"
    );
}

const STEP_SRC: &str = r#"import java.util.List;

public class Steps {
    static double total(double v) {
        return v * 2;
    }
    public static void main(String[] args) {
        double t = total(Double.valueOf(2.5));
        List.of(1, 2).forEach(x -> {
            int y = x + 1;
        });
        System.out.println(t);
    }
}
"#;

/// Step-into skips `Double.valueOf` and enters the user method.
#[tokio::test(flavor = "multi_thread")]
async fn step_into_skips_jdk_boxing_and_enters_the_user_method() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP step_into_skips_jdk_boxing: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_step_into", &[("Steps.java", STEP_SRC)]);
    let mut client =
        launch_with_breakpoints(&dir, "Steps", "Steps.java", json!([{ "line": 8 }])).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["line"], 8);
    let frame = step(&mut client, "stepIn", thread_id).await;
    assert_eq!(frame["name"], "total", "stepped into {frame:?}");
    assert_eq!(frame["source"]["name"], "Steps.java");
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

/// Step-over from the end of a lambda goes to the next lambda call, never to a
/// source-less proxy or JDK frame.
#[tokio::test(flavor = "multi_thread")]
async fn step_over_out_of_a_lambda_lands_on_a_frame_with_source() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP step_over_out_of_a_lambda: no JDK");
        return;
    }
    let dir = setup_project("hyp_dap_step_lambda", &[("Steps.java", STEP_SRC)]);
    let mut client =
        launch_with_breakpoints(&dir, "Steps", "Steps.java", json!([{ "line": 10 }])).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["line"], 10);
    // The closing brace, then the second iteration of the lambda.
    let frame = step(&mut client, "next", thread_id).await;
    assert_eq!(frame["line"], 11, "{frame:?}");
    let frame = step(&mut client, "next", thread_id).await;
    assert_eq!(frame["source"]["name"], "Steps.java", "{frame:?}");
    assert_eq!(frame["line"], 10, "second lambda call expected: {frame:?}");
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}

/// Hit conditions and logpoints.
#[tokio::test(flavor = "multi_thread")]
async fn hit_count_breakpoint_stops_on_the_nth_hit_and_logpoint_logs_without_stopping() {
    if !tool_available("java") || !tool_available("javac") {
        eprintln!("SKIP hit_count_and_logpoint: no JDK");
        return;
    }
    let dir = setup_fixture("hyp_dap_hit_log");
    let mut client = DapClient::spawn();
    let init = client
        .request("initialize", json!({ "linesStartAt1": true }))
        .await;
    assert_eq!(init["body"]["supportsLogPoints"], true);
    assert_eq!(init["body"]["supportsHitConditionalBreakpoints"], true);
    client.wait_event("initialized").await;
    let dir_str = dir.to_string_lossy().to_string();
    client
        .request(
            "launch",
            json!({ "mainClass": "Main", "classpath": dir_str, "cwd": dir_str, "projectRoot": dir_str }),
        )
        .await;
    let path = dir.join("Main.java").to_string_lossy().to_string();
    client
        .request(
            "setBreakpoints",
            json!({ "source": { "path": path, "name": "Main.java" }, "breakpoints": [
                { "line": BREAKPOINT_LINE, "hitCondition": "4" },
                { "line": BREAKPOINT_LINE - 1, "logMessage": "logged i={i} twice={i * 2}" },
            ]}),
        )
        .await;
    client.request("configurationDone", json!({})).await;
    let (thread_id, frame) = next_stop(&mut client).await;
    assert_eq!(frame["line"], BREAKPOINT_LINE);
    let stack = client
        .request("stackTrace", json!({ "threadId": thread_id }))
        .await;
    let frame_id = stack["body"]["stackFrames"][0]["id"].as_u64().unwrap();
    let scopes = client
        .request("scopes", json!({ "frameId": frame_id }))
        .await;
    let locals = scopes["body"]["scopes"][0]["variablesReference"]
        .as_u64()
        .unwrap();
    let vars = client
        .request("variables", json!({ "variablesReference": locals }))
        .await;
    let i = vars["body"]["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "i")
        .unwrap()["value"]
        .clone();
    assert_eq!(i, "3", "4th hit is i == 3");
    // The logpoint on the line before never stopped but logged every call.
    let log = client.wait_output_containing("logged i=3 twice=6").await;
    assert_eq!(log["body"]["category"], "console");
    client
        .request("disconnect", json!({ "terminateDebuggee": true }))
        .await;
}
