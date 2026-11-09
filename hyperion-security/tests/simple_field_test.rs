// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner};
use std::path::Path;

#[test]
fn test_simple_field_case() {
    let source = r#"
public class UserService {
    private String userId;

    public void setUserId(HttpServletRequest request) {
        this.userId = request.getParameter("id");
    }

    public void search() {
        String query = "SELECT * FROM users WHERE id = " + this.userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through field"
    );
}
