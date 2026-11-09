// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle integration for bytecode execution
//!
//! This module provides:
//! - Plugin registry for Gradle plugin introspection
//! - Sandbox environment for safe execution of Gradle scripts
//! - Configuration extraction from execution results
//! - Security policies for sandboxed execution

use crate::interpreter::{Heap, Thread};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Shared Gradle user-home discovery for artifact resolution and sandbox access.
pub fn gradle_user_home() -> PathBuf {
    resolve_gradle_user_home(
        dirs::home_dir(),
        std::env::var_os("GRADLE_USER_HOME").map(PathBuf::from),
    )
}

fn resolve_gradle_user_home(home: Option<PathBuf>, configured: Option<PathBuf>) -> PathBuf {
    configured
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.unwrap_or_else(|| PathBuf::from(".")).join(".gradle"))
}

static PROPERTY_INTERPOLATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\$\{(\w+)\}"#).expect("valid interpolation regex"));

// ============================================================================
// Plugin Registry
// ============================================================================

/// Gradle plugin metadata
#[derive(Debug, Clone)]
pub struct GradlePlugin {
    /// Plugin ID (e.g., "org.jetbrains.kotlin.jvm")
    pub id: String,

    /// Display name
    pub name: String,

    /// Methods added to Project when applied
    pub project_methods: Vec<String>,

    /// Extensions added to Project
    pub extensions: Vec<String>,

    /// Task types contributed
    pub task_types: Vec<String>,
}

/// Gradle extension metadata
#[derive(Debug, Clone)]
pub struct GradleExtension {
    /// Extension name (e.g., "android", "kotlin")
    pub name: String,

    /// Type name for delegate in closures
    pub delegate_type: String,

    /// Nested extensions
    pub nested: Vec<String>,
}

/// Registry of known Gradle plugins and their metadata
pub struct PluginRegistry {
    /// Plugin ID -> Plugin metadata
    plugins: HashMap<String, GradlePlugin>,

    /// Extension name -> Extension metadata
    extensions: HashMap<String, GradleExtension>,

    /// Core Gradle methods available on Project
    core_methods: Vec<String>,

    /// Common configurations (implementation, api, etc.)
    configurations: Vec<String>,
}

impl PluginRegistry {
    /// Create a new plugin registry with core Gradle API
    pub fn new() -> Self {
        let mut registry = Self {
            plugins: HashMap::new(),
            extensions: HashMap::new(),
            core_methods: Vec::new(),
            configurations: Vec::new(),
        };

        // Register core Gradle methods
        registry.core_methods = vec![
            "apply".to_string(),
            "dependencies".to_string(),
            "repositories".to_string(),
            "configurations".to_string(),
            "tasks".to_string(),
            "plugins".to_string(),
            "allprojects".to_string(),
            "subprojects".to_string(),
            "group".to_string(),
            "version".to_string(),
            "description".to_string(),
        ];

        // Register common configurations
        registry.configurations = vec![
            "implementation".to_string(),
            "api".to_string(),
            "compileOnly".to_string(),
            "runtimeOnly".to_string(),
            "testImplementation".to_string(),
            "testCompileOnly".to_string(),
            "testRuntimeOnly".to_string(),
            "annotationProcessor".to_string(),
            "classpath".to_string(),
        ];

        registry
    }

    /// Register a plugin
    pub fn register_plugin(&mut self, plugin: GradlePlugin) {
        self.plugins.insert(plugin.id.clone(), plugin);
    }

    /// Register an extension
    pub fn register_extension(&mut self, extension: GradleExtension) {
        self.extensions.insert(extension.name.clone(), extension);
    }

    /// Get a plugin by ID
    pub fn get_plugin(&self, id: &str) -> Option<&GradlePlugin> {
        self.plugins.get(id)
    }

    /// Get all registered plugins
    pub fn plugins(&self) -> impl Iterator<Item = (&String, &GradlePlugin)> {
        self.plugins.iter()
    }

    /// Get an extension by name
    pub fn get_extension(&self, name: &str) -> Option<&GradleExtension> {
        self.extensions.get(name)
    }

    /// Check if a method is available on Project
    pub fn is_project_method(&self, method: &str) -> bool {
        self.core_methods.contains(&method.to_string())
    }

    /// Get all configurations
    pub fn configurations(&self) -> &[String] {
        &self.configurations
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Security Policy
// ============================================================================

/// Security policy for sandboxed Gradle script execution
///
/// Controls resource limits and access permissions to prevent
/// malicious or runaway scripts from causing harm.
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    /// Maximum bytecode instructions to execute
    pub instruction_limit: u64,

    /// Maximum heap memory (bytes)
    pub memory_limit: usize,

    /// Maximum call stack depth
    pub stack_depth_limit: usize,

    /// Maximum execution time
    pub time_limit: Duration,

    /// Paths allowed for file reading
    pub allowed_read_paths: HashSet<PathBuf>,

    /// Paths allowed for file writing
    pub allowed_write_paths: HashSet<PathBuf>,

    /// Whether network access is permitted
    pub allow_network: bool,

    /// Whether process execution is permitted
    pub allow_process_exec: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            instruction_limit: 10_000_000,       // 10M instructions
            memory_limit: 256 * 1024 * 1024,     // 256MB
            stack_depth_limit: 1024,             // 1K frames
            time_limit: Duration::from_secs(30), // 30 seconds
            allowed_read_paths: HashSet::new(),  // No file reads by default
            allowed_write_paths: HashSet::new(), // No file writes by default
            allow_network: false,                // No network by default
            allow_process_exec: false,           // No exec by default
        }
    }
}

impl SecurityPolicy {
    /// Create a permissive policy for trusted scripts
    pub fn permissive() -> Self {
        Self {
            instruction_limit: 100_000_000,       // 100M instructions
            memory_limit: 1024 * 1024 * 1024,     // 1GB
            stack_depth_limit: 4096,              // 4K frames
            time_limit: Duration::from_secs(300), // 5 minutes
            allowed_read_paths: HashSet::new(),   // Configured per-use
            allowed_write_paths: HashSet::new(),  // Configured per-use
            allow_network: true,                  // Allow network
            allow_process_exec: true,             // Allow exec
        }
    }

    /// Create a strict policy for untrusted scripts
    pub fn strict() -> Self {
        Self {
            instruction_limit: 1_000_000,        // 1M instructions
            memory_limit: 64 * 1024 * 1024,      // 64MB
            stack_depth_limit: 256,              // 256 frames
            time_limit: Duration::from_secs(5),  // 5 seconds
            allowed_read_paths: HashSet::new(),  // No file access
            allowed_write_paths: HashSet::new(), // No file access
            allow_network: false,                // No network
            allow_process_exec: false,           // No exec
        }
    }

    /// Create a restrictive security policy for Gradle execution
    /// (Legacy compatibility method)
    pub fn gradle_sandbox() -> Self {
        Self {
            instruction_limit: 10_000_000,
            memory_limit: 512 * 1024 * 1024, // 512MB
            stack_depth_limit: 1024,
            time_limit: Duration::from_secs(30),
            allowed_read_paths: HashSet::new(),
            allowed_write_paths: HashSet::new(),
            allow_network: false,
            allow_process_exec: false,
        }
    }

    /// Allow reading from a specific path
    pub fn allow_read_path(&mut self, path: PathBuf) {
        self.allowed_read_paths.insert(path);
    }

    /// Allow writing to a specific path
    pub fn allow_write_path(&mut self, path: PathBuf) {
        self.allowed_write_paths.insert(path);
    }

    /// Check if reading from a path is allowed
    pub fn can_read(&self, path: &Path) -> bool {
        if self.allowed_read_paths.is_empty() {
            return false;
        }
        self.allowed_read_paths
            .iter()
            .any(|allowed| path.starts_with(allowed))
    }

    /// Check if writing to a path is allowed
    pub fn can_write(&self, path: &Path) -> bool {
        if self.allowed_write_paths.is_empty() {
            return false;
        }
        self.allowed_write_paths
            .iter()
            .any(|allowed| path.starts_with(allowed))
    }
}

// ============================================================================
// Build Configuration Types
// ============================================================================

/// Gradle dependency with full metadata
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Configuration name (implementation, api, etc.)
    pub configuration: String,
    /// Group ID
    pub group: String,
    /// Artifact ID
    pub artifact: String,
    /// Version (may include version ranges)
    pub version: String,
    /// Optional classifier (sources, javadoc, etc.)
    pub classifier: Option<String>,
    /// Optional extension (jar, aar, etc.)
    pub extension: Option<String>,
    /// Whether this is a project dependency
    pub is_project: bool,
    /// Transitive dependency behavior
    pub transitive: bool,
    /// Declared with `enforcedPlatform`: its versions override the rest of the graph
    pub enforced: bool,
}

impl Dependency {
    /// Create a new dependency
    pub fn new(configuration: &str, group: &str, artifact: &str, version: &str) -> Self {
        Self {
            configuration: configuration.to_string(),
            group: group.to_string(),
            artifact: artifact.to_string(),
            version: version.to_string(),
            classifier: None,
            extension: None,
            is_project: false,
            transitive: true,
            enforced: false,
        }
    }

    /// Create from Maven coordinate notation (group:artifact:version)
    pub fn parse(configuration: &str, notation: &str) -> Option<Self> {
        let parts: Vec<&str> = notation.split(':').collect();
        // `group:artifact` names no version: a platform or BOM supplies one, and the
        // build parser resolves it the same way. A call such as project(':a') is not a
        // coordinate.
        if parts.len() == 2 && !notation.contains('(') {
            return Some(Self::new(configuration, parts[0], parts[1], ""));
        }
        if parts.len() >= 3 {
            let mut dep = Self::new(configuration, parts[0], parts[1], parts[2]);
            if parts.len() >= 4 {
                dep.classifier = Some(parts[3].to_string());
            }
            if parts.len() >= 5 {
                dep.extension = Some(parts[4].to_string());
            }
            Some(dep)
        } else {
            None
        }
    }

    /// Convert to Maven coordinate notation
    pub fn to_notation(&self) -> String {
        let mut notation = format!("{}:{}:{}", self.group, self.artifact, self.version);
        if let Some(ref classifier) = self.classifier {
            notation.push(':');
            notation.push_str(classifier);
        }
        if let Some(ref ext) = self.extension {
            notation.push('@');
            notation.push_str(ext);
        }
        notation
    }
}

/// Gradle dependency (simplified - for backward compatibility)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleDependency {
    /// Group ID
    pub group: String,
    /// Artifact ID
    pub artifact: String,
    /// Version
    pub version: String,
}

impl GradleDependency {
    /// Parse Maven coordinate notation
    pub fn parse(notation: &str) -> Option<Self> {
        let parts: Vec<&str> = notation.split(':').collect();
        if parts.len() >= 3 {
            Some(GradleDependency {
                group: parts[0].to_string(),
                artifact: parts[1].to_string(),
                version: parts[2].to_string(),
            })
        } else {
            None
        }
    }

    /// Convert to Maven coordinate notation
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        format!("{}:{}:{}", self.group, self.artifact, self.version)
    }
}

/// Repository configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    /// Repository name
    pub name: String,
    /// Repository URL
    pub url: String,
    /// Repository type
    pub repo_type: RepositoryType,
}

/// Repository type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryType {
    Maven,
    Ivy,
    FlatDir,
    MavenLocal,
    MavenCentral,
    Google,
    GradlePluginPortal,
}

impl Repository {
    /// Create Maven Central repository
    pub fn maven_central() -> Self {
        Self {
            name: "mavenCentral".to_string(),
            url: "https://repo.maven.apache.org/maven2/".to_string(),
            repo_type: RepositoryType::MavenCentral,
        }
    }

    /// Create Maven Local repository
    pub fn maven_local() -> Self {
        Self {
            name: "mavenLocal".to_string(),
            url: "file://~/.m2/repository".to_string(),
            repo_type: RepositoryType::MavenLocal,
        }
    }

    /// Create Google Maven repository
    pub fn google() -> Self {
        Self {
            name: "google".to_string(),
            url: "https://dl.google.com/dl/android/maven2/".to_string(),
            repo_type: RepositoryType::Google,
        }
    }

    /// Create Gradle Plugin Portal repository
    pub fn gradle_plugin_portal() -> Self {
        Self {
            name: "gradlePluginPortal".to_string(),
            url: "https://plugins.gradle.org/m2/".to_string(),
            repo_type: RepositoryType::GradlePluginPortal,
        }
    }
}

/// Plugin application
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// Plugin ID
    pub id: String,
    /// Plugin version (if specified)
    pub version: Option<String>,
    /// Whether the plugin is applied
    pub apply: bool,
}

impl Plugin {
    /// Create a new plugin
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            version: None,
            apply: true,
        }
    }

    /// Create a plugin with version
    pub fn with_version(id: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            version: Some(version.to_string()),
            apply: true,
        }
    }
}

/// Task definition
#[derive(Debug, Clone)]
pub struct Task {
    /// Task name
    pub name: String,
    /// Task type (if typed)
    pub task_type: Option<String>,
    /// Task group
    pub group: Option<String>,
    /// Task description
    pub description: Option<String>,
    /// Task dependencies (dependsOn)
    pub depends_on: Vec<String>,
    /// Finalized by tasks
    pub finalized_by: Vec<String>,
    /// Must run after tasks
    pub must_run_after: Vec<String>,
    /// Should run after tasks
    pub should_run_after: Vec<String>,
}

impl Task {
    /// Create a new task
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            task_type: None,
            group: None,
            description: None,
            depends_on: Vec::new(),
            finalized_by: Vec::new(),
            must_run_after: Vec::new(),
            should_run_after: Vec::new(),
        }
    }

    /// Create a typed task
    pub fn typed(name: &str, task_type: &str) -> Self {
        Self {
            name: name.to_string(),
            task_type: Some(task_type.to_string()),
            group: None,
            description: None,
            depends_on: Vec::new(),
            finalized_by: Vec::new(),
            must_run_after: Vec::new(),
            should_run_after: Vec::new(),
        }
    }
}

/// Source set configuration
#[derive(Debug, Clone)]
pub struct SourceSet {
    /// Source set name (main, test, etc.)
    pub name: String,
    /// Java source directories
    pub java_src_dirs: Vec<PathBuf>,
    /// Kotlin source directories
    pub kotlin_src_dirs: Vec<PathBuf>,
    /// Groovy source directories
    pub groovy_src_dirs: Vec<PathBuf>,
    /// Resource directories
    pub resources_dirs: Vec<PathBuf>,
    /// Output directory
    pub output_dir: Option<PathBuf>,
}

impl SourceSet {
    /// Create a new source set with default directories
    pub fn new(name: &str, base_dir: &Path) -> Self {
        let src_base = base_dir.join("src").join(name);
        Self {
            name: name.to_string(),
            java_src_dirs: vec![src_base.join("java")],
            kotlin_src_dirs: vec![src_base.join("kotlin")],
            groovy_src_dirs: vec![src_base.join("groovy")],
            resources_dirs: vec![src_base.join("resources")],
            output_dir: Some(base_dir.join("build").join("classes").join(name)),
        }
    }

    /// Create default main source set
    pub fn main(base_dir: &Path) -> Self {
        Self::new("main", base_dir)
    }

    /// Create default test source set
    pub fn test(base_dir: &Path) -> Self {
        Self::new("test", base_dir)
    }
}

// ============================================================================
// Build Configuration
// ============================================================================

/// Gradle build configuration extracted from execution
#[derive(Debug, Clone, Default)]
pub struct GradleBuildConfig {
    /// Project name
    pub project_name: Option<String>,

    /// Project group
    pub group: Option<String>,

    /// Project version
    pub version: Option<String>,

    /// Project description
    pub description: Option<String>,

    /// Project base directory
    pub project_dir: Option<PathBuf>,

    /// Build directory
    pub build_dir: Option<PathBuf>,

    /// Applied plugins (by ID)
    pub plugins: Vec<String>,

    /// Full plugin definitions
    pub plugin_definitions: Vec<Plugin>,

    /// Dependencies by configuration (legacy format)
    pub dependencies: HashMap<String, Vec<GradleDependency>>,

    /// Full dependency definitions
    pub dependency_list: Vec<Dependency>,

    /// Repositories
    pub repositories: Vec<String>,

    /// Full repository definitions
    pub repository_list: Vec<Repository>,

    /// Registered tasks
    pub tasks: Vec<String>,

    /// Full task definitions
    pub task_definitions: Vec<Task>,

    /// Source sets
    pub source_sets: HashMap<String, SourceSet>,

    /// Custom properties
    pub properties: HashMap<String, String>,

    /// Extra properties (ext block)
    pub extra_properties: HashMap<String, String>,

    /// Java/Kotlin toolchain version
    pub java_version: Option<String>,

    /// Kotlin version
    pub kotlin_version: Option<String>,

    /// Subprojects
    pub subprojects: Vec<String>,

    /// Whether this is a multi-project build
    pub is_multi_project: bool,
}

impl GradleBuildConfig {
    /// Create a new empty build configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with project name and directory
    pub fn with_project(name: &str, dir: PathBuf) -> Self {
        let mut config = Self::new();
        config.project_name = Some(name.to_string());
        config.project_dir = Some(dir.clone());
        config.build_dir = Some(dir.join("build"));
        config
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, dep: Dependency) {
        // Add to legacy format
        let gradle_dep = GradleDependency {
            group: dep.group.clone(),
            artifact: dep.artifact.clone(),
            version: dep.version.clone(),
        };
        self.dependencies
            .entry(dep.configuration.clone())
            .or_default()
            .push(gradle_dep);

        // Add to new format
        self.dependency_list.push(dep);
    }

    /// Get dependencies for a configuration
    pub fn get_dependencies(&self, configuration: &str) -> Vec<&Dependency> {
        self.dependency_list
            .iter()
            .filter(|d| d.configuration == configuration)
            .collect()
    }

    /// Get all configurations that have dependencies
    pub fn configurations(&self) -> Vec<&str> {
        let mut configs: Vec<&str> = self
            .dependency_list
            .iter()
            .map(|d| d.configuration.as_str())
            .collect();
        configs.sort();
        configs.dedup();
        configs
    }

    /// Add a repository
    pub fn add_repository(&mut self, repo: Repository) {
        self.repositories.push(repo.name.clone());
        self.repository_list.push(repo);
    }

    /// Add a plugin
    pub fn add_plugin(&mut self, plugin: Plugin) {
        // `apply false` declares the plugin without applying it to this project.
        if plugin.apply && !self.plugins.contains(&plugin.id) {
            self.plugins.push(plugin.id.clone());
        }
        self.plugin_definitions.push(plugin);
    }

    /// Add a task
    pub fn add_task(&mut self, task: Task) {
        if !self.tasks.contains(&task.name) {
            self.tasks.push(task.name.clone());
        }
        self.task_definitions.push(task);
    }

    /// Add a source set
    pub fn add_source_set(&mut self, source_set: SourceSet) {
        self.source_sets.insert(source_set.name.clone(), source_set);
    }

    /// Set a property
    pub fn set_property(&mut self, key: &str, value: &str) {
        self.properties.insert(key.to_string(), value.to_string());
    }

    /// Set an extra property (ext block)
    pub fn set_extra_property(&mut self, key: &str, value: &str) {
        self.extra_properties
            .insert(key.to_string(), value.to_string());
    }

    /// Get a property
    pub fn get_property(&self, key: &str) -> Option<&String> {
        self.properties
            .get(key)
            .or_else(|| self.extra_properties.get(key))
    }

    /// Interpolate GString expressions like ${varName} using available properties
    pub fn interpolate(&self, input: &str) -> String {
        if !input.contains("${") {
            return input.to_string();
        }

        let mut result = String::with_capacity(input.len());
        let mut last_end = 0usize;

        for captures in PROPERTY_INTERPOLATION_RE.captures_iter(input) {
            let Some(full_match) = captures.get(0) else {
                continue;
            };
            let Some(var_name) = captures.get(1) else {
                continue;
            };

            result.push_str(&input[last_end..full_match.start()]);
            if let Some(value) = self.get_property(var_name.as_str()) {
                result.push_str(value);
            } else {
                result.push_str(full_match.as_str());
            }
            last_end = full_match.end();
        }

        result.push_str(&input[last_end..]);
        result
    }

    /// Check if a plugin is applied
    pub fn has_plugin(&self, id: &str) -> bool {
        self.plugins.contains(&id.to_string())
    }

    /// Check if this is a Kotlin project
    pub fn is_kotlin_project(&self) -> bool {
        self.plugins.iter().any(|p| p.contains("kotlin"))
    }

    /// Check if this is an Android project
    pub fn is_android_project(&self) -> bool {
        self.plugins.iter().any(|p| p.contains("android"))
    }

    /// Check if this is a Spring Boot project
    pub fn is_spring_boot_project(&self) -> bool {
        self.plugins
            .iter()
            .any(|p| p.contains("spring-boot") || p.contains("springframework.boot"))
    }
}

// ============================================================================
// Sandbox Execution
// ============================================================================

/// Result of sandboxed execution
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// Whether execution completed successfully
    pub success: bool,

    /// Error message if execution failed
    pub error: Option<String>,

    /// Number of instructions executed
    pub instructions_executed: u64,

    /// Peak memory usage (bytes)
    pub peak_memory: usize,

    /// Execution duration
    pub execution_time: Duration,

    /// Maximum stack depth reached
    pub max_stack_depth: usize,

    /// Return value (if any)
    pub return_value: Option<String>,
}

impl SandboxResult {
    /// Create a successful result
    pub fn success(instructions: u64, memory: usize, time: Duration, depth: usize) -> Self {
        Self {
            success: true,
            error: None,
            instructions_executed: instructions,
            peak_memory: memory,
            execution_time: time,
            max_stack_depth: depth,
            return_value: None,
        }
    }

    /// Create a failed result
    pub fn failure(error: String, instructions: u64, memory: usize, time: Duration) -> Self {
        Self {
            success: false,
            error: Some(error),
            instructions_executed: instructions,
            peak_memory: memory,
            execution_time: time,
            max_stack_depth: 0,
            return_value: None,
        }
    }
}

/// Sandbox environment for executing Gradle scripts safely
///
/// Uses the bytecode interpreter with resource limits to safely
/// execute Gradle build scripts for configuration extraction.
pub struct GradleSandbox {
    /// Plugin registry
    plugin_registry: PluginRegistry,

    /// Security policy
    security_policy: SecurityPolicy,

    /// Build configuration extracted from execution
    pub build_config: GradleBuildConfig,

    /// Gradle home directory
    gradle_home: PathBuf,

    /// Execution thread
    thread: Option<Thread>,

    /// Heap storage
    heap: Option<Heap>,
}

impl GradleSandbox {
    /// Create a new Gradle sandbox
    pub fn new(gradle_home: PathBuf) -> Self {
        Self {
            plugin_registry: PluginRegistry::new(),
            security_policy: SecurityPolicy::gradle_sandbox(),
            build_config: GradleBuildConfig::default(),
            gradle_home,
            thread: None,
            heap: None,
        }
    }

    /// Create with custom security policy
    pub fn with_policy(gradle_home: PathBuf, policy: SecurityPolicy) -> Self {
        Self {
            plugin_registry: PluginRegistry::new(),
            security_policy: policy,
            build_config: GradleBuildConfig::default(),
            gradle_home,
            thread: None,
            heap: None,
        }
    }

    /// Get plugin registry
    pub fn plugins(&self) -> &PluginRegistry {
        &self.plugin_registry
    }

    /// Get mutable plugin registry
    pub fn plugins_mut(&mut self) -> &mut PluginRegistry {
        &mut self.plugin_registry
    }

    /// Get security policy
    pub fn security(&self) -> &SecurityPolicy {
        &self.security_policy
    }

    /// Get mutable security policy
    pub fn security_mut(&mut self) -> &mut SecurityPolicy {
        &mut self.security_policy
    }

    /// Get gradle home directory
    pub fn gradle_home(&self) -> &PathBuf {
        &self.gradle_home
    }

    /// Initialize execution environment
    fn init_execution(&mut self) {
        let step_limit = Some(self.security_policy.instruction_limit);
        self.thread = Some(Thread::new(
            self.security_policy.stack_depth_limit,
            step_limit,
        ));
        self.heap = Some(Heap::new(self.security_policy.memory_limit));
    }

    /// Execute bytecode in the sandbox
    ///
    /// Returns execution statistics and any errors that occurred.
    pub fn execute(&mut self, bytecode: &[u8]) -> SandboxResult {
        self.init_execution();

        let start_time = Instant::now();
        let thread = self.thread.as_mut().expect("Thread not initialized");
        let heap = self.heap.as_ref().expect("Heap not initialized");

        // Check time limit periodically during execution
        let time_limit = self.security_policy.time_limit;

        // For now, we just validate the bytecode structure
        // Full execution would require setting up frames and running the executor
        if bytecode.is_empty() {
            return SandboxResult::failure(
                "Empty bytecode".to_string(),
                0,
                0,
                start_time.elapsed(),
            );
        }

        // Check for timeout
        if start_time.elapsed() > time_limit {
            return SandboxResult::failure(
                "Execution timeout".to_string(),
                thread.steps_executed(),
                heap.memory_used(),
                start_time.elapsed(),
            );
        }

        // Frame setup and dispatch through the interpreter's `Executor` are not
        // wired up here. Report that as a failure instead of claiming the
        // bytecode ran: callers must not treat an unexecuted script as a
        // successful build configuration.
        SandboxResult::failure(
            "Bytecode execution is not implemented in GradleSandbox".to_string(),
            thread.steps_executed(),
            heap.memory_used(),
            start_time.elapsed(),
        )
    }

    /// Record a dependency in the build configuration
    pub fn add_dependency(
        &mut self,
        configuration: String,
        group: String,
        artifact: String,
        version: String,
    ) {
        let dep = Dependency::new(&configuration, &group, &artifact, &version);
        self.build_config.add_dependency(dep);
    }

    /// Record an applied plugin
    pub fn add_plugin(&mut self, plugin_id: String) {
        if !self.build_config.plugins.contains(&plugin_id) {
            self.build_config.plugins.push(plugin_id.clone());
        }
        self.build_config
            .plugin_definitions
            .push(Plugin::new(&plugin_id));
    }

    /// Record a registered task
    pub fn add_task(&mut self, task_name: String) {
        if !self.build_config.tasks.contains(&task_name) {
            self.build_config.tasks.push(task_name.clone());
        }
        self.build_config
            .task_definitions
            .push(Task::new(&task_name));
    }

    /// Record a repository
    pub fn add_repository(&mut self, repo_name: String) {
        if !self.build_config.repositories.contains(&repo_name) {
            self.build_config.repositories.push(repo_name);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_declared_with_apply_false_is_recorded_but_not_applied() {
        let mut config = GradleBuildConfig::default();
        config.add_plugin(Plugin {
            id: "org.example.shared".to_string(),
            version: None,
            apply: false,
        });

        assert!(!config.plugins.contains(&"org.example.shared".to_string()));
        assert_eq!(config.plugin_definitions.len(), 1);
    }

    // Plugin Registry Tests

    #[test]
    fn configured_gradle_user_home_overrides_the_default_without_mutating_the_environment() {
        let home = PathBuf::from("/users/developer");
        assert_eq!(
            resolve_gradle_user_home(Some(home.clone()), Some(PathBuf::from("/cache/gradle"))),
            PathBuf::from("/cache/gradle")
        );
        assert_eq!(
            resolve_gradle_user_home(Some(home.clone()), None),
            home.join(".gradle")
        );
        assert_eq!(
            resolve_gradle_user_home(Some(home.clone()), Some(PathBuf::new())),
            home.join(".gradle")
        );
        assert_eq!(
            resolve_gradle_user_home(None, None),
            PathBuf::from("./.gradle")
        );
    }

    #[test]
    fn test_plugin_registry_creation() {
        let registry = PluginRegistry::new();
        assert!(registry.is_project_method("dependencies"));
        assert!(registry.is_project_method("repositories"));
        assert!(!registry.is_project_method("nonexistent"));
    }

    #[test]
    fn test_plugin_registry_configurations() {
        let registry = PluginRegistry::new();
        let configs = registry.configurations();
        assert!(configs.contains(&"implementation".to_string()));
        assert!(configs.contains(&"testImplementation".to_string()));
    }

    #[test]
    fn test_register_plugin() {
        let mut registry = PluginRegistry::new();
        let plugin = GradlePlugin {
            id: "org.jetbrains.kotlin.jvm".to_string(),
            name: "Kotlin JVM".to_string(),
            project_methods: vec!["kotlin".to_string()],
            extensions: vec!["kotlin".to_string()],
            task_types: vec!["KotlinCompile".to_string()],
        };
        registry.register_plugin(plugin);

        let retrieved = registry.get_plugin("org.jetbrains.kotlin.jvm");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Kotlin JVM");
    }

    // Dependency Tests

    #[test]
    fn test_gradle_dependency_parse() {
        let dep = GradleDependency::parse("com.example:mylib:1.0").unwrap();
        assert_eq!(dep.group, "com.example");
        assert_eq!(dep.artifact, "mylib");
        assert_eq!(dep.version, "1.0");
    }

    #[test]
    fn test_gradle_dependency_to_string() {
        let dep = GradleDependency {
            group: "org.junit".to_string(),
            artifact: "junit".to_string(),
            version: "4.13".to_string(),
        };
        assert_eq!(dep.to_string(), "org.junit:junit:4.13");
    }

    #[test]
    fn test_dependency_parse() {
        let dep = Dependency::parse("implementation", "com.example:lib:1.0:sources").unwrap();
        assert_eq!(dep.configuration, "implementation");
        assert_eq!(dep.group, "com.example");
        assert_eq!(dep.artifact, "lib");
        assert_eq!(dep.version, "1.0");
        assert_eq!(dep.classifier, Some("sources".to_string()));
    }

    #[test]
    fn test_dependency_to_notation() {
        let mut dep = Dependency::new("api", "org.example", "lib", "2.0");
        dep.classifier = Some("javadoc".to_string());
        assert_eq!(dep.to_notation(), "org.example:lib:2.0:javadoc");
    }

    // Security Policy Tests

    #[test]
    fn test_security_policy_default() {
        let policy = SecurityPolicy::default();
        assert_eq!(policy.instruction_limit, 10_000_000);
        assert_eq!(policy.memory_limit, 256 * 1024 * 1024);
        assert!(!policy.allow_network);
        assert!(!policy.allow_process_exec);
    }

    #[test]
    fn test_security_policy_strict() {
        let policy = SecurityPolicy::strict();
        assert_eq!(policy.instruction_limit, 1_000_000);
        assert_eq!(policy.memory_limit, 64 * 1024 * 1024);
        assert_eq!(policy.time_limit, Duration::from_secs(5));
    }

    #[test]
    fn test_security_policy_permissive() {
        let policy = SecurityPolicy::permissive();
        assert!(policy.allow_network);
        assert!(policy.allow_process_exec);
        assert_eq!(policy.instruction_limit, 100_000_000);
    }

    #[test]
    fn test_security_policy_path_access() {
        let mut policy = SecurityPolicy::default();
        let path = PathBuf::from("/project/src");

        assert!(!policy.can_read(&path));

        policy.allow_read_path(PathBuf::from("/project"));
        assert!(policy.can_read(&path));
        assert!(!policy.can_write(&path));

        policy.allow_write_path(PathBuf::from("/project/build"));
        assert!(policy.can_write(&PathBuf::from("/project/build/output")));
        assert!(!policy.can_write(&path));
    }

    // Repository Tests

    #[test]
    fn test_repository_presets() {
        let maven_central = Repository::maven_central();
        assert_eq!(&*maven_central.name, "mavenCentral");
        assert_eq!(maven_central.repo_type, RepositoryType::MavenCentral);

        let google = Repository::google();
        assert_eq!(google.repo_type, RepositoryType::Google);
    }

    // Build Config Tests

    #[test]
    fn test_build_config_add_dependency() {
        let mut config = GradleBuildConfig::new();
        let dep = Dependency::new("implementation", "org.example", "lib", "1.0");
        config.add_dependency(dep);

        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].artifact, "lib");
    }

    #[test]
    fn test_build_config_configurations() {
        let mut config = GradleBuildConfig::new();
        config.add_dependency(Dependency::new("implementation", "a", "b", "1.0"));
        config.add_dependency(Dependency::new("testImplementation", "c", "d", "2.0"));
        config.add_dependency(Dependency::new("implementation", "e", "f", "3.0"));

        let configs = config.configurations();
        assert_eq!(configs.len(), 2);
        assert!(configs.contains(&"implementation"));
        assert!(configs.contains(&"testImplementation"));
    }

    #[test]
    fn test_build_config_plugins() {
        let mut config = GradleBuildConfig::new();
        config.add_plugin(Plugin::new("java"));
        config.add_plugin(Plugin::with_version("org.jetbrains.kotlin.jvm", "1.9.0"));

        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("org.jetbrains.kotlin.jvm"));
        assert!(!config.has_plugin("groovy"));
    }

    #[test]
    fn test_build_config_project_type_detection() {
        let mut config = GradleBuildConfig::new();
        config.plugins.push("org.jetbrains.kotlin.jvm".to_string());
        assert!(config.is_kotlin_project());
        assert!(!config.is_android_project());

        config.plugins.push("com.android.application".to_string());
        assert!(config.is_android_project());
    }

    #[test]
    fn test_build_config_interpolate_multiple_placeholders() {
        let mut config = GradleBuildConfig::new();
        config.set_property("group", "com.example");
        config.set_extra_property("version", "1.2.3");

        let resolved = config.interpolate("${group}:core:${version}");
        assert_eq!(resolved, "com.example:core:1.2.3");
    }

    #[test]
    fn test_build_config_interpolate_preserves_unknown_property() {
        let mut config = GradleBuildConfig::new();
        config.set_property("name", "app");

        let resolved = config.interpolate("${name}-${missing}");
        assert_eq!(resolved, "app-${missing}");
    }

    // Sandbox Tests

    #[test]
    fn test_gradle_sandbox_creation() {
        let sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        assert_eq!(sandbox.build_config.dependencies.len(), 0);
        assert_eq!(sandbox.build_config.plugins.len(), 0);
    }

    #[test]
    fn test_gradle_sandbox_with_policy() {
        let policy = SecurityPolicy::strict();
        let sandbox = GradleSandbox::with_policy(PathBuf::from("/gradle"), policy);
        assert_eq!(sandbox.security().instruction_limit, 1_000_000);
    }

    #[test]
    fn test_gradle_sandbox_add_dependency() {
        let mut sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        sandbox.add_dependency(
            "implementation".to_string(),
            "org.junit".to_string(),
            "junit".to_string(),
            "4.13".to_string(),
        );

        let deps = sandbox
            .build_config
            .dependencies
            .get("implementation")
            .unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group, "org.junit");
    }

    #[test]
    fn test_gradle_sandbox_add_plugin() {
        let mut sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        sandbox.add_plugin("org.jetbrains.kotlin.jvm".to_string());
        assert!(sandbox
            .build_config
            .plugins
            .contains(&"org.jetbrains.kotlin.jvm".to_string()));
    }

    #[test]
    fn test_gradle_sandbox_add_task() {
        let mut sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        sandbox.add_task("build".to_string());
        assert!(sandbox.build_config.tasks.contains(&"build".to_string()));
    }

    #[test]
    fn test_gradle_sandbox_execute_empty() {
        let mut sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        let result = sandbox.execute(&[]);
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_gradle_sandbox_execute_reports_unimplemented_execution() {
        let mut sandbox = GradleSandbox::new(PathBuf::from("/gradle"));
        // Simple bytecode: just a return instruction. Execution is not wired up,
        // so a valid-looking input must still not report success.
        let bytecode = vec![0xB1]; // return void
        let result = sandbox.execute(&bytecode);
        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|e| e.contains("not implemented")),
            "unexpected error: {:?}",
            result.error
        );
    }

    // Task Tests

    #[test]
    fn test_task_creation() {
        let task = Task::new("compileJava");
        assert_eq!(&*task.name, "compileJava");
        assert!(task.depends_on.is_empty());
    }

    #[test]
    fn test_typed_task() {
        let task = Task::typed("copyDocs", "Copy");
        assert_eq!(&*task.name, "copyDocs");
        assert_eq!(task.task_type, Some("Copy".to_string()));
    }

    // SourceSet Tests

    #[test]
    fn test_source_set_creation() {
        let base = PathBuf::from("/project");
        let source_set = SourceSet::new("main", &base);

        assert_eq!(&*source_set.name, "main");
        assert!(source_set
            .java_src_dirs
            .contains(&PathBuf::from("/project/src/main/java")));
        assert!(source_set
            .kotlin_src_dirs
            .contains(&PathBuf::from("/project/src/main/kotlin")));
    }

    // SandboxResult Tests

    #[test]
    fn test_sandbox_result_success() {
        let result = SandboxResult::success(1000, 1024, Duration::from_millis(50), 5);
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.instructions_executed, 1000);
    }

    #[test]
    fn test_sandbox_result_failure() {
        let result = SandboxResult::failure(
            "Out of memory".to_string(),
            500,
            1024 * 1024,
            Duration::from_millis(100),
        );
        assert!(!result.success);
        assert_eq!(result.error, Some("Out of memory".to_string()));
    }
}
