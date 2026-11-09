// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{Finding, ScanConfig, SecurityScanner, VulnType};
use std::fmt::Write;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const CHAIN_LENGTH: usize = 40;

/// Scans on a separate thread so that a regression fails the test instead of
/// hanging the test run. Taint evaluation of a method chain used to be 2^n in
/// the chain length; 40 calls never finished.
fn scan_with_timeout(source: String) -> Vec<Finding> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let scanner = SecurityScanner::new(ScanConfig::default());
        let findings = scanner.scan_source(&source, Path::new("Chain.java"));
        let _ = sender.send(findings);
    });
    receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("scanning a long method chain should finish quickly")
}

fn chain(call: &str) -> String {
    let mut chain = String::new();
    for i in 0..CHAIN_LENGTH {
        write!(chain, "\n            .{call}{i}(value)").expect("writing a String cannot fail");
    }
    chain
}

#[test]
fn test_long_setter_chain_in_local_variable() {
    let source = format!(
        r#"
public class PaymentService {{
    public Payment create(String value) {{
        Payment payment = new Payment(){};
        return payment;
    }}
}}
"#,
        chain("setField")
    );

    scan_with_timeout(source);
}

#[test]
fn test_long_builder_chain_in_field_assignment() {
    let source = format!(
        r#"
public class PaymentDao {{
    private final Orm orm;

    public PaymentDao(String value) {{
        this.orm = Orm.builder(Payment.class){};
    }}
}}
"#,
        chain("with")
    );

    scan_with_timeout(source);
}

#[test]
fn test_taint_flows_through_long_chain() {
    let trims: String = (0..CHAIN_LENGTH).map(|_| ".trim()").collect();
    let source = format!(
        r#"
public class UserService {{
    public void search(HttpServletRequest request) {{
        String id = request.getParameter("id"){trims};
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }}
}}
"#
    );

    let findings = scan_with_timeout(source);

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "taint should survive a long chain, got: {findings:?}"
    );
}
