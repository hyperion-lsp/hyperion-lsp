// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for the Hyperion LSP server
//!
//! These tests verify end-to-end functionality of the LSP server by
//! simulating a language client and testing real LSP protocol interactions.
//!
//! ## Implementation
//!
//! The test harness spawns a real LSP server process and communicates with it
//! over stdin/stdout using the JSON-RPC protocol. A background thread continuously
//! reads messages from the server, and an atomic shutdown flag ensures clean
//! thread lifecycle management.
//!
//! ## Shutdown Mechanism
//!
//! The shutdown process:
//! 1. Sends "shutdown" request and waits for response
//! 2. Sends "exit" notification
//! 3. Sets atomic shutdown flag to signal reader thread
//! 4. Kills child process if it doesn't exit gracefully
//! 5. Joins reader thread (which checks the shutdown flag)

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const INTERACTIVE_RESPONSE_TIMEOUT_MS: u64 = 10_000;
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(10);

/// LSP client for integration testing
struct LSPTestClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    message_id: AtomicU64,
    responses: Arc<Mutex<HashMap<u64, Value>>>,
    _notifications: Arc<Mutex<Vec<Value>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    reader_thread: Option<thread::JoinHandle<()>>,
    stderr_thread: Option<thread::JoinHandle<()>>,
}

impl LSPTestClient {
    /// Start the LSP server and create a new client
    fn new() -> Self {
        // The server under test: the crate's own binary, or another build of it
        // (`HYPERION_E2E_BINARY=/path/to/hyperion-lsp`) for before/after benchmarks.
        let binary_path = std::env::var("HYPERION_E2E_BINARY")
            .unwrap_or_else(|_| env!("CARGO_BIN_EXE_hyperion-lsp").to_owned());

        let mut child = Command::new(binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn LSP server");

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");
        let mut child_stderr = child.stderr.take().expect("Failed to get stderr");

        let responses = Arc::new(Mutex::new(HashMap::new()));
        let notifications = Arc::new(Mutex::new(Vec::new()));
        // Spawn background thread to read messages
        let responses_clone = Arc::clone(&responses);
        let notifications_clone = Arc::clone(&notifications);
        let reader_thread = thread::spawn(move || {
            Self::read_messages_loop(stdout, responses_clone, notifications_clone);
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let captured_stderr = Arc::clone(&stderr);
        let stderr_thread = thread::spawn(move || {
            child_stderr
                .read_to_end(&mut captured_stderr.lock())
                .expect("read LSP stderr");
        });

        Self {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            message_id: AtomicU64::new(0),
            responses,
            _notifications: notifications,
            stderr,
            reader_thread: Some(reader_thread),
            stderr_thread: Some(stderr_thread),
        }
    }

    /// Wait for the server's startup publications before exercising a stable
    /// reference snapshot. The nested-source test opens an ignored file after
    /// startup so its discovery cannot race the initial workspace replacement.
    fn wait_for_initial_indexing(&self) {
        let deadline = Instant::now() + Duration::from_mins(1);
        loop {
            let messages: Vec<_> = self
                ._notifications
                .lock()
                .iter()
                .filter(|message| {
                    message.get("method").and_then(Value::as_str) == Some("window/logMessage")
                })
                .filter_map(|message| {
                    message
                        .pointer("/params/message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect();
            let workspace = messages.iter().any(|message| {
                message.contains("✓ Workspace indexed:")
                    || message.contains("✓ Restored from cache:")
            });
            let jdk = messages
                .iter()
                .any(|message| message.contains("✓ JDK stdlib indexed"));
            let build = messages
                .iter()
                .any(|message| message.contains("✓ Build system indexed:"));
            if workspace && jdk && build {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "startup did not finish: {messages:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Background thread function to read all messages from server
    fn read_messages_loop(
        stdout: ChildStdout,
        responses: Arc<Mutex<HashMap<u64, Value>>>,
        notifications: Arc<Mutex<Vec<Value>>>,
    ) {
        let mut reader = BufReader::new(stdout);

        while let Some(message) = Self::read_one_message(&mut reader) {
            if let Some(id) = message.get("id").and_then(serde_json::Value::as_u64) {
                // It's a response
                responses.lock().insert(id, message);
            } else {
                // It's a notification
                notifications.lock().push(message);
            }
        }
    }

    /// Read a single LSP message from the stream
    fn read_one_message(reader: &mut BufReader<ChildStdout>) -> Option<Value> {
        // Read headers
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 {
                return None;
            }

            if line == "\r\n" {
                break;
            }

            if line.starts_with("Content-Length:") {
                content_length = line
                    .trim_start_matches("Content-Length:")
                    .trim()
                    .parse()
                    .ok()?;
            }
        }

        // Read content
        let mut buffer = vec![0u8; content_length];
        reader.read_exact(&mut buffer).ok()?;

        let content = String::from_utf8(buffer).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Send an LSP request
    fn send_request(&self, method: &str, params: Value) -> u64 {
        let id = self.message_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.send_message(&message);
        id
    }

    /// Send an LSP notification (no response expected)
    fn send_notification(&self, method: &str, params: Value) {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        self.send_message(&message);
    }

    /// Send a JSON-RPC message
    fn send_message(&self, message: &Value) {
        let content = serde_json::to_string(message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", content.len());
        let full_message = format!("{header}{content}");

        let mut stdin = self.stdin.lock();
        stdin.write_all(full_message.as_bytes()).unwrap();
        stdin.flush().unwrap();
    }

    /// Wait for a response with a specific ID
    fn wait_for_response(&self, id: u64, timeout_ms: u64) -> Option<Value> {
        let start = std::time::Instant::now();

        while start.elapsed().as_millis() < u128::from(timeout_ms) {
            // Check if response is available
            {
                let mut responses = self.responses.lock();
                if let Some(response) = responses.remove(&id) {
                    return Some(response);
                }
            }

            thread::sleep(Duration::from_millis(10));
        }

        None
    }

    /// Initialize the LSP server
    fn initialize(&self, root_uri: &str) -> Value {
        let params = json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "hover": {
                        "contentFormat": ["markdown"]
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true
                        }
                    },
                    "definition": {},
                    "references": {},
                    "documentSymbol": {}
                }
            }
        });

        let id = self.send_request("initialize", params);
        let response = self
            .wait_for_response(id, INTERACTIVE_RESPONSE_TIMEOUT_MS)
            .unwrap_or_else(|| panic!("initialize request {id} timed out"));
        assert!(
            response.get("error").is_none(),
            "initialize returned an error: {response}"
        );
        let result = response
            .get("result")
            .cloned()
            .expect("initialize response must contain a result");
        self.send_notification("initialized", json!({}));
        result
    }

    /// Open a document
    fn did_open(&self, uri: &str, language_id: &str, version: i32, text: &str) {
        let params = json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text
            }
        });

        self.send_notification("textDocument/didOpen", params);
    }

    /// Request hover information
    fn hover(&self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let id = self.send_request("textDocument/hover", params);
        self.wait_for_response(id, INTERACTIVE_RESPONSE_TIMEOUT_MS)
            .and_then(|r| r.get("result").cloned())
    }

    /// Request completion
    fn completion(&self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let id = self.send_request("textDocument/completion", params);
        self.wait_for_response(id, INTERACTIVE_RESPONSE_TIMEOUT_MS)
            .and_then(|r| r.get("result").cloned())
    }

    /// Request document symbols
    fn document_symbols(&self, uri: &str) -> Option<Value> {
        self.document_symbols_within(uri, INTERACTIVE_RESPONSE_TIMEOUT_MS)
    }

    /// Request document symbols and wait up to `timeout_ms` for the answer
    fn document_symbols_within(&self, uri: &str, timeout_ms: u64) -> Option<Value> {
        let params = json!({
            "textDocument": { "uri": uri }
        });

        let id = self.send_request("textDocument/documentSymbol", params);
        self.wait_for_response(id, timeout_ms)
            .and_then(|r| r.get("result").cloned())
    }

    /// Request references
    fn references(
        &self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Option<Value> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        });

        let id = self.send_request("textDocument/references", params);
        self.wait_for_response(id, 5000)
            .and_then(|r| r.get("result").cloned())
    }

    /// Request whole-document formatting and retain an explicit JSON null result.
    fn formatting(&self, uri: &str) -> Value {
        let params = json!({
            "textDocument": { "uri": uri },
            "options": {
                "tabSize": 4,
                "insertSpaces": true
            }
        });

        let id = self.send_request("textDocument/formatting", params);
        let response = self
            .wait_for_response(id, INTERACTIVE_RESPONSE_TIMEOUT_MS)
            .unwrap_or_else(|| panic!("formatting request {id} timed out"));
        assert!(
            response.get("error").is_none(),
            "formatting returned an error: {response}"
        );
        response
            .get("result")
            .cloned()
            .expect("formatting response must contain a result")
    }

    /// Shutdown the server
    fn shutdown(mut self) {
        let id = self.send_request("shutdown", Value::Null);
        let response = self
            .wait_for_response(id, INTERACTIVE_RESPONSE_TIMEOUT_MS)
            .unwrap_or_else(|| panic!("shutdown request {id} timed out"));
        assert!(
            response.get("error").is_none(),
            "shutdown returned an error: {response}"
        );
        self.send_notification("exit", json!({}));

        let deadline = Instant::now() + PROCESS_EXIT_TIMEOUT;
        let status = loop {
            match self.child.try_wait().expect("poll LSP child") {
                Some(status) => break status,
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                None => panic!("LSP process did not exit after shutdown/exit"),
            }
        };
        assert!(status.success(), "LSP process exited with {status}");

        if let Some(thread) = self.reader_thread.take() {
            thread.join().expect("LSP stdout reader panicked");
        }
        if let Some(thread) = self.stderr_thread.take() {
            thread.join().expect("LSP stderr reader panicked");
        }
        let stderr_bytes = self.stderr.lock();
        let stderr = String::from_utf8_lossy(&stderr_bytes);
        assert!(
            !stderr.contains("panicked at") && !stderr.contains("request handler panicked"),
            "LSP server panicked while handling the workflow:\n{stderr}"
        );
    }
}

impl Drop for LSPTestClient {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.stderr_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Test fixture: Simple Calculator class
const CALCULATOR_JAVA: &str = r"package com.example;

/**
 * A simple calculator for basic arithmetic operations.
 */
public class Calculator {

    /** The last result computed */
    private double lastResult;

    /**
     * Adds two numbers together.
     *
     * @param a the first number
     * @param b the second number
     * @return the sum of a and b
     */
    public double add(double a, double b) {
        lastResult = a + b;
        return lastResult;
    }

    /**
     * Gets the last computed result.
     *
     * @return the last result
     */
    public double getLastResult() {
        return lastResult;
    }
}
";

const GREETER_KOTLIN: &str = r"package com.example

class Greeter {
    fun greet(name: String): String = name
}
";

const BUILD_GRADLE: &str = r"repositories {
    mavenCentral()
}
";

const USER_SCALA: &str = r#"package com.example

class User {
    def displayName(): String = "Ada"
}
"#;

fn hover_markdown<'a>(hover: &'a Value, description: &str) -> &'a str {
    hover
        .pointer("/contents/value")
        .or_else(|| hover.get("contents"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{description} hover must contain markdown: {hover}"))
}

#[test]
fn binary_java_editor_workflow_and_graceful_shutdown() {
    let workspace = TempDir::new().expect("temp workspace");
    let source_path = workspace.path().join("Calculator.java");
    std::fs::write(&source_path, CALCULATOR_JAVA).expect("write Calculator.java");
    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace.path())
        .expect("workspace URI")
        .to_string();
    let document_uri = tower_lsp::lsp_types::Url::from_file_path(&source_path)
        .expect("document URI")
        .to_string();

    let client = LSPTestClient::new();
    let initialize = client.initialize(&root_uri);
    assert_eq!(
        initialize.pointer("/capabilities/positionEncoding"),
        Some(&json!("utf-16")),
        "the shipped server must negotiate UTF-16 positions"
    );
    assert!(
        initialize
            .pointer("/capabilities/completionProvider")
            .is_some(),
        "the shipped server must advertise completion"
    );

    client.did_open(&document_uri, "java", 1, CALCULATOR_JAVA);

    let class_hover = client
        .hover(&document_uri, 5, 14)
        .expect("hover on Calculator");
    let class_hover_text = class_hover
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .expect("Calculator hover markdown");
    assert!(class_hover_text.contains("Calculator"));
    assert!(class_hover_text.contains("A simple calculator"));

    let field_hover = client
        .hover(&document_uri, 8, 20)
        .expect("hover on lastResult");
    let field_hover_text = field_hover
        .pointer("/contents/value")
        .and_then(Value::as_str)
        .expect("lastResult hover markdown");
    assert!(field_hover_text.contains("lastResult"));
    assert!(field_hover_text.contains("last result computed"));

    let completion = client
        .completion(&document_uri, 19, 8)
        .expect("completion response");
    let items = completion
        .as_array()
        .or_else(|| completion.get("items").and_then(Value::as_array))
        .expect("completion items");
    assert!(
        items.iter().any(|item| {
            item.get("label")
                .and_then(Value::as_str)
                .is_some_and(|label| matches!(label, "class" | "public" | "private"))
        }),
        "completion should include Java keywords: {items:?}"
    );

    let symbols = client
        .document_symbols(&document_uri)
        .expect("document symbol response");
    let symbols = symbols.as_array().expect("document symbol array");
    assert!(
        symbols
            .iter()
            .any(|symbol| { symbol.get("name").and_then(Value::as_str) == Some("Calculator") }),
        "document symbols should contain Calculator: {symbols:?}"
    );

    client.shutdown();
}

#[test]
fn binary_dispatches_each_advertised_language_without_java_fallback() {
    let workspace = TempDir::new().expect("temp workspace");
    let fixtures = [
        ("Greeter.kt", GREETER_KOTLIN),
        ("build.gradle", BUILD_GRADLE),
        ("User.scala", USER_SCALA),
    ];
    for (name, source) in fixtures {
        std::fs::write(workspace.path().join(name), source)
            .unwrap_or_else(|error| panic!("write {name}: {error}"));
    }

    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace.path())
        .expect("workspace URI")
        .to_string();
    let document_uri = |name| {
        tower_lsp::lsp_types::Url::from_file_path(workspace.path().join(name))
            .expect("document URI")
            .to_string()
    };

    let client = LSPTestClient::new();
    let _initialize = client.initialize(&root_uri);

    let kotlin_uri = document_uri("Greeter.kt");
    client.did_open(&kotlin_uri, "kotlin", 1, GREETER_KOTLIN);
    let kotlin_hover = client.hover(&kotlin_uri, 2, 8).expect("Kotlin class hover");
    assert!(hover_markdown(&kotlin_hover, "Kotlin").contains("class Greeter"));
    let kotlin_symbols = client
        .document_symbols(&kotlin_uri)
        .expect("Kotlin document symbols");
    assert!(
        kotlin_symbols.as_array().is_some_and(|symbols| symbols
            .iter()
            .any(|symbol| symbol.get("name").and_then(Value::as_str) == Some("Greeter"))),
        "Kotlin document symbols should contain Greeter: {kotlin_symbols}"
    );

    let groovy_uri = document_uri("build.gradle");
    client.did_open(&groovy_uri, "groovy", 1, BUILD_GRADLE);
    let groovy_hover = client.hover(&groovy_uri, 1, 8).expect("Gradle DSL hover");
    assert!(hover_markdown(&groovy_hover, "Groovy").contains("mavenCentral"));
    let groovy_completion = client
        .completion(&groovy_uri, 1, 4)
        .expect("Gradle DSL completion");
    let groovy_items = groovy_completion
        .as_array()
        .or_else(|| groovy_completion.get("items").and_then(Value::as_array))
        .expect("Gradle completion items");
    assert!(
        groovy_items.iter().any(|item| {
            item.get("label")
                .and_then(Value::as_str)
                .is_some_and(|label| label == "mavenCentral")
        }),
        "Gradle repository completion should contain mavenCentral: {groovy_items:?}"
    );

    let scala_uri = document_uri("User.scala");
    client.did_open(&scala_uri, "scala", 1, USER_SCALA);
    let scala_hover = client.hover(&scala_uri, 2, 7).expect("Scala class hover");
    assert!(hover_markdown(&scala_hover, "Scala").contains("class User"));

    client.shutdown();
}

#[test]
fn binary_formatting_formats_java_and_declines_unsafe_or_unsupported_input() {
    const UNFORMATTED_JAVA: &str = "class Example{int value;}";
    const FORMATTED_JAVA: &str = "class Example {\n    int value;\n}\n";
    const MALFORMED_JAVA: &str = "class Broken { void run( {";
    const KOTLIN: &str = "class Example{fun value()=42}";

    let workspace = TempDir::new().expect("temp workspace");
    let fixtures = [
        ("Example.java", UNFORMATTED_JAVA),
        ("Broken.java", MALFORMED_JAVA),
        ("Example.kt", KOTLIN),
    ];
    for (name, source) in fixtures {
        std::fs::write(workspace.path().join(name), source)
            .unwrap_or_else(|error| panic!("write {name}: {error}"));
    }

    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace.path())
        .expect("workspace URI")
        .to_string();
    let document_uri = |name| {
        tower_lsp::lsp_types::Url::from_file_path(workspace.path().join(name))
            .expect("document URI")
            .to_string()
    };

    let client = LSPTestClient::new();
    let initialize = client.initialize(&root_uri);
    assert_eq!(
        initialize.pointer("/capabilities/documentFormattingProvider"),
        Some(&Value::Bool(true)),
        "the shipped server must advertise document formatting"
    );

    let java_uri = document_uri("Example.java");
    client.did_open(&java_uri, "java", 1, UNFORMATTED_JAVA);
    let edits = client.formatting(&java_uri);
    let edits = edits
        .as_array()
        .unwrap_or_else(|| panic!("Java formatting must return edits: {edits}"));
    assert_eq!(edits.len(), 1, "whole-document formatting uses one edit");
    assert_eq!(
        edits[0].pointer("/range/start"),
        Some(&json!({ "line": 0, "character": 0 }))
    );
    assert_eq!(
        edits[0].pointer("/range/end"),
        Some(&json!({ "line": 0, "character": 25 }))
    );
    assert_eq!(
        edits[0].get("newText").and_then(Value::as_str),
        Some(FORMATTED_JAVA)
    );

    let malformed_uri = document_uri("Broken.java");
    client.did_open(&malformed_uri, "java", 1, MALFORMED_JAVA);
    assert_eq!(
        client.formatting(&malformed_uri),
        Value::Null,
        "malformed Java must fail closed without destructive edits"
    );

    let kotlin_uri = document_uri("Example.kt");
    client.did_open(&kotlin_uri, "kotlin", 1, KOTLIN);
    assert_eq!(
        client.formatting(&kotlin_uri),
        Value::Null,
        "the Java-only formatter must decline Kotlin"
    );

    client.shutdown();
}

#[test]
fn test_references_in_gitignored_nested_repo() {
    fn file_uri(path: &std::path::Path) -> String {
        format!("file://{}", path.to_string_lossy())
    }

    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("mkdir root");

    // Mark outer workspace as a git repo and ignore the nested repo.
    std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    std::fs::write(root.join(".gitignore"), "nested-repo/\n").expect("write .gitignore");

    // Create a nested git repo under the ignored directory.
    let nested = root.join("nested-repo");
    std::fs::create_dir_all(nested.join(".git")).expect("mkdir nested/.git");

    let src_root = nested.join("src/main/java/p");
    std::fs::create_dir_all(&src_root).expect("mkdir src");

    let utils_path = src_root.join("Utils.java");
    let main_path = src_root.join("Main.java");

    let utils_src = r"package p;

public class Utils {
    public static void recreateTopics() {}
}
";

    let main_src = r"package p;

public class Main {
    public static void main(String[] args) {
        Utils.recreateTopics();
    }
}
";

    std::fs::write(&utils_path, utils_src).expect("write Utils.java");
    std::fs::write(&main_path, main_src).expect("write Main.java");

    // Cursor at recreateTopics in the declaration.
    let (line, col) = utils_src
        .lines()
        .enumerate()
        .find_map(|(i, l)| l.find("recreateTopics").map(|c| (i as u32, c as u32)))
        .expect("method name location");

    let client = LSPTestClient::new();
    let _initialize = client.initialize(&file_uri(&root));

    client.wait_for_initial_indexing();

    let utils_uri = file_uri(&utils_path);
    client.did_open(&utils_uri, "java", 1, utils_src);

    // References are async (indexing happens in the background), so poll briefly.
    // `includeDeclaration=false` should exclude the definition in Utils.java.
    let mut found_only_usages = false;
    for _ in 0..20 {
        if let Some(Value::Array(locs)) = client.references(&utils_uri, line, col, false) {
            let has_any_usage = locs.iter().any(|loc| {
                loc.get("uri")
                    .and_then(Value::as_str)
                    .is_some_and(|u| u != utils_uri)
            });
            let includes_declaration = locs.iter().any(|loc| {
                loc.get("uri")
                    .and_then(Value::as_str)
                    .is_some_and(|u| u == utils_uri)
            });

            found_only_usages = has_any_usage && !includes_declaration;
            if found_only_usages {
                break;
            }
        }
        thread::sleep(Duration::from_millis(250));
    }

    assert!(
        found_only_usages,
        "Expected at least one usage and no declaration when includeDeclaration=false"
    );

    client.shutdown();
}

/// Number of files in the burst workload. Overridable so the same test doubles
/// as the burst benchmark (`HYPERION_BURST_FILES=20 cargo test ... --nocapture`).
fn burst_file_count() -> usize {
    std::env::var("HYPERION_BURST_FILES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50)
}

fn percentile(sorted: &[Duration], quantile: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = ((sorted.len() - 1) as f64 * quantile).round() as usize;
    sorted[index]
}

/// Bursts of `didOpen`/`didChange`/`didClose` over files that
/// reference each other must never wedge the server (a deadlock between the
/// workspace index lock, the Salsa revision lock and dependency hydration did),
/// it must keep answering *during* the burst, and it must drain the burst
/// within a fixed budget.
#[test]
fn binary_survives_open_change_close_bursts_over_dependent_files() {
    let files = burst_file_count();
    let workspace = TempDir::new().expect("temp workspace");
    let mut sources = Vec::new();
    for index in 0..files {
        // Every class calls the previous one, so each change has dependents.
        let previous = if index == 0 { 0 } else { index - 1 };
        let source = format!(
            "package burst;\npublic class C{index} {{\n    public int value() {{ return {index}; }}\n    public int chain(C{previous} other) {{ return other.value() + value(); }}\n}}\n"
        );
        let path = workspace.path().join(format!("C{index}.java"));
        std::fs::write(&path, &source).expect("write source");
        let uri = tower_lsp::lsp_types::Url::from_file_path(&path)
            .expect("document URI")
            .to_string();
        sources.push((uri, source));
    }
    let probe_source =
        "package burst;\npublic class Probe {\n    void f(C0 c) {\n        c.\n    }\n}\n";
    let probe_path = workspace.path().join("Probe.java");
    std::fs::write(&probe_path, probe_source).expect("write probe");
    let probe_uri = tower_lsp::lsp_types::Url::from_file_path(&probe_path)
        .expect("probe URI")
        .to_string();
    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace.path())
        .expect("workspace URI")
        .to_string();

    let client = Arc::new(LSPTestClient::new());
    client.initialize(&root_uri);
    client.wait_for_initial_indexing();
    client.did_open(&probe_uri, "java", 1, probe_source);

    let burst_start = Instant::now();
    let bursting = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let probe_client = Arc::clone(&client);
    let probe_flag = Arc::clone(&bursting);
    let probe_uri_for_thread = probe_uri.clone();
    let prober = thread::spawn(move || {
        let mut latencies = Vec::new();
        while probe_flag.load(Ordering::SeqCst) {
            let started = Instant::now();
            let answered = probe_client
                .completion(&probe_uri_for_thread, 3, 10)
                .is_some();
            latencies.push(if answered {
                started.elapsed()
            } else {
                Duration::from_millis(INTERACTIVE_RESPONSE_TIMEOUT_MS)
            });
            thread::sleep(Duration::from_millis(20));
        }
        latencies
    });

    let burst_rounds: usize = std::env::var("HYPERION_BURST_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let burst_client = Arc::clone(&client);
    let burst_sources = sources.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for round in 0..burst_rounds {
            for (uri, source) in &burst_sources {
                burst_client.did_open(uri, "java", 1, source);
            }
            for (version, (uri, source)) in burst_sources.iter().enumerate() {
                burst_client.send_notification(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": version + 2 },
                        "contentChanges": [{ "text": format!("{source}\n// round {round}\n") }],
                    }),
                );
            }
            for (uri, _) in burst_sources.iter().step_by(2) {
                burst_client.send_notification(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": uri } }),
                );
            }
            for (uri, source) in burst_sources.iter().step_by(2) {
                burst_client.did_open(uri, "java", 1, source);
            }
            for (uri, _) in &burst_sources {
                burst_client.send_notification(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": uri } }),
                );
            }
        }
        let _ = done_tx.send(());
    });
    done_rx
        .recv_timeout(Duration::from_mins(2))
        .expect("the server stopped reading the notification burst (wedged)");
    let send_time = burst_start.elapsed();

    let (uri, source) = &sources[files - 1];
    client.did_open(uri, "java", 1, source);
    // The answer is part of draining the burst, so it gets the drain budget rather
    // than the interactive deadline. A loaded machine can take longer than 10 s here.
    let drain_budget_ms = u64::try_from(BURST_DRAIN_BUDGET.as_millis())
        .expect("the drain budget fits in u64 milliseconds");
    let symbols = client
        .document_symbols_within(uri, drain_budget_ms)
        .expect("the server must answer after the burst");
    let drain_time = burst_start.elapsed();
    assert!(symbols
        .as_array()
        .is_some_and(|symbols| !symbols.is_empty()));

    bursting.store(false, Ordering::SeqCst);
    let mut latencies = prober.join().expect("completion prober");
    latencies.sort();
    let (p50, p95, max) = (
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.95),
        latencies.last().copied().unwrap_or_default(),
    );
    println!(
        "BURST files={files} notifications_sent_in={send_time:?} drained_in={drain_time:?} \
         completions={} p50={p50:?} p95={p95:?} max={max:?}",
        latencies.len()
    );
    assert!(
        drain_time < BURST_DRAIN_BUDGET,
        "burst of {files} files took {drain_time:?} (budget {BURST_DRAIN_BUDGET:?})"
    );
    assert!(
        p95 < COMPLETION_P95_BUDGET,
        "completion p95 during the burst was {p95:?} (budget {COMPLETION_P95_BUDGET:?})"
    );

    Arc::into_inner(client)
        .expect("burst thread finished")
        .shutdown();
}

/// Generous ceilings: they catch a wedge or a return to the pre-fix
/// 75-84 s behaviour without flaking on a loaded shared machine.
const BURST_DRAIN_BUDGET: Duration = Duration::from_mins(1);
const COMPLETION_P95_BUDGET: Duration = Duration::from_secs(10);

/// Re-analysing an unchanged document (a dependency changed) must
/// not clear the diagnostics the client shows and then re-send them: the
/// parse-only first phase used to publish an empty set in between.
#[test]
fn diagnostics_of_an_unchanged_document_do_not_flap_when_a_dependency_changes() {
    let workspace = TempDir::new().expect("temp workspace");
    let a_source =
        "package flap;\npublic class A {\n    public void f(B b) {\n        int x = missingSymbol;\n        b.helper();\n    }\n}\n";
    let b_source = "package flap;\npublic class B {\n    public void helper() {}\n}\n";
    let a_path = workspace.path().join("A.java");
    let b_path = workspace.path().join("B.java");
    std::fs::write(&a_path, a_source).unwrap();
    std::fs::write(&b_path, b_source).unwrap();
    let uri_of = |path: &std::path::Path| {
        tower_lsp::lsp_types::Url::from_file_path(path)
            .unwrap()
            .to_string()
    };
    let (a_uri, b_uri) = (uri_of(&a_path), uri_of(&b_path));
    let root_uri = tower_lsp::lsp_types::Url::from_directory_path(workspace.path())
        .unwrap()
        .to_string();

    let client = LSPTestClient::new();
    client.initialize(&root_uri);
    client.wait_for_initial_indexing();
    client.did_open(&a_uri, "java", 1, a_source);
    client.did_open(&b_uri, "java", 1, b_source);

    let publications_of_a = |client: &LSPTestClient| -> Vec<usize> {
        client
            ._notifications
            .lock()
            .iter()
            .filter(|message| {
                message.get("method").and_then(Value::as_str)
                    == Some("textDocument/publishDiagnostics")
                    && message.pointer("/params/uri").and_then(Value::as_str) == Some(&a_uri)
            })
            .map(|message| {
                message
                    .pointer("/params/diagnostics")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            })
            .collect()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !publications_of_a(&client).iter().any(|count| *count > 0) {
        assert!(
            Instant::now() < deadline,
            "A never got its unresolved-symbol diagnostic"
        );
        thread::sleep(Duration::from_millis(50));
    }
    thread::sleep(Duration::from_millis(500));
    let seen_before = publications_of_a(&client).len();

    // Change the dependency several times: A is re-analysed each time.
    for version in 2..8 {
        client.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": b_uri, "version": version },
                "contentChanges": [{ "text": format!("package flap;\npublic class B {{\n    public void helper() {{}}\n    public void extra{version}() {{}}\n}}\n") }],
            }),
        );
        thread::sleep(Duration::from_millis(400));
    }
    thread::sleep(Duration::from_secs(2));
    let after: Vec<usize> = publications_of_a(&client).split_off(seen_before);
    assert!(
        after.iter().all(|count| *count > 0),
        "A's diagnostics were cleared while it was re-analysed: {after:?}"
    );
    client.shutdown();
}
