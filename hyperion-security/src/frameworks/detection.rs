// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Automatic framework detection for security scanning
//!
//! This module implements framework detection with confidence scoring:
//!
//! 1. dependency analysis (`pom.xml`, `build.gradle`) — confidence 1.0;
//! 2. import statement scanning — confidence 0.7; and
//! 3. annotation usage detection — confidence 0.4.
//!
//! Frameworks with cumulative confidence >= 0.5 are auto-enabled.

use crate::frameworks::Framework;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Framework detection is best-effort and should never consume unbounded memory on a generated
/// or otherwise unexpected source file.
const MAX_SOURCE_FILE_BYTES: usize = 4 * 1024 * 1024;

/// Avoid retaining arbitrarily many paths before the parallel scan starts. A project exceeding
/// this limit still gets useful detection results from the first files in traversal order.
const MAX_SOURCE_FILES: usize = 100_000;

#[derive(Default)]
struct SourceEvidence {
    imports: Vec<String>,
    annotations: Vec<String>,
}

struct FrameworkPatterns {
    id: &'static str,
    imports: Vec<&'static str>,
    annotations: HashSet<&'static str>,
}

/// Confidence score for framework detection
///
/// Higher scores indicate more definitive evidence that a framework is in use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confidence(f64);

impl Confidence {
    /// Strong evidence from build file dependencies
    pub const DEPENDENCY: f64 = 1.0;

    /// Moderate evidence from import statements
    pub const IMPORT: f64 = 0.7;

    /// Weak evidence from annotation usage
    pub const ANNOTATION: f64 = 0.4;

    /// Threshold for auto-enabling a framework
    pub const THRESHOLD: f64 = 0.5;

    pub const fn new(score: f64) -> Self {
        Self(score.clamp(0.0, 1.0))
    }

    pub const fn value(&self) -> f64 {
        self.0
    }

    pub fn exceeds_threshold(&self) -> bool {
        self.0 >= Self::THRESHOLD
    }

    /// Combine multiple confidence scores (max, not sum)
    pub const fn combine(&self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self(0.0)
    }
}

/// Result of framework detection
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Framework ID
    pub framework_id: String,

    /// Framework name
    pub framework_name: String,

    /// Strongest confidence score among the collected evidence
    pub confidence: Confidence,

    /// Evidence found for each stage
    pub evidence: DetectionEvidence,
}

/// Evidence collected during framework detection
#[derive(Debug, Clone, Default)]
pub struct DetectionEvidence {
    /// Dependencies found in build files
    pub dependencies: Vec<String>,

    /// Import statements found in source files
    pub imports: Vec<String>,

    /// Annotations found in source files
    pub annotations: Vec<String>,
}

impl DetectionEvidence {
    const fn is_empty(&self) -> bool {
        self.dependencies.is_empty() && self.imports.is_empty() && self.annotations.is_empty()
    }
}

/// Framework detector
///
/// Scans a project directory to automatically detect frameworks.
pub struct FrameworkDetector {
    /// Project root directory
    project_root: PathBuf,
}

impl FrameworkDetector {
    /// Create a new framework detector
    ///
    /// # Arguments
    /// * `project_root` - Root directory of the project to scan
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    /// Detect frameworks in the project
    ///
    /// # Arguments
    /// * `frameworks` - Candidate frameworks to check for
    ///
    /// # Returns
    /// Detection results for each framework, sorted by confidence (highest first)
    pub fn detect(&self, frameworks: &[&dyn Framework]) -> Vec<DetectionResult> {
        info!(
            "Starting framework detection in {}",
            self.project_root.display()
        );

        let mut results: HashMap<String, DetectionResult> = HashMap::new();

        // Initialize results for all frameworks
        for framework in frameworks {
            results.insert(
                framework.id().to_string(),
                DetectionResult {
                    framework_id: framework.id().to_string(),
                    framework_name: framework.name().to_string(),
                    confidence: Confidence::default(),
                    evidence: DetectionEvidence::default(),
                },
            );
        }

        // Stage 1: Parse build files for dependencies
        debug!("Stage 1: Analyzing build files");
        self.detect_dependencies(frameworks, &mut results);

        // Stages 2 and 3 use the same source files. Scan each file once and extract both forms of
        // evidence while its contents are in memory.
        debug!("Stages 2 and 3: Scanning imports and annotation usage");
        self.detect_source_evidence(frameworks, &mut results);

        // Convert to sorted vector
        let mut results: Vec<DetectionResult> = results.into_values().collect();
        results.sort_by(|a, b| {
            b.confidence
                .value()
                .partial_cmp(&a.confidence.value())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Log detection summary
        for result in &results {
            if !result.evidence.is_empty() {
                info!(
                    "Detected {} (confidence: {:.2})",
                    result.framework_name,
                    result.confidence.value()
                );
                debug!("  Evidence: {:?}", result.evidence);
            }
        }

        results
    }

    /// Stage 1: Detect frameworks from build file dependencies
    fn detect_dependencies(
        &self,
        frameworks: &[&dyn Framework],
        results: &mut HashMap<String, DetectionResult>,
    ) {
        // Find build files
        let pom_xml = self.project_root.join("pom.xml");
        let build_gradle = self.project_root.join("build.gradle");
        let build_gradle_kts = self.project_root.join("build.gradle.kts");

        // Parse Maven pom.xml
        if pom_xml.exists() {
            debug!("Found pom.xml");
            if let Ok(content) = Self::read_source_file(&pom_xml) {
                Self::detect_maven_dependencies(&content, frameworks, results);
            } else {
                warn!("Failed to read pom.xml");
            }
        }

        // Parse Gradle build.gradle
        if build_gradle.exists() {
            debug!("Found build.gradle");
            if let Ok(content) = Self::read_source_file(&build_gradle) {
                Self::detect_gradle_dependencies(&content, frameworks, results);
            } else {
                warn!("Failed to read build.gradle");
            }
        }

        // Parse Gradle build.gradle.kts
        if build_gradle_kts.exists() {
            debug!("Found build.gradle.kts");
            if let Ok(content) = Self::read_source_file(&build_gradle_kts) {
                Self::detect_gradle_dependencies(&content, frameworks, results);
            } else {
                warn!("Failed to read build.gradle.kts");
            }
        }
    }

    /// Detect Maven dependencies from pom.xml content
    fn detect_maven_dependencies(
        content: &str,
        frameworks: &[&dyn Framework],
        results: &mut HashMap<String, DetectionResult>,
    ) {
        for framework in frameworks {
            for pattern in framework.dependency_patterns() {
                // Pattern format: "groupId:artifactId"
                let parts: Vec<&str> = pattern.split(':').collect();
                if parts.len() != 2 {
                    continue;
                }

                let group_id = parts[0];
                let artifact_id = parts[1];

                // Simple pattern matching: look for <groupId>xxx</groupId> followed by <artifactId>yyy</artifactId>
                // This is crude but sufficient for detection purposes
                if content.contains(&format!("<groupId>{group_id}</groupId>"))
                    && content.contains(&format!("<artifactId>{artifact_id}</artifactId>"))
                {
                    debug!("Found Maven dependency {} in pom.xml", pattern);

                    if let Some(result) = results.get_mut(framework.id()) {
                        result.evidence.dependencies.push(pattern.to_string());
                        result.confidence = result
                            .confidence
                            .combine(Confidence::new(Confidence::DEPENDENCY));
                    }
                }
            }
        }
    }

    /// Detect Gradle dependencies from build.gradle or build.gradle.kts content
    fn detect_gradle_dependencies(
        content: &str,
        frameworks: &[&dyn Framework],
        results: &mut HashMap<String, DetectionResult>,
    ) {
        for framework in frameworks {
            for pattern in framework.dependency_patterns() {
                // Pattern format: "groupId:artifactId"
                // Look for patterns like:
                // - implementation 'org.springframework:spring-web'
                // - implementation "org.springframework:spring-web"
                // - implementation("org.springframework:spring-web")
                // - implementation 'org.springframework:spring-web:5.3.0'
                // - compile 'org.springframework:spring-web'
                //
                // We need to match the pattern even if there's a version suffix
                // So we'll look for the pattern followed by either a quote or a colon (version separator)
                let pattern_variations = [
                    format!("'{pattern}'"),     // 'org.springframework:spring-web'
                    format!("\"{pattern}\""),   // "org.springframework:spring-web"
                    format!("'{pattern}:"),     // 'org.springframework:spring-web:
                    format!("\"{pattern}:"),    // "org.springframework:spring-web:
                    format!("(\"{pattern}\")"), // ("org.springframework:spring-web")
                    format!("('{pattern}')"),   // ('org.springframework:spring-web')
                    format!("(\"{pattern}:"),   // ("org.springframework:spring-web:
                    format!("('{pattern}:"),    // ('org.springframework:spring-web:
                ];

                let found = pattern_variations.iter().any(|p| content.contains(p));

                if found {
                    debug!("Found Gradle dependency {} in build.gradle", pattern);

                    if let Some(result) = results.get_mut(framework.id()) {
                        result.evidence.dependencies.push(pattern.to_string());
                        result.confidence = result
                            .confidence
                            .combine(Confidence::new(Confidence::DEPENDENCY));
                    }
                }
            }
        }
    }

    /// Stages 2 and 3: detect frameworks from imports and annotations in Java files.
    fn detect_source_evidence(
        &self,
        frameworks: &[&dyn Framework],
        results: &mut HashMap<String, DetectionResult>,
    ) {
        let java_files = self.java_source_files();
        debug!(
            "Scanning {} Java files for imports and annotations",
            java_files.len()
        );

        if java_files.is_empty() {
            debug!("No Java files found in {}", self.project_root.display());
        }

        let findings: Vec<SourceEvidence> = java_files
            .par_iter()
            .filter_map(|path| match Self::read_source_file(path) {
                Ok(content) => Some(SourceEvidence {
                    imports: Self::extract_imports(&content)
                        .into_iter()
                        .map(|(package, name)| format!("{package}.{name}"))
                        .collect(),
                    annotations: Self::extract_annotations(&content),
                }),
                Err(error) => {
                    debug!("Skipping source file {}: {}", path.display(), error);
                    None
                }
            })
            .collect();

        // Framework implementations return owned vectors, so cache them once instead of
        // rebuilding the same pattern lists for every source file and every finding.
        let patterns: Vec<FrameworkPatterns> = frameworks
            .iter()
            .map(|framework| FrameworkPatterns {
                id: framework.id(),
                imports: framework.import_patterns(),
                annotations: framework.signature_annotations().into_iter().collect(),
            })
            .collect();

        let mut matched_imports: HashMap<String, HashSet<String>> = HashMap::new();
        let mut matched_annotations: HashMap<String, HashSet<String>> = HashMap::new();

        for finding in findings {
            for framework in &patterns {
                if finding.imports.is_empty() && finding.annotations.is_empty() {
                    continue;
                }

                for import in &finding.imports {
                    if framework
                        .imports
                        .iter()
                        .any(|pattern| Self::import_matches_pattern(import, pattern))
                    {
                        matched_imports
                            .entry(framework.id.to_string())
                            .or_default()
                            .insert(import.clone());
                    }
                }

                for annotation in &finding.annotations {
                    if framework.annotations.contains(annotation.as_str()) {
                        matched_annotations
                            .entry(framework.id.to_string())
                            .or_default()
                            .insert(annotation.clone());
                    }
                }
            }
        }

        for (framework_id, imports) in matched_imports {
            if let Some(result) = results.get_mut(&framework_id) {
                result.evidence.imports = imports.into_iter().collect();
                result.evidence.imports.sort_unstable();
                result.confidence = result
                    .confidence
                    .combine(Confidence::new(Confidence::IMPORT));
            }
        }

        for (framework_id, annotations) in matched_annotations {
            if let Some(result) = results.get_mut(&framework_id) {
                result.evidence.annotations = annotations.into_iter().collect();
                result.evidence.annotations.sort_unstable();
                result.confidence = result
                    .confidence
                    .combine(Confidence::new(Confidence::ANNOTATION));
            }
        }
    }

    fn java_source_files(&self) -> Vec<PathBuf> {
        let mut java_files: Vec<PathBuf> = WalkDir::new(&self.project_root)
            .into_iter()
            .filter_entry(|e| {
                let path = e.path();
                if path == self.project_root.as_path() {
                    return true;
                }

                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if file_name.starts_with('.') {
                    return false;
                }

                !matches!(
                    file_name,
                    "target" | "build" | "out" | "node_modules" | ".idea"
                )
            })
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("java"))
            .map(|e| e.path().to_path_buf())
            .take(MAX_SOURCE_FILES + 1)
            .collect();

        if java_files.len() > MAX_SOURCE_FILES {
            java_files.truncate(MAX_SOURCE_FILES);
            warn!("Framework detection limited to the first {MAX_SOURCE_FILES} Java source files");
        }

        java_files
    }

    fn read_source_file(path: &Path) -> io::Result<String> {
        let file = File::open(path)?;
        let capacity = file
            .metadata()
            .ok()
            .and_then(|metadata| usize::try_from(metadata.len()).ok())
            .unwrap_or_default()
            .min(MAX_SOURCE_FILE_BYTES);
        let mut bytes = Vec::with_capacity(capacity);
        file.take(MAX_SOURCE_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_SOURCE_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file exceeds {MAX_SOURCE_FILE_BYTES} bytes"),
            ));
        }
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn import_matches_pattern(import: &str, pattern: &str) -> bool {
        import == pattern
            || import
                .strip_prefix(pattern)
                .is_some_and(|remainder| remainder.starts_with('.'))
    }

    /// Extract import statements from Java source code
    ///
    /// Returns list of (import_package, import_name) tuples
    fn extract_imports(content: &str) -> Vec<(String, String)> {
        let mut imports = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Simple pattern: "import package.name.ClassName;"
            if trimmed.starts_with("import ") && trimmed.ends_with(';') {
                let import_stmt = trimmed
                    .strip_prefix("import ")
                    .and_then(|s| s.strip_suffix(';'))
                    .map_or("", str::trim);

                // Remove "static" if present
                let import_stmt = import_stmt.strip_prefix("static ").unwrap_or(import_stmt);

                // Extract package (everything before the last dot)
                if let Some(last_dot) = import_stmt.rfind('.') {
                    let package = import_stmt[..last_dot].to_string();
                    let name = import_stmt[last_dot + 1..].to_string();
                    imports.push((package, name));
                }
            }
        }

        imports
    }

    /// Extract annotations from Java source code
    ///
    /// Returns simple annotation names (without @)
    fn extract_annotations(content: &str) -> Vec<String> {
        let mut annotations = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Simple pattern: "@AnnotationName" or "@AnnotationName(...)"
            if let Some(stripped) = trimmed.strip_prefix('@') {
                // Extract annotation name (up to '(' or whitespace)
                let name = stripped
                    .split(|c: char| c == '(' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !name.is_empty() {
                    annotations.push(name);
                }
            }
        }

        annotations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // Helper to create a mock framework for testing
    struct MockFramework {
        id: &'static str,
        name: &'static str,
        deps: Vec<&'static str>,
        imports: Vec<&'static str>,
        annotations: Vec<&'static str>,
    }

    impl Framework for MockFramework {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn dependency_patterns(&self) -> Vec<&'static str> {
            self.deps.clone()
        }

        fn import_patterns(&self) -> Vec<&'static str> {
            self.imports.clone()
        }

        fn signature_annotations(&self) -> Vec<&'static str> {
            self.annotations.clone()
        }

        fn register_sources(&self, _: &mut crate::sources::SourceRegistry) {}
        fn register_sinks(&self, _: &mut crate::sinks::SinkRegistry) {}
        fn register_sanitizers(&self, _: &mut crate::sanitizers::SanitizerRegistry) {}
        fn is_source_annotation(&self, _: &str) -> bool {
            false
        }
    }

    #[test]
    fn test_confidence_threshold() {
        assert!(Confidence::new(1.0).exceeds_threshold());
        assert!(Confidence::new(0.7).exceeds_threshold());
        assert!(Confidence::new(0.5).exceeds_threshold());
        assert!(!Confidence::new(0.4).exceeds_threshold());
        assert!(!Confidence::new(0.0).exceeds_threshold());
    }

    #[test]
    fn test_confidence_combine() {
        let c1 = Confidence::new(0.4);
        let c2 = Confidence::new(0.7);
        assert_eq!(c1.combine(c2).value(), 0.7);
        assert_eq!(c2.combine(c1).value(), 0.7);
    }

    #[test]
    fn test_maven_dependency_detection() {
        let temp_dir = TempDir::new().unwrap();
        let pom_path = temp_dir.path().join("pom.xml");

        let pom_content = r#"
            <project>
                <dependencies>
                    <dependency>
                        <groupId>org.springframework</groupId>
                        <artifactId>spring-web</artifactId>
                        <version>5.3.0</version>
                    </dependency>
                </dependencies>
            </project>
        "#;

        fs::write(&pom_path, pom_content).unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec!["org.springframework:spring-web"],
            imports: vec![],
            annotations: vec![],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].framework_id, "spring");
        assert_eq!(results[0].confidence.value(), Confidence::DEPENDENCY);
        assert_eq!(results[0].evidence.dependencies.len(), 1);
    }

    #[test]
    fn test_gradle_dependency_detection() {
        let temp_dir = TempDir::new().unwrap();
        let gradle_path = temp_dir.path().join("build.gradle");

        let gradle_content = r#"
            dependencies {
                implementation 'org.springframework:spring-web:5.3.0'
                testImplementation "junit:junit:4.13"
            }
        "#;

        fs::write(&gradle_path, gradle_content).unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec!["org.springframework:spring-web"],
            imports: vec![],
            annotations: vec![],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].framework_id, "spring");
        assert_eq!(results[0].confidence.value(), Confidence::DEPENDENCY);
    }

    #[test]
    fn test_import_detection() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src/main/java");
        fs::create_dir_all(&src_dir).unwrap();

        let java_file = src_dir.join("Test.java");
        let java_content = r#"
            package com.example;

            import org.springframework.web.bind.annotation.RestController;
            import org.springframework.web.bind.annotation.GetMapping;

            @RestController
            public class Test {
                @GetMapping("/test")
                public String test() {
                    return "test";
                }
            }
        "#;

        fs::write(&java_file, java_content).unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec![],
            imports: vec!["org.springframework.web"],
            annotations: vec![],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].framework_id, "spring");
        assert_eq!(results[0].confidence.value(), Confidence::IMPORT);
        assert!(!results[0].evidence.imports.is_empty());
    }

    #[test]
    fn test_annotation_detection() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src/main/java");
        fs::create_dir_all(&src_dir).unwrap();

        let java_file = src_dir.join("Controller.java");
        let java_content = r#"
            package com.example;

            @RestController
            @RequestMapping("/api")
            public class Controller {
                @GetMapping("/test")
                public String test() {
                    return "test";
                }
            }
        "#;

        fs::write(&java_file, java_content).unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec![],
            imports: vec![],
            annotations: vec!["RestController", "RequestMapping", "GetMapping"],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].framework_id, "spring");
        assert_eq!(results[0].confidence.value(), Confidence::ANNOTATION);
        assert_eq!(results[0].evidence.annotations.len(), 3);
    }

    #[test]
    fn test_combined_detection() {
        let temp_dir = TempDir::new().unwrap();

        // Create pom.xml with Spring dependency
        let pom_content = r#"
            <project>
                <dependencies>
                    <dependency>
                        <groupId>org.springframework</groupId>
                        <artifactId>spring-web</artifactId>
                    </dependency>
                </dependencies>
            </project>
        "#;
        fs::write(temp_dir.path().join("pom.xml"), pom_content).unwrap();

        // Create Java file with Spring imports and annotations
        let src_dir = temp_dir.path().join("src/main/java");
        fs::create_dir_all(&src_dir).unwrap();

        let java_content = r#"
            import org.springframework.web.bind.annotation.RestController;

            @RestController
            public class Controller {}
        "#;
        fs::write(src_dir.join("Controller.java"), java_content).unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec!["org.springframework:spring-web"],
            imports: vec!["org.springframework.web"],
            annotations: vec!["RestController"],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        // Should use highest confidence (dependency = 1.0)
        assert_eq!(results[0].confidence.value(), Confidence::DEPENDENCY);
        assert!(!results[0].evidence.dependencies.is_empty());
        assert!(!results[0].evidence.imports.is_empty());
        assert!(!results[0].evidence.annotations.is_empty());
    }

    #[test]
    fn test_extract_imports() {
        let content = r#"
            package com.example;

            import java.util.List;
            import static java.util.Collections.emptyList;
            import org.springframework.web.bind.annotation.RestController;

            public class Test {}
        "#;

        let imports = FrameworkDetector::extract_imports(content);

        assert_eq!(imports.len(), 3);
        assert!(imports.contains(&("java.util".to_string(), "List".to_string())));
        // Static import: package includes the class name, name is the method
        assert!(imports.contains(&("java.util.Collections".to_string(), "emptyList".to_string())));
        assert!(imports.contains(&(
            "org.springframework.web.bind.annotation".to_string(),
            "RestController".to_string()
        )));
    }

    #[test]
    fn test_extract_annotations() {
        let content = r#"
            @RestController
            @RequestMapping("/api")
            public class Test {
                @GetMapping("/test")
                public String test() {}

                @PostMapping(value = "/create", consumes = "application/json")
                public void create() {}
            }
        "#;

        let annotations = FrameworkDetector::extract_annotations(content);

        assert!(annotations.contains(&"RestController".to_string()));
        assert!(annotations.contains(&"RequestMapping".to_string()));
        assert!(annotations.contains(&"GetMapping".to_string()));
        assert!(annotations.contains(&"PostMapping".to_string()));
    }

    #[test]
    fn test_no_false_positives() {
        let temp_dir = TempDir::new().unwrap();

        // Create empty Java file
        let src_dir = temp_dir.path().join("src/main/java");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("Empty.java"), "public class Empty {}").unwrap();

        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec!["org.springframework:spring-web"],
            imports: vec!["org.springframework"],
            annotations: vec!["RestController"],
        };

        let detector = FrameworkDetector::new(temp_dir.path());
        let results = detector.detect(&[&framework]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].confidence.value(), 0.0);
        assert!(results[0].evidence.is_empty());
    }

    #[test]
    fn import_patterns_end_at_package_boundaries() {
        assert!(FrameworkDetector::import_matches_pattern(
            "org.springframework.web.Controller",
            "org.springframework.web"
        ));
        assert!(!FrameworkDetector::import_matches_pattern(
            "org.springframework.webfake.Controller",
            "org.springframework.web"
        ));
    }

    #[test]
    fn detection_records_complete_import_names() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("Controller.java"),
            "import org.springframework.web.Controller;\nclass Example {}",
        )
        .unwrap();
        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec![],
            imports: vec!["org.springframework.web"],
            annotations: vec![],
        };

        let results = FrameworkDetector::new(temp_dir.path()).detect(&[&framework]);

        assert_eq!(
            results[0].evidence.imports,
            ["org.springframework.web.Controller"]
        );
    }

    #[test]
    fn oversized_sources_are_skipped_without_preventing_detection() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("Huge.java"),
            vec![b' '; MAX_SOURCE_FILE_BYTES + 1],
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("Controller.java"),
            "@RestController\nclass Example {}",
        )
        .unwrap();
        let framework = MockFramework {
            id: "spring",
            name: "Spring",
            deps: vec![],
            imports: vec![],
            annotations: vec!["RestController"],
        };

        let results = FrameworkDetector::new(temp_dir.path()).detect(&[&framework]);

        assert_eq!(results[0].evidence.annotations, ["RestController"]);
    }
}
