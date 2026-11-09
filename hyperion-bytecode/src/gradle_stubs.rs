// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle API stub implementations for configuration extraction
//!
//! These stubs intercept Gradle DSL calls during bytecode execution to extract
//! build configuration without actually executing Gradle. They integrate with
//! the interpreter's native method system.
//!
//! # Architecture
//!
//! ```text
//! Groovy Script Execution
//!         ↓
//! Method Call (e.g., dependencies { ... })
//!         ↓
//! Stub Interception
//!         ↓
//! Configuration Recording → GradleBuildConfig
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_bytecode::gradle_stubs::StubRegistry;
//! use hyperion_bytecode::gradle::GradleBuildConfig;
//!
//! let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
//! let stubs = StubRegistry::new(config.clone());
//!
//! // During execution, stubs record configuration
//! stubs.project().dependencies(closure);
//!
//! // Extract results
//! let build_config = config.lock().unwrap();
//! for dep in &build_config.dependency_list {
//!     println!("{}: {}", dep.configuration, dep.to_notation());
//! }
//! ```

use super::gradle::{Dependency, GradleBuildConfig, Plugin, Repository, RepositoryType, Task};
use std::sync::{Arc, Mutex};

// ============================================================================
// Closure Reference (placeholder for actual closure handling)
// ============================================================================

/// Reference to a Groovy closure for deferred execution
///
/// During stub execution, we capture closure references and either:
/// 1. Execute them immediately with a delegate object
/// 2. Record them for later analysis
#[derive(Debug, Clone)]
pub struct ClosureRef {
    /// Unique identifier for this closure
    pub id: u64,
    /// The delegate class type (if known)
    pub delegate_type: Option<String>,
    /// Whether the closure has been executed
    pub executed: bool,
}

impl ClosureRef {
    /// Create a new closure reference
    pub fn new(id: u64) -> Self {
        Self {
            id,
            delegate_type: None,
            executed: false,
        }
    }

    /// Create with a known delegate type
    pub fn with_delegate(id: u64, delegate_type: &str) -> Self {
        Self {
            id,
            delegate_type: Some(delegate_type.to_string()),
            executed: false,
        }
    }
}

// ============================================================================
// Project Stub
// ============================================================================

/// Stub implementation of Gradle's Project interface
///
/// This is the main entry point for Gradle DSL. When a build.gradle script
/// is executed, `this` is bound to the Project object.
#[derive(Clone)]
pub struct ProjectStub {
    config: Arc<Mutex<GradleBuildConfig>>,
    dependency_handler: DependencyHandlerStub,
    repository_handler: RepositoryHandlerStub,
    plugin_manager: PluginManagerStub,
}

impl ProjectStub {
    /// Create a new Project stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self {
            dependency_handler: DependencyHandlerStub::new(config.clone()),
            repository_handler: RepositoryHandlerStub::new(config.clone()),
            plugin_manager: PluginManagerStub::new(config.clone()),
            config,
        }
    }

    /// Get the project name
    pub fn get_name(&self) -> String {
        self.config
            .lock()
            .unwrap()
            .project_name
            .clone()
            .unwrap_or_else(|| "unnamed".to_string())
    }

    /// Set the project name
    pub fn set_name(&self, name: &str) {
        self.config.lock().unwrap().project_name = Some(name.to_string());
    }

    /// Get the project version
    pub fn get_version(&self) -> String {
        self.config
            .lock()
            .unwrap()
            .version
            .clone()
            .unwrap_or_else(|| "unspecified".to_string())
    }

    /// Set the project version
    pub fn set_version(&self, version: &str) {
        self.config.lock().unwrap().version = Some(version.to_string());
    }

    /// Get the project group
    pub fn get_group(&self) -> String {
        self.config
            .lock()
            .unwrap()
            .group
            .clone()
            .unwrap_or_default()
    }

    /// Set the project group
    pub fn set_group(&self, group: &str) {
        self.config.lock().unwrap().group = Some(group.to_string());
    }

    /// Get the project description
    pub fn get_description(&self) -> Option<String> {
        self.config.lock().unwrap().description.clone()
    }

    /// Set the project description
    pub fn set_description(&self, description: &str) {
        self.config.lock().unwrap().description = Some(description.to_string());
    }

    /// Configure dependencies
    ///
    /// In Groovy: `dependencies { implementation 'com.example:lib:1.0' }`
    pub fn dependencies(&self, _closure: ClosureRef) -> &DependencyHandlerStub {
        // The closure would be executed with DependencyHandler as delegate
        // For now, we return the handler for direct method calls
        &self.dependency_handler
    }

    /// Get the dependency handler for direct access
    pub fn dependency_handler(&self) -> &DependencyHandlerStub {
        &self.dependency_handler
    }

    /// Configure repositories
    ///
    /// In Groovy: `repositories { mavenCentral() }`
    pub fn repositories(&self, _closure: ClosureRef) -> &RepositoryHandlerStub {
        &self.repository_handler
    }

    /// Get the repository handler for direct access
    pub fn repository_handler(&self) -> &RepositoryHandlerStub {
        &self.repository_handler
    }

    /// Configure plugins
    ///
    /// In Groovy: `plugins { id 'java' }`
    pub fn plugins(&self, _closure: ClosureRef) -> &PluginManagerStub {
        &self.plugin_manager
    }

    /// Get the plugin manager for direct access
    pub fn plugin_manager(&self) -> &PluginManagerStub {
        &self.plugin_manager
    }

    /// Register a task
    ///
    /// In Groovy: `task myTask { ... }`
    pub fn task(&self, name: &str, _closure: Option<ClosureRef>) -> TaskStub {
        let task = Task::new(name);
        self.config.lock().unwrap().add_task(task);
        TaskStub::new(name.to_string(), self.config.clone())
    }

    /// Register a typed task
    ///
    /// In Groovy: `task myTask(type: Copy) { ... }`
    pub fn task_typed(
        &self,
        name: &str,
        task_type: &str,
        _closure: Option<ClosureRef>,
    ) -> TaskStub {
        let task = Task::typed(name, task_type);
        self.config.lock().unwrap().add_task(task);
        TaskStub::new(name.to_string(), self.config.clone())
    }

    /// Set a project property
    pub fn set_property(&self, key: &str, value: &str) {
        self.config.lock().unwrap().set_property(key, value);
    }

    /// Get a project property
    pub fn get_property(&self, key: &str) -> Option<String> {
        self.config.lock().unwrap().get_property(key).cloned()
    }

    /// Set an extra property (ext block)
    pub fn set_ext(&self, key: &str, value: &str) {
        self.config.lock().unwrap().set_extra_property(key, value);
    }

    /// Apply a plugin by ID (legacy method)
    ///
    /// In Groovy: `apply plugin: 'java'`
    pub fn apply_plugin(&self, plugin_id: &str) {
        let plugin = Plugin::new(plugin_id);
        self.config.lock().unwrap().add_plugin(plugin);
    }

    /// Apply a plugin from a map (legacy method)
    ///
    /// In Groovy: `apply plugin: 'java'` or `apply from: 'other.gradle'`
    pub fn apply(&self, map: &std::collections::HashMap<String, String>) {
        if let Some(plugin_id) = map.get("plugin") {
            self.apply_plugin(plugin_id);
        }
        // 'from' would require loading another script - record it for now
        if let Some(from_path) = map.get("from") {
            self.config
                .lock()
                .unwrap()
                .set_property("applied_from", from_path);
        }
    }

    /// Configure all projects (multi-project builds)
    pub fn allprojects(&self, _closure: ClosureRef) {
        // In a multi-project build, this would iterate over all projects
        // For now, we just apply to the root project
    }

    /// Configure subprojects (multi-project builds)
    pub fn subprojects(&self, _closure: ClosureRef) {
        // In a multi-project build, this would iterate over subprojects
    }

    /// Configure Java toolchain
    pub fn java(&self, _closure: ClosureRef) {
        // Would configure java extension
    }

    /// Configure source sets
    #[allow(non_snake_case)]
    pub fn sourceSets(&self, _closure: ClosureRef) {
        // Would configure source sets
        // Note: Using Groovy naming convention (sourceSets) for API compatibility
    }
}

// ============================================================================
// DependencyHandler Stub
// ============================================================================

/// Stub implementation of Gradle's DependencyHandler
///
/// Handles dependency declarations for all configurations.
#[derive(Clone)]
pub struct DependencyHandlerStub {
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl DependencyHandlerStub {
    /// Create a new DependencyHandler stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { config }
    }

    /// Add a dependency to the implementation configuration
    pub fn implementation(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("implementation", notation)
    }

    /// Add a dependency to the api configuration
    pub fn api(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("api", notation)
    }

    /// Add a dependency to the compileOnly configuration
    pub fn compile_only(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("compileOnly", notation)
    }

    /// Add a dependency to the runtimeOnly configuration
    pub fn runtime_only(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("runtimeOnly", notation)
    }

    /// Add a dependency to the testImplementation configuration
    pub fn test_implementation(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("testImplementation", notation)
    }

    /// Add a dependency to the testCompileOnly configuration
    pub fn test_compile_only(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("testCompileOnly", notation)
    }

    /// Add a dependency to the testRuntimeOnly configuration
    pub fn test_runtime_only(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("testRuntimeOnly", notation)
    }

    /// Add a dependency to the annotationProcessor configuration
    pub fn annotation_processor(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("annotationProcessor", notation)
    }

    /// Add a dependency to the kapt configuration (Kotlin annotation processing)
    pub fn kapt(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("kapt", notation)
    }

    /// Add a dependency to the classpath configuration (buildscript)
    pub fn classpath(&self, notation: &str) -> Option<DependencySpecStub> {
        self.add("classpath", notation)
    }

    /// Add a project dependency
    ///
    /// In Groovy: `implementation project(':submodule')`
    pub fn project(&self, config_name: &str, project_path: &str) -> Option<DependencySpecStub> {
        let mut dep = Dependency::new(config_name, "", project_path, "");
        dep.is_project = true;
        self.config.lock().unwrap().add_dependency(dep);

        Some(DependencySpecStub::new(
            config_name.to_string(),
            project_path.to_string(),
            self.config.clone(),
        ))
    }

    /// Generic add method for any configuration
    pub fn add(&self, config_name: &str, notation: &str) -> Option<DependencySpecStub> {
        if let Some(dep) = Dependency::parse(config_name, notation) {
            let notation_copy = notation.to_string();
            self.config.lock().unwrap().add_dependency(dep);
            Some(DependencySpecStub::new(
                config_name.to_string(),
                notation_copy,
                self.config.clone(),
            ))
        } else {
            // Handle special cases like project dependencies or file dependencies
            None
        }
    }

    /// Add with closure for additional configuration
    ///
    /// In Groovy: `implementation('com.example:lib:1.0') { transitive = false }`
    pub fn add_with_closure(
        &self,
        config_name: &str,
        notation: &str,
        _closure: ClosureRef,
    ) -> Option<DependencySpecStub> {
        self.add(config_name, notation)
    }

    /// Create a dependency constraint
    pub fn constraints(&self, _closure: ClosureRef) {
        // Dependency constraints would be recorded here
    }

    /// Platform dependency (BOM)
    pub fn platform(&self, config_name: &str, notation: &str) -> Option<DependencySpecStub> {
        // Mark as platform/BOM dependency
        if let Some(mut dep) = Dependency::parse(config_name, notation) {
            dep.extension = Some("pom".to_string());
            let notation_copy = notation.to_string();
            self.config.lock().unwrap().add_dependency(dep);
            Some(DependencySpecStub::new(
                config_name.to_string(),
                notation_copy,
                self.config.clone(),
            ))
        } else {
            None
        }
    }

    /// `enforcedPlatform(notation)`: a platform whose versions are enforced on the graph.
    pub fn enforced_platform(
        &self,
        config_name: &str,
        notation: &str,
    ) -> Option<DependencySpecStub> {
        let stub = self.platform(config_name, notation)?;
        if let Some(dep) = self.config.lock().unwrap().dependency_list.last_mut() {
            dep.enforced = true;
        }
        Some(stub)
    }
}

// ============================================================================
// DependencySpec Stub
// ============================================================================

/// Stub for additional dependency configuration
#[derive(Clone)]
pub struct DependencySpecStub {
    configuration: String,
    notation: String,
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl DependencySpecStub {
    /// Create a new dependency spec stub
    pub fn new(
        configuration: String,
        notation: String,
        config: Arc<Mutex<GradleBuildConfig>>,
    ) -> Self {
        Self {
            configuration,
            notation,
            config,
        }
    }

    /// Exclude a transitive dependency
    ///
    /// Recorded as a property only. Configuration blocks are not executed by this
    /// stub, so the build model reads `exclude` from the build script text instead.
    pub fn exclude(&self, group: Option<&str>, module: Option<&str>) -> &Self {
        let key = format!(
            "exclude:{}:{}:{}",
            self.configuration,
            group.unwrap_or("*"),
            module.unwrap_or("*")
        );
        self.config.lock().unwrap().set_property(&key, "true");
        self
    }

    /// Set transitivity
    ///
    /// Recorded as a property only. As with `exclude`, the build model reads the
    /// setting from the build script text.
    pub fn transitive(&self, value: bool) -> &Self {
        let key = format!("transitive:{}:{}", self.configuration, self.notation);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &value.to_string());
        self
    }

    /// Force a specific version
    ///
    /// Recorded as a property only. As with `exclude`, the build model reads the
    /// setting from the build script text.
    pub fn force(&self, value: bool) -> &Self {
        let key = format!("force:{}:{}", self.configuration, self.notation);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &value.to_string());
        self
    }

    /// Change the version
    pub fn version(&self, version: &str) -> &Self {
        // This would update the dependency version in the config
        let key = format!("version:{}:{}", self.configuration, self.notation);
        self.config.lock().unwrap().set_property(&key, version);
        self
    }
}

// ============================================================================
// RepositoryHandler Stub
// ============================================================================

/// Stub implementation of Gradle's RepositoryHandler
#[derive(Clone)]
pub struct RepositoryHandlerStub {
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl RepositoryHandlerStub {
    /// Create a new RepositoryHandler stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { config }
    }

    /// Add Maven Central repository
    pub fn maven_central(&self) -> RepositoryStub {
        let repo = Repository::maven_central();
        let name = repo.name.clone();
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add Maven Local repository
    pub fn maven_local(&self) -> RepositoryStub {
        let repo = Repository::maven_local();
        let name = repo.name.clone();
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add Google Maven repository
    pub fn google(&self) -> RepositoryStub {
        let repo = Repository::google();
        let name = repo.name.clone();
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add JCenter repository (deprecated but still used)
    pub fn jcenter(&self) -> RepositoryStub {
        let repo = Repository {
            name: "jcenter".to_string(),
            url: "https://jcenter.bintray.com/".to_string(),
            repo_type: RepositoryType::Maven,
        };
        let name = repo.name.clone();
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add Gradle Plugin Portal repository
    pub fn gradle_plugin_portal(&self) -> RepositoryStub {
        let repo = Repository::gradle_plugin_portal();
        let name = repo.name.clone();
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add a custom Maven repository
    ///
    /// In Groovy: `maven { url 'https://example.com/repo' }`
    pub fn maven(&self, _closure: ClosureRef) -> MavenRepositoryStub {
        MavenRepositoryStub::new(self.config.clone())
    }

    /// Add a custom Maven repository with URL
    pub fn maven_url(&self, url: &str) -> RepositoryStub {
        let name = format!(
            "maven-{}",
            self.config.lock().unwrap().repository_list.len()
        );
        let repo = Repository {
            name: name.clone(),
            url: url.to_string(),
            repo_type: RepositoryType::Maven,
        };
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }

    /// Add a custom Maven repository with name and URL
    pub fn maven_named(&self, name: &str, url: &str) -> RepositoryStub {
        let repo = Repository {
            name: name.to_string(),
            url: url.to_string(),
            repo_type: RepositoryType::Maven,
        };
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name.to_string(), self.config.clone())
    }

    /// Add an Ivy repository
    pub fn ivy(&self, _closure: ClosureRef) -> IvyRepositoryStub {
        IvyRepositoryStub::new(self.config.clone())
    }

    /// Add a flat directory repository
    pub fn flat_dir(&self, dirs: &[&str]) -> RepositoryStub {
        let name = "flatDir".to_string();
        let repo = Repository {
            name: name.clone(),
            url: dirs.join(", "),
            repo_type: RepositoryType::FlatDir,
        };
        self.config.lock().unwrap().add_repository(repo);
        RepositoryStub::new(name, self.config.clone())
    }
}

// ============================================================================
// Repository Stubs
// ============================================================================

/// Stub for a configured repository
#[derive(Clone)]
pub struct RepositoryStub {
    name: String,
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl RepositoryStub {
    /// Create a new repository stub
    pub fn new(name: String, config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { name, config }
    }

    /// Get the repository name
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Set credentials (for authenticated repositories)
    pub fn credentials(&self, username: &str, password: &str) {
        let key = format!("repo:{}:credentials", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &format!("{}:****", username));
        // Note: We intentionally don't store the actual password
        let _ = password; // Consume to avoid unused warning
    }
}

/// Stub for Maven repository configuration
#[derive(Clone)]
pub struct MavenRepositoryStub {
    config: Arc<Mutex<GradleBuildConfig>>,
    url: Option<String>,
    name: Option<String>,
}

impl MavenRepositoryStub {
    /// Create a new Maven repository stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self {
            config,
            url: None,
            name: None,
        }
    }

    /// Set the repository URL
    pub fn url(&mut self, url: &str) -> &mut Self {
        self.url = Some(url.to_string());
        self
    }

    /// Set the repository name
    pub fn name(&mut self, name: &str) -> &mut Self {
        self.name = Some(name.to_string());
        self
    }

    /// Finalize and add the repository
    pub fn finalize(&self) -> Option<RepositoryStub> {
        if let Some(url) = &self.url {
            let name = self
                .name
                .clone()
                .unwrap_or_else(|| format!("maven-{}", url.len()));
            let repo = Repository {
                name: name.clone(),
                url: url.clone(),
                repo_type: RepositoryType::Maven,
            };
            self.config.lock().unwrap().add_repository(repo);
            Some(RepositoryStub::new(name, self.config.clone()))
        } else {
            None
        }
    }

    /// Set credentials
    pub fn credentials(&mut self, username: &str, _password: &str) -> &mut Self {
        if let Some(ref url) = self.url {
            let key = format!("repo:{}:credentials", url);
            self.config
                .lock()
                .unwrap()
                .set_property(&key, &format!("{}:****", username));
        }
        self
    }
}

/// Stub for Ivy repository configuration
#[derive(Clone)]
pub struct IvyRepositoryStub {
    config: Arc<Mutex<GradleBuildConfig>>,
    url: Option<String>,
}

impl IvyRepositoryStub {
    /// Create a new Ivy repository stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { config, url: None }
    }

    /// Set the repository URL
    pub fn url(&mut self, url: &str) -> &mut Self {
        self.url = Some(url.to_string());
        self
    }

    /// Finalize and add the repository
    pub fn finalize(&self) -> Option<RepositoryStub> {
        if let Some(url) = &self.url {
            let name = format!("ivy-{}", url.len());
            let repo = Repository {
                name: name.clone(),
                url: url.clone(),
                repo_type: RepositoryType::Ivy,
            };
            self.config.lock().unwrap().add_repository(repo);
            Some(RepositoryStub::new(name, self.config.clone()))
        } else {
            None
        }
    }
}

// ============================================================================
// Plugin Manager Stub
// ============================================================================

/// Stub implementation of Gradle's PluginManager/PluginDependenciesSpec
#[derive(Clone)]
pub struct PluginManagerStub {
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl PluginManagerStub {
    /// Create a new PluginManager stub
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { config }
    }

    /// Declare a plugin
    ///
    /// In Groovy: `id 'java'` or `id 'org.jetbrains.kotlin.jvm' version '1.9.0'`
    pub fn id(&self, plugin_id: &str) -> PluginSpecStub {
        PluginSpecStub::new(plugin_id.to_string(), self.config.clone())
    }

    /// Apply a plugin immediately
    pub fn apply(&self, plugin_id: &str) {
        let plugin = Plugin::new(plugin_id);
        self.config.lock().unwrap().add_plugin(plugin);
    }

    /// Apply Kotlin plugin
    pub fn kotlin(&self, module: &str) -> PluginSpecStub {
        let plugin_id = format!("org.jetbrains.kotlin.{}", module);
        self.id(&plugin_id)
    }

    /// Check if a plugin is applied
    pub fn has_plugin(&self, plugin_id: &str) -> bool {
        self.config.lock().unwrap().has_plugin(plugin_id)
    }

    /// Get all applied plugins
    pub fn get_plugins(&self) -> Vec<String> {
        self.config.lock().unwrap().plugins.clone()
    }
}

// ============================================================================
// Plugin Spec Stub
// ============================================================================

/// Stub for plugin specification (builder pattern)
#[derive(Clone)]
pub struct PluginSpecStub {
    id: String,
    version: Option<String>,
    apply: bool,
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl PluginSpecStub {
    /// Create a new plugin spec
    pub fn new(id: String, config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self {
            id,
            version: None,
            apply: true,
            config,
        }
    }

    /// Set the plugin version
    pub fn version(mut self, version: &str) -> Self {
        self.version = Some(version.to_string());
        self
    }

    /// Set whether to apply the plugin
    pub fn apply(mut self, apply: bool) -> Self {
        self.apply = apply;
        self
    }

    /// Finalize and register the plugin
    ///
    /// This is called implicitly at the end of the plugins block
    pub fn finalize(&self) {
        let plugin = if let Some(ref version) = self.version {
            let mut p = Plugin::with_version(&self.id, version);
            p.apply = self.apply;
            p
        } else {
            let mut p = Plugin::new(&self.id);
            p.apply = self.apply;
            p
        };

        if self.apply {
            self.config.lock().unwrap().add_plugin(plugin);
        }
    }

    /// Get the plugin ID
    pub fn get_id(&self) -> &str {
        &self.id
    }

    /// Get the plugin version
    pub fn get_version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

// When PluginSpecStub is dropped, finalize the plugin
impl Drop for PluginSpecStub {
    fn drop(&mut self) {
        // Only finalize if apply is true and plugin isn't already registered
        if self.apply {
            let already_registered = self.config.lock().unwrap().has_plugin(&self.id);
            if !already_registered {
                self.finalize();
            }
        }
    }
}

// ============================================================================
// Configuration Stub
// ============================================================================

/// Stub for named configurations
#[derive(Clone)]
pub struct ConfigurationStub {
    name: String,
    config: Arc<Mutex<GradleBuildConfig>>,
    extends_from: Vec<String>,
}

impl ConfigurationStub {
    /// Create a new configuration stub
    pub fn new(name: String, config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self {
            name,
            config,
            extends_from: Vec::new(),
        }
    }

    /// Get the configuration name
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Set configurations this extends from
    pub fn extends_from(&mut self, configs: &[&str]) -> &mut Self {
        self.extends_from = configs.iter().map(|s| s.to_string()).collect();
        // Record the extension relationship
        let key = format!("config:{}:extends", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &configs.join(","));
        self
    }

    /// Set whether this configuration is transitive
    pub fn transitive(&mut self, value: bool) -> &mut Self {
        let key = format!("config:{}:transitive", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &value.to_string());
        self
    }

    /// Set whether this configuration is visible
    pub fn visible(&mut self, value: bool) -> &mut Self {
        let key = format!("config:{}:visible", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &value.to_string());
        self
    }

    /// Set description
    pub fn description(&mut self, desc: &str) -> &mut Self {
        let key = format!("config:{}:description", self.name);
        self.config.lock().unwrap().set_property(&key, desc);
        self
    }
}

// ============================================================================
// Task Stub
// ============================================================================

/// Stub for task configuration
#[derive(Clone)]
pub struct TaskStub {
    name: String,
    config: Arc<Mutex<GradleBuildConfig>>,
}

impl TaskStub {
    /// Create a new task stub
    pub fn new(name: String, config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self { name, config }
    }

    /// Get the task name
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Set task group
    pub fn group(&self, group: &str) -> &Self {
        let key = format!("task:{}:group", self.name);
        self.config.lock().unwrap().set_property(&key, group);
        self
    }

    /// Set task description
    pub fn description(&self, desc: &str) -> &Self {
        let key = format!("task:{}:description", self.name);
        self.config.lock().unwrap().set_property(&key, desc);
        self
    }

    /// Add task dependency
    pub fn depends_on(&self, tasks: &[&str]) -> &Self {
        let key = format!("task:{}:dependsOn", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &tasks.join(","));
        self
    }

    /// Add finalized by relationship
    pub fn finalized_by(&self, tasks: &[&str]) -> &Self {
        let key = format!("task:{}:finalizedBy", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &tasks.join(","));
        self
    }

    /// Add must run after relationship
    pub fn must_run_after(&self, tasks: &[&str]) -> &Self {
        let key = format!("task:{}:mustRunAfter", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &tasks.join(","));
        self
    }

    /// Add should run after relationship
    pub fn should_run_after(&self, tasks: &[&str]) -> &Self {
        let key = format!("task:{}:shouldRunAfter", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &tasks.join(","));
        self
    }

    /// Set enabled state
    pub fn enabled(&self, value: bool) -> &Self {
        let key = format!("task:{}:enabled", self.name);
        self.config
            .lock()
            .unwrap()
            .set_property(&key, &value.to_string());
        self
    }

    /// Configure do first action
    pub fn do_first(&self, _closure: ClosureRef) -> &Self {
        // Record that doFirst was configured
        let key = format!("task:{}:hasDoFirst", self.name);
        self.config.lock().unwrap().set_property(&key, "true");
        self
    }

    /// Configure do last action
    pub fn do_last(&self, _closure: ClosureRef) -> &Self {
        // Record that doLast was configured
        let key = format!("task:{}:hasDoLast", self.name);
        self.config.lock().unwrap().set_property(&key, "true");
        self
    }
}

// ============================================================================
// Stub Registry
// ============================================================================

/// Registry for managing all Gradle API stubs
///
/// Creates and manages stub instances that share a common configuration.
pub struct StubRegistry {
    config: Arc<Mutex<GradleBuildConfig>>,
    project: ProjectStub,
}

impl StubRegistry {
    /// Create a new stub registry
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        Self {
            project: ProjectStub::new(config.clone()),
            config,
        }
    }

    /// Get the Project stub
    pub fn project(&self) -> &ProjectStub {
        &self.project
    }

    /// Get the shared configuration
    pub fn config(&self) -> Arc<Mutex<GradleBuildConfig>> {
        self.config.clone()
    }

    /// Create a new configuration stub
    pub fn create_configuration(&self, name: &str) -> ConfigurationStub {
        ConfigurationStub::new(name.to_string(), self.config.clone())
    }

    /// Get extracted build configuration
    pub fn get_build_config(&self) -> GradleBuildConfig {
        self.config.lock().unwrap().clone()
    }

    /// Reset the configuration (for testing or re-execution)
    pub fn reset(&self) {
        *self.config.lock().unwrap() = GradleBuildConfig::new();
    }
}

// ============================================================================
// Native Method Integration
// ============================================================================

/// Signature for a stub method that can be registered as a native
pub type StubMethod = fn(&StubRegistry, &[String]) -> Result<String, String>;

/// Registry of stub methods for native integration
pub struct StubMethodRegistry {
    methods: std::collections::HashMap<String, StubMethod>,
    stubs: StubRegistry,
}

impl StubMethodRegistry {
    /// Create a new stub method registry
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        let mut registry = Self {
            methods: std::collections::HashMap::new(),
            stubs: StubRegistry::new(config),
        };
        registry.register_defaults();
        registry
    }

    /// Register default stub methods
    fn register_defaults(&mut self) {
        self.register("Project.getName", project_get_name);
        self.register("Project.getVersion", project_get_version);
        self.register("Project.setVersion", project_set_version);
        self.register("Project.getGroup", project_get_group);
        self.register("Project.setGroup", project_set_group);
        self.register(
            "DependencyHandler.implementation",
            dependency_implementation,
        );
        self.register("DependencyHandler.api", dependency_api);
        self.register(
            "DependencyHandler.testImplementation",
            dependency_test_implementation,
        );
        self.register("RepositoryHandler.mavenCentral", repository_maven_central);
        self.register("RepositoryHandler.mavenLocal", repository_maven_local);
        self.register("RepositoryHandler.google", repository_google);
        self.register("PluginManager.id", plugin_id);
        self.register("PluginManager.apply", plugin_apply);
    }

    /// Register a stub method
    pub fn register(&mut self, name: &str, method: StubMethod) {
        self.methods.insert(name.to_string(), method);
    }

    /// Invoke a stub method
    pub fn invoke(&self, name: &str, args: &[String]) -> Result<String, String> {
        match self.methods.get(name) {
            Some(method) => method(&self.stubs, args),
            None => Err(format!("Unknown stub method: {}", name)),
        }
    }

    /// Get the stub registry
    pub fn stubs(&self) -> &StubRegistry {
        &self.stubs
    }
}

// Default stub method implementations
fn project_get_name(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    Ok(stubs.project().get_name())
}

fn project_get_version(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    Ok(stubs.project().get_version())
}

fn project_set_version(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("setVersion requires a version argument".to_string());
    }
    stubs.project().set_version(&args[0]);
    Ok("".to_string())
}

fn project_get_group(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    Ok(stubs.project().get_group())
}

fn project_set_group(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("setGroup requires a group argument".to_string());
    }
    stubs.project().set_group(&args[0]);
    Ok("".to_string())
}

fn dependency_implementation(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("implementation requires a dependency notation".to_string());
    }
    stubs
        .project()
        .dependency_handler()
        .implementation(&args[0]);
    Ok("".to_string())
}

fn dependency_api(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("api requires a dependency notation".to_string());
    }
    stubs.project().dependency_handler().api(&args[0]);
    Ok("".to_string())
}

fn dependency_test_implementation(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("testImplementation requires a dependency notation".to_string());
    }
    stubs
        .project()
        .dependency_handler()
        .test_implementation(&args[0]);
    Ok("".to_string())
}

fn repository_maven_central(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    stubs.project().repository_handler().maven_central();
    Ok("mavenCentral".to_string())
}

fn repository_maven_local(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    stubs.project().repository_handler().maven_local();
    Ok("mavenLocal".to_string())
}

fn repository_google(stubs: &StubRegistry, _args: &[String]) -> Result<String, String> {
    stubs.project().repository_handler().google();
    Ok("google".to_string())
}

fn plugin_id(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("id requires a plugin ID".to_string());
    }
    let spec = stubs.project().plugin_manager().id(&args[0]);
    // Apply version if provided
    if args.len() > 1 {
        spec.version(&args[1]).finalize();
    } else {
        spec.finalize();
    }
    Ok(args[0].clone())
}

fn plugin_apply(stubs: &StubRegistry, args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err("apply requires a plugin ID".to_string());
    }
    stubs.project().plugin_manager().apply(&args[0]);
    Ok("".to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a fresh stub registry
    fn create_stubs() -> StubRegistry {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        StubRegistry::new(config)
    }

    // Project Stub Tests

    #[test]
    fn test_project_name() {
        let stubs = create_stubs();
        assert_eq!(stubs.project().get_name(), "unnamed");

        stubs.project().set_name("my-project");
        assert_eq!(stubs.project().get_name(), "my-project");
    }

    #[test]
    fn test_project_version() {
        let stubs = create_stubs();
        assert_eq!(stubs.project().get_version(), "unspecified");

        stubs.project().set_version("1.0.0");
        assert_eq!(stubs.project().get_version(), "1.0.0");
    }

    #[test]
    fn test_project_group() {
        let stubs = create_stubs();
        assert_eq!(stubs.project().get_group(), "");

        stubs.project().set_group("com.example");
        assert_eq!(stubs.project().get_group(), "com.example");
    }

    #[test]
    fn test_project_properties() {
        let stubs = create_stubs();
        stubs.project().set_property("custom.key", "custom.value");
        assert_eq!(
            stubs.project().get_property("custom.key"),
            Some("custom.value".to_string())
        );
    }

    #[test]
    fn test_project_apply_plugin() {
        let stubs = create_stubs();
        stubs.project().apply_plugin("java");

        let config = stubs.get_build_config();
        assert!(config.has_plugin("java"));
    }

    // Dependency Handler Tests

    #[test]
    fn test_add_implementation_dependency() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .implementation("com.example:lib:1.0");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group, "com.example");
        assert_eq!(deps[0].artifact, "lib");
        assert_eq!(deps[0].version, "1.0");
    }

    #[test]
    fn test_add_api_dependency() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .api("org.jetbrains:annotations:24.0.0");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("api");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group, "org.jetbrains");
    }

    #[test]
    fn test_add_test_dependency() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .test_implementation("org.junit.jupiter:junit-jupiter:5.9.0");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("testImplementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].artifact, "junit-jupiter");
    }

    #[test]
    fn test_add_multiple_dependencies() {
        let stubs = create_stubs();
        let handler = stubs.project().dependency_handler();

        handler.implementation("com.google.guava:guava:31.0");
        handler.implementation("org.slf4j:slf4j-api:2.0.0");
        handler.test_implementation("org.mockito:mockito-core:4.0.0");

        let config = stubs.get_build_config();
        assert_eq!(config.get_dependencies("implementation").len(), 2);
        assert_eq!(config.get_dependencies("testImplementation").len(), 1);
    }

    #[test]
    fn test_project_dependency() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .project("implementation", ":submodule");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert!(deps[0].is_project);
    }

    #[test]
    fn test_generic_add_dependency() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .add("customConfig", "custom:dep:1.0");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("customConfig");
        assert_eq!(deps.len(), 1);
    }

    // Repository Handler Tests

    #[test]
    fn test_maven_central() {
        let stubs = create_stubs();
        stubs.project().repository_handler().maven_central();

        let config = stubs.get_build_config();
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_maven_local() {
        let stubs = create_stubs();
        stubs.project().repository_handler().maven_local();

        let config = stubs.get_build_config();
        assert!(config.repositories.contains(&"mavenLocal".to_string()));
    }

    #[test]
    fn test_google_repository() {
        let stubs = create_stubs();
        stubs.project().repository_handler().google();

        let config = stubs.get_build_config();
        assert!(config.repositories.contains(&"google".to_string()));
    }

    #[test]
    fn test_jcenter_repository() {
        let stubs = create_stubs();
        stubs.project().repository_handler().jcenter();

        let config = stubs.get_build_config();
        assert!(config.repositories.contains(&"jcenter".to_string()));
    }

    #[test]
    fn test_multiple_repositories() {
        let stubs = create_stubs();
        let handler = stubs.project().repository_handler();

        handler.maven_central();
        handler.google();
        handler.maven_local();

        let config = stubs.get_build_config();
        assert_eq!(config.repository_list.len(), 3);
    }

    #[test]
    fn test_custom_maven_repository() {
        let stubs = create_stubs();
        stubs
            .project()
            .repository_handler()
            .maven_named("custom", "https://example.com/repo");

        let config = stubs.get_build_config();
        let repo = config
            .repository_list
            .iter()
            .find(|r| r.name == "custom")
            .unwrap();
        assert_eq!(repo.url, "https://example.com/repo");
    }

    // Plugin Manager Tests

    #[test]
    fn test_apply_java_plugin() {
        let stubs = create_stubs();
        stubs.project().plugin_manager().apply("java");

        let config = stubs.get_build_config();
        assert!(config.has_plugin("java"));
    }

    #[test]
    fn test_plugin_with_version() {
        let stubs = create_stubs();
        let spec = stubs
            .project()
            .plugin_manager()
            .id("org.jetbrains.kotlin.jvm");
        spec.version("1.9.0").finalize();

        let config = stubs.get_build_config();
        assert!(config.has_plugin("org.jetbrains.kotlin.jvm"));

        let plugin = config
            .plugin_definitions
            .iter()
            .find(|p| p.id == "org.jetbrains.kotlin.jvm")
            .unwrap();
        assert_eq!(plugin.version, Some("1.9.0".to_string()));
    }

    #[test]
    fn test_kotlin_plugin_shorthand() {
        let stubs = create_stubs();
        let spec = stubs.project().plugin_manager().kotlin("jvm");
        spec.version("1.9.0").finalize();

        let config = stubs.get_build_config();
        assert!(config.has_plugin("org.jetbrains.kotlin.jvm"));
    }

    #[test]
    fn test_plugin_not_applied() {
        let stubs = create_stubs();
        let spec = stubs.project().plugin_manager().id("java-library");
        spec.apply(false).finalize();

        let config = stubs.get_build_config();
        // Plugin should not be in the applied list because apply=false
        // Note: The Drop impl will still try to finalize, but the apply=false prevents it
        assert!(!config.has_plugin("java-library"));
    }

    // Task Stub Tests

    #[test]
    fn test_create_task() {
        let stubs = create_stubs();
        stubs.project().task("myTask", None);

        let config = stubs.get_build_config();
        assert!(config.tasks.contains(&"myTask".to_string()));
    }

    #[test]
    fn test_typed_task() {
        let stubs = create_stubs();
        stubs.project().task_typed("copyDocs", "Copy", None);

        let config = stubs.get_build_config();
        let task = config
            .task_definitions
            .iter()
            .find(|t| t.name == "copyDocs")
            .unwrap();
        assert_eq!(task.task_type, Some("Copy".to_string()));
    }

    #[test]
    fn test_task_configuration() {
        let stubs = create_stubs();
        let task = stubs.project().task("build", None);
        task.group("Build")
            .description("Builds the project")
            .depends_on(&["compile", "test"]);

        let config = stubs.get_build_config();
        assert_eq!(
            config.get_property("task:build:group"),
            Some(&"Build".to_string())
        );
        assert_eq!(
            config.get_property("task:build:dependsOn"),
            Some(&"compile,test".to_string())
        );
    }

    // Configuration Stub Tests

    #[test]
    fn test_create_configuration() {
        let stubs = create_stubs();
        let mut config_stub = stubs.create_configuration("myConfig");
        config_stub
            .extends_from(&["implementation"])
            .transitive(true);

        let config = stubs.get_build_config();
        assert_eq!(
            config.get_property("config:myConfig:extends"),
            Some(&"implementation".to_string())
        );
    }

    // Stub Registry Tests

    #[test]
    fn test_stub_registry_reset() {
        let stubs = create_stubs();
        stubs.project().set_name("test-project");
        stubs
            .project()
            .dependency_handler()
            .implementation("com.example:lib:1.0");

        assert_eq!(stubs.project().get_name(), "test-project");
        assert_eq!(stubs.get_build_config().dependency_list.len(), 1);

        stubs.reset();

        assert_eq!(stubs.project().get_name(), "unnamed");
        assert_eq!(stubs.get_build_config().dependency_list.len(), 0);
    }

    // Stub Method Registry Tests

    #[test]
    fn test_stub_method_registry_invoke() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let registry = StubMethodRegistry::new(config);

        let result = registry.invoke("Project.getName", &[]);
        assert_eq!(result, Ok("unnamed".to_string()));

        let result = registry.invoke("Project.setVersion", &["2.0.0".to_string()]);
        assert!(result.is_ok());

        let result = registry.invoke("Project.getVersion", &[]);
        assert_eq!(result, Ok("2.0.0".to_string()));
    }

    #[test]
    fn test_stub_method_registry_dependencies() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let registry = StubMethodRegistry::new(config);

        let result = registry.invoke(
            "DependencyHandler.implementation",
            &["com.example:lib:1.0".to_string()],
        );
        assert!(result.is_ok());

        let build_config = registry.stubs().get_build_config();
        assert_eq!(build_config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_stub_method_registry_repositories() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let registry = StubMethodRegistry::new(config);

        let result = registry.invoke("RepositoryHandler.mavenCentral", &[]);
        assert_eq!(result, Ok("mavenCentral".to_string()));

        let build_config = registry.stubs().get_build_config();
        assert!(build_config
            .repositories
            .contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_stub_method_registry_plugins() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let registry = StubMethodRegistry::new(config);

        let result = registry.invoke("PluginManager.apply", &["java".to_string()]);
        assert!(result.is_ok());

        let build_config = registry.stubs().get_build_config();
        assert!(build_config.has_plugin("java"));
    }

    #[test]
    fn test_stub_method_registry_unknown_method() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let registry = StubMethodRegistry::new(config);

        let result = registry.invoke("Unknown.method", &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown stub method"));
    }

    // Integration Tests

    #[test]
    fn test_typical_gradle_build_script() {
        let stubs = create_stubs();

        // Simulate: group = 'com.example'
        stubs.project().set_group("com.example");

        // Simulate: version = '1.0.0'
        stubs.project().set_version("1.0.0");

        // Simulate: plugins { id 'java' }
        stubs.project().plugin_manager().apply("java");

        // Simulate: repositories { mavenCentral() }
        stubs.project().repository_handler().maven_central();

        // Simulate: dependencies { ... }
        let deps = stubs.project().dependency_handler();
        deps.implementation("com.google.guava:guava:31.0.1-jre");
        deps.test_implementation("org.junit.jupiter:junit-jupiter:5.9.0");

        // Verify extracted configuration
        let config = stubs.get_build_config();
        assert_eq!(config.group, Some("com.example".to_string()));
        assert_eq!(config.version, Some("1.0.0".to_string()));
        assert!(config.has_plugin("java"));
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("testImplementation").len(), 1);
    }

    #[test]
    fn test_kotlin_android_build_script() {
        let stubs = create_stubs();

        // Android/Kotlin project setup
        stubs
            .project()
            .plugin_manager()
            .apply("com.android.application");
        stubs
            .project()
            .plugin_manager()
            .apply("org.jetbrains.kotlin.android");

        let repos = stubs.project().repository_handler();
        repos.google();
        repos.maven_central();

        let deps = stubs.project().dependency_handler();
        deps.implementation("androidx.core:core-ktx:1.9.0");
        deps.implementation("org.jetbrains.kotlin:kotlin-stdlib:1.8.0");
        deps.test_implementation("junit:junit:4.13.2");

        let config = stubs.get_build_config();
        assert!(config.is_kotlin_project());
        assert!(config.is_android_project());
        assert_eq!(config.repository_list.len(), 2);
        assert_eq!(config.get_dependencies("implementation").len(), 2);
    }

    #[test]
    fn test_spring_boot_build_script() {
        let stubs = create_stubs();

        // Spring Boot project setup
        stubs.project().set_group("com.example");
        stubs.project().set_version("0.0.1-SNAPSHOT");

        // Note: The actual plugin ID contains "spring-boot" which is_spring_boot_project() checks for
        stubs
            .project()
            .plugin_manager()
            .apply("org.springframework.boot.spring-boot");
        stubs
            .project()
            .plugin_manager()
            .apply("io.spring.dependency-management");
        stubs.project().plugin_manager().apply("java");

        stubs.project().repository_handler().maven_central();

        let deps = stubs.project().dependency_handler();
        deps.implementation("org.springframework.boot:spring-boot-starter-web:3.0.0");
        deps.implementation("org.springframework.boot:spring-boot-starter-data-jpa:3.0.0");
        deps.runtime_only("org.postgresql:postgresql:42.5.0");
        deps.test_implementation("org.springframework.boot:spring-boot-starter-test:3.0.0");

        let config = stubs.get_build_config();
        assert!(config.is_spring_boot_project());
        assert_eq!(config.get_dependencies("implementation").len(), 2);
        assert_eq!(config.get_dependencies("runtimeOnly").len(), 1);
        assert_eq!(config.get_dependencies("testImplementation").len(), 1);
    }

    // Closure Reference Tests

    #[test]
    fn test_closure_ref_creation() {
        let closure = ClosureRef::new(42);
        assert_eq!(closure.id, 42);
        assert!(closure.delegate_type.is_none());
        assert!(!closure.executed);
    }

    #[test]
    fn test_closure_ref_with_delegate() {
        let closure = ClosureRef::with_delegate(1, "DependencyHandler");
        assert_eq!(closure.delegate_type, Some("DependencyHandler".to_string()));
    }

    // Edge Cases

    #[test]
    fn test_empty_dependency_notation() {
        let stubs = create_stubs();
        let result = stubs
            .project()
            .dependency_handler()
            .add("implementation", "");
        assert!(result.is_none());
    }

    #[test]
    fn test_versionless_dependency_notation_is_recorded() {
        let stubs = create_stubs();
        // Gradle accepts group:artifact at declaration. A platform supplies the version,
        // and a missing one fails at resolution, not here.
        let result = stubs
            .project()
            .dependency_handler()
            .add("implementation", "com.example:lib");
        assert!(result.is_some());
    }

    #[test]
    fn test_project_call_is_not_a_dependency_notation() {
        let stubs = create_stubs();
        let result = stubs
            .project()
            .dependency_handler()
            .add("implementation", "project(':lib')");
        assert!(result.is_none());
    }

    #[test]
    fn test_duplicate_plugin_application() {
        let stubs = create_stubs();
        stubs.project().plugin_manager().apply("java");
        stubs.project().plugin_manager().apply("java");

        let config = stubs.get_build_config();
        // Should not have duplicates in the plugins list
        let java_count = config.plugins.iter().filter(|p| *p == "java").count();
        assert_eq!(java_count, 1);
    }

    #[test]
    fn test_dependency_with_classifier() {
        let stubs = create_stubs();
        stubs
            .project()
            .dependency_handler()
            .implementation("com.example:lib:1.0:sources");

        let config = stubs.get_build_config();
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps[0].classifier, Some("sources".to_string()));
    }
}
