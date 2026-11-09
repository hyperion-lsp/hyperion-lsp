// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Phase 9: Conservative method return tainting
//!
//! These tests verify that taint is conservatively propagated through methods
//! without explicit summaries, using TrustLevel::Derived for lower confidence.

use hyperion_security::{ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::Path;

/// Test: Custom getter method propagates taint conservatively
///
/// When a custom method takes tainted input and returns a value,
/// we conservatively assume the return value may be tainted.
#[test]
fn test_custom_getter_propagates_taint_conservatively() {
    let source = r#"
public class UserService {
    private Repository repository;

    public String getUserName(String id) {
        return repository.findById(id).getName();
    }

    public void logUser(@RequestParam String id) {
        String name = getUserName(id);
        log.info("User: " + name);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should detect log injection through conservative taint propagation
    assert!(
        !findings.is_empty(),
        "Should detect log injection through custom getUserName() method"
    );

    let log_injection = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::LogInjection);
    assert!(
        log_injection.is_some(),
        "Should find log injection vulnerability"
    );

    // Verify severity is LOW for conservative (Derived) taint
    let finding = log_injection.unwrap();
    assert_eq!(
        finding.severity,
        Severity::Low,
        "Conservative taint should result in LOW severity"
    );
}

/// Test: Method without tainted input should not produce findings
#[test]
fn test_method_without_tainted_input_is_safe() {
    let source = r#"
public class UserService {
    public String getDefaultName() {
        return "default";
    }

    public void logDefault() {
        String name = getDefaultName();
        log.info("User: " + name);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // No tainted input - should not produce findings
    assert!(
        findings.is_empty(),
        "Should NOT detect vulnerability when method has no tainted input"
    );
}

/// Test: Void methods should not propagate taint (no return value)
#[test]
fn test_void_method_no_propagation() {
    let source = r#"
public class UserService {
    public void processUser(String id) {
        // Does something but doesn't return
    }

    public void handleRequest(@RequestParam String id) {
        processUser(id);
        // No value returned from processUser to flow to sink
        log.info("Processed");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    // Void method doesn't return a value, so no taint to propagate to log.info
    // The literal "Processed" is safe
    assert!(
        findings.is_empty(),
        "Should NOT detect vulnerability for void method calls"
    );
}

/// Test: Methods with "get" prefix are recognized as data-returning
#[test]
fn test_get_prefix_method_propagates_taint() {
    let source = r#"
public class DataService {
    public String getData(String key) {
        return cache.lookup(key);
    }

    public void useData(@RequestParam String key) {
        String data = getData(key);
        Runtime.getRuntime().exec(data);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DataService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through getData() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test: Methods with "find" prefix are recognized as data-returning
#[test]
fn test_find_prefix_method_propagates_taint() {
    let source = r#"
public class SearchService {
    public String findResult(String query) {
        return database.search(query);
    }

    public void displayResult(@RequestParam String query) {
        String result = findResult(query);
        log.info("Result: " + result);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SearchService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect log injection through findResult() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::LogInjection));
}

/// Test: Methods with "load" prefix are recognized as data-returning
#[test]
fn test_load_prefix_method_propagates_taint() {
    let source = r#"
public class ConfigService {
    public String loadConfig(String path) {
        return Files.readString(Paths.get(path));
    }

    public void showConfig(@RequestParam String path) {
        String config = loadConfig(path);
        log.info("Config: " + config);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect log injection through loadConfig() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::LogInjection));
}

/// Test: Methods with "transform" prefix are recognized as data-returning
#[test]
fn test_transform_prefix_method_propagates_taint() {
    let source = r#"
public class TransformService {
    public String transformInput(String raw) {
        return raw.toUpperCase() + "_transformed";
    }

    public void processInput(@RequestParam String input) {
        String transformed = transformInput(input);
        String query = "SELECT * FROM data WHERE name = '" + transformed + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TransformService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through transformInput() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test: Methods with "parse" prefix are recognized as data-returning
#[test]
fn test_parse_prefix_method_propagates_taint() {
    let source = r#"
public class ParserService {
    public String parseUserData(String json) {
        return new JsonParser().parse(json).get("name");
    }

    public void showUser(@RequestParam String json) {
        String name = parseUserData(json);
        log.info("User: " + name);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ParserService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect log injection through parseUserData() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::LogInjection));
}

/// Test: Methods with "convert" prefix are recognized as data-returning
#[test]
fn test_convert_prefix_method_propagates_taint() {
    let source = r#"
public class ConversionService {
    public String convertToHtml(String markdown) {
        return markdownProcessor.convert(markdown);
    }

    public void displayMarkdown(@RequestParam String md) {
        String html = convertToHtml(md);
        log.info("HTML: " + html);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConversionService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect log injection through convertToHtml() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::LogInjection));
}

/// Test: Methods with "format" prefix are recognized as data-returning
#[test]
fn test_format_prefix_method_propagates_taint() {
    let source = r#"
public class FormatterService {
    public String formatMessage(String template) {
        return String.format("User: %s", template);
    }

    public void logMessage(@RequestParam String msg) {
        String formatted = formatMessage(msg);
        log.info(formatted);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FormatterService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect log injection through formatMessage() method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::LogInjection));
}

/// Test: Arbitrary method still propagates taint (with Derived trust level)
/// Even methods without recognizable prefixes should conservatively propagate taint
#[test]
fn test_arbitrary_method_propagates_taint() {
    let source = r#"
public class SomeService {
    public String doSomething(String input) {
        return "result_" + input;
    }

    public void handle(@RequestParam String data) {
        String result = doSomething(data);
        Runtime.getRuntime().exec(result);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SomeService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through arbitrary method with tainted arg"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test: Multiple chained custom method calls propagate taint
#[test]
fn test_chained_custom_methods_propagate_taint() {
    let source = r#"
public class ProcessingService {
    public String step1(String input) {
        return input.trim();
    }

    public String step2(String input) {
        return input.toLowerCase();
    }

    public void process(@RequestParam String data) {
        String s1 = step1(data);
        String s2 = step2(s1);
        String query = "SELECT * FROM t WHERE x = '" + s2 + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProcessingService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through chained custom methods"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test: Severity is MEDIUM for all vulnerability types with Derived trust level
#[test]
fn test_derived_trust_level_medium_severity_sql_injection() {
    let source = r#"
public class DataService {
    public String lookup(String key) {
        return cache.get(key);
    }

    public void query(@RequestParam String key) {
        String value = lookup(key);
        String sql = "SELECT * FROM t WHERE x = '" + value + "'";
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DataService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty());
    let sql_finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::SqlInjection);
    assert!(sql_finding.is_some());

    // SQL injection from Derived trust level should be MEDIUM severity
    assert_eq!(
        sql_finding.unwrap().severity,
        Severity::Medium,
        "SQL injection from conservative taint should be MEDIUM severity"
    );
}

/// Test: Severity is MEDIUM for command injection with Derived trust level
#[test]
fn test_derived_trust_level_medium_severity_command_injection() {
    let source = r#"
public class CommandService {
    public String buildCommand(String input) {
        return "echo " + input;
    }

    public void execute(@RequestParam String data) {
        String cmd = buildCommand(data);
        Runtime.getRuntime().exec(cmd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty());
    let cmd_finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::CommandInjection);
    assert!(cmd_finding.is_some());

    // Command injection from Derived trust level should be MEDIUM severity
    assert_eq!(
        cmd_finding.unwrap().severity,
        Severity::Medium,
        "Command injection from conservative taint should be MEDIUM severity"
    );
}

/// Test: Method with explicit summary uses original trust level (not Derived)
#[test]
fn test_method_with_summary_preserves_trust_level() {
    let source = r#"
public class StringService {
    public void process(@RequestParam String input) {
        // String.toLowerCase() has an explicit summary - should preserve Untrusted level
        String lower = input.toLowerCase();
        log.info("Value: " + lower);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("StringService.java"));

    eprintln!("Findings: {:?}", findings);

    // Should find log injection
    let log_finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::LogInjection);
    assert!(log_finding.is_some());

    // Log injection from direct @RequestParam (Untrusted) should be MEDIUM severity
    assert_eq!(
        log_finding.unwrap().severity,
        Severity::Medium,
        "Log injection from direct user input should be MEDIUM severity"
    );
}
