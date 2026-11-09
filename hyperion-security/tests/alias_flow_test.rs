// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A request parameter that reaches a sink through a local alias and a helper method is a SQL
//! injection in the per-file scan. The alias is a local declaration whose initializer is tainted.

use hyperion_security::finding::VulnType;
use hyperion_security::{ScanConfig, SecurityScanner};

const SERVICE: &str = r#"package app;

import java.sql.Connection;
import java.sql.Statement;
import javax.servlet.http.HttpServletRequest;

public class Service {
    public void handle(HttpServletRequest request, Connection conn) throws Exception {
        String raw = request.getParameter("id");
        String key = raw;
        query(conn, key);
    }

    private void query(Connection conn, String id) throws Exception {
        Statement st = conn.createStatement();
        st.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#;

#[test]
fn a_request_parameter_reaches_a_sink_through_a_local_alias_and_a_helper() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let package = dir.path().join("app");
    std::fs::create_dir_all(&package).expect("mkdir");
    std::fs::write(package.join("Service.java"), SERVICE).expect("write");

    let findings = SecurityScanner::new(ScanConfig::default()).scan_directory(dir.path());

    let sql = findings
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::SqlInjection)
        .count();
    assert_eq!(sql, 1, "{findings:?}");
}
