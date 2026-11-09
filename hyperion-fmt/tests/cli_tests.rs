// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn format_stdin(source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hyperion-fmt"))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("formatter starts");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(source.as_bytes())
        .expect("write Java input");
    child.wait_with_output().expect("formatter exits")
}

#[test]
fn dash_formats_standard_input() {
    let output = format_stdin("class  C{int  value;}");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::str::from_utf8(&output.stdout).expect("UTF-8 output"),
        "class C {\n    int value;\n}\n"
    );
    assert_eq!(output.stderr, b"", "{output:?}");
}

#[test]
fn dash_reports_invalid_standard_input_without_partial_output() {
    let output = format_stdin("class C {");
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"", "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    assert!(stderr.contains("parse error:"), "{stderr}");
    assert!(!stderr.contains("Unknown option"), "{stderr}");
}
