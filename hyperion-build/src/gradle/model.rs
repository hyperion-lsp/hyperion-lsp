// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Data structures representing Gradle projects and their components.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A Gradle version catalog (from gradle/libs.versions.toml)
#[derive(Debug, Clone, Default)]
pub struct VersionCatalog {
    /// Library aliases -> Maven coordinates
    /// e.g., "junit.jupiter.api" -> "org.junit.jupiter:junit-jupiter-api:5.13.4"
    pub libraries: HashMap<String, String>,

    /// Version references
    /// e.g., "junit-jupiter" -> "5.13.4"
    pub versions: HashMap<String, String>,

    /// Bundles map to lists of library aliases
    /// e.g., "logging" -> ["slf4j-api", "logback-classic"]
    pub bundles: HashMap<String, Vec<String>>,
}

/// A Gradle dependency (e.g., "com.google.guava:guava:32.1.2-jre")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GradleDependency {
    /// Group ID (e.g., "com.google.guava")
    pub group: String,

    /// Artifact ID (e.g., "guava")
    pub artifact: String,

    /// Version (e.g., "32.1.2-jre")
    /// Can be None for versionless dependencies that rely on BOM/platform
    pub version: Option<String>,

    /// Configuration/scope (implementation, api, compileOnly, etc.)
    pub configuration: DependencyConfiguration,

    /// Optional classifier (e.g., "sources", "javadoc")
    pub classifier: Option<String>,

    /// Optional packaging (defaults to "jar")
    pub packaging: Option<String>,

    /// Whether this is a platform/BOM dependency
    pub is_platform: bool,

    /// Gradle `exclude` rules from this declaration's configuration block, as
    /// `(group, module)` pairs. `*` matches any group or module. The resolver drops
    /// matching modules from this dependency's transitive graph.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusions: Vec<(String, String)>,

    /// Gradle `transitive` setting. `false` keeps this dependency's own JAR but
    /// drops its transitive dependencies.
    #[serde(default = "transitive_by_default")]
    pub transitive: bool,

    /// Gradle `force` setting. The declared version wins over any version that
    /// conflict resolution would otherwise select.
    #[serde(default)]
    pub forced: bool,
}

const fn transitive_by_default() -> bool {
    true
}

impl GradleDependency {
    /// Create a new dependency from Maven coordinates
    ///
    /// Accepts both 2-part (group:artifact) and 3-part (group:artifact:version) coordinates.
    /// 2-part coordinates are used with BOMs/platforms that manage versions.
    pub fn from_coordinates(
        coordinates: &str,
        configuration: DependencyConfiguration,
    ) -> Option<Self> {
        Self::from_coordinates_with_platform(coordinates, configuration, false)
    }

    /// Create a new dependency from Maven coordinates with platform flag
    pub fn from_coordinates_with_platform(
        coordinates: &str,
        configuration: DependencyConfiguration,
        is_platform: bool,
    ) -> Option<Self> {
        let parts: Vec<&str> = coordinates.split(':').collect();

        // Accept both group:artifact (2 parts) and group:artifact:version (3+ parts)
        if parts.len() < 2 {
            tracing::warn!(
                "Skipping Gradle dependency '{}': expected at least 'group:artifact'",
                coordinates
            );
            return None;
        }

        let version = if parts.len() >= 3 {
            Some(parts[2].to_string())
        } else {
            None
        };

        Some(Self {
            group: parts[0].to_string(),
            artifact: parts[1].to_string(),
            version,
            configuration,
            classifier: parts.get(3).map(|s| s.to_string()),
            packaging: parts.get(4).map(|s| s.to_string()),
            is_platform,
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
        })
    }

    /// Whether `other` declares the same artifact in the same configuration.
    ///
    /// Ignores `exclusions` and `transitive`, which a configuration block can add
    /// to a declaration that was already parsed.
    pub fn same_declaration(&self, other: &Self) -> bool {
        self.group == other.group
            && self.artifact == other.artifact
            && self.version == other.version
            && self.configuration == other.configuration
            && self.classifier == other.classifier
            && self.packaging == other.packaging
            && self.is_platform == other.is_platform
    }

    /// Get Maven coordinates (group:artifact:version or group:artifact if no version)
    pub fn coordinates(&self) -> String {
        if let Some(ref version) = self.version {
            format!("{}:{}:{}", self.group, self.artifact, version)
        } else {
            format!("{}:{}", self.group, self.artifact)
        }
    }

    /// Get full coordinates including classifier and packaging
    pub fn full_coordinates(&self) -> String {
        let mut coords = self.coordinates();
        if let Some(ref classifier) = self.classifier {
            coords.push(':');
            coords.push_str(classifier);
        }
        if let Some(ref packaging) = self.packaging {
            coords.push(':');
            coords.push_str(packaging);
        }
        coords
    }
}

/// Gradle dependency configuration/scope
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyConfiguration {
    /// Compile-time and runtime dependency
    Implementation,

    /// API dependency (exposed to consumers)
    Api,

    /// Compile-only dependency (not included at runtime)
    CompileOnly,

    /// Runtime-only dependency
    RuntimeOnly,

    /// Annotation processor
    AnnotationProcessor,

    /// Test implementation
    TestImplementation,

    /// Test compile-only
    TestCompileOnly,

    /// Test runtime-only
    TestRuntimeOnly,

    /// Development-only (e.g., Spring Boot DevTools)
    DevelopmentOnly,

    /// Custom configuration
    Custom(String),
}

impl DependencyConfiguration {
    /// Parse configuration from string
    pub fn from_config_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "implementation" => Self::Implementation,
            "api" => Self::Api,
            "compileonly" => Self::CompileOnly,
            "runtimeonly" => Self::RuntimeOnly,
            "annotationprocessor" => Self::AnnotationProcessor,
            "testimplementation" => Self::TestImplementation,
            "testcompileonly" => Self::TestCompileOnly,
            "testruntimeonly" => Self::TestRuntimeOnly,
            "developmentonly" => Self::DevelopmentOnly,
            _ => Self::Custom(s.to_string()),
        }
    }

    /// Check if this is a compile-time configuration
    pub fn is_compile(&self) -> bool {
        matches!(
            self,
            Self::Implementation
                | Self::Api
                | Self::CompileOnly
                | Self::AnnotationProcessor
                | Self::TestImplementation
                | Self::TestCompileOnly
        )
    }

    /// Check if this is a test configuration
    pub fn is_test(&self) -> bool {
        matches!(
            self,
            Self::TestImplementation | Self::TestCompileOnly | Self::TestRuntimeOnly
        )
    }

    /// Check if this configuration contributes to a runtime classpath.
    pub fn is_runtime(&self) -> bool {
        matches!(
            self,
            Self::Implementation
                | Self::Api
                | Self::RuntimeOnly
                | Self::TestImplementation
                | Self::TestRuntimeOnly
                | Self::DevelopmentOnly
        )
    }

    pub fn is_api(&self) -> bool {
        matches!(self, Self::Api)
    }
}

/// JPA persistence provider detected from build configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JpaProvider {
    /// Hibernate ORM (most common, Spring Boot default)
    Hibernate,
    /// EclipseLink (reference implementation)
    EclipseLink,
    /// Apache OpenJPA
    OpenJpa,
    /// JPA API present but provider not identified
    Unknown,
}

/// A Gradle plugin
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GradlePlugin {
    /// Plugin ID (e.g., "java", "org.springframework.boot")
    pub id: String,

    /// Optional version
    pub version: Option<String>,
}

impl GradlePlugin {
    /// Create a new plugin
    pub fn new(id: String) -> Self {
        Self { id, version: None }
    }

    /// Create a new plugin with version
    pub fn with_version(id: String, version: String) -> Self {
        Self {
            id,
            version: Some(version),
        }
    }
}

/// A Maven repository
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GradleRepository {
    /// Repository name
    pub name: String,

    /// Repository URL
    pub url: String,
}

impl GradleRepository {
    /// Maven Central repository
    pub fn maven_central() -> Self {
        Self {
            name: "MavenCentral".to_string(),
            url: "https://repo.maven.apache.org/maven2".to_string(),
        }
    }

    /// Maven Local repository (~/.m2/repository)
    pub fn maven_local() -> Self {
        Self {
            name: "MavenLocal".to_string(),
            url: "file://~/.m2/repository".to_string(),
        }
    }

    /// Google Maven repository
    pub fn google() -> Self {
        Self {
            name: "Google".to_string(),
            url: "https://maven.google.com".to_string(),
        }
    }

    /// JCenter repository (deprecated but still used)
    pub fn jcenter() -> Self {
        Self {
            name: "JCenter".to_string(),
            url: "https://jcenter.bintray.com".to_string(),
        }
    }

    /// Create a custom repository
    pub fn custom(name: String, url: String) -> Self {
        Self { name, url }
    }
}

/// A Gradle source set
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSet {
    /// Source set name (e.g., "main", "test")
    pub name: String,

    /// Java source directories
    pub java_dirs: Vec<PathBuf>,

    /// Resource directories
    pub resource_dirs: Vec<PathBuf>,

    /// Output directory for compiled classes
    pub output_dir: Option<PathBuf>,
}

impl SourceSet {
    /// Create a new source set
    pub fn new(name: String) -> Self {
        Self {
            name,
            java_dirs: Vec::new(),
            resource_dirs: Vec::new(),
            output_dir: None,
        }
    }

    /// Create a standard "main" source set
    pub fn main(base_dir: &std::path::Path) -> Self {
        let mut set = Self::new("main".to_string());
        set.java_dirs
            .push(base_dir.join("src/main/java").to_path_buf());
        set.resource_dirs
            .push(base_dir.join("src/main/resources").to_path_buf());
        set.output_dir = Some(hyperion_layout::gradle_class_dir(base_dir, "java", "main"));
        set
    }

    /// Create a standard "test" source set
    pub fn test(base_dir: &std::path::Path) -> Self {
        let mut set = Self::new("test".to_string());
        set.java_dirs
            .push(base_dir.join("src/test/java").to_path_buf());
        set.resource_dirs
            .push(base_dir.join("src/test/resources").to_path_buf());
        set.output_dir = Some(hyperion_layout::gradle_class_dir(base_dir, "java", "test"));
        set
    }
}

/// A complete Gradle project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradleProject {
    /// Project name
    pub name: String,

    /// Canonical Gradle project path (for example : or :services:auth).
    /// This is deliberately separate from name, which is only the display
    /// name and is not unique in nested builds.
    pub project_path: String,

    /// Project directory
    pub root_dir: PathBuf,

    /// Group ID (e.g., "com.example")
    pub group: Option<String>,

    /// Project version
    pub version: Option<String>,

    /// Applied plugins
    pub plugins: Vec<GradlePlugin>,

    /// Project dependencies
    pub dependencies: Vec<GradleDependency>,

    /// Configured repositories
    pub repositories: Vec<GradleRepository>,

    /// Source sets (main, test, etc.)
    pub source_sets: Vec<SourceSet>,

    /// Subproject names (for multi-project builds)
    pub subprojects: Vec<String>,

    /// Mapping from subproject name to its directory (absolute path)
    #[serde(skip)]
    pub subproject_dirs: HashMap<String, PathBuf>,

    /// Inter-project dependencies (e.g., project(':core'))
    /// Maps configuration -> Vec of project names
    #[serde(skip)]
    pub project_dependencies: HashMap<DependencyConfiguration, Vec<String>>,

    /// Whether this project has a Gradle wrapper
    pub has_wrapper: bool,

    /// Gradle version (if known)
    pub gradle_version: Option<String>,

    /// Version catalog (from gradle/libs.versions.toml)
    #[serde(skip)]
    pub version_catalog: Option<VersionCatalog>,
}

impl GradleProject {
    /// Create a new empty Gradle project
    pub fn new(name: String, root_dir: PathBuf) -> Self {
        Self {
            project_path: format!(":{name}"),
            name,
            root_dir,
            group: None,
            version: None,
            plugins: Vec::new(),
            dependencies: Vec::new(),
            repositories: Vec::new(),
            source_sets: Vec::new(),
            subprojects: Vec::new(),
            subproject_dirs: HashMap::new(),
            project_dependencies: HashMap::new(),
            has_wrapper: false,
            gradle_version: None,
            version_catalog: None,
        }
    }

    /// Get all dependencies for a specific configuration
    pub fn get_dependencies(&self, config: DependencyConfiguration) -> Vec<&GradleDependency> {
        self.dependencies
            .iter()
            .filter(|dep| dep.configuration == config)
            .collect()
    }

    /// Get all compile dependencies (including test for test source sets)
    pub fn get_compile_dependencies(&self) -> Vec<&GradleDependency> {
        self.dependencies
            .iter()
            .filter(|dep| dep.configuration.is_compile())
            .collect()
    }

    /// Check if a plugin is applied
    pub fn has_plugin(&self, plugin_id: &str) -> bool {
        self.plugins.iter().any(|p| p.id == plugin_id)
    }

    /// Check if this is a Java project
    pub fn is_java_project(&self) -> bool {
        self.has_plugin("java") || self.has_plugin("java-library") || self.has_plugin("application")
    }

    /// Check if this is a Spring Boot project
    pub fn is_spring_boot_project(&self) -> bool {
        self.has_plugin("org.springframework.boot")
    }

    /// Check if this project uses JPA (any provider)
    pub fn is_jpa_project(&self) -> bool {
        self.jpa_provider().is_some()
    }

    /// Detect which JPA provider (if any) this project uses
    pub fn jpa_provider(&self) -> Option<JpaProvider> {
        // Plugin-based detection (most reliable)
        if self.has_plugin("org.hibernate.orm") {
            return Some(JpaProvider::Hibernate);
        }

        // Dependency-based detection
        let has_dep = |group: &str, artifact: &str| {
            self.dependencies
                .iter()
                .any(|d| d.group == group && d.artifact == artifact)
        };
        let has_group_prefix = |prefix: &str| {
            self.dependencies
                .iter()
                .any(|d| d.group.starts_with(prefix))
        };

        // Spring Data JPA implies Hibernate (Spring's default JPA provider)
        if has_dep("org.springframework.boot", "spring-boot-starter-data-jpa") {
            return Some(JpaProvider::Hibernate);
        }

        // Direct Hibernate dependency
        if has_group_prefix("org.hibernate") {
            return Some(JpaProvider::Hibernate);
        }

        // EclipseLink
        if has_group_prefix("org.eclipse.persistence") {
            return Some(JpaProvider::EclipseLink);
        }

        // OpenJPA
        if has_group_prefix("org.apache.openjpa") {
            return Some(JpaProvider::OpenJpa);
        }

        // Generic JPA API without a specific provider
        if has_dep("jakarta.persistence", "jakarta.persistence-api")
            || has_dep("javax.persistence", "javax.persistence-api")
        {
            return Some(JpaProvider::Unknown);
        }

        None
    }

    /// Get the build file path (build.gradle or build.gradle.kts)
    pub fn build_file(&self) -> Option<PathBuf> {
        let groovy = self.root_dir.join("build.gradle");
        if groovy.exists() {
            return Some(groovy);
        }

        let kotlin = self.root_dir.join("build.gradle.kts");
        if kotlin.exists() {
            return Some(kotlin);
        }

        None
    }

    /// Get the settings file path (settings.gradle or settings.gradle.kts)
    pub fn settings_file(&self) -> Option<PathBuf> {
        let groovy = self.root_dir.join("settings.gradle");
        if groovy.exists() {
            return Some(groovy);
        }

        let kotlin = self.root_dir.join("settings.gradle.kts");
        if kotlin.exists() {
            return Some(kotlin);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_from_coordinates() {
        let dep = GradleDependency::from_coordinates(
            "com.google.guava:guava:32.1.2-jre",
            DependencyConfiguration::Implementation,
        )
        .unwrap();

        assert_eq!(dep.group, "com.google.guava");
        assert_eq!(dep.artifact, "guava");
        assert_eq!(dep.version.as_deref(), Some("32.1.2-jre"));
        assert_eq!(dep.configuration, DependencyConfiguration::Implementation);
        assert!(!dep.is_platform);
    }

    #[test]
    fn test_dependency_coordinates() {
        let dep = GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.springframework".to_string(),
            artifact: "spring-core".to_string(),
            version: Some("6.0.11".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        };

        assert_eq!(dep.coordinates(), "org.springframework:spring-core:6.0.11");
    }

    #[test]
    fn test_configuration_from_str() {
        assert_eq!(
            DependencyConfiguration::from_config_str("implementation"),
            DependencyConfiguration::Implementation
        );
        assert_eq!(
            DependencyConfiguration::from_config_str("api"),
            DependencyConfiguration::Api
        );
        assert_eq!(
            DependencyConfiguration::from_config_str("testImplementation"),
            DependencyConfiguration::TestImplementation
        );
    }

    #[test]
    fn test_configuration_is_compile() {
        assert!(DependencyConfiguration::Implementation.is_compile());
        assert!(DependencyConfiguration::Api.is_compile());
        assert!(DependencyConfiguration::CompileOnly.is_compile());
        assert!(!DependencyConfiguration::RuntimeOnly.is_compile());
    }

    #[test]
    fn test_configuration_is_test() {
        assert!(DependencyConfiguration::TestImplementation.is_test());
        assert!(DependencyConfiguration::TestCompileOnly.is_test());
        assert!(!DependencyConfiguration::Implementation.is_test());
    }

    #[test]
    fn test_configuration_runtime_scopes() {
        assert!(DependencyConfiguration::Implementation.is_runtime());
        assert!(DependencyConfiguration::Api.is_runtime());
        assert!(DependencyConfiguration::RuntimeOnly.is_runtime());
        assert!(DependencyConfiguration::TestImplementation.is_runtime());
        assert!(DependencyConfiguration::TestRuntimeOnly.is_runtime());
        assert!(!DependencyConfiguration::CompileOnly.is_runtime());
        assert!(!DependencyConfiguration::AnnotationProcessor.is_runtime());
        assert!(!DependencyConfiguration::TestCompileOnly.is_runtime());
    }

    #[test]
    fn test_gradle_plugin() {
        let plugin = GradlePlugin::new("java".to_string());
        assert_eq!(plugin.id, "java");
        assert_eq!(plugin.version, None);

        let plugin_with_version =
            GradlePlugin::with_version("org.springframework.boot".to_string(), "3.1.3".to_string());
        assert_eq!(plugin_with_version.id, "org.springframework.boot");
        assert_eq!(plugin_with_version.version, Some("3.1.3".to_string()));
    }

    #[test]
    fn test_gradle_repository() {
        let central = GradleRepository::maven_central();
        assert_eq!(central.name.as_str(), "MavenCentral");
        assert_eq!(central.url, "https://repo.maven.apache.org/maven2");

        let custom =
            GradleRepository::custom("MyRepo".to_string(), "https://my.repo.com".to_string());
        assert_eq!(custom.name.as_str(), "MyRepo");
        assert_eq!(custom.url, "https://my.repo.com");
    }

    #[test]
    fn test_source_set() {
        let base = PathBuf::from("/project");
        let main = SourceSet::main(&base);

        assert_eq!(main.name.as_str(), "main");
        assert_eq!(
            main.java_dirs,
            vec![PathBuf::from("/project/src/main/java")]
        );
        assert_eq!(
            main.resource_dirs,
            vec![PathBuf::from("/project/src/main/resources")]
        );
        assert_eq!(
            main.output_dir,
            Some(PathBuf::from("/project/build/classes/java/main"))
        );
    }

    #[test]
    fn test_gradle_project() {
        let mut project = GradleProject::new("test-project".to_string(), PathBuf::from("/test"));

        project.plugins.push(GradlePlugin::new("java".to_string()));
        assert!(project.is_java_project());
        assert!(!project.is_spring_boot_project());

        project
            .plugins
            .push(GradlePlugin::new("org.springframework.boot".to_string()));
        assert!(project.is_spring_boot_project());
    }

    #[test]
    fn test_jpa_detection_hibernate_plugin() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        assert!(!project.is_jpa_project());
        assert_eq!(project.jpa_provider(), None);

        project
            .plugins
            .push(GradlePlugin::new("org.hibernate.orm".to_string()));
        assert!(project.is_jpa_project());
        assert_eq!(project.jpa_provider(), Some(JpaProvider::Hibernate));
    }

    #[test]
    fn test_jpa_detection_spring_data_jpa() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.springframework.boot".to_string(),
            artifact: "spring-boot-starter-data-jpa".to_string(),
            version: Some("3.2.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
        assert_eq!(project.jpa_provider(), Some(JpaProvider::Hibernate));
    }

    #[test]
    fn test_jpa_detection_eclipselink() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.eclipse.persistence".to_string(),
            artifact: "eclipselink".to_string(),
            version: Some("4.0.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
        assert_eq!(project.jpa_provider(), Some(JpaProvider::EclipseLink));
    }

    #[test]
    fn test_jpa_detection_openjpa() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.apache.openjpa".to_string(),
            artifact: "openjpa".to_string(),
            version: Some("4.0.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
        assert_eq!(project.jpa_provider(), Some(JpaProvider::OpenJpa));
    }

    #[test]
    fn test_jpa_detection_jakarta_api_only() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "jakarta.persistence".to_string(),
            artifact: "jakarta.persistence-api".to_string(),
            version: Some("3.1.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
        assert_eq!(project.jpa_provider(), Some(JpaProvider::Unknown));
    }
}
