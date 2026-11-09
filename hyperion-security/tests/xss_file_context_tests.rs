// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for XSS file output context filtering (Phase 2.1)
//!
//! XSS vulnerabilities only apply when data is written to HTTP responses.
//! Writing to files should NOT be flagged as XSS.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

/// Test that file writer output is NOT flagged as XSS
#[test]
fn test_file_writer_not_xss() {
    let source = r#"
public class DataExporter {
    public void export(@RequestBody String data) {
        PrintWriter fileWriter = new PrintWriter("output.txt");
        fileWriter.println(data);
        fileWriter.close();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DataExporter.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect XSS - fileWriter indicates file output
    let xss_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Xss)
        .collect();
    assert!(
        xss_findings.is_empty(),
        "Should NOT detect XSS when writing to fileWriter. Found: {:?}",
        xss_findings
    );
}

/// Test that HTTP response output IS flagged as XSS
#[test]
fn test_http_response_is_xss() {
    let source = r#"
public class UserController {
    public void handle(@RequestBody String data, HttpServletResponse response) throws IOException {
        PrintWriter out = response.getWriter();
        out.println("<h1>" + data + "</h1>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - this is HTTP response output
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS when writing to HTTP response"
    );
}

/// Test that direct response.getWriter() output IS flagged as XSS
/// Note: Chained method calls like response.getWriter().println() require
/// intermediate variable tracking which is a separate enhancement.
/// This test uses a stored reference pattern that the scanner can track.
#[test]
fn test_direct_response_writer_is_xss() {
    let source = r#"
public class TemplateController {
    @GetMapping("/render")
    public void render(@RequestParam String title, HttpServletResponse response) throws IOException {
        PrintWriter writer = response.getWriter();
        writer.println("<title>" + title + "</title>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - HTTP response output via stored reference
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS when writing to response.getWriter()"
    );
}

/// Test that file output stream is NOT flagged as XSS
#[test]
fn test_file_output_stream_not_xss() {
    let source = r#"
public class LogWriter {
    private PrintStream fileOutput;

    @PostMapping("/log")
    public void log(@RequestBody String message) {
        fileOutput.println(message);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("LogWriter.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect XSS - fileOutput indicates file output
    let xss_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Xss)
        .collect();
    assert!(
        xss_findings.is_empty(),
        "Should NOT detect XSS when writing to fileOutput. Found: {:?}",
        xss_findings
    );
}

/// Test that servlet output stream IS flagged as XSS
#[test]
fn test_servlet_output_stream_is_xss() {
    let source = r#"
public class BinaryController {
    @GetMapping("/data")
    public void getData(@RequestParam String filename, HttpServletResponse response) throws IOException {
        ServletOutputStream out = response.getOutputStream();
        out.println(filename);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("BinaryController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - servlet output is HTTP output
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS when writing to ServletOutputStream"
    );
}

/// Test that class name containing "File" prevents XSS detection
#[test]
fn test_file_class_name_not_xss() {
    let source = r#"
public class FileProcessor {
    private PrintStream outputStream;

    public void process(@RequestBody String data) {
        outputStream.println(data);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileProcessor.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect XSS - class name contains "File"
    let xss_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Xss)
        .collect();
    assert!(
        xss_findings.is_empty(),
        "Should NOT detect XSS in FileProcessor class. Found: {:?}",
        xss_findings
    );
}

/// Test that servlet class IS flagged for XSS even with "Stream" in name
#[test]
fn test_servlet_stream_is_xss() {
    let source = r#"
public class ServletOutputStream {
    public void write(@RequestParam String data, PrintWriter out) {
        out.println(data);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ServletOutputStream.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - "Servlet" in name indicates HTTP context
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS in servlet-related classes"
    );
}

/// Test that controller class IS flagged for XSS
#[test]
fn test_controller_is_xss() {
    let source = r#"
public class UserController {
    public void display(@RequestParam String name, PrintWriter writer) {
        writer.println("<p>Hello " + name + "</p>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - controller class likely handles HTTP
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS in Controller classes"
    );
}

/// Test that model addAttribute IS flagged for XSS (Spring MVC)
#[test]
fn test_model_add_attribute_is_xss() {
    let source = r#"
public class UserController {
    @GetMapping("/user")
    public String getUser(@RequestParam String name, Model model) {
        model.addAttribute("name", name);
        return "user";
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect XSS - model.addAttribute can lead to XSS in templates
    assert!(
        findings.iter().any(|f| f.vuln_type == VulnType::Xss),
        "Should detect XSS when adding tainted data to Model"
    );
}
