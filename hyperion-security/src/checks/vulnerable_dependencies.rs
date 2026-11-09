// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Vulnerable dependency detection
//!
//! Detects known vulnerable dependencies in Gradle build files.
//! Uses hardcoded patterns of known CVEs to identify risky library versions.

use super::{Check, CheckContext};
use crate::finding::{Finding, Severity, VulnType};
use hyperion_build::gradle::GradleResolutionReport;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Known vulnerable dependency patterns
struct VulnerablePattern {
    group_id: &'static str,
    artifact_id: &'static str,
    vulnerable_versions: &'static str,
    cve: &'static str,
    severity: Severity,
    description: &'static str,
}

impl VulnerablePattern {
    const fn new(
        group_id: &'static str,
        artifact_id: &'static str,
        vulnerable_versions: &'static str,
        cve: &'static str,
        severity: Severity,
        description: &'static str,
    ) -> Self {
        Self {
            group_id,
            artifact_id,
            vulnerable_versions,
            cve,
            severity,
            description,
        }
    }

    /// Check if a version matches the vulnerable pattern
    fn matches_version(&self, version: &str) -> bool {
        version_matches_pattern(version, self.vulnerable_versions)
    }
}

/// Database of known vulnerable dependencies
const KNOWN_VULNERABLES: &[VulnerablePattern] = &[
    // Log4j vulnerabilities (CRITICAL)
    VulnerablePattern::new(
        "org.apache.logging.log4j",
        "log4j-core",
        "2.0-2.14.1",
        "CVE-2021-44228",
        Severity::Critical,
        "Log4Shell RCE - Remote code execution via JNDI LDAP lookup in log messages",
    ),
    VulnerablePattern::new(
        "log4j",
        "log4j",
        "1.0-1.2.17",
        "CVE-2019-17571",
        Severity::Critical,
        "Log4j 1.x deserialization RCE via SocketServer",
    ),
    // Commons Collections (CRITICAL)
    VulnerablePattern::new(
        "commons-collections",
        "commons-collections",
        "3.0-3.2.1",
        "CVE-2015-7501",
        Severity::Critical,
        "Apache Commons Collections deserialization RCE",
    ),
    // Jackson databind (CRITICAL)
    VulnerablePattern::new(
        "com.fasterxml.jackson.core",
        "jackson-databind",
        "2.0-2.9.10.3",
        "CVE-2020-25649",
        Severity::Critical,
        "Jackson databind deserialization RCE via polymorphic typing",
    ),
    VulnerablePattern::new(
        "com.fasterxml.jackson.core",
        "jackson-databind",
        "2.10.0-2.10.5.1",
        "CVE-2020-25649",
        Severity::Critical,
        "Jackson databind deserialization RCE via polymorphic typing",
    ),
    // Fastjson (CRITICAL)
    VulnerablePattern::new(
        "com.alibaba",
        "fastjson",
        "1.2.0-1.2.68",
        "CVE-2020-26258",
        Severity::Critical,
        "Fastjson deserialization RCE via autoType bypass",
    ),
    // Struts2 (CRITICAL)
    VulnerablePattern::new(
        "org.apache.struts",
        "struts2-core",
        "2.0-2.5.25",
        "CVE-2020-17530",
        Severity::Critical,
        "Struts2 OGNL injection RCE",
    ),
    // Spring Framework (HIGH)
    VulnerablePattern::new(
        "org.springframework",
        "spring-core",
        "4.0-5.2.19",
        "CVE-2022-22965",
        Severity::Critical,
        "Spring4Shell RCE via class loader manipulation",
    ),
    VulnerablePattern::new(
        "org.springframework",
        "spring-core",
        "5.3.0-5.3.17",
        "CVE-2022-22965",
        Severity::Critical,
        "Spring4Shell RCE via class loader manipulation",
    ),
    VulnerablePattern::new(
        "org.springframework",
        "spring-beans",
        "4.0-5.2.19",
        "CVE-2022-22965",
        Severity::Critical,
        "Spring4Shell RCE via class loader manipulation",
    ),
    VulnerablePattern::new(
        "org.springframework",
        "spring-beans",
        "5.3.0-5.3.17",
        "CVE-2022-22965",
        Severity::Critical,
        "Spring4Shell RCE via class loader manipulation",
    ),
    // Apache HTTP Client (HIGH)
    VulnerablePattern::new(
        "org.apache.httpcomponents",
        "httpclient",
        "4.0-4.5.12",
        "CVE-2020-13956",
        Severity::High,
        "Apache HTTP Client request smuggling vulnerability",
    ),
    // Hibernate (HIGH)
    VulnerablePattern::new(
        "org.hibernate",
        "hibernate-core",
        "5.0-5.4.23",
        "CVE-2020-25638",
        Severity::High,
        "Hibernate SQL injection via named queries",
    ),
    // Guava (MEDIUM - DoS)
    VulnerablePattern::new(
        "com.google.guava",
        "guava",
        "11.0-29.0",
        "CVE-2018-10237",
        Severity::Medium,
        "Guava unbounded memory allocation DoS via AtomicDoubleArray",
    ),
    // Snappy-java (MEDIUM)
    VulnerablePattern::new(
        "org.xerial.snappy",
        "snappy-java",
        "1.0-1.1.8.0",
        "CVE-2021-29425",
        Severity::Medium,
        "Snappy-java arbitrary file write vulnerability",
    ),
    // OkHttp (HIGH - certificate validation)
    VulnerablePattern::new(
        "com.squareup.okhttp3",
        "okhttp",
        "3.0-3.12.12",
        "CVE-2021-0341",
        Severity::High,
        "OkHttp certificate pinning bypass",
    ),
    // Gson (MEDIUM - DoS)
    VulnerablePattern::new(
        "com.google.code.gson",
        "gson",
        "2.0-2.8.8",
        "CVE-2022-25647",
        Severity::Medium,
        "Gson stack overflow DoS via deeply nested objects",
    ),
    // Netty (HIGH)
    VulnerablePattern::new(
        "io.netty",
        "netty-all",
        "4.0-4.1.59",
        "CVE-2021-21290",
        Severity::High,
        "Netty HTTP request smuggling vulnerability",
    ),
    VulnerablePattern::new(
        "io.netty",
        "netty-codec-http",
        "4.0-4.1.59",
        "CVE-2021-21290",
        Severity::High,
        "Netty HTTP request smuggling vulnerability",
    ),
    // JJWT (HIGH - algorithm confusion)
    VulnerablePattern::new(
        "io.jsonwebtoken",
        "jjwt",
        "0.1-0.9.1",
        "CVE-2019-20444",
        Severity::High,
        "JJWT algorithm confusion vulnerability allowing signature bypass",
    ),
    // XStream (CRITICAL - RCE)
    VulnerablePattern::new(
        "com.thoughtworks.xstream",
        "xstream",
        "1.0-1.4.17",
        "CVE-2021-39139",
        Severity::Critical,
        "XStream deserialization RCE via arbitrary code execution",
    ),
    // Apache Commons Text (CRITICAL - RCE)
    VulnerablePattern::new(
        "org.apache.commons",
        "commons-text",
        "1.5-1.9",
        "CVE-2022-42889",
        Severity::Critical,
        "Apache Commons Text Text4Shell RCE via string interpolation",
    ),
];

/// Parse a build file dependency declaration (Gradle or Maven)
#[derive(Debug, Clone, PartialEq)]
struct Dependency {
    group_id: String,
    artifact_id: String,
    version: String,
}

impl Dependency {
    /// Parse from various Gradle formats:
    /// - "group:artifact:version"
    /// - group: "group", name: "artifact", version: "version"
    fn parse_string_notation(text: &str) -> Option<Self> {
        // Remove quotes and whitespace
        let text = text.trim().trim_matches('"').trim_matches('\'');

        // Split by colon: group:artifact:version
        let parts: Vec<&str> = text.split(':').collect();
        if parts.len() == 3 {
            return Some(Self {
                group_id: parts[0].trim().to_string(),
                artifact_id: parts[1].trim().to_string(),
                version: parts[2].trim().to_string(),
            });
        }

        None
    }
}

/// Version matching logic
fn version_matches_pattern(version: &str, pattern: &str) -> bool {
    // Remove version qualifiers like -SNAPSHOT, -beta, etc.
    let version = version.split('-').next().unwrap_or(version);
    let version = version.split('.').collect::<Vec<_>>();

    // Parse pattern: "2.0-2.14.1" or "1.*"
    if pattern.contains('-') {
        // Range: "2.0-2.14.1"
        let parts: Vec<&str> = pattern.split('-').collect();
        if parts.len() == 2 {
            let min = parts[0];
            let max = parts[1];
            return version_gte(&version, min) && version_lte(&version, max);
        }
    } else if pattern.contains('*') {
        // Wildcard: "1.*"
        let pattern_parts: Vec<&str> = pattern.split('.').collect();
        for (i, p) in pattern_parts.iter().enumerate() {
            if *p == "*" {
                return true; // Rest doesn't matter
            }
            if i >= version.len() {
                return false;
            }
            if version[i] != *p {
                return false;
            }
        }
        return true;
    } else {
        // Exact version
        return version.join(".") == pattern;
    }

    false
}

fn version_gte(version: &[&str], min: &str) -> bool {
    let min_parts: Vec<&str> = min.split('.').collect();
    version_compare(version, &min_parts) >= 0
}

fn version_lte(version: &[&str], max: &str) -> bool {
    let max_parts: Vec<&str> = max.split('.').collect();
    version_compare(version, &max_parts) <= 0
}

fn version_compare(v1: &[&str], v2: &[&str]) -> i32 {
    let max_len = v1.len().max(v2.len());
    for i in 0..max_len {
        let p1 = v1.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
        let p2 = v2.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);

        if p1 < p2 {
            return -1;
        } else if p1 > p2 {
            return 1;
        }
    }
    0
}

/// Check for vulnerable dependencies
pub struct VulnerableDependencyCheck;

impl VulnerableDependencyCheck {
    /// Extract dependencies from build.gradle content
    fn extract_gradle_dependencies(content: &str) -> Vec<Dependency> {
        let mut dependencies = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip comments
            if line.starts_with("//") || line.starts_with('#') {
                continue;
            }

            // Look for dependency declarations
            if line.contains("implementation")
                || line.contains("compile")
                || line.contains("api")
                || line.contains("runtimeOnly")
            {
                // Extract the dependency string
                if let Some(start) = line.find('"') {
                    if let Some(end) = line.rfind('"') {
                        if start < end {
                            let dep_str = &line[start..=end];
                            if let Some(dep) = Dependency::parse_string_notation(dep_str) {
                                dependencies.push(dep);
                            }
                        }
                    }
                } else if let Some(start) = line.find('\'') {
                    if let Some(end) = line.rfind('\'') {
                        if start < end {
                            let dep_str = &line[start..=end];
                            if let Some(dep) = Dependency::parse_string_notation(dep_str) {
                                dependencies.push(dep);
                            }
                        }
                    }
                }
            }
        }

        dependencies
    }

    /// Extract dependencies from pom.xml content
    /// Handles both direct version specifications and property references
    fn extract_maven_dependencies(content: &str) -> Vec<Dependency> {
        let mut dependencies = Vec::new();
        let mut properties: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        // First pass: extract properties using simple tag matching
        // Look for <propertyName>value</propertyName> patterns in <properties> section
        if let Some(props_start) = content.find("<properties>") {
            if let Some(props_end) = content[props_start..].find("</properties>") {
                let props_section = &content[props_start + 12..props_start + props_end]; // skip "<properties>"

                // Find property tags: look for <name>value</name> patterns
                let mut remaining = props_section;
                while let Some(tag_start) = remaining.find('<') {
                    // Skip if it's a closing tag or comment
                    if remaining[tag_start..].starts_with("</")
                        || remaining[tag_start..].starts_with("<!")
                    {
                        remaining = &remaining[tag_start + 1..];
                        continue;
                    }

                    // Find end of opening tag
                    if let Some(tag_end) = remaining[tag_start..].find('>') {
                        let tag_name = &remaining[tag_start + 1..tag_start + tag_end];

                        // Skip if contains attributes or is self-closing
                        if tag_name.contains(' ') || tag_name.ends_with('/') {
                            remaining = &remaining[tag_start + tag_end + 1..];
                            continue;
                        }

                        // Find the closing tag
                        let close_tag = format!("</{tag_name}>");
                        let after_open = &remaining[tag_start + tag_end + 1..];
                        if let Some(close_pos) = after_open.find(&close_tag) {
                            let value = after_open[..close_pos].trim();
                            if !value.is_empty() && !value.contains('<') {
                                properties.insert(tag_name.to_string(), value.to_string());
                            }
                            remaining = &after_open[close_pos + close_tag.len()..];
                        } else {
                            remaining = &remaining[tag_start + 1..];
                        }
                    } else {
                        break;
                    }
                }
            }
        }

        // Second pass: extract dependencies
        // Look for <dependency> blocks
        let mut remaining = content;
        while let Some(dep_start) = remaining.find("<dependency>") {
            if let Some(dep_end) = remaining[dep_start..].find("</dependency>") {
                let dep_block = &remaining[dep_start..dep_start + dep_end];

                let group_id = Self::extract_xml_tag(dep_block, "groupId");
                let artifact_id = Self::extract_xml_tag(dep_block, "artifactId");
                let version = Self::extract_xml_tag(dep_block, "version");

                if let (Some(g), Some(a), Some(v)) = (group_id, artifact_id, version) {
                    // Resolve property references like ${version.name}
                    let resolved_version = if v.starts_with("${") && v.ends_with('}') {
                        let prop_name = &v[2..v.len() - 1];
                        properties.get(prop_name).cloned().unwrap_or(v)
                    } else {
                        v
                    };

                    dependencies.push(Dependency {
                        group_id: g,
                        artifact_id: a,
                        version: resolved_version,
                    });
                }

                remaining = &remaining[dep_start + dep_end..];
            } else {
                break;
            }
        }

        dependencies
    }

    /// Extract content of an XML tag
    fn extract_xml_tag(content: &str, tag: &str) -> Option<String> {
        let open_tag = format!("<{tag}>");
        let close_tag = format!("</{tag}>");

        if let Some(start) = content.find(&open_tag) {
            let value_start = start + open_tag.len();
            if let Some(end) = content[value_start..].find(&close_tag) {
                return Some(content[value_start..value_start + end].trim().to_string());
            }
        }
        None
    }

    /// Check a single dependency against known vulnerabilities
    fn check_dependency(
        dep: &Dependency,
        context: &CheckContext,
        dependency_path: Option<&[String]>,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();

        for pattern in KNOWN_VULNERABLES {
            if dep.group_id == pattern.group_id
                && dep.artifact_id == pattern.artifact_id
                && pattern.matches_version(&dep.version)
            {
                let mut message = format!(
                    "Vulnerable dependency detected: {}:{}:{} - {} ({})",
                    dep.group_id, dep.artifact_id, dep.version, pattern.cve, pattern.description
                );
                if let Some(path) = dependency_path.filter(|path| path.len() > 1) {
                    message.push_str(" (transitive dependency via ");
                    message.push_str(&path.join(" -> "));
                    message.push(')');
                }

                // Create a finding at line 1 (build file level)
                let location = crate::finding::Location {
                    file: context.file.to_path_buf(),
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                };

                let code = format!("{}:{}:{}", dep.group_id, dep.artifact_id, dep.version);

                let mut finding =
                    Finding::new(VulnType::VulnerableDependency, message, location, code);
                finding.severity = pattern.severity;
                finding.remediation = format!(
                    "Upgrade {}:{} to a patched version. See {} for details.",
                    dep.group_id, dep.artifact_id, pattern.cve
                );

                findings.push(finding);
            }
        }

        findings
    }
}

impl Check for VulnerableDependencyCheck {
    fn name(&self) -> &'static str {
        "vulnerable-dependency"
    }

    fn target_kinds(&self) -> &[SyntaxKind] {
        // This check doesn't use AST - it reads file content directly
        // Return empty to avoid being called on AST nodes
        &[]
    }

    fn check(&self, _node: &SyntaxNode, _context: &CheckContext) -> Vec<Finding> {
        // Not used - we override in check_file
        vec![]
    }
}

impl VulnerableDependencyCheck {
    /// Check a build file for vulnerable dependencies
    pub fn check_file(&self, context: &CheckContext) -> Vec<Finding> {
        let file_name = context
            .file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Determine file type and extract dependencies accordingly
        let dependencies = if file_name.contains("build.gradle") {
            Self::extract_gradle_dependencies(context.source)
        } else if file_name == "pom.xml" {
            Self::extract_maven_dependencies(context.source)
        } else {
            return vec![];
        };

        let mut findings = Vec::new();

        for dep in dependencies {
            findings.extend(Self::check_dependency(&dep, context, None));
        }

        findings
    }

    /// Check declared and resolved Gradle dependencies as one diagnostic set.
    ///
    /// Source declarations remain the primary input. Exact resolved
    /// coordinates add BOM-selected and transitive dependencies, with the
    /// shortest known path attached to transitive findings. Duplicate CVEs for
    /// a dependency already visible in the source are suppressed by finding
    /// identity.
    pub fn check_file_with_gradle_resolution(
        &self,
        context: &CheckContext,
        report: &GradleResolutionReport,
    ) -> Vec<Finding> {
        let mut findings = self.check_file(context);
        let mut finding_keys: std::collections::HashSet<_> = findings
            .iter()
            .map(|finding| {
                (
                    finding.vuln_type,
                    finding.code_snippet.clone(),
                    finding.remediation.clone(),
                )
            })
            .collect();
        let mut resolved_coordinates = std::collections::HashSet::new();

        for coordinates in report.dependency_edges.iter().map(|edge| edge.to.as_str()) {
            if !resolved_coordinates.insert(coordinates) {
                continue;
            }
            let Some(dependency) = Dependency::parse_string_notation(coordinates) else {
                continue;
            };
            let path = report.explain_path(coordinates);
            for finding in Self::check_dependency(&dependency, context, path.as_deref()) {
                if finding_keys.insert((
                    finding.vuln_type,
                    finding.code_snippet.clone(),
                    finding.remediation.clone(),
                )) {
                    findings.push(finding);
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_build::gradle::DependencyEdge;
    use std::path::Path;

    #[test]
    fn test_parse_gradle_dependency_string_notation() {
        let dep =
            Dependency::parse_string_notation("\"org.apache.logging.log4j:log4j-core:2.14.0\"");
        assert_eq!(
            dep,
            Some(Dependency {
                group_id: "org.apache.logging.log4j".to_string(),
                artifact_id: "log4j-core".to_string(),
                version: "2.14.0".to_string(),
            })
        );
    }

    #[test]
    fn test_version_matches_exact() {
        assert!(version_matches_pattern("2.14.0", "2.14.0"));
        assert!(!version_matches_pattern("2.14.1", "2.14.0"));
    }

    #[test]
    fn test_version_matches_range() {
        assert!(version_matches_pattern("2.10.0", "2.0-2.14.1"));
        assert!(version_matches_pattern("2.14.1", "2.0-2.14.1"));
        assert!(!version_matches_pattern("2.15.0", "2.0-2.14.1"));
        assert!(!version_matches_pattern("1.9.0", "2.0-2.14.1"));
    }

    #[test]
    fn test_version_matches_wildcard() {
        assert!(version_matches_pattern("1.2.17", "1.*"));
        assert!(version_matches_pattern("1.0.0", "1.*"));
        assert!(!version_matches_pattern("2.0.0", "1.*"));
    }

    #[test]
    fn test_extract_gradle_dependencies() {
        let build_gradle = r#"
            dependencies {
                implementation "org.apache.logging.log4j:log4j-core:2.14.0"
                compile 'commons-collections:commons-collections:3.2.1'
                api "com.google.guava:guava:29.0-jre"
            }
        "#;

        let deps = VulnerableDependencyCheck::extract_gradle_dependencies(build_gradle);
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].artifact_id, "log4j-core");
        assert_eq!(deps[1].artifact_id, "commons-collections");
        assert_eq!(deps[2].artifact_id, "guava");
    }

    #[test]
    fn test_version_comparison() {
        assert!(version_gte(&["2", "14", "0"], "2.0"));
        assert!(version_lte(&["2", "14", "0"], "2.14.1"));
        assert!(!version_gte(&["1", "9"], "2.0"));
        assert!(!version_lte(&["2", "15", "0"], "2.14.1"));
    }

    #[test]
    fn test_version_with_qualifiers() {
        // Should strip -SNAPSHOT, -jre, etc.
        assert!(version_matches_pattern("2.14.0-SNAPSHOT", "2.0-2.14.1"));
        assert!(version_matches_pattern("29.0-jre", "11.0-29.0"));
    }

    #[test]
    fn test_extract_maven_dependencies() {
        let pom_xml = r#"
            <project>
                <properties>
                    <jjwt.version>0.9.1</jjwt.version>
                    <guava.version>28.0-jre</guava.version>
                </properties>
                <dependencies>
                    <dependency>
                        <groupId>io.jsonwebtoken</groupId>
                        <artifactId>jjwt</artifactId>
                        <version>${jjwt.version}</version>
                    </dependency>
                    <dependency>
                        <groupId>commons-collections</groupId>
                        <artifactId>commons-collections</artifactId>
                        <version>3.2.1</version>
                    </dependency>
                    <dependency>
                        <groupId>com.google.guava</groupId>
                        <artifactId>guava</artifactId>
                        <version>${guava.version}</version>
                    </dependency>
                </dependencies>
            </project>
        "#;

        let deps = VulnerableDependencyCheck::extract_maven_dependencies(pom_xml);
        assert_eq!(deps.len(), 3);

        // Property-resolved version
        assert_eq!(deps[0].group_id, "io.jsonwebtoken");
        assert_eq!(deps[0].artifact_id, "jjwt");
        assert_eq!(deps[0].version, "0.9.1");

        // Direct version
        assert_eq!(deps[1].group_id, "commons-collections");
        assert_eq!(deps[1].artifact_id, "commons-collections");
        assert_eq!(deps[1].version, "3.2.1");

        // Another property-resolved version
        assert_eq!(deps[2].group_id, "com.google.guava");
        assert_eq!(deps[2].artifact_id, "guava");
        assert_eq!(deps[2].version, "28.0-jre");
    }

    #[test]
    fn test_extract_xml_tag() {
        let content =
            "<dependency><groupId>org.example</groupId><artifactId>test</artifactId></dependency>";

        assert_eq!(
            VulnerableDependencyCheck::extract_xml_tag(content, "groupId"),
            Some("org.example".to_string())
        );
        assert_eq!(
            VulnerableDependencyCheck::extract_xml_tag(content, "artifactId"),
            Some("test".to_string())
        );
        assert_eq!(
            VulnerableDependencyCheck::extract_xml_tag(content, "version"),
            None
        );
    }

    #[test]
    fn resolved_transitive_vulnerability_includes_the_actionable_path() {
        let report = GradleResolutionReport {
            dependency_edges: vec![
                DependencyEdge::direct("org.example:application:1.0"),
                DependencyEdge::transitive(
                    "org.example:application:1.0",
                    "org.example:logging:1.0",
                ),
                DependencyEdge::transitive(
                    "org.example:logging:1.0",
                    "org.apache.logging.log4j:log4j-core:2.14.1",
                ),
            ],
            ..GradleResolutionReport::default()
        };
        let context = CheckContext::new(
            Path::new("build.gradle"),
            "dependencies { implementation 'org.example:application:1.0' }",
        );

        let findings =
            VulnerableDependencyCheck.check_file_with_gradle_resolution(&context, &report);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].code_snippet,
            "org.apache.logging.log4j:log4j-core:2.14.1"
        );
        assert!(findings[0].message.contains(
            "transitive dependency via org.example:application:1.0 -> \
             org.example:logging:1.0 -> org.apache.logging.log4j:log4j-core:2.14.1"
        ));
    }

    #[test]
    fn resolved_direct_dependency_does_not_duplicate_the_source_finding() {
        let coordinates = "org.apache.logging.log4j:log4j-core:2.14.1";
        let report = GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct(coordinates)],
            ..GradleResolutionReport::default()
        };
        let source = format!("dependencies {{ implementation '{coordinates}' }}");
        let context = CheckContext::new(Path::new("build.gradle"), &source);

        let findings =
            VulnerableDependencyCheck.check_file_with_gradle_resolution(&context, &report);

        assert_eq!(findings.len(), 1);
        assert!(!findings[0].message.contains("transitive dependency via"));
    }
}
