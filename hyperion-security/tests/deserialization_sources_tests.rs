// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for deserialization taint sources (Phase 6)
//!
//! Tests that deserialization methods like ObjectInputStream.readObject()
//! return tainted data that can lead to command injection and other attacks.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

// =============================================================================
// ObjectInputStream.readObject() Tests
// =============================================================================

/// Test ObjectInputStream.readObject() returns tainted value
#[test]
fn test_objectinputstream_readobject_returns_tainted() {
    let source = r#"
public class VulnerableService {
    public void process(ObjectInputStream ois) throws Exception {
        Object obj = ois.readObject();
        String command = obj.toString();
        Runtime.getRuntime().exec(command);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("VulnerableService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through ObjectInputStream.readObject()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test ObjectInputStream.readUnshared() returns tainted value
#[test]
fn test_objectinputstream_readunshared_returns_tainted() {
    let source = r#"
public class VulnerableService {
    public void process(ObjectInputStream ois) throws Exception {
        Object obj = ois.readUnshared();
        String query = "SELECT * FROM users WHERE id = " + obj;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("VulnerableService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through ObjectInputStream.readUnshared()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test command injection through deserialized object method call
#[test]
fn test_deserialized_object_method_to_command_injection() {
    let source = r#"
public class CommandService {
    public void execute(ObjectInputStream ois) throws Exception {
        CommandHolder holder = (CommandHolder) ois.readObject();
        String cmd = holder.getCommand();
        Runtime.getRuntime().exec(cmd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through deserialized object method"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

// =============================================================================
// XStream Tests
// =============================================================================

/// Test XStream.fromXML(String) returns tainted value
#[test]
fn test_xstream_fromxml_string_returns_tainted() {
    let source = r#"
public class XmlService {
    private XStream xstream = new XStream();

    public void process(@RequestBody String xml) {
        Object obj = xstream.fromXML(xml);
        String query = "SELECT * FROM users WHERE id = " + obj.toString();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("XmlService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through XStream.fromXML()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test XStream.fromXML with command injection
#[test]
fn test_xstream_command_injection() {
    let source = r#"
public class XmlCommandService {
    private XStream xstream = new XStream();

    public void process(@RequestBody String xml) {
        CommandConfig config = (CommandConfig) xstream.fromXML(xml);
        Runtime.getRuntime().exec(config.getCommand());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("XmlCommandService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through XStream.fromXML()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

// =============================================================================
// Safe Usage Tests (No False Positives)
// =============================================================================

/// Test safe usage with validation after deserialization
#[test]
fn test_deserialized_with_numeric_validation() {
    let source = r#"
public class SafeService {
    public void process(ObjectInputStream ois) throws Exception {
        Object obj = ois.readObject();
        // Numeric conversion sanitizes the value
        int id = Integer.parseInt(obj.toString());
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection - Integer.parseInt sanitizes the value"
    );
}

/// Test safe deserialization with PreparedStatement
#[test]
fn test_deserialized_with_prepared_statement() {
    let source = r#"
public class SafeDbService {
    public void process(ObjectInputStream ois, Connection conn) throws Exception {
        User user = (User) ois.readObject();
        PreparedStatement stmt = conn.prepareStatement("SELECT * FROM users WHERE name = ?");
        stmt.setString(1, user.getName());
        stmt.executeQuery();
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeDbService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection - PreparedStatement is used properly"
    );
}

// =============================================================================
// Additional Deserialization Sources Tests
// =============================================================================

/// Test Kryo deserialization returns tainted value
#[test]
fn test_kryo_readobject_returns_tainted() {
    let source = r#"
public class KryoService {
    public void process(Kryo kryo, Input input) {
        Object obj = kryo.readObject(input, Command.class);
        Runtime.getRuntime().exec(obj.toString());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KryoService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through Kryo.readObject()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test Hessian deserialization returns tainted value
#[test]
fn test_hessian_readobject_returns_tainted() {
    let source = r#"
public class HessianService {
    public void process(HessianInput hessianInput) throws Exception {
        Object obj = hessianInput.readObject();
        String query = "SELECT * FROM data WHERE key = '" + obj + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("HessianService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through HessianInput.readObject()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test YAML load returns tainted value (SnakeYAML)
#[test]
fn test_yaml_load_returns_tainted() {
    let source = r#"
public class YamlService {
    private Yaml yaml = new Yaml();

    public void process(@RequestBody String yamlContent) {
        Object config = yaml.load(yamlContent);
        String cmd = config.toString();
        Runtime.getRuntime().exec(cmd);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("YamlService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through Yaml.load()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

// =============================================================================
// Path Traversal Through Deserialization Tests
// =============================================================================

/// Test path traversal through deserialized file path
#[test]
fn test_deserialized_path_traversal() {
    let source = r#"
public class FileService {
    public void process(ObjectInputStream ois) throws Exception {
        FileConfig config = (FileConfig) ois.readObject();
        File file = new File("/data/" + config.getFilename());
        FileInputStream fis = new FileInputStream(file);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal through deserialized object"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

// =============================================================================
// XSS Through Deserialization Tests
// =============================================================================

/// Test XSS through deserialized content
#[test]
fn test_deserialized_xss() {
    let source = r#"
public class DisplayService {
    public void display(ObjectInputStream ois, HttpServletResponse response) throws Exception {
        UserData data = (UserData) ois.readObject();
        PrintWriter out = response.getWriter();
        out.write("<h1>" + data.getTitle() + "</h1>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DisplayService.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect XSS through deserialized object"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Xss));
}

// =============================================================================
// Method Summary Tests (Unit-level)
// =============================================================================

#[test]
fn test_objectinputstream_readobject_summary() {
    use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};

    let registry = MethodSummaryRegistry::new();
    let summary = registry.get_summary(Some("ObjectInputStream"), "readObject");

    assert!(
        summary.is_some(),
        "Should have summary for ObjectInputStream.readObject"
    );
    assert_eq!(
        summary.unwrap().taint_behavior,
        TaintBehavior::AlwaysTainted,
        "readObject should always return tainted data"
    );
}

#[test]
fn test_objectinputstream_readunshared_summary() {
    use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};

    let registry = MethodSummaryRegistry::new();
    let summary = registry.get_summary(Some("ObjectInputStream"), "readUnshared");

    assert!(
        summary.is_some(),
        "Should have summary for ObjectInputStream.readUnshared"
    );
    assert_eq!(
        summary.unwrap().taint_behavior,
        TaintBehavior::AlwaysTainted,
        "readUnshared should always return tainted data"
    );
}

#[test]
fn test_xstream_fromxml_summary() {
    use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};

    let registry = MethodSummaryRegistry::new();
    let summary = registry.get_summary(Some("XStream"), "fromXML");

    assert!(summary.is_some(), "Should have summary for XStream.fromXML");
    assert_eq!(
        summary.unwrap().taint_behavior,
        TaintBehavior::PropagatesArg { arg_index: 0 },
        "fromXML should propagate taint from input argument"
    );
}

#[test]
fn test_kryo_readobject_summary() {
    use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};

    let registry = MethodSummaryRegistry::new();
    let summary = registry.get_summary(Some("Kryo"), "readObject");

    assert!(summary.is_some(), "Should have summary for Kryo.readObject");
    assert_eq!(
        summary.unwrap().taint_behavior,
        TaintBehavior::AlwaysTainted,
        "Kryo.readObject should always return tainted data"
    );
}

#[test]
fn test_hessian_readobject_summary() {
    use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};

    let registry = MethodSummaryRegistry::new();
    let summary = registry.get_summary(Some("HessianInput"), "readObject");

    assert!(
        summary.is_some(),
        "Should have summary for HessianInput.readObject"
    );
    assert_eq!(
        summary.unwrap().taint_behavior,
        TaintBehavior::AlwaysTainted,
        "HessianInput.readObject should always return tainted data"
    );
}
