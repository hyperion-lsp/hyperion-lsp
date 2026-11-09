// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test cases for message broker taint sources (Phase 1)
//!
//! Tests detection of tainted data from Kafka, RabbitMQ, and JMS message brokers.

use hyperion_security::{ScanConfig, SecurityScanner, VulnType};
use std::path::Path;

#[test]
fn test_kafka_consumer_record_value() {
    let source = r#"
public class KafkaConsumer {
    @KafkaListener(topics = "user-commands")
    public void processMessage(ConsumerRecord<String, String> record) {
        String command = record.value();
        Runtime.getRuntime().exec(command);  // Should detect CommandInjection
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaConsumer.java"));

    eprintln!("Findings: {:#?}", findings);
    eprintln!("Number of findings: {}", findings.len());
    for finding in &findings {
        eprintln!("  Vuln type: {:?}", finding.vuln_type);
        eprintln!("  Message: {}", finding.message);
    }

    assert!(
        !findings.is_empty(),
        "Should detect command injection from Kafka"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_kafka_consumer_record_key() {
    let source = r#"
public class KafkaHandler {
    @KafkaListener(topics = "sql-input")
    public void handle(ConsumerRecord<String, String> record) {
        String key = record.key();
        String query = "SELECT * FROM users WHERE id = '" + key + "'";
        stmt.executeQuery(query);  // Should detect SQLInjection
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from Kafka key"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_kafka_annotation_param() {
    let source = r#"
public class KafkaPayload {
    @KafkaListener(topics = "sql-input")
    public void handle(@Payload String sql) {
        stmt.executeQuery(sql);  // Should detect SQLInjection
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaPayload.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from @Payload annotation"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_rabbitmq_message_body() {
    let source = r#"
public class RabbitConsumer {
    @RabbitListener(queues = "commands")
    public void receive(Message message) {
        String cmd = new String(message.getBody());
        Runtime.getRuntime().exec(cmd);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RabbitConsumer.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from RabbitMQ message body"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_rabbitmq_annotation_param() {
    let source = r#"
public class RabbitHandler {
    @RabbitListener(queues = "sql-queue")
    public void handle(@Payload String query) {
        stmt.executeQuery(query);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("RabbitHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from RabbitMQ @Payload"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_jms_text_message() {
    let source = r#"
public class JmsListener {
    public void onMessage(Message message) {
        if (message instanceof TextMessage) {
            TextMessage textMsg = (TextMessage) message;
            String query = textMsg.getText();
            stmt.executeQuery(query);
        }
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JmsListener.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from JMS TextMessage"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_jms_get_string_property() {
    let source = r#"
public class JmsCommandHandler {
    @JmsListener(destination = "commandQueue")
    public void handleMessage(Message message) {
        String command = message.getStringProperty("command");
        Runtime.getRuntime().exec(command);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JmsCommandHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection from JMS message property"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::CommandInjection));
}

#[test]
fn test_jms_annotation_payload() {
    let source = r#"
public class JmsPayloadHandler {
    @JmsListener(destination = "sqlQueue")
    public void process(@Payload String sql) {
        stmt.executeQuery(sql);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("JmsPayloadHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect SQL injection from @Payload in JMS listener"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::SqlInjection));
}

#[test]
fn test_kafka_to_path_traversal() {
    let source = r#"
public class KafkaFileHandler {
    @KafkaListener(topics = "file-operations")
    public void processFileOp(ConsumerRecord<String, String> record) {
        String filename = record.value();
        File file = new File("/data/" + filename);
        FileInputStream fis = new FileInputStream(file);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaFileHandler.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect path traversal from Kafka message"
    );
    assert!(findings
        .iter()
        .any(|f| f.vuln_type == VulnType::PathTraversal));
}

#[test]
fn test_message_broker_data_flow() {
    let source = r#"
public class KafkaDataFlow {
    @KafkaListener(topics = "commands")
    public void handleKafkaMessage(ConsumerRecord<String, String> record) {
        String input = record.value();
        String temp = input;
        String command = temp;
        Runtime.getRuntime().exec(command);
    }
}
"#;
    let scanner = SecurityScanner::new(ScanConfig::default());
    let findings = scanner.scan_source(source, Path::new("KafkaDataFlow.java"));

    eprintln!("Findings: {:#?}", findings);

    assert!(
        !findings.is_empty(),
        "Should detect command injection with data flow tracking"
    );

    let finding = &findings[0];
    assert_eq!(finding.vuln_type, VulnType::CommandInjection);

    // Verify data flow path is tracked
    eprintln!("Dataflow steps: {}", finding.dataflow.len());
    for (i, step) in finding.dataflow.iter().enumerate() {
        eprintln!("  Step {}: {}", i, step.description);
    }

    assert!(
        !finding.dataflow.is_empty(),
        "Should track data flow from source to sink"
    );
}
