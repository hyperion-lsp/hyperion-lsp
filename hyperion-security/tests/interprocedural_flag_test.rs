// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `ScanConfig::interprocedural` adds cross-file taint findings to the per-file ones. These
//! tests scan a project whose SQL injection crosses a file boundary, with the flag off and on.

use hyperion_security::finding::{Finding, VulnType};
use hyperion_security::{ScanConfig, SecurityScanner};

/// The caller passes a request parameter into `UserDao.find`, which concatenates it into SQL.
const CONTROLLER: &str = r#"package app;

import java.sql.Connection;
import javax.servlet.http.HttpServletRequest;

public class Controller {
    public void handle(HttpServletRequest request, Connection conn) throws Exception {
        String id = request.getParameter("id");
        UserDao dao = new UserDao();
        dao.find(conn, id);
    }
}
"#;

const USER_DAO: &str = r#"package app;

import java.sql.Connection;
import java.sql.Statement;

public class UserDao {
    public void find(Connection conn, String id) throws Exception {
        Statement st = conn.createStatement();
        st.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#;

/// A SQL injection within one file, which the per-file scan finds without the flag.
const SAME_FILE: &str = r#"package app;

import java.sql.Connection;
import java.sql.Statement;
import javax.servlet.http.HttpServletRequest;

public class Search {
    public void run(HttpServletRequest request, Connection conn) throws Exception {
        Statement st = conn.createStatement();
        st.executeQuery("SELECT * FROM users WHERE name = " + request.getParameter("name"));
    }
}
"#;

fn project() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let package = dir.path().join("app");
    std::fs::create_dir_all(&package).expect("mkdir");
    std::fs::write(package.join("Controller.java"), CONTROLLER).expect("write");
    std::fs::write(package.join("UserDao.java"), USER_DAO).expect("write");
    std::fs::write(package.join("Search.java"), SAME_FILE).expect("write");
    dir
}

fn sql_injections(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::SqlInjection)
        .collect()
}

#[test]
fn without_the_flag_a_cross_file_sql_injection_is_not_found() {
    let dir = project();
    let findings = SecurityScanner::new(ScanConfig::default()).scan_directory(dir.path());

    let sql = sql_injections(&findings);
    assert_eq!(sql.len(), 1, "only the same-file injection: {sql:?}");
    assert!(sql[0].source_location.file.ends_with("Search.java"));
}

#[test]
fn the_flag_adds_the_cross_file_sql_injection_and_keeps_the_per_file_findings() {
    let dir = project();
    let config = ScanConfig::default().with_interprocedural(true);
    let findings = SecurityScanner::new(config).scan_directory(dir.path());

    let sql = sql_injections(&findings);
    assert_eq!(
        sql.len(),
        2,
        "the same-file and the cross-file injection: {sql:?}"
    );
    assert!(sql
        .iter()
        .any(|finding| finding.source_location.file.ends_with("Search.java")));
    assert!(sql
        .iter()
        .any(|finding| finding.source_location.file.ends_with("UserDao.java")));
}

#[test]
fn with_the_flag_the_stats_count_the_cross_file_findings() {
    let dir = project();
    let config = ScanConfig::default().with_interprocedural(true);
    let (findings, stats) = SecurityScanner::new(config).scan_directory_with_stats(dir.path());

    assert_eq!(stats.total_findings, findings.len());
    assert_eq!(
        stats.findings_by_rule.values().sum::<usize>(),
        findings.len()
    );
}

/// Two SQL injections on one line: a cross-file one through `id`, and a same-file one
/// through the request.
const ONE_LINE_DAO: &str = r#"package app;

import java.sql.Connection;
import java.sql.Statement;
import javax.servlet.http.HttpServletRequest;

public class UserDao {
    public void find(Connection conn, String id, HttpServletRequest request) throws Exception {
        Statement st = conn.createStatement();
        st.executeQuery("SELECT * FROM users WHERE id = " + id); st.executeQuery("SELECT * FROM users WHERE name = " + request.getParameter("name"));
    }
}
"#;

const ONE_LINE_CONTROLLER: &str = r#"package app;

import java.sql.Connection;
import javax.servlet.http.HttpServletRequest;

public class Controller {
    public void handle(HttpServletRequest request, Connection conn) throws Exception {
        String id = request.getParameter("id");
        UserDao dao = new UserDao();
        dao.find(conn, id, request);
    }
}
"#;

#[test]
fn a_cross_file_finding_on_the_line_of_a_same_file_finding_is_kept() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let package = dir.path().join("app");
    std::fs::create_dir_all(&package).expect("mkdir");
    std::fs::write(package.join("Controller.java"), ONE_LINE_CONTROLLER).expect("write");
    std::fs::write(package.join("UserDao.java"), ONE_LINE_DAO).expect("write");

    let config = ScanConfig::default().with_interprocedural(true);
    let findings = SecurityScanner::new(config).scan_directory(dir.path());

    let mut columns: Vec<u32> = sql_injections(&findings)
        .iter()
        .map(|finding| finding.source_location.column)
        .collect();
    columns.sort_unstable();
    assert_eq!(
        columns.len(),
        2,
        "both injections on the line: {findings:?}"
    );
    assert_ne!(columns[0], columns[1], "{findings:?}");
}
