// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Data structures representing Maven projects and their components.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const MAX_PROPERTY_EXPANSIONS: usize = 256;
const MAX_PROPERTY_VALUE_BYTES: usize = 1024 * 1024;

/// A Maven dependency from pom.xml
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MavenDependency {
    /// Group ID (e.g., "org.junit.jupiter")
    pub group_id: String,

    /// Artifact ID (e.g., "junit-jupiter-api")
    pub artifact_id: String,

    /// Version (e.g., "5.13.4")
    /// Can be None if managed by dependencyManagement or parent POM
    pub version: Option<String>,

    /// Scope (compile, test, provided, runtime, system)
    pub scope: DependencyScope,

    /// Whether `<scope>` was explicitly present. Dependency management only
    /// supplies scope when the declaration omitted it.
    #[serde(default)]
    pub scope_explicit: bool,

    /// Optional classifier (e.g., "sources", "javadoc")
    pub classifier: Option<String>,

    /// Packaging type (defaults to "jar")
    pub packaging: String,

    /// Whether `<type>` was explicitly present.
    #[serde(default)]
    pub packaging_explicit: bool,

    /// Exact artifact path for Maven's non-transitive `system` scope.
    ///
    /// This must never be substituted with the same GAV from the local Maven
    /// repository: `systemPath` is the dependency identity selected by Maven.
    #[serde(default)]
    pub system_path: Option<String>,

    /// Whether this dependency is optional
    pub optional: bool,

    /// Whether `<optional>` was explicitly present.
    #[serde(default)]
    pub optional_explicit: bool,

    /// Exclusions (dependencies to exclude from transitive resolution)
    /// Each exclusion is a tuple of (groupId, artifactId)
    pub exclusions: Vec<(String, String)>,
}

/// Maven's dependency-management identity is group, artifact, type, and
/// classifier. Treating it as GA alone silently applies the wrong variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManagedDependencyKey(String);

impl ManagedDependencyKey {
    pub fn for_dependency(dependency: &MavenDependency) -> Self {
        // NUL cannot occur in a valid Maven coordinate component or file
        // name, and keeps this key unambiguous while remaining a JSON map key.
        Self(format!(
            "{}\0{}\0{}\0{}",
            dependency.group_id,
            dependency.artifact_id,
            dependency.packaging,
            dependency.classifier.as_deref().unwrap_or("")
        ))
    }
}

impl MavenDependency {
    /// Get Maven coordinates (groupId:artifactId:version)
    pub fn coordinates(&self) -> String {
        if let Some(ref version) = self.version {
            format!("{}:{}:{}", self.group_id, self.artifact_id, version)
        } else {
            format!("{}:{}", self.group_id, self.artifact_id)
        }
    }

    /// Check if this is a compile-time dependency
    pub fn is_compile(&self) -> bool {
        matches!(
            self.scope,
            DependencyScope::Compile | DependencyScope::Provided | DependencyScope::System
        )
    }

    /// Check if this is a test dependency
    pub fn is_test(&self) -> bool {
        self.scope == DependencyScope::Test
    }

    /// Check if a dependency is excluded by this dependency
    pub fn is_excluded(&self, group_id: &str, artifact_id: &str) -> bool {
        self.exclusions
            .iter()
            .any(|(g, a)| (g == "*" || g == group_id) && (a == "*" || a == artifact_id))
    }
}

/// Maven dependency scope
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum DependencyScope {
    /// Compile and runtime (default)
    #[default]
    Compile,

    /// Provided by JDK or container
    Provided,

    /// Runtime only
    Runtime,

    /// Test only
    Test,

    /// System path
    System,

    /// Import scope (only valid in dependencyManagement for BOM imports)
    Import,
}

impl DependencyScope {
    /// Parse scope from string
    pub fn from_scope_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "compile" => Self::Compile,
            "provided" => Self::Provided,
            "runtime" => Self::Runtime,
            "test" => Self::Test,
            "system" => Self::System,
            "import" => Self::Import,
            _ => Self::Compile, // Default to compile
        }
    }
}

/// A Maven repository
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MavenRepository {
    /// Repository ID
    pub id: String,

    /// Repository URL
    pub url: String,
}

impl MavenRepository {
    /// Create a new repository
    pub fn new(id: String, url: String) -> Self {
        Self { id, url }
    }

    /// Maven Central repository
    pub fn maven_central() -> Self {
        Self {
            id: "central".to_string(),
            url: "https://repo.maven.apache.org/maven2".to_string(),
        }
    }

    /// Maven local repository (~/.m2/repository)
    pub fn maven_local() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());

        Self {
            id: "local".to_string(),
            url: format!(
                "file://{}",
                PathBuf::from(home).join(".m2/repository").display()
            ),
        }
    }
}

/// A complete Maven project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenProject {
    /// Group ID
    pub group_id: Option<String>,

    /// Artifact ID
    pub artifact_id: String,

    /// Version
    pub version: Option<String>,

    /// Packaging (jar, war, pom, etc.)
    pub packaging: String,

    /// Project directory
    pub root_dir: PathBuf,

    /// Project dependencies
    pub dependencies: Vec<MavenDependency>,

    /// Managed dependencies (from dependencyManagement)
    pub managed_dependencies: HashMap<ManagedDependencyKey, MavenDependency>,

    /// BOM imports (dependencies with type=pom, scope=import in dependencyManagement)
    pub bom_imports: Vec<MavenDependency>,

    /// Configured repositories
    pub repositories: Vec<MavenRepository>,

    /// Properties (for variable substitution)
    pub properties: HashMap<String, String>,

    /// Parent POM coordinates (if any)
    pub parent: Option<ParentPom>,

    /// Modules (for multi-module projects)
    pub modules: Vec<String>,

    /// Profiles (conditional configuration)
    pub profiles: Vec<MavenProfile>,

    /// POM constructs observed by the parser but not represented by the
    /// effective-model implementation. A non-empty list makes completeness
    /// fail closed while retaining the partial model for navigation.
    #[serde(default)]
    pub unmodeled_features: Vec<String>,
}

impl MavenProject {
    /// Create a new empty Maven project
    pub fn new(artifact_id: String, root_dir: PathBuf) -> Self {
        Self {
            group_id: None,
            artifact_id,
            version: None,
            packaging: "jar".to_string(),
            root_dir,
            dependencies: Vec::new(),
            managed_dependencies: HashMap::new(),
            bom_imports: Vec::new(),
            repositories: vec![MavenRepository::maven_central()],
            properties: HashMap::new(),
            parent: None,
            modules: Vec::new(),
            profiles: Vec::new(),
            unmodeled_features: Vec::new(),
        }
    }

    /// Resolve a property value
    pub fn resolve_property(&self, value: &str) -> String {
        let mut resolved = value.to_string();
        let mut seen = HashSet::from([resolved.clone()]);
        for _ in 0..MAX_PROPERTY_EXPANSIONS {
            let Some(next) = self.expand_one_property(&resolved) else {
                return resolved;
            };
            if !seen.insert(next.clone()) {
                // Keep cyclic definitions unresolved instead of oscillating
                // until the expansion limit.
                return value.to_string();
            }
            resolved = next;
        }
        resolved
    }

    fn expand_one_property(&self, value: &str) -> Option<String> {
        let mut search_end = value.len();
        while let Some(start) = value[..search_end].rfind("${") {
            let Some(relative_end) = value[start + 2..].find('}') else {
                search_end = start;
                continue;
            };
            let end = start + 2 + relative_end;
            let name = &value[start + 2..end];
            if let Some(replacement) = self.property_value(name) {
                let new_len = start
                    .checked_add(replacement.len())?
                    .checked_add(value.len() - end - 1)?;
                if new_len > MAX_PROPERTY_VALUE_BYTES {
                    return None;
                }
                let mut expanded = String::with_capacity(new_len);
                expanded.push_str(&value[..start]);
                expanded.push_str(replacement);
                expanded.push_str(&value[end + 1..]);
                return Some(expanded);
            }
            search_end = start;
        }
        None
    }

    fn property_value(&self, name: &str) -> Option<&str> {
        match name {
            "project.groupId" | "pom.groupId" => self.group_id.as_deref(),
            "project.artifactId" | "pom.artifactId" => Some(&self.artifact_id),
            "project.version" | "pom.version" => self.version.as_deref(),
            "project.packaging" | "pom.packaging" => Some(&self.packaging),
            "project.basedir" | "basedir" => self.root_dir.to_str(),
            _ => self.properties.get(name).map(String::as_str),
        }
    }

    pub fn insert_managed_dependency(&mut self, dependency: MavenDependency) {
        self.managed_dependencies.insert(
            ManagedDependencyKey::for_dependency(&dependency),
            dependency,
        );
    }

    /// Fill attributes omitted by a direct declaration from the matching
    /// managed declaration. Explicit child attributes always win.
    pub fn apply_managed_dependency_from(
        managed_dependencies: &HashMap<ManagedDependencyKey, MavenDependency>,
        dependency: &mut MavenDependency,
    ) {
        let Some(managed) =
            managed_dependencies.get(&ManagedDependencyKey::for_dependency(dependency))
        else {
            return;
        };
        if dependency.version.is_none() {
            dependency.version.clone_from(&managed.version);
        }
        if !dependency.scope_explicit {
            dependency.scope.clone_from(&managed.scope);
        }
        if !dependency.optional_explicit {
            dependency.optional = managed.optional;
        }
        if dependency.system_path.is_none() {
            dependency.system_path.clone_from(&managed.system_path);
        }
        for exclusion in &managed.exclusions {
            if !dependency.exclusions.contains(exclusion) {
                dependency.exclusions.push(exclusion.clone());
            }
        }
    }

    pub fn apply_managed_dependency(&self, dependency: &mut MavenDependency) {
        Self::apply_managed_dependency_from(&self.managed_dependencies, dependency);
    }
}

/// Parent POM reference
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParentPom {
    /// Parent group ID
    pub group_id: String,

    /// Parent artifact ID
    pub artifact_id: String,

    /// Parent version
    pub version: String,

    /// Relative path to parent (optional)
    pub relative_path: Option<String>,
}

/// A Maven profile that can be conditionally activated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenProfile {
    /// Profile ID
    pub id: String,

    /// Activation conditions
    pub activation: ProfileActivation,

    /// Dependencies specific to this profile
    pub dependencies: Vec<MavenDependency>,

    /// Properties specific to this profile
    pub properties: HashMap<String, String>,
}

/// Profile activation conditions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileActivation {
    /// Always activate (no activation conditions)
    Always,

    /// Activate by property existence or value
    ByProperty {
        /// Property name
        name: String,
        /// Optional value to match
        value: Option<String>,
    },

    /// Activate by operating system
    ByOS {
        /// OS name pattern (e.g., "Windows", "Linux", "Mac OS X")
        name: Option<String>,
        /// OS architecture pattern (e.g., "x86_64", "aarch64")
        arch: Option<String>,
        /// OS version pattern
        version: Option<String>,
    },

    /// Activate by JDK version
    ByJDK(String),

    /// Activate by file existence
    ByFile {
        /// File that must exist
        exists: Option<PathBuf>,
        /// File that must be missing
        missing: Option<PathBuf>,
    },

    /// All conditions must be true (AND)
    All(Vec<ProfileActivation>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_properties(props: Vec<(&str, &str)>) -> MavenProject {
        let mut project = MavenProject::new("test".to_string(), PathBuf::from("/tmp"));
        for (k, v) in props {
            project.properties.insert(k.to_string(), v.to_string());
        }
        project
    }

    #[test]
    fn test_resolve_property_simple() {
        let project = project_with_properties(vec![("java.version", "17")]);
        assert_eq!(project.resolve_property("${java.version}"), "17");
    }

    #[test]
    fn test_resolve_property_missing_returns_original() {
        let project = project_with_properties(vec![]);
        assert_eq!(
            project.resolve_property("${missing.property}"),
            "${missing.property}"
        );
    }

    #[test]
    fn test_resolve_property_plain_text_unchanged() {
        let project = project_with_properties(vec![]);
        assert_eq!(project.resolve_property("plain-text"), "plain-text");
    }

    #[test]
    fn test_resolve_property_embedded() {
        let project = project_with_properties(vec![("maven.home", "/usr/share/maven")]);
        assert_eq!(
            project.resolve_property("${maven.home}/lib"),
            "/usr/share/maven/lib"
        );
    }

    #[test]
    fn test_resolve_property_multiple() {
        let project = project_with_properties(vec![("base.dir", "/opt"), ("app.name", "myapp")]);
        assert_eq!(
            project.resolve_property("${base.dir}/${app.name}"),
            "/opt/myapp"
        );
    }

    #[test]
    fn test_resolve_property_nested() {
        let project = project_with_properties(vec![
            ("env", "prod"),
            ("prod.url", "https://prod.example.com"),
        ]);
        assert_eq!(
            project.resolve_property("${${env}.url}"),
            "https://prod.example.com"
        );
    }

    #[test]
    fn test_resolve_property_project_version() {
        // project.version is a built-in Maven property
        let mut project = MavenProject::new("test".to_string(), PathBuf::from("/tmp"));
        project.version = Some("1.0.0".to_string());
        assert_eq!(project.resolve_property("${project.version}"), "1.0.0");
    }

    #[test]
    fn test_resolve_property_recursive_value() {
        let project = project_with_properties(vec![("a", "${b}"), ("b", "resolved")]);
        assert_eq!(project.resolve_property("prefix-${a}"), "prefix-resolved");
    }

    #[test]
    fn test_resolve_property_cycle_stays_unresolved() {
        let project = project_with_properties(vec![("a", "${b}"), ("b", "${a}")]);
        assert_eq!(project.resolve_property("${a}"), "${a}");
    }
}
