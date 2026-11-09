// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for lambda taint propagation (Phase 4.2)
//!
//! Tests that tainted data flowing through lambda expressions and method references
//! is properly tracked.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

// =============================================================================
// Lambda Capture Tests - Pattern 1: Captured variable taint
// =============================================================================

#[test]
fn test_lambda_captures_tainted_variable() {
    // Lambda that captures a tainted variable from outer scope
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String tainted = request.getParameter("id");
        Runnable r = () -> {
            stmt.executeQuery(tainted);
        };
        r.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through lambda capture"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_lambda_captures_tainted_variable_in_foreach() {
    // Lambda in forEach that captures tainted variable
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String tainted = request.getParameter("x");
        list.forEach(s -> {
            stmt.executeQuery(tainted);
        });
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection in forEach lambda with captured tainted variable"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_lambda_captures_tainted_field() {
    // Lambda that captures a tainted field
    let source = r#"
public class UserController {
    private String userId;

    public void setUserId(HttpServletRequest request) {
        this.userId = request.getParameter("id");
    }

    public void process() {
        Runnable r = () -> {
            stmt.executeQuery(userId);
        };
        r.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through lambda capturing tainted field"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

// =============================================================================
// Lambda as Taint Passthrough - Pattern 2: Lambda parameter -> return value
// =============================================================================

#[test]
fn test_lambda_identity_propagates_taint() {
    // Identity lambda should propagate taint
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        Function<String, String> identity = s -> s;
        String result = identity.apply(taintedInput);
        stmt.executeQuery(result);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through identity lambda"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_lambda_with_tainted_concatenation() {
    // Lambda that concatenates with tainted input
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        Function<String, String> addPrefix = s -> "SELECT * FROM users WHERE id = " + s;
        String query = addPrefix.apply(taintedInput);
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through lambda with tainted concatenation"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_lambda_assigned_to_variable_then_called() {
    // Lambda assigned to variable, then called with tainted input
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String input = request.getParameter("id");
        Function<String, String> processor = s -> "SELECT * FROM users WHERE id = " + s;
        String query = processor.apply(input);
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection when lambda is assigned then called"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

// =============================================================================
// Stream Operations - Pattern 4: Lambda with captured taint in streams
// =============================================================================

#[test]
fn test_stream_map_with_captured_tainted_variable() {
    // Stream with map() that captures tainted variable
    // Taint flows from the captured value through map() into the terminal
    // lambda parameter.
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        list.stream()
            .map(x -> x + taintedInput)
            .forEach(s -> stmt.executeQuery(s));
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::SqlInjection),
        "Should propagate captured taint through map into the forEach parameter"
    );
}

#[test]
fn test_stream_filter_then_sink() {
    // Stream with filter and tainted forEach
    // NOTE: This should work because the forEach lambda directly captures taintedInput.
    // However, the lambda might not be visited correctly in the chained method call context.
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        list.stream()
            .filter(x -> x.length() > 0)
            .forEach(s -> stmt.executeQuery(taintedInput));
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_injection_findings: Vec<_> = findings
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::SqlInjection)
        .collect();

    assert_eq!(
        sql_injection_findings.len(),
        1,
        "the nested sink should produce exactly one SQL injection finding"
    );
    let finding = sql_injection_findings[0];
    assert_eq!(finding.code_snippet, "stmt.executeQuery(taintedInput)");
    assert_eq!(finding.source_location.line, 7);
}

#[test]
fn test_distinct_sinks_on_same_line_are_not_deduplicated() {
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        stmt.executeQuery(taintedInput); stmt.executeQuery(taintedInput);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));
    let sql_injection_findings: Vec<_> = findings
        .iter()
        .filter(|finding| finding.vuln_type == VulnType::SqlInjection)
        .collect();

    assert_eq!(
        sql_injection_findings.len(),
        2,
        "distinct sink locations on one line must remain distinct"
    );
    assert_eq!(
        sql_injection_findings[0].source_location.line,
        sql_injection_findings[1].source_location.line
    );
    assert_ne!(
        sql_injection_findings[0].source_location.column,
        sql_injection_findings[1].source_location.column
    );
    assert_eq!(
        sql_injection_findings[0].id, sql_injection_findings[1].id,
        "this fixture must exercise the known same-line finding-ID collision"
    );
}

#[test]
fn test_stream_collect_with_tainted_map() {
    // Stream that maps tainted data and collects
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String taintedInput = request.getParameter("id");
        List<String> queries = list.stream()
            .map(x -> "SELECT * FROM " + taintedInput + " WHERE id = " + x)
            .collect(Collectors.toList());
        stmt.executeQuery(queries.get(0));
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through stream map with captured tainted variable"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

// =============================================================================
// Method Reference Tests - Pattern 3: Method reference taint behavior
// =============================================================================

#[test]
fn test_method_reference_on_tainted_object() {
    // Method reference on tainted object should propagate taint
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        StringBuilder taintedBuilder = new StringBuilder();
        taintedBuilder.append(request.getParameter("id"));
        Function<StringBuilder, String> converter = StringBuilder::toString;
        String result = converter.apply(taintedBuilder);
        stmt.executeQuery(result);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through method reference on tainted object"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

// =============================================================================
// Negative Tests - Clean lambdas should not trigger false positives
// =============================================================================

#[test]
fn test_lambda_with_clean_variables_no_false_positive() {
    // Lambda that only uses clean variables
    let source = r#"
public class UserController {
    public void process() {
        String clean = "safe_value";
        Runnable r = () -> {
            stmt.executeQuery(clean);
        };
        r.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection with clean lambda capture"
    );
}

#[test]
fn test_lambda_parameter_shadows_tainted_outer_variable() {
    // Lambda parameter shadows outer tainted variable - should be clean
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String tainted = request.getParameter("id");
        Function<String, String> processor = tainted -> {
            return "safe_" + tainted;
        };
        String result = processor.apply("clean_input");
        stmt.executeQuery(result);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings
            .iter()
            .any(|finding| finding.vuln_type == VulnType::SqlInjection),
        "A lambda parameter must shadow the tainted outer variable"
    );
}

// =============================================================================
// Complex Lambda Tests - Real-world patterns
// =============================================================================

#[test]
fn test_nested_lambdas_with_tainted_capture() {
    // Nested lambdas both capturing tainted variable
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String tainted = request.getParameter("id");
        Supplier<Runnable> outer = () -> {
            return () -> {
                stmt.executeQuery(tainted);
            };
        };
        outer.get().run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through nested lambda capture"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_lambda_with_multiple_captured_variables_one_tainted() {
    // Lambda captures multiple variables, one is tainted
    let source = r#"
public class UserController {
    public void process(HttpServletRequest request) {
        String clean = "safe";
        String tainted = request.getParameter("id");
        Runnable r = () -> {
            String query = clean + " " + tainted;
            stmt.executeQuery(query);
        };
        r.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection when lambda captures both clean and tainted variables"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

// =============================================================================
// XSS Tests with Lambdas
// =============================================================================

#[test]
fn test_lambda_xss_through_captured_variable() {
    // Lambda captures tainted variable and uses it in XSS sink
    let source = r#"
public class UserController {
    public void render(HttpServletRequest request, HttpServletResponse response) {
        String userInput = request.getParameter("name");
        Runnable renderer = () -> {
            writer.write("<div>" + userInput + "</div>");
        };
        renderer.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect XSS through lambda capture"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Xss));
}

// =============================================================================
// Command Injection Tests with Lambdas
// =============================================================================

#[test]
fn test_lambda_command_injection_through_captured_variable() {
    // Lambda captures tainted variable and uses it in command execution
    let source = r#"
public class SystemController {
    public void execute(HttpServletRequest request) {
        String command = request.getParameter("cmd");
        Runnable executor = () -> {
            Runtime.getRuntime().exec(command);
        };
        executor.run();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SystemController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through lambda capture"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}
