// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test cases for external API response taint sources (Phase 1)
//!
//! Tests detection of tainted data from HTTP client responses.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_rest_template_response_to_exec() {
    let source = r#"
class ApiClient {
    public void fetchAndExecute(String apiUrl) {
        String command = restTemplate.getForObject(apiUrl, String.class);
        Runtime.getRuntime().exec(command);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from RestTemplate response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_rest_template_response_to_sql() {
    let source = r#"
class ApiQueryClient {
    public void fetchAndQuery(String apiUrl) {
        String data = restTemplate.getForObject(apiUrl, String.class);
        String query = "SELECT * FROM logs WHERE data = '" + data + "'";
        stmt.executeQuery(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiQueryClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from RestTemplate response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_rest_template_get_for_entity() {
    let source = r#"
class EntityClient {
    public void fetchEntity(String apiUrl) {
        ResponseEntity<String> response = restTemplate.getForEntity(apiUrl, String.class);
        String body = response.getBody();
        Runtime.getRuntime().exec(body);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("EntityClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from ResponseEntity.getBody()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_rest_template_post_for_object() {
    let source = r#"
class PostClient {
    public void postAndExecute(String apiUrl, Object request) {
        String result = restTemplate.postForObject(apiUrl, request, String.class);
        Runtime.getRuntime().exec(result);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("PostClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from postForObject response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_rest_template_exchange() {
    let source = r#"
class ExchangeClient {
    public void exchangeAndQuery(String apiUrl) {
        ResponseEntity<String> response = restTemplate.exchange(
            apiUrl, HttpMethod.GET, null, String.class);
        String data = response.getBody();
        stmt.executeQuery("SELECT * FROM users WHERE name = '" + data + "'");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ExchangeClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from exchange() response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_http_response_body_to_sql() {
    let source = r#"
class ResponseHandler {
    public void processResponse(HttpResponse<String> response) {
        String data = response.body();
        String query = "INSERT INTO logs VALUES ('" + data + "')";
        stmt.executeQuery(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ResponseHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from HttpResponse.body()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_http_client_send() {
    let source = r#"
class ClientSender {
    public void sendAndExecute(HttpClient client, HttpRequest request) {
        HttpResponse<String> response = client.send(request, BodyHandlers.ofString());
        String command = response.body();
        Runtime.getRuntime().exec(command);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ClientSender.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from HttpClient.send()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_apache_http_client_entity_utils() {
    let source = r#"
class ApacheClient {
    public void processHttpResponse(HttpResponse response) {
        HttpEntity entity = response.getEntity();
        String content = EntityUtils.toString(entity);
        stmt.executeQuery("SELECT * FROM data WHERE value = '" + content + "'");
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApacheClient.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from Apache HttpClient response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_http_entity_get_content() {
    let source = r#"
class EntityReader {
    public void readEntity(HttpEntity entity) {
        InputStream content = entity.getContent();
        // Simplified: treating InputStream as tainted
        String data = readInputStream(content);
        Runtime.getRuntime().exec(data);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("EntityReader.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from HttpEntity.getContent()"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_api_response_to_path_traversal() {
    let source = r#"
class ApiFileLoader {
    public void loadFileFromApi(String apiUrl) {
        String filename = restTemplate.getForObject(apiUrl, String.class);
        File file = new File("/data/" + filename);
        FileInputStream fis = new FileInputStream(file);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiFileLoader.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal from API response"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

#[test]
fn test_api_response_data_flow() {
    let source = r#"
class ApiDataFlow {
    public void processApiResponse(String apiUrl) {
        String response = restTemplate.getForObject(apiUrl, String.class);
        String temp1 = response;
        String temp2 = temp1;
        String command = temp2;
        Runtime.getRuntime().exec(command);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("ApiDataFlow.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection with data flow tracking"
    );

    let finding = &findings[0];
    assert_eq!(finding.vuln_type, VulnType::CommandInjection);

    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    assert!(
        !finding.dataflow.is_empty(),
        "Should track data flow from API source to sink"
    );
}

#[test]
fn test_user_controlled_api_url() {
    // This tests the scenario where the API URL itself is user-controlled (SSRF risk)
    // combined with the response being used in a sink
    let source = r#"
class UserUrlClient {
    public void fetchFromUserUrl(@RequestParam String url) {
        String data = restTemplate.getForObject(url, String.class);
        Runtime.getRuntime().exec(data);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserUrlClient.java"));

    eprintln!("Findings: {:#?}", findings);

    // Should detect both SSRF (URL parameter) and command injection (response data)
    assert!(
        !findings.is_empty(),
        "Should detect at least command injection"
    );

    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::CommandInjection),
        "Should detect command injection from API response"
    );
}
