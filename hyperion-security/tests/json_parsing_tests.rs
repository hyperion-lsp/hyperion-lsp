// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Integration tests for JSON parsing taint propagation
//!
//! Tests that taint flows correctly through JSON parsing methods like
//! ObjectMapper.readValue() (Jackson) and Gson.fromJson() (Gson).

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

/// Test ObjectMapper.readValue(String, Class) propagates taint
#[test]
fn test_objectmapper_readvalue_propagates_taint() {
    let source = r#"
public class UserController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/user")
    public void handle(@RequestBody String json) {
        User user = mapper.readValue(json, User.class);
        String query = "SELECT * FROM users WHERE id = " + user.getId();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through ObjectMapper.readValue()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test ObjectMapper.readTree(String) propagates taint
#[test]
fn test_objectmapper_readtree_propagates_taint() {
    let source = r#"
public class UserController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/data")
    public void handle(@RequestBody String json) {
        JsonNode tree = mapper.readTree(json);
        String userId = tree.get("userId").asText();
        String query = "SELECT * FROM users WHERE id = '" + userId + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through ObjectMapper.readTree()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test ObjectMapper.convertValue(Object, Class) propagates taint
#[test]
fn test_objectmapper_convertvalue_propagates_taint() {
    let source = r#"
public class UserController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/convert")
    public void handle(@RequestBody Map<String, Object> data) {
        User user = mapper.convertValue(data, User.class);
        String query = "SELECT * FROM users WHERE name = '" + user.getName() + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through ObjectMapper.convertValue()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test Gson.fromJson(String, Class) propagates taint
#[test]
fn test_gson_fromjson_propagates_taint() {
    let source = r#"
public class UserController {
    private Gson gson = new Gson();

    @PostMapping("/user")
    public void handle(@RequestBody String json) {
        User user = gson.fromJson(json, User.class);
        String query = "SELECT * FROM users WHERE id = " + user.getId();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through Gson.fromJson()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test JsonParser.readValueAs(Class) propagates taint
#[test]
fn test_jsonparser_readvalueas_propagates_taint() {
    let source = r#"
public class UserController {
    @PostMapping("/stream")
    public void handle(InputStream input) {
        JsonFactory factory = new JsonFactory();
        JsonParser parser = factory.createParser(input);
        User user = parser.readValueAs(User.class);
        String query = "SELECT * FROM users WHERE id = " + user.getId();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings
            .iter()
            .any(|finding| finding.vuln_type == VulnType::SqlInjection),
        "JsonParser.readValueAs() must propagate untrusted stream data"
    );
}

/// An internal parser over trusted data must not become a request source merely
/// because Jackson uses the same streaming API.
#[test]
fn test_jsonparser_trusted_stream_is_not_tainted() {
    let source = r#"
public class InternalConfigLoader {
    public void load(InputStream input) {
        JsonFactory factory = new JsonFactory();
        JsonParser parser = factory.createParser(input);
        User user = parser.readValueAs(User.class);
        String query = "SELECT * FROM users WHERE id = " + user.getId();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("InternalConfigLoader.java"));

    assert!(
        findings
            .iter()
            .all(|finding| finding.vuln_type != VulnType::SqlInjection),
        "an unannotated internal stream must remain trusted"
    );
}

/// Test chained method calls after readValue
#[test]
fn test_readvalue_chained_methods_propagate_taint() {
    let source = r#"
public class UserController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/user")
    public void handle(@RequestBody String json) {
        User user = mapper.readValue(json, User.class);
        String name = user.getName().toLowerCase().trim();
        String query = "SELECT * FROM users WHERE name = '" + name + "'";
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection through chained methods after readValue()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

/// Test command injection through JSON parsing
#[test]
fn test_json_parsing_command_injection() {
    let source = r#"
public class CommandController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/execute")
    public void handle(@RequestBody String json) {
        CommandRequest cmd = mapper.readValue(json, CommandRequest.class);
        Runtime.getRuntime().exec(cmd.getCommand());
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CommandController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection through ObjectMapper.readValue()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

/// Test XSS through JSON parsing
#[test]
fn test_json_parsing_xss() {
    let source = r#"
public class TemplateController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/render")
    public void handle(@RequestBody String json, HttpServletResponse response) throws IOException {
        UserData data = mapper.readValue(json, UserData.class);
        PrintWriter out = response.getWriter();
        out.write("<h1>" + data.getTitle() + "</h1>");
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("TemplateController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect XSS through ObjectMapper.readValue()"
    );
    assert!(findings.iter().any(|f| f.vuln_type == VulnType::Xss));
}

/// Test safe usage - no taint source
#[test]
fn test_json_parsing_with_safe_data() {
    let source = r#"
public class ConfigController {
    private ObjectMapper mapper = new ObjectMapper();

    public void loadConfig() {
        String safeJson = "{\"timeout\": 30}";
        Config config = mapper.readValue(safeJson, Config.class);
        String query = "SELECT * FROM settings WHERE timeout = " + config.getTimeout();
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ConfigController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        findings.is_empty(),
        "Should NOT detect SQL injection when using safe (non-tainted) JSON data"
    );
}

/// Test sanitization after JSON parsing
#[test]
fn test_json_parsing_with_sanitization() {
    let source = r#"
public class UserController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/user")
    public void handle(@RequestBody String json) {
        User user = mapper.readValue(json, User.class);
        int id = Integer.parseInt(user.getIdString());
        String query = "SELECT * FROM users WHERE id = " + id;
        stmt.executeQuery(query);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserController.java"));

    eprintln!("Findings: {:?}", findings);

    // Filter for SQL injection findings only (ignore rate limiting findings)
    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        sql_findings.is_empty(),
        "Should NOT detect SQL injection - Integer.parseInt sanitizes after JSON parsing"
    );
}

/// Test multiple JSON parsing libraries in same class
#[test]
fn test_multiple_json_libraries() {
    let source = r#"
public class DataController {
    private ObjectMapper jackson = new ObjectMapper();
    private Gson gson = new Gson();

    @PostMapping("/process")
    public void handle(@RequestBody String json) {
        // Jackson
        JacksonUser jUser = jackson.readValue(json, JacksonUser.class);
        String query1 = "SELECT * FROM users WHERE id = " + jUser.getId();
        stmt.executeQuery(query1);

        // Gson
        GsonUser gUser = gson.fromJson(json, GsonUser.class);
        String query2 = "SELECT * FROM users WHERE name = '" + gUser.getName() + "'";
        stmt.executeQuery(query2);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("DataController.java"));

    eprintln!("Findings: {:?}", findings);

    // Filter for SQL injection findings only (ignore rate limiting findings)
    let sql_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::SqlInjection)
        .collect();

    assert!(
        sql_findings.len() >= 2,
        "Should detect SQL injection through both Jackson and Gson"
    );
}

/// Test path traversal through JSON parsing
#[test]
fn test_json_parsing_path_traversal() {
    let source = r#"
public class FileController {
    private ObjectMapper mapper = new ObjectMapper();

    @PostMapping("/file")
    public void handle(@RequestBody String json) {
        FileRequest req = mapper.readValue(json, FileRequest.class);
        File file = new File("/data/" + req.getFilename());
        FileInputStream fis = new FileInputStream(file);
    }
}
"#;

    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("FileController.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal through ObjectMapper.readValue()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}
