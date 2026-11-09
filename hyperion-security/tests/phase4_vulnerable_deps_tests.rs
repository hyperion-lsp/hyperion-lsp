// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::needless_collect)]

//! Phase 4: Vulnerable Dependencies Detection Tests

use hyperion_security::{Finding, ScanConfig, SecurityScanner, Severity, VulnType};
use std::path::PathBuf;

fn create_scanner() -> SecurityScanner {
    SecurityScanner::new(ScanConfig::default())
}

fn scan_build_file(content: &str) -> Vec<Finding> {
    let scanner = create_scanner();
    let path = PathBuf::from("build.gradle");
    scanner.scan_source(content, &path)
}

#[test]
fn test_log4j_2_14_0_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "org.apache.logging.log4j:log4j-core:2.14.0"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Log4j 2.14.0 vulnerability"
    );

    let log4j_finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(
        log4j_finding.is_some(),
        "Should have vulnerable dependency finding"
    );

    let finding = log4j_finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2021-44228"));
    assert!(finding.message.contains("Log4Shell"));
}

#[test]
fn test_log4j_2_15_0_safe() {
    let build_gradle = r#"
        dependencies {
            implementation "org.apache.logging.log4j:log4j-core:2.15.0"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(findings.is_empty(), "Log4j 2.15.0 should be safe");
}

#[test]
fn test_commons_collections_3_2_1_vulnerable() {
    let build_gradle = r#"
        dependencies {
            compile 'commons-collections:commons-collections:3.2.1'
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Commons Collections vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2015-7501"));
}

#[test]
fn test_commons_collections_4_0_safe() {
    let build_gradle = r#"
        dependencies {
            implementation 'commons-collections:commons-collections:4.0'
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        findings.is_empty(),
        "Commons Collections 4.0 should be safe"
    );
}

#[test]
fn test_jackson_databind_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "com.fasterxml.jackson.core:jackson-databind:2.9.8"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Jackson databind vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2020-25649"));
}

#[test]
fn test_fastjson_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "com.alibaba:fastjson:1.2.68"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect Fastjson vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2020-26258"));
}

#[test]
fn test_spring_framework_spring4shell() {
    let build_gradle = r#"
        dependencies {
            implementation "org.springframework:spring-core:5.3.16"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Spring4Shell vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2022-22965"));
    assert!(finding.message.contains("Spring4Shell"));
}

#[test]
fn test_spring_framework_safe_version() {
    let build_gradle = r#"
        dependencies {
            implementation "org.springframework:spring-core:5.3.18"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(findings.is_empty(), "Spring 5.3.18 should be safe");
}

#[test]
fn test_struts2_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "org.apache.struts:struts2-core:2.5.20"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect Struts2 vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2020-17530"));
}

#[test]
fn test_guava_vulnerable_dos() {
    let build_gradle = r#"
        dependencies {
            implementation "com.google.guava:guava:29.0-jre"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Guava DoS vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Medium);
    assert!(finding.message.contains("CVE-2018-10237"));
}

#[test]
fn test_guava_safe_version() {
    let build_gradle = r#"
        dependencies {
            implementation "com.google.guava:guava:30.0-jre"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(findings.is_empty(), "Guava 30.0 should be safe");
}

#[test]
fn test_multiple_vulnerabilities() {
    let build_gradle = r#"
        dependencies {
            implementation "org.apache.logging.log4j:log4j-core:2.14.0"
            implementation "commons-collections:commons-collections:3.2.1"
            implementation "com.google.guava:guava:25.0-jre"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    let vuln_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::VulnerableDependency)
        .collect();

    assert_eq!(
        vuln_findings.len(),
        3,
        "Should detect all three vulnerabilities"
    );
}

#[test]
fn test_mixed_safe_and_vulnerable() {
    let build_gradle = r#"
        dependencies {
            // Safe
            implementation "org.apache.logging.log4j:log4j-core:2.17.0"
            implementation "com.google.guava:guava:31.0-jre"

            // Vulnerable
            implementation "commons-collections:commons-collections:3.2.1"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    let vuln_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::VulnerableDependency)
        .collect();

    assert_eq!(
        vuln_findings.len(),
        1,
        "Should only detect the vulnerable dependency"
    );
    assert!(vuln_findings[0].message.contains("commons-collections"));
}

#[test]
fn test_version_with_snapshot_qualifier() {
    let build_gradle = r#"
        dependencies {
            implementation "org.apache.logging.log4j:log4j-core:2.14.0-SNAPSHOT"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect vulnerability even with SNAPSHOT qualifier"
    );
}

#[test]
fn test_version_with_jre_qualifier() {
    let build_gradle = r#"
        dependencies {
            implementation "com.google.guava:guava:29.0-jre"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect vulnerability with -jre qualifier"
    );
}

#[test]
fn test_okhttp_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "com.squareup.okhttp3:okhttp:3.12.0"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect OkHttp vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::High);
    assert!(finding.message.contains("CVE-2021-0341"));
}

#[test]
fn test_netty_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "io.netty:netty-codec-http:4.1.50"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect Netty vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::High);
    assert!(finding.message.contains("CVE-2021-21290"));
}

#[test]
fn test_jjwt_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "io.jsonwebtoken:jjwt:0.9.0"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect JJWT vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::High);
    assert!(finding.message.contains("CVE-2019-20444"));
}

#[test]
fn test_different_dependency_declarations() {
    let build_gradle = r#"
        dependencies {
            implementation "commons-collections:commons-collections:3.2.1"
            compile 'commons-collections:commons-collections:3.2.1'
            api "commons-collections:commons-collections:3.2.1"
            runtimeOnly 'commons-collections:commons-collections:3.2.1'
        }
    "#;

    let findings = scan_build_file(build_gradle);
    let vuln_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::VulnerableDependency)
        .collect();

    assert_eq!(
        vuln_findings.len(),
        4,
        "Should detect vulnerability in all declaration types"
    );
}

#[test]
fn test_gradle_kts_syntax() {
    let build_gradle_kts = r#"
        dependencies {
            implementation("org.apache.logging.log4j:log4j-core:2.14.0")
            compile("commons-collections:commons-collections:3.2.1")
        }
    "#;

    let scanner = create_scanner();
    let path = PathBuf::from("build.gradle.kts");
    let findings = scanner.scan_source(build_gradle_kts, &path);

    let vuln_findings: Vec<_> = findings
        .iter()
        .filter(|f| f.vuln_type == VulnType::VulnerableDependency)
        .collect();

    assert_eq!(
        vuln_findings.len(),
        2,
        "Should detect vulnerabilities in Kotlin DSL syntax"
    );
}

#[test]
fn test_log4j_1_x_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "log4j:log4j:1.2.17"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Log4j 1.x vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.message.contains("CVE-2019-17571"));
}

#[test]
fn test_hibernate_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "org.hibernate:hibernate-core:5.4.20"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Hibernate vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::High);
    assert!(finding.message.contains("CVE-2020-25638"));
}

#[test]
fn test_snappy_java_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "org.xerial.snappy:snappy-java:1.1.8.0"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(
        !findings.is_empty(),
        "Should detect Snappy-java vulnerability"
    );

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Medium);
    assert!(finding.message.contains("CVE-2021-29425"));
}

#[test]
fn test_gson_vulnerable() {
    let build_gradle = r#"
        dependencies {
            implementation "com.google.code.gson:gson:2.8.6"
        }
    "#;

    let findings = scan_build_file(build_gradle);
    assert!(!findings.is_empty(), "Should detect Gson vulnerability");

    let finding = findings
        .iter()
        .find(|f| f.vuln_type == VulnType::VulnerableDependency);
    assert!(finding.is_some());

    let finding = finding.unwrap();
    assert_eq!(finding.severity, Severity::Medium);
    assert!(finding.message.contains("CVE-2022-25647"));
}
