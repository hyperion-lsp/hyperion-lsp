// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use crate::config::ScanConfig;
use crate::finding::Severity;
use crate::frameworks::FrameworkRegistry;
use crate::VulnType;
use hyperion_parser::{Lexer, Parser, SyntaxNode as RawSyntaxNode};
use std::path::Path;

fn print_tree(node: &SyntaxNode, indent: usize) {
    let indent_str = "  ".repeat(indent);
    eprintln!("{}NODE: {:?}", indent_str, node.kind());

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if !token.kind().is_trivia() {
                    eprintln!(
                        "{}  TOKEN: {:?} = {:?}",
                        indent_str,
                        token.kind(),
                        token.text()
                    );
                }
            }
            rowan::NodeOrToken::Node(child_node) => {
                print_tree(&child_node, indent + 1);
            }
        }
    }
}

#[test]
fn test_parser_structure() {
    let source = r#"
public class UserController {
	    public void getUser(HttpServletRequest request) {
	        String id = request.getParameter("id");
	        stmt.executeQuery(query);
	    }
	}
	"#;
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    eprintln!("Parse errors: {:?}", errors);

    // Find the local var decl
    for desc in root.descendants() {
        if desc.kind() == SyntaxKind::LOCAL_VAR_DECL {
            eprintln!("\n=== LOCAL_VAR_DECL structure ===");
            print_tree(&desc, 0);
        }
        if desc.kind() == SyntaxKind::EXPR_STMT {
            eprintln!("\n=== EXPR_STMT structure ===");
            print_tree(&desc, 0);
        }
    }

    assert!(errors.is_empty(), "Should parse without errors");
}

#[test]
fn test_chained_call_structure() {
    let source = r#"
public class CommandRunner {
    public void run(String cmd) {
        Runtime.getRuntime().exec(cmd);
    }
}
"#;
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    eprintln!("Parse errors: {:?}", errors);

    for desc in root.descendants() {
        if desc.kind() == SyntaxKind::EXPR_STMT {
            eprintln!("\n=== EXPR_STMT structure ===");
            print_tree(&desc, 0);
        }
    }

    assert!(errors.is_empty(), "Should parse without errors");
}

#[test]
fn test_source_detection() {
    let mut fw = FrameworkRegistry::with_builtin();
    let source_registry = fw.source_registry();

    // Test that getParameter is a source for request
    let result = source_registry.is_source(Some("request"), "getParameter");
    eprintln!("is_source(request, getParameter) = {:?}", result);
    assert!(
        result.is_some(),
        "getParameter on request should be a source"
    );
}

#[test]
fn test_sink_detection() {
    let mut fw = FrameworkRegistry::with_builtin();
    let sink_registry = fw.sink_registry();

    // Test that executeQuery is a sink for stmt
    let result = sink_registry.is_sink(Some("stmt"), "executeQuery");
    eprintln!("is_sink(stmt, executeQuery) = {:?}", result);
    assert!(result.is_some(), "executeQuery on stmt should be a sink");
}

#[test]
fn test_sql_injection_detection() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty(), "Should detect SQL injection");
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_command_injection_detection() {
    let source = r#"
public class CommandRunner {
    public void runCommand(@RequestParam String cmd) {
        Runtime.getRuntime().exec(cmd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandRunner.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty(), "Should detect command injection");
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_numeric_conversion_no_false_positive() {
    // Integer.parseInt should NOT cause SQL injection finding
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        int id = Integer.parseInt(idParam);
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect SQL injection because Integer.parseInt returns numeric type
    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for numeric conversion"
    );
}

#[test]
fn test_prepared_statement_no_false_positive() {
    // PreparedStatement with setString should NOT cause finding
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String name = request.getParameter("name");
        PreparedStatement stmt = conn.prepareStatement("SELECT * FROM users WHERE name = ?");
        stmt.setString(1, name);
        stmt.executeQuery();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should NOT detect SQL injection because PreparedStatement is used properly
    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for PreparedStatement with setString"
    );
}

#[test]
fn test_string_concat_still_detected() {
    // Direct string concatenation should still be detected
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String name = request.getParameter("name");
        String query = "SELECT * FROM users WHERE name = '" + name + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Should still detect SQL injection for string concatenation
    assert!(!findings.is_empty(), "Should detect SQL injection");
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_data_flow_tracking() {
    // Test that data flow path is tracked through assignments
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        String userId = id;
        String query = "SELECT * FROM users WHERE id = " + userId;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty(), "Should detect SQL injection");
    let finding = &findings[0];

    // Check that data flow path is captured
    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
    assert!(
        finding.dataflow.len() >= 2,
        "Should have at least source and assignment steps"
    );
}

#[test]
fn test_multi_step_flow() {
    // Test that multi-step flows are captured correctly
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String input = request.getParameter("input");
        String temp1 = input;
        String temp2 = temp1;
        String command = temp2;
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty(), "Should detect command injection");
    let finding = &findings[0];

    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    // Should track all assignment steps
    assert!(
        finding.dataflow.len() >= 4,
        "Should track source + 3 assignments, got {}",
        finding.dataflow.len()
    );
}

#[test]
fn test_sanitization_tracking() {
    // This test demonstrates the taint state infrastructure
    // Actual sanitization will be implemented in future phases
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String id = request.getParameter("id");
        stmt.executeQuery("SELECT * FROM users WHERE id = " + id);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    assert!(!findings.is_empty(), "Should detect SQL injection");

    // Verify that the finding has source information
    let finding = &findings[0];
    assert!(finding.sink_location.is_some(), "Should have sink location");
}

#[test]
fn test_long_parse_long_sanitizer() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        long id = Long.parseLong(idParam);
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Long.parseLong conversion"
    );
}

#[test]
fn test_double_parse_double_sanitizer() {
    let source = r#"
public class PriceController {
    public void updatePrice(HttpServletRequest request) {
        String priceParam = request.getParameter("price");
        double price = Double.parseDouble(priceParam);
        String query = "UPDATE products SET price = " + price + " WHERE id = 1";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PriceController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Double.parseDouble conversion"
    );
}

#[test]
fn test_float_parse_float_sanitizer() {
    let source = r#"
public class RatingController {
    public void updateRating(HttpServletRequest request) {
        String ratingParam = request.getParameter("rating");
        float rating = Float.parseFloat(ratingParam);
        String query = "UPDATE items SET rating = " + rating;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RatingController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Float.parseFloat conversion"
    );
}

#[test]
fn test_short_parse_short_sanitizer() {
    let source = r#"
public class CountController {
    public void updateCount(HttpServletRequest request) {
        String countParam = request.getParameter("count");
        short count = Short.parseShort(countParam);
        String query = "UPDATE counters SET value = " + count;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CountController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Short.parseShort conversion"
    );
}

#[test]
fn test_byte_parse_byte_sanitizer() {
    let source = r#"
public class FlagController {
    public void updateFlag(HttpServletRequest request) {
        String flagParam = request.getParameter("flag");
        byte flag = Byte.parseByte(flagParam);
        String query = "UPDATE settings SET flag = " + flag;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FlagController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Byte.parseByte conversion"
    );
}

#[test]
fn test_integer_value_of_sanitizer() {
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        Integer id = Integer.valueOf(idParam);
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect SQL injection for Integer.valueOf conversion"
    );
}

#[test]
fn test_numeric_conversion_still_detects_without_conversion() {
    // Without numeric conversion, taint should still flow through
    let source = r#"
public class UserController {
    public void getUser(HttpServletRequest request) {
        String idParam = request.getParameter("id");
        String id = idParam;  // No numeric conversion
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should still detect SQL injection when no numeric conversion"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_numeric_conversion_command_injection() {
    // Numeric conversion should also protect against command injection
    let source = r#"
public class ProcessController {
    public void runProcess(@RequestParam String portParam) {
        int port = Integer.parseInt(portParam);
        String command = "nc localhost " + port;
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProcessController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should not detect command injection for numeric conversion"
    );
}

#[test]
fn test_no_numeric_conversion_command_injection_detected() {
    // Without numeric conversion, command injection should be detected
    let source = r#"
public class ProcessController {
    public void runProcess(@RequestParam String host) {
        String command = "ping " + host;  // No sanitization
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ProcessController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection without sanitization"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_log_injection_http_source_medium_severity() {
    // HTTP request parameters are Untrusted -> MEDIUM severity for log injection
    let source = r#"
public class UserController {
    public void logUser(HttpServletRequest request, Logger log) {
        String username = request.getParameter("username");
        log.info("User logged in: " + username);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(!log_findings.is_empty(), "Should detect log injection");
    assert_eq!(
        log_findings[0].severity,
        Severity::Medium,
        "HTTP source should result in MEDIUM severity log injection"
    );
}

#[test]
fn test_log_injection_kafka_source_low_severity() {
    // Kafka records are SemiTrusted -> LOW severity for log injection
    let source = r#"
public class KafkaConsumer {
    public void processRecord(ConsumerRecord record, Logger log) {
        String key = record.key();
        log.info("Processing record: " + key);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaConsumer.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        !log_findings.is_empty(),
        "Should detect log injection from Kafka source"
    );
    assert_eq!(
        log_findings[0].severity,
        Severity::Low,
        "Kafka source should result in LOW severity log injection"
    );
}

#[test]
fn test_log_injection_database_source_low_severity() {
    // Database records are SemiTrusted -> LOW severity for log injection
    let source = r#"
public class UserRepository {
    public void logUserFromDb(ResultSet rs, Logger log) {
        String name = rs.getString("name");
        log.info("Found user: " + name);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        !log_findings.is_empty(),
        "Should detect log injection from database source"
    );
    assert_eq!(
        log_findings[0].severity,
        Severity::Low,
        "Database source should result in LOW severity log injection"
    );
}

#[test]
fn test_log_injection_env_source_low_severity() {
    // Environment variables are SemiTrusted -> LOW severity
    let source = r#"
public class ConfigLogger {
    public void logConfig(Logger log) {
        String value = System.getenv("CONFIG_VALUE");
        log.info("Config: " + value);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigLogger.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        !log_findings.is_empty(),
        "Should detect log injection from env source"
    );
    assert_eq!(
        log_findings[0].severity,
        Severity::Low,
        "Environment source should result in LOW severity log injection"
    );
}

#[test]
fn test_sql_injection_severity_unchanged() {
    // SQL injection severity should NOT be affected by trust level
    // (SQL injection from any source is critical)
    let source = r#"
public class KafkaProcessor {
    public void processRecord(ConsumerRecord record) {
        String key = record.key();
        String query = "SELECT * FROM users WHERE id = '" + key + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaProcessor.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(!sql_findings.is_empty(), "Should detect SQL injection");
    assert_eq!(
        sql_findings[0].severity,
        Severity::Critical,
        "SQL injection should remain CRITICAL regardless of source"
    );
}

// =============================================================================
// Phase 7: ReDoS (Regular Expression Denial of Service) Tests
// =============================================================================

#[test]
fn test_redos_pattern_compile() {
    // Pattern.compile with user input should be detected
    let source = r#"
public class RegexController {
    public void search(@RequestParam String pattern) {
        Pattern p = Pattern.compile(pattern);
        Matcher m = p.matcher(input);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RegexController.java"));

    eprintln!("Findings: {:?}", findings);

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        !redos_findings.is_empty(),
        "Should detect ReDoS in Pattern.compile"
    );
    assert_eq!(
        redos_findings[0].severity,
        Severity::High,
        "ReDoS should be HIGH severity"
    );
}

#[test]
fn test_redos_string_matches() {
    // String.matches with user input pattern should be detected
    let source = r#"
public class ValidationController {
    public boolean validate(@RequestParam String regex) {
        String input = "test input";
        return input.matches(regex);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ValidationController.java"));

    eprintln!("Findings: {:?}", findings);

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        !redos_findings.is_empty(),
        "Should detect ReDoS in String.matches"
    );
}

#[test]
fn test_redos_string_replace_all() {
    // String.replaceAll with user input pattern should be detected
    let source = r#"
public class SanitizeController {
    public String sanitize(@RequestParam String pattern, String input) {
        return input.replaceAll(pattern, "");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SanitizeController.java"));

    eprintln!("Findings: {:?}", findings);

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        !redos_findings.is_empty(),
        "Should detect ReDoS in String.replaceAll"
    );
}

#[test]
fn test_redos_string_split() {
    // String.split with user input pattern should be detected
    let source = r#"
public class ParseController {
    public String[] parse(@RequestParam String delimiter) {
        String data = "a,b,c";
        return data.split(delimiter);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ParseController.java"));

    eprintln!("Findings: {:?}", findings);

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        !redos_findings.is_empty(),
        "Should detect ReDoS in String.split"
    );
}

#[test]
fn test_redos_pattern_quote_sanitizer() {
    // Pattern.quote should sanitize user input for regex usage
    let source = r#"
public class RegexController {
    public void search(@RequestParam String pattern) {
        String safe = Pattern.quote(pattern);
        Pattern p = Pattern.compile(safe);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RegexController.java"));

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        redos_findings.is_empty(),
        "Should not detect ReDoS for Pattern.quote sanitization"
    );
}

#[test]
fn test_redos_stringutils_split_no_false_positive() {
    // StringUtils.split is literal, not regex-based
    let source = r#"
public class SplitController {
    public void split(@RequestParam String delimiter) {
        String[] parts = StringUtils.split("a,b,c", delimiter);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SplitController.java"));

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        redos_findings.is_empty(),
        "Should not detect ReDoS for StringUtils.split"
    );
}

#[test]
fn test_redos_literal_pattern_no_false_positive() {
    // Literal patterns should NOT be flagged as ReDoS
    let source = r#"
public class SafeRegexController {
    public void process(String input) {
        Pattern p = Pattern.compile("^[a-z]+$");
        if (input.matches("[0-9]+")) {
            String[] parts = input.split(",");
        }
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeRegexController.java"));

    eprintln!("Findings: {:?}", findings);

    let redos_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::ReDoS)
        .collect();

    assert!(
        redos_findings.is_empty(),
        "Should not detect ReDoS for literal patterns"
    );
}

// =============================================================================
// Phase 7: Open Redirect Tests
// =============================================================================

#[test]
fn test_open_redirect_send_redirect() {
    // response.sendRedirect with user input should be detected
    let source = r#"
public class RedirectController {
    public void redirect(HttpServletRequest request, HttpServletResponse response) {
        String url = request.getParameter("url");
        response.sendRedirect(url);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let redirect_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::OpenRedirect)
        .collect();

    assert!(
        !redirect_findings.is_empty(),
        "Should detect OpenRedirect in sendRedirect"
    );
}

#[test]
fn test_open_redirect_redirect_view_constructor() {
    // new RedirectView(userInput) should be detected
    let source = r#"
public class RedirectController {
    public RedirectView redirect(@RequestParam String target) {
        return new RedirectView(target);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let redirect_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::OpenRedirect)
        .collect();

    assert!(
        !redirect_findings.is_empty(),
        "Should detect OpenRedirect in RedirectView constructor"
    );
}

#[test]
fn test_open_redirect_model_and_view_constructor() {
    // new ModelAndView("redirect:" + url) should be detected
    let source = r#"
public class RedirectController {
    public ModelAndView redirect(@RequestParam String url) {
        return new ModelAndView("redirect:" + url);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let redirect_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::OpenRedirect)
        .collect();

    assert!(
        !redirect_findings.is_empty(),
        "Should detect OpenRedirect in ModelAndView constructor"
    );
}

#[test]
fn test_open_redirect_redirect_view_set_url() {
    // redirectView.setUrl(userInput) should be detected
    let source = r#"
public class RedirectController {
    public RedirectView redirect(@RequestParam String target) {
        RedirectView redirectView = new RedirectView();
        redirectView.setUrl(target);
        return redirectView;
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let redirect_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::OpenRedirect)
        .collect();

    assert!(
        !redirect_findings.is_empty(),
        "Should detect OpenRedirect in redirectView.setUrl"
    );
}

#[test]
fn test_open_redirect_safe_relative_url() {
    // Literal/constant URLs should NOT be flagged
    let source = r#"
public class SafeRedirectController {
    public void redirect(HttpServletResponse response) {
        response.sendRedirect("/home");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeRedirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let redirect_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::OpenRedirect)
        .collect();

    assert!(
        redirect_findings.is_empty(),
        "Should not detect OpenRedirect for literal URLs"
    );
}

// =============================================================================
// Phase 8: Cross-Method Field Tracking Tests
// =============================================================================

#[test]
fn test_cross_method_field_sql_injection() {
    // Field tainted in setUser() should be detected as dangerous in query()
    let source = r#"
public class UserController {
    private String userId;

    public void setUser(@RequestParam String id) {
        this.userId = id;
    }

    public void query() {
        String sql = "SELECT * FROM users WHERE id = '" + userId + "'";
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection from cross-method field taint"
    );
}

#[test]
fn test_cross_method_field_command_injection() {
    // Field tainted in setCommand() should be detected as dangerous in run()
    let source = r#"
public class Executor {
    private String command;

    public void setCommand(@RequestParam String cmd) {
        command = cmd;
    }

    public void run() {
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Executor.java"));

    eprintln!("Findings: {:?}", findings);

    let cmd_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::CommandInjection)
        .collect();

    assert!(
        !cmd_findings.is_empty(),
        "Should detect command injection from cross-method field taint"
    );
}

#[test]
fn test_cross_method_field_safe_literal_assignment() {
    // Field assigned from literal should NOT be detected
    let source = r#"
public class Config {
    private String dbName = "mydb";

    public void query() {
        stmt.executeQuery("SELECT * FROM " + dbName);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Config.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        sql_findings.is_empty(),
        "Should not detect SQL injection for literal field assignment"
    );
}

#[test]
fn test_cross_method_field_sanitized_numeric() {
    // Field sanitized via Integer.parseInt should NOT be detected
    let source = r#"
public class Controller {
    private int userId;

    public void setUser(@RequestParam String id) {
        this.userId = Integer.parseInt(id);
    }

    public void query() {
        stmt.executeQuery("SELECT * FROM users WHERE id = " + userId);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Controller.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        sql_findings.is_empty(),
        "Should not detect SQL injection for field sanitized via Integer.parseInt"
    );
}

#[test]
fn test_cross_method_field_this_prefix() {
    // Field with this.field syntax should be tracked
    let source = r#"
public class UserService {
    private String searchTerm;

    public void setSearchTerm(@RequestParam String term) {
        this.searchTerm = term;
    }

    public void search() {
        String sql = "SELECT * FROM products WHERE name LIKE '%" + this.searchTerm + "%'";
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection from field with this.field syntax"
    );
}

#[test]
fn test_cross_method_field_multiple_methods_taint_same_field() {
    // Multiple methods can taint the same field
    let source = r#"
public class MultiTaintController {
    private String query;

    public void setFromParam(@RequestParam String p) {
        this.query = p;
    }

    public void setFromHeader(@RequestHeader String h) {
        this.query = h;
    }

    public void execute() {
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MultiTaintController.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection when multiple methods can taint the same field"
    );
}

#[test]
fn test_cross_method_field_taint_through_intermediate_variable() {
    // Taint flowing through intermediate variable to field
    let source = r#"
public class IndirectController {
    private String data;

    public void setData(@RequestParam String input) {
        String temp = input;
        this.data = temp;
    }

    public void process() {
        Runtime.getRuntime().exec(data);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("IndirectController.java"));

    eprintln!("Findings: {:?}", findings);

    let cmd_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::CommandInjection)
        .collect();

    assert!(
        !cmd_findings.is_empty(),
        "Should detect command injection from field tainted through intermediate variable"
    );
}

#[test]
fn test_cross_method_field_path_traversal() {
    // Field used in path traversal scenario
    let source = r#"
public class FileController {
    private String filename;

    public void setFilename(@RequestParam String name) {
        this.filename = name;
    }

    public void readFile() {
        new File(filename);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileController.java"));

    eprintln!("Findings: {:?}", findings);

    let path_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::PathTraversal)
        .collect();

    assert!(
        !path_findings.is_empty(),
        "Should detect path traversal from cross-method field taint"
    );
}

#[test]
fn test_cross_method_field_without_this_prefix() {
    // Field without this. prefix should also be tracked
    let source = r#"
public class SimpleController {
    private String userId;

    public void setUser(@RequestParam String id) {
        userId = id;
    }

    public void query() {
        String sql = "SELECT * FROM users WHERE id = '" + userId + "'";
        stmt.executeQuery(sql);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SimpleController.java"));

    eprintln!("Findings: {:?}", findings);

    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        !sql_findings.is_empty(),
        "Should detect SQL injection from field without this. prefix"
    );
}

// =============================================================================
// Phase 1.2: Exception Logging Detection Tests
// =============================================================================

#[test]
fn test_exception_logging_direct_exception_object() {
    // Exception object passed directly to logging should NOT be flagged
    let source = r#"
public class ErrorHandler {
    public void handleError(Exception exception, Logger log) {
        log.error("Operation failed", exception);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorHandler.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        log_findings.is_empty(),
        "Should NOT flag exception object passed to logging"
    );
}

#[test]
fn test_exception_logging_getmessage() {
    // e.getMessage() should NOT be flagged as log injection
    let source = r#"
public class ErrorHandler {
    public void handleError(Exception e, Logger log) {
        log.error("Error: " + e.getMessage());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorHandler.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        log_findings.is_empty(),
        "Should NOT flag e.getMessage() in logging"
    );
}

#[test]
fn test_exception_logging_getcause() {
    // ex.getCause() should NOT be flagged as log injection
    let source = r#"
public class ErrorHandler {
    public void handleError(Throwable ex, Logger log) {
        log.warn("Cause: {}", ex.getCause());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorHandler.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        log_findings.is_empty(),
        "Should NOT flag ex.getCause() in logging"
    );
}

#[test]
fn test_exception_logging_short_var_name() {
    // Common short exception variable names like 'e', 't' should be recognized
    let source = r#"
public class ErrorHandler {
    public void handleError(Logger log) {
        try {
            doSomething();
        } catch (Exception e) {
            log.error("Failed", e);
        }
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorHandler.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        log_findings.is_empty(),
        "Should NOT flag common exception variable 'e' in logging"
    );
}

#[test]
fn test_user_input_in_log_still_flagged() {
    // User input in logs should STILL be flagged
    let source = r#"
public class UserController {
    public void logUser(@RequestParam String username, Logger log) {
        log.info("User logged in: " + username);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        !log_findings.is_empty(),
        "Should STILL flag user input in logging"
    );
}

#[test]
fn test_tainted_data_with_exception_still_flagged() {
    // If BOTH user input AND exception are logged, user input should still be flagged
    let source = r#"
public class ErrorController {
    public void handleError(@RequestParam String userId, Exception e, Logger log) {
        log.error("User " + userId + " failed", e);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorController.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        !log_findings.is_empty(),
        "Should flag user input even when exception is also logged"
    );
}

#[test]
fn test_exception_logging_tostring() {
    // exception.toString() should NOT be flagged
    let source = r#"
public class ErrorHandler {
    public void handleError(Exception error, Logger log) {
        log.debug("Error details: {}", error.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ErrorHandler.java"));

    eprintln!("Findings: {:?}", findings);

    let log_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::LogInjection)
        .collect();

    assert!(
        log_findings.is_empty(),
        "Should NOT flag error.toString() in logging"
    );
}

// =============================================================================
// Phase 1.3: SSRF URL/URI Constructor Detection Tests
// =============================================================================

#[test]
fn test_try_with_resources_structure() {
    let source = r#"
public class Test {
    public void test(@RequestParam String url) throws Exception {
        try (InputStream in = new URL(url).openStream()) {
            // body
        }
    }
}
"#;
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    let root = RawSyntaxNode::new_root(green_node);

    eprintln!("Parse errors: {:?}", errors);

    for desc in root.descendants() {
        if desc.kind() == SyntaxKind::TRY_WITH_RESOURCES_STMT {
            eprintln!("\n=== TRY_WITH_RESOURCES_STMT structure ===");
            print_tree(&desc, 0);
        }
    }

    // Also test scanning
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));
    eprintln!("\nFindings: {:?}", findings);
}

#[test]
fn test_ssrf_url_constructor() {
    // Test URL constructor SSRF detection
    let source = r#"
public class Test {
    public void test(@RequestParam String url) throws Exception {
        URL u = new URL(url);
        InputStream in = u.openStream();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SSRF in URL constructor"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Ssrf));
}

#[test]
fn test_ssrf_uri_constructor() {
    // Test URI constructor SSRF detection
    let source = r#"
public class Test {
    public void test(@RequestParam String uri) throws Exception {
        new URI(uri).toURL().openStream();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SSRF in URI constructor"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Ssrf));
}

#[test]
fn test_ssrf_uri_create() {
    // Test URI.create() SSRF detection
    let source = r#"
public class Test {
    public void test(@RequestParam String uri) throws Exception {
        URI.create(uri).toURL().openStream();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(!findings.is_empty(), "Should detect SSRF in URI.create()");
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Ssrf));
}

#[test]
fn test_ssrf_hardcoded_url_safe() {
    // Test that hardcoded URLs are NOT flagged
    let source = r#"
public class Test {
    public void test() throws Exception {
        new URL("https://example.com").openStream();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    let ssrf_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Ssrf)
        .collect();

    assert!(ssrf_findings.is_empty(), "Should NOT flag hardcoded URL");
}

#[test]
fn test_ssrf_constant_url_safe() {
    // Test that constant URLs are NOT flagged
    let source = r#"
public class Test {
    private static final String API_URL = "https://api.internal.com";

    public void test() throws Exception {
        new URL(API_URL).openStream();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    let ssrf_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::Ssrf)
        .collect();

    assert!(ssrf_findings.is_empty(), "Should NOT flag constant URL");
}

#[test]
fn test_ssrf_chained_url_open_connection() {
    // Test chained URL.openConnection()
    let source = r#"
public class Test {
    public void test(@RequestParam String url) throws Exception {
        new URL(url).openConnection();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("Test.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SSRF in new URL().openConnection()"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Ssrf));
}
