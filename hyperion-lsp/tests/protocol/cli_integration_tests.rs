// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! CLI Integration Tests
//!
//! These tests verify the binary version, formatting and DAP contracts.

use std::process::Command;

const fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_hyperion-lsp")
}

#[test]
fn binary_version_has_scriptable_exit_code() {
    let version = Command::new(binary())
        .arg("--version")
        .output()
        .expect("run --version");
    assert!(version.status.success());
    let stdout = String::from_utf8(version.stdout).expect("UTF-8 version output");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout:?}");
}

#[test]
fn binary_format_list_modes_preserve_source_bytes() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let java = temp.path().join("Example.java");
    let source = b"// caf\xc3\xa9\r\nclass Example{int value;}\r\n";
    std::fs::write(&java, source).expect("write Java fixture");

    for flags in [vec!["--list"], vec!["--check"], vec!["--list", "--check"]] {
        let output = Command::new(binary())
            .arg("format")
            .args(&flags)
            .arg(&java)
            .output()
            .expect("run non-mutating formatter");
        assert_eq!(output.status.success(), !flags.contains(&"--check"));
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 list"),
            format!("{}\n", java.display()),
            "list/check should identify the unformatted file"
        );
        assert_eq!(std::fs::read(&java).expect("source bytes"), source);
    }
}

async fn assert_lsp_logging_falls_back(log_dir: &std::path::Path) {
    use serde_json::{json, Value};
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut child = tokio::process::Command::new(binary())
            .env("LOG_FILE", log_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn LSP");
        let mut stdin = child.stdin.take().expect("LSP stdin");
        let mut stdout = BufReader::new(child.stdout.take().expect("LSP stdout"));
        let mut stderr = child.stderr.take().expect("LSP stderr");
        let logs = tokio::spawn(async move {
            let mut logs = String::new();
            stderr.read_to_string(&mut logs).await.expect("stderr logs");
            logs
        });

        for (id, method, params) in [
            (1, "initialize", json!({"capabilities": {}})),
            (2, "shutdown", Value::Null),
        ] {
            let body =
                json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
            stdin
                .write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
                .await
                .expect("send request");
            let mut header = String::new();
            stdout
                .read_line(&mut header)
                .await
                .expect("response header");
            let length: usize = header
                .strip_prefix("Content-Length: ")
                .expect("stdout contains only LSP frames")
                .trim()
                .parse()
                .expect("frame length");
            let mut separator = String::new();
            stdout
                .read_line(&mut separator)
                .await
                .expect("header separator");
            assert_eq!(separator, "\r\n");
            let mut bytes = vec![0; length];
            stdout.read_exact(&mut bytes).await.expect("response body");
            let response: Value = serde_json::from_slice(&bytes).expect("JSON response");
            assert_eq!(response["id"], id);
            assert!(response.get("error").is_none(), "{response}");
        }
        let exit = r#"{"jsonrpc":"2.0","method":"exit"}"#;
        stdin
            .write_all(format!("Content-Length: {}\r\n\r\n{exit}", exit.len()).as_bytes())
            .await
            .expect("send exit");
        assert!(child.wait().await.expect("LSP exit").success());
        let logs = logs.await.expect("stderr reader");
        assert!(logs.contains("falling back to stderr"), "{logs}");
        assert!(logs.contains("Starting Hyperion LSP Server"), "{logs}");
    })
    .await
    .expect("LSP initialization and shutdown deadline");
}

#[tokio::test]
async fn binary_lsp_initializes_when_log_directory_is_a_file() {
    let temp = tempfile::tempdir().expect("temp logs");
    let log_dir = temp.path().join("not-a-directory");
    std::fs::write(&log_dir, "occupied").expect("create invalid directory");
    assert_lsp_logging_falls_back(&log_dir).await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn binary_lsp_initializes_when_log_directory_is_not_writable() {
    // procfs cannot contain an ordinary log file, including when tests run as root.
    assert_lsp_logging_falls_back(std::path::Path::new("/proc")).await;
}

#[test]
fn binary_format_check_write_and_parse_failure_contract() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let java = temp.path().join("Example.java");
    let unformatted = "class Example{int value;}";
    let formatted = "class Example {\n    int value;\n}\n";
    std::fs::write(&java, unformatted).expect("write Java fixture");

    let check = Command::new(binary())
        .arg("format")
        .arg("--check")
        .arg(&java)
        .output()
        .expect("run format --check");
    assert!(
        !check.status.success(),
        "unformatted source must fail --check"
    );
    assert_eq!(
        std::fs::read_to_string(&java).expect("read checked source"),
        unformatted,
        "--check must not modify the file"
    );

    let write = Command::new(binary())
        .arg("format")
        .arg(&java)
        .output()
        .expect("run format");
    assert!(write.status.success(), "format failed: {write:?}");
    assert_eq!(
        std::fs::read_to_string(&java).expect("read formatted source"),
        formatted
    );

    let clean_check = Command::new(binary())
        .arg("format")
        .arg("--check")
        .arg(&java)
        .output()
        .expect("run clean format --check");
    assert!(
        clean_check.status.success(),
        "clean --check failed: {clean_check:?}"
    );

    let malformed = temp.path().join("Broken.java");
    let malformed_source = "class Broken { void run( {";
    std::fs::write(&malformed, malformed_source).expect("write malformed fixture");
    let malformed_result = Command::new(binary())
        .arg("format")
        .arg(&malformed)
        .output()
        .expect("run malformed format");
    assert!(
        !malformed_result.status.success(),
        "malformed Java must fail closed"
    );
    assert_eq!(
        std::fs::read_to_string(&malformed).expect("read malformed source"),
        malformed_source,
        "a formatter parse failure must leave the file untouched"
    );
}

/// `hyperion-lsp dap` answers on stdout and logs to a file instead of being
/// silent.
#[test]
fn dap_subcommand_speaks_dap_and_writes_a_log_file() {
    use std::io::{Read, Write};
    use std::process::Stdio;

    let temp = tempfile::tempdir().expect("temporary log dir");
    let mut child = Command::new(binary())
        .arg("dap")
        .env("LOG_DIR", temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dap adapter");

    let request =
        r#"{"seq":1,"type":"request","command":"initialize","arguments":{"linesStartAt1":true}}"#;
    let mut stdin = child.stdin.take().expect("adapter stdin");
    write!(stdin, "Content-Length: {}\r\n\r\n{request}", request.len()).expect("send initialize");
    stdin.flush().expect("flush initialize");

    // Read the initialize response and the `initialized` event, then close
    // stdin so the adapter shuts down.
    let mut stdout = child.stdout.take().expect("adapter stdout");
    let mut received = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !String::from_utf8_lossy(&received).contains("\"event\":\"initialized\"") {
        let read = stdout.read(&mut buffer).expect("read adapter output");
        assert!(
            read > 0,
            "adapter closed stdout early: {}",
            String::from_utf8_lossy(&received)
        );
        received.extend_from_slice(&buffer[..read]);
    }
    drop(stdin);
    let status = child.wait().expect("adapter exit");
    assert!(status.success(), "adapter exit: {status:?}");
    let received = String::from_utf8_lossy(&received);
    assert!(
        received.contains("\"command\":\"initialize\""),
        "{received}"
    );
    assert!(
        received.contains("supportsTerminateRequest\":true"),
        "{received}"
    );

    let log =
        std::fs::read_to_string(temp.path().join("hyperion-dap.log")).expect("adapter log file");
    assert!(log.contains("Starting Hyperion DAP adapter"), "{log}");
}
