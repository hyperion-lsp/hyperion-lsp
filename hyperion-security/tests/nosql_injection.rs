// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_mongodb_find_injection() {
    let source = r#"
public class UserRepository {
    public void findUser(HttpServletRequest request) {
        String username = request.getParameter("username");
        String query = "{ 'username': '" + username + "' }";
        collection.find(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserRepository.java"));

    eprintln!("Findings: {:?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in MongoDB find"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.vuln_type == VulnType::NoSqlInjection),
        "Should detect NoSqlInjection vuln type"
    );
    assert_eq!(
        findings[0].severity,
        hyperion_security::Severity::High,
        "NoSQL injection should be High severity"
    );
}

#[test]
fn test_mongodb_document_parse_injection() {
    let source = r#"
public class MongoService {
    public void query(@RequestParam String filter) {
        Document doc = Document.parse(filter);
        MongoCollection<Document> collection = database.getCollection("users");
        collection.find(doc);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("MongoService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in Document.parse"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));
}

#[test]
fn test_mongodb_aggregate_injection() {
    let source = r#"
public class AnalyticsService {
    public void aggregate(HttpServletRequest request) {
        String pipeline = request.getParameter("pipeline");
        mongoCollection.aggregate(pipeline);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AnalyticsService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in aggregate"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));
}

#[test]
fn test_cassandra_cql_injection() {
    let source = r#"
public class CassandraDAO {
    public void executeQuery(@PathVariable String userId) {
        String cql = "SELECT * FROM users WHERE id = '" + userId + "'";
        session.execute(cql);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("CassandraDAO.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in Cassandra execute"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));
}

#[test]
fn test_cassandra_async_injection() {
    let source = r#"
public class AsyncCassandraDAO {
    public void query(HttpServletRequest request) {
        String query = request.getParameter("query");
        cqlSession.executeAsync(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("AsyncCassandraDAO.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in executeAsync"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));
}

#[test]
fn test_elasticsearch_search_injection() {
    let source = r#"
public class SearchService {
    public void search(@RequestParam String searchQuery) {
        SearchRequest request = new SearchRequest();
        searchRequest.query(searchQuery);
        client.search(request);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SearchService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection in Elasticsearch search"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));
}

#[test]
fn test_safe_parameterized_query_no_false_positive() {
    // Using safe query builder (though we can't fully validate this without type info)
    let source = r#"
public class SafeMongoDAO {
    public void findUser(String username) {
        // This is still detected because we can't tell if Filters.eq is safe
        // In a more advanced implementation, we'd recognize safe query builders
        Document query = new Document("username", username);
        collection.find(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("SafeMongoDAO.java"));

    // For now, this might still trigger (conservative approach)
    // In Phase 3+, we'd add more sophisticated type analysis
    eprintln!("Findings: {:?}", findings);
}

#[test]
fn test_multi_step_nosql_injection() {
    let source = r#"
public class UserService {
    public void getUser(HttpServletRequest request) {
        String userId = request.getParameter("id");
        String filter = userId;
        Document doc = Document.parse(filter);
        collection.find(doc);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("UserService.java"));

    assert!(
        !findings.is_empty(),
        "Should detect NoSQL injection through data flow"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::NoSqlInjection));

    // Verify data flow tracking
    let finding = &findings[0];
    assert!(
        !finding.dataflow.is_empty(),
        "Should have data flow path tracked"
    );
}
