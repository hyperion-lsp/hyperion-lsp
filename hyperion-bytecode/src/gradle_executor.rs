// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle build file executor and configuration extractor
//!
//! This module provides the high-level API for executing build.gradle files
//! and extracting configuration. It integrates the sandbox, stub registry,
//! and Groovy compiler components.
//!
//! # Architecture
//!
//! ```text
//! build.gradle source
//!       ↓
//! Groovy Parser (hyperion-groovy)
//!       ↓
//! Groovy AST lowering
//!       ↓
//! JVM bytecode compilation
//!       ↓
//! Sandboxed in-process execution against Gradle stubs
//!       ↓
//! Merge with conservative pattern extraction
//!       ↓
//! GradleBuildConfig
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_bytecode::gradle_executor::GradleExecutor;
//!
//! let mut executor = GradleExecutor::new();
//! let config = executor.execute_source(r#"
//!     plugins {
//!         id 'java'
//!     }
//!     dependencies {
//!         implementation 'com.google.guava:guava:32.1.2-jre'
//!     }
//! "#)?;
//!
//! assert!(config.has_plugin("java"));
//! ```

use crate::gradle::{
    Dependency, GradleBuildConfig, GradleSandbox, Plugin, Repository, RepositoryType,
    SecurityPolicy,
};
use crate::gradle_stubs::StubRegistry;
use regex::Regex;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use thiserror::Error;
use tracing::{debug, info};

const MAX_GRADLE_SCRIPT_BYTES: usize = 16 * 1024 * 1024;

static PLUGIN_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"id\s+['"]([\w.-]+)['"]"#).expect("valid plugin id regex"));

static PLUGIN_ID_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"id\s+['"]([\w.-]+)['"]\s+version\s+['"]([\w.-]+)['"]"#)
        .expect("valid plugin id+version regex")
});

static LEGACY_APPLY_PLUGIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"apply\s+plugin:\s*['"]([\w.-]+)['"]"#).expect("valid apply plugin regex")
});

static MAVEN_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"maven\s*\{\s*url\s+['"](https?://[^'"]+)['"]"#).expect("valid maven repo regex")
});

static DEPENDENCY_NOTATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\b(\w+)\s+['"]([^:'"]+):([^:'"]+):([^'"]+)['"]"#)
        .expect("valid dependency notation regex")
});

static DEPENDENCY_PAREN_NOTATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\b(\w+)\s*\(\s*['"]([^:'"]+):([^:'"]+):([^'"]+)['"]\s*\)"#)
        .expect("valid dependency paren notation regex")
});

static PROJECT_DEPENDENCY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\w+)\s+project\s*\(\s*['"]([^'"]+)['"]\s*\)"#)
        .expect("valid project dependency regex")
});

static GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*group\s*=\s*['"]([\w.-]+)['"]"#).expect("valid group regex")
});

static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*version\s*=\s*['"]([\w.-]+)['"]"#).expect("valid version regex")
});

static DESCRIPTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"description\s*=?\s*['"]([\w\s.-]+)['"]"#).expect("valid description regex")
});

static EXT_PROPERTY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(\w+)\s*=\s*['"]([\w.-]+)['"]"#).expect("valid ext property regex")
});

static DEF_PROPERTY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"def\s+(\w+)\s*=\s*['"]([\w.-]+)['"]"#).expect("valid def property regex")
});

static TASK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"task\s+(\w+)\s*[\({]"#).expect("valid task regex"));

static TYPED_TASK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"task\s+(\w+)\s*\(\s*type:\s*(\w+)\s*\)"#).expect("valid typed task regex")
});

static REGISTER_TASK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"tasks\.register\s*\(\s*['"]([\w]+)['"]"#).expect("valid register task regex")
});

fn is_supported_dependency_config(name: &str) -> bool {
    matches!(
        name,
        "implementation"
            | "api"
            | "compileOnly"
            | "runtimeOnly"
            | "testImplementation"
            | "testCompileOnly"
            | "testRuntimeOnly"
            | "annotationProcessor"
            | "kapt"
            | "classpath"
    )
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during Gradle execution
#[derive(Debug, Error)]
pub enum GradleExecutionError {
    /// Failed to read build file
    #[error("Failed to read build file: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to parse Groovy source
    #[error("Failed to parse Groovy: {0}")]
    ParseError(String),

    /// Failed to compile Groovy to bytecode
    #[error("Failed to compile: {0}")]
    CompilationError(String),

    /// Execution failed in the VM
    #[error("Execution failed: {0}")]
    ExecutionError(String),

    /// Sandbox limit exceeded
    #[error("Sandbox limit exceeded: {0}")]
    SandboxViolation(String),

    /// Invalid Gradle DSL
    #[error("Invalid Gradle DSL: {0}")]
    InvalidDsl(String),

    /// Script input exceeded the executor's resource limit.
    #[error("Gradle script is too large: {size} bytes (maximum {maximum})")]
    SourceTooLarge { size: u64, maximum: usize },
}

impl From<crate::interpreter::VMError> for GradleExecutionError {
    fn from(err: crate::interpreter::VMError) -> Self {
        GradleExecutionError::ExecutionError(err.to_string())
    }
}

// ============================================================================
// Gradle Executor
// ============================================================================

/// Executor for Gradle build files
///
/// Provides the main API for executing build.gradle files and extracting
/// configuration information. The normal path attempts bounded bytecode
/// execution first, then merges conservative pattern extraction so an
/// unsupported Groovy construct does not erase otherwise visible settings.
pub struct GradleExecutor {
    /// Sandbox for safe execution
    sandbox: GradleSandbox,

    /// Stub registry for Gradle API interception
    stub_registry: StubRegistry,

    /// Shared configuration being built
    config: Arc<Mutex<GradleBuildConfig>>,

    /// When true, the bytecode executor creates proxy stubs for unknown methods
    /// instead of returning null. Enables best-effort extraction from custom DSL blocks.
    recording_proxy: bool,
}

impl GradleExecutor {
    /// Create a new Gradle executor with default settings
    pub fn new() -> Self {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let gradle_home = crate::gradle::gradle_user_home();

        Self {
            sandbox: GradleSandbox::new(gradle_home),
            stub_registry: StubRegistry::new(config.clone()),
            config,
            recording_proxy: false,
        }
    }

    /// Create a new executor with a custom security policy
    pub fn with_policy(policy: SecurityPolicy) -> Self {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let gradle_home = crate::gradle::gradle_user_home();

        Self {
            sandbox: GradleSandbox::with_policy(gradle_home, policy),
            stub_registry: StubRegistry::new(config.clone()),
            config,
            recording_proxy: false,
        }
    }

    /// Enable or disable recording proxy mode for bytecode execution.
    /// When enabled, unknown Gradle methods create proxy stubs instead of
    /// returning null, enabling best-effort extraction from custom DSL blocks.
    pub fn set_recording_proxy(&mut self, enabled: bool) {
        self.recording_proxy = enabled;
    }

    /// Execute a build.gradle file and extract configuration
    pub fn execute_build_file(
        &mut self,
        path: &Path,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        info!("Executing build file: {}", path.display());

        let source = read_gradle_script(path)?;

        // Set project directory based on file location
        if let Some(parent) = path.parent() {
            let mut config = self.config.lock().unwrap();
            config.project_dir = Some(parent.to_path_buf());
            config.build_dir = Some(parent.join("build"));

            // Extract project name from directory
            if let Some(dir_name) = parent.file_name() {
                config.project_name = Some(dir_name.to_string_lossy().to_string());
            }
        }

        self.execute_source(&source)
    }

    /// Execute a build.gradle file using bytecode only (no pattern fallback).
    pub fn execute_build_file_bytecode_only(
        &mut self,
        path: &Path,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        info!("Executing build file via bytecode only: {}", path.display());

        let source = read_gradle_script(path)?;

        let project_dir = path
            .parent()
            .ok_or_else(|| GradleExecutionError::InvalidDsl("Missing project dir".to_string()))?;
        self.execute_source_bytecode_only_with_project(&source, project_dir)
    }

    /// Execute build.gradle source directly
    pub fn execute_source(
        &mut self,
        source: &str,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        Self::validate_source_size(source)?;
        debug!("Executing Gradle source ({} bytes)", source.len());

        // Reset configuration for fresh execution
        self.stub_registry.reset();

        // Best-effort execution path: keep bytecode execution on a short leash so
        // malformed scripts (or unsupported Groovy features) don't hang the LSP.
        //
        // We still run the pattern extractor afterwards and merge the configs.
        let (orig_instruction_limit, orig_time_limit) = {
            let policy = self.sandbox.security();
            (policy.instruction_limit, policy.time_limit)
        };
        let best_effort_instruction_limit = orig_instruction_limit.min(1_000_000);
        let best_effort_time_limit = orig_time_limit.min(std::time::Duration::from_secs(2));

        {
            let policy = self.sandbox.security_mut();
            policy.instruction_limit = best_effort_instruction_limit;
            policy.time_limit = best_effort_time_limit;
        }

        // Execute via bytecode first, then merge a pattern-based extraction pass.
        //
        // Bytecode execution gives us richer semantics when it works, but the Gradle/Groovy
        // surface area is enormous and parse failures can be localized (e.g. a broken plugins
        // block shouldn't prevent extracting a simple dependency).
        //
        // Running a cheap regex-based pass as a secondary source of truth makes the extractor
        // far more resilient, while merge_config avoids most duplicates.
        //
        // Enable recording proxy for this best-effort path so unknown DSL blocks
        // (custom dependency configurations, plugin extensions) get proxy stubs
        // instead of null, extracting as much as possible.
        let orig_recording_proxy = self.recording_proxy;
        self.recording_proxy = true;
        let bytecode_result = self.compile_and_execute(source);
        self.recording_proxy = orig_recording_proxy;

        // Restore policy for subsequent bytecode-only calls (e.g. script plugins).
        {
            let policy = self.sandbox.security_mut();
            policy.instruction_limit = orig_instruction_limit;
            policy.time_limit = orig_time_limit;
        }

        match bytecode_result {
            Ok(()) => debug!("Bytecode execution succeeded"),
            Err(e) => debug!(
                "Bytecode execution failed: {:?}, falling back to pattern extraction",
                e
            ),
        };

        let extracted = self.extract_via_patterns(source);
        {
            let mut config = self.config.lock().unwrap();
            Self::merge_config(&mut config, extracted);
        }

        Ok(self.stub_registry.get_build_config())
    }

    /// Execute build.gradle source using bytecode only (no pattern fallback).
    pub fn execute_source_bytecode_only(
        &mut self,
        source: &str,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        Self::validate_source_size(source)?;
        debug!(
            "Executing Gradle source via bytecode only ({} bytes)",
            source.len()
        );

        // Reset configuration for fresh execution
        self.stub_registry.reset();

        self.compile_and_execute(source)?;

        Ok(self.stub_registry.get_build_config())
    }

    /// Execute build.gradle source using bytecode only with script plugins.
    pub fn execute_source_bytecode_only_with_plugins(
        &mut self,
        source: &str,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        debug!(
            "Executing Gradle source via bytecode only with plugins ({} bytes)",
            source.len()
        );
        self.execute_bytecode_only_with_plugins(source, true)
    }

    /// Execute bytecode-only source with project context and script plugins.
    pub fn execute_source_bytecode_only_with_project(
        &mut self,
        source: &str,
        project_dir: &Path,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        self.stub_registry.reset();
        {
            let mut config = self.config.lock().unwrap();
            config.project_dir = Some(project_dir.to_path_buf());
            config.build_dir = Some(project_dir.join("build"));
            if let Some(dir_name) = project_dir.file_name() {
                config.project_name = Some(dir_name.to_string_lossy().to_string());
            }
        }

        self.execute_bytecode_only_with_plugins(source, false)
    }

    fn execute_bytecode_only_with_plugins(
        &mut self,
        source: &str,
        reset: bool,
    ) -> Result<GradleBuildConfig, GradleExecutionError> {
        Self::validate_source_size(source)?;
        if reset {
            self.stub_registry.reset();
        }
        self.compile_and_execute(source)?;
        self.apply_script_plugins()?;
        Ok(self.stub_registry.get_build_config())
    }

    fn execute_additional_source(&mut self, source: &str) -> Result<(), GradleExecutionError> {
        self.compile_and_execute(source)
    }

    fn apply_script_plugins(&mut self) -> Result<(), GradleExecutionError> {
        let project_dir = self
            .config
            .lock()
            .unwrap()
            .project_dir
            .clone()
            .ok_or_else(|| {
                GradleExecutionError::InvalidDsl("Missing project directory".to_string())
            })?;
        let buildsrc_dir = match Self::find_buildsrc_dir(&project_dir) {
            Some(dir) => dir,
            None => return Ok(()),
        };
        let script_root = buildsrc_dir.join("src/main/groovy");

        let mut applied = HashSet::new();
        loop {
            let plugin_ids = {
                let config = self.config.lock().unwrap();
                if config.plugins.is_empty() {
                    config
                        .plugin_definitions
                        .iter()
                        .map(|plugin| plugin.id.clone())
                        .collect()
                } else {
                    config.plugins.clone()
                }
            };
            let mut applied_any = false;
            for plugin_id in plugin_ids {
                if applied.contains(&plugin_id) {
                    continue;
                }
                let script_path = script_root.join(format!("{}.gradle", plugin_id));
                if !script_path.exists() {
                    continue;
                }
                let script = read_gradle_script(&script_path)?;
                self.execute_additional_source(&script)?;
                applied.insert(plugin_id);
                applied_any = true;
            }
            if !applied_any {
                break;
            }
        }

        Ok(())
    }

    fn find_buildsrc_dir(start: &Path) -> Option<PathBuf> {
        for ancestor in start.ancestors() {
            let candidate = ancestor.join("buildSrc");
            if candidate.join("src/main/groovy").exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn validate_source_size(source: &str) -> Result<(), GradleExecutionError> {
        if source.len() > MAX_GRADLE_SCRIPT_BYTES {
            return Err(GradleExecutionError::SourceTooLarge {
                size: source.len() as u64,
                maximum: MAX_GRADLE_SCRIPT_BYTES,
            });
        }
        Ok(())
    }

    fn has_unbalanced_delimiters(source: &str) -> bool {
        // Fast, conservative syntax sanity check to avoid hanging the Groovy compiler on
        // obviously malformed input (e.g. missing closing braces).
        //
        // This is not a full Groovy lexer; we only skip delimiters inside basic strings and
        // comments. False positives are acceptable because execute_source() will still run
        // pattern extraction afterwards.
        let mut stack: Vec<char> = Vec::new();
        let mut chars = source.chars().peekable();

        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut escaped = false;

        while let Some(ch) = chars.next() {
            if in_line_comment {
                if ch == '\n' {
                    in_line_comment = false;
                }
                continue;
            }

            if in_block_comment {
                if ch == '*' && matches!(chars.peek(), Some('/')) {
                    let _ = chars.next();
                    in_block_comment = false;
                }
                continue;
            }

            if in_single_quote {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '\'' {
                    in_single_quote = false;
                }
                continue;
            }

            if in_double_quote {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    in_double_quote = false;
                }
                continue;
            }

            // Comment starts
            if ch == '/' {
                match chars.peek() {
                    Some('/') => {
                        let _ = chars.next();
                        in_line_comment = true;
                        continue;
                    }
                    Some('*') => {
                        let _ = chars.next();
                        in_block_comment = true;
                        continue;
                    }
                    _ => {}
                }
            }

            // String starts
            if ch == '\'' {
                in_single_quote = true;
                continue;
            }
            if ch == '"' {
                in_double_quote = true;
                continue;
            }

            match ch {
                '{' | '(' | '[' => stack.push(ch),
                '}' | ')' | ']' => {
                    let Some(open) = stack.pop() else {
                        return true;
                    };
                    let ok = matches!((open, ch), ('{', '}') | ('(', ')') | ('[', ']'));
                    if !ok {
                        return true;
                    }
                }
                _ => {}
            }
        }

        !stack.is_empty()
    }

    /// Attempt to compile and execute Groovy source as bytecode
    ///
    /// Executes build.gradle bytecode with Project stub context:
    /// - Unqualified method calls (dependencies, repositories, etc.) → Project stub
    /// - gradle/* class invocations → GradleNativeBridge → StubRegistry
    /// - Configuration captured in shared GradleBuildConfig
    fn compile_and_execute(&mut self, source: &str) -> Result<(), GradleExecutionError> {
        if Self::has_unbalanced_delimiters(source) {
            return Err(GradleExecutionError::ParseError(
                "Unbalanced delimiters".to_string(),
            ));
        }

        use crate::interpreter::gradle_natives::GradleNativeBridge;
        use crate::interpreter::heap::ClassRef;
        use crate::interpreter::linker::ClassLinker;
        use crate::interpreter::runtime_mop::RuntimeMOP;
        use crate::interpreter::value::ObjectRef;
        use crate::interpreter::{parse_constant_pool, Executor, Frame, MethodArea, MonitorTable};
        use crate::interpreter::{ClassLoader, Heap, Thread, Value};
        use cafebabe::attributes::AttributeData;
        use cafebabe::parse_class;
        use std::sync::{Arc, RwLock};
        use std::time::Instant;

        debug!("Compiling and executing Gradle script via bytecode");

        let (main_class, classes) = self.compile_groovy_to_classes(source)?;
        let main_bytes = classes.get(&main_class).ok_or_else(|| {
            GradleExecutionError::CompilationError("Missing main Groovy class".to_string())
        })?;

        let constant_pool = parse_constant_pool(main_bytes).map_err(|e| {
            GradleExecutionError::CompilationError(format!("Constant pool error: {}", e))
        })?;

        let class_file = parse_class(main_bytes)
            .map_err(|e| GradleExecutionError::CompilationError(format!("{:?}", e)))?;

        let run_method = class_file
            .methods
            .iter()
            .find(|m| m.name == "run" && m.descriptor.to_string() == "()Ljava/lang/Object;")
            .ok_or_else(|| {
                GradleExecutionError::CompilationError("Missing run() method".to_string())
            })?;

        let code_attr = run_method
            .attributes
            .iter()
            .find_map(|attr| match &attr.data {
                AttributeData::Code(code) => Some(code),
                _ => None,
            })
            .ok_or_else(|| {
                GradleExecutionError::CompilationError("Missing run() code".to_string())
            })?;

        let exception_table = code_attr
            .exception_table
            .iter()
            .map(|entry| crate::interpreter::frame::ExceptionTableEntry {
                start_pc: entry.start_pc,
                end_pc: entry.end_pc,
                handler_pc: entry.handler_pc,
                catch_type: entry.catch_type.as_ref().map(|s| s.to_string()),
            })
            .collect();

        let mut method_registry = std::collections::HashMap::new();
        for (class_name, bytes) in &classes {
            if let Ok(parsed) = parse_class(bytes) {
                for method in &parsed.methods {
                    if let Some(code_attr) =
                        method.attributes.iter().find_map(|attr| match &attr.data {
                            AttributeData::Code(code) => Some(code),
                            _ => None,
                        })
                    {
                        let method_exception_table = code_attr
                            .exception_table
                            .iter()
                            .map(|entry| crate::interpreter::frame::ExceptionTableEntry {
                                start_pc: entry.start_pc,
                                end_pc: entry.end_pc,
                                handler_pc: entry.handler_pc,
                                catch_type: entry.catch_type.as_ref().map(|s| s.to_string()),
                            })
                            .collect();

                        method_registry.insert(
                            (
                                class_name.clone(),
                                method.name.to_string(),
                                method.descriptor.to_string(),
                            ),
                            crate::interpreter::executor::MethodBytecode {
                                max_locals: code_attr.max_locals,
                                max_stack: code_attr.max_stack,
                                code: code_attr.code.to_vec(),
                                exception_table: method_exception_table,
                                access_flags: method.access_flags.bits(),
                            },
                        );
                    }
                }
            }
        }

        let class_loader = ClassLoader::new();
        let method_area = Arc::new(RwLock::new(MethodArea::new()));

        for bytes in classes.values() {
            let mut class = ClassLinker::parse_class(bytes, &class_loader)
                .map_err(|e| GradleExecutionError::CompilationError(format!("{}", e)))?;
            ClassLinker::link_class(&mut class, &class_loader)
                .map_err(|e| GradleExecutionError::CompilationError(format!("{}", e)))?;
            method_area
                .write()
                .map_err(|_| {
                    GradleExecutionError::ExecutionError("MethodArea lock failed".to_string())
                })?
                .register_class(class)
                .map_err(|e| GradleExecutionError::ExecutionError(format!("{}", e)))?;
        }

        let runtime_mop = Arc::new(RwLock::new(RuntimeMOP::new()));
        let gradle_bridge = Arc::new(
            GradleNativeBridge::new(self.config.clone()).with_recording_proxy(self.recording_proxy),
        );

        let mut executor = Executor::new(code_attr.code.to_vec())
            .with_constant_pool(constant_pool)
            .with_methods(method_registry)
            .with_method_area(method_area)
            .with_runtime_mop(runtime_mop)
            .with_gradle_native_bridge(gradle_bridge);

        for (class_name, bytes) in &classes {
            executor.add_class_bytecode(class_name.clone(), bytes.clone());
        }

        let (stack_depth_limit, instruction_limit, memory_limit, time_limit) = {
            let policy = self.sandbox.security();
            (
                policy.stack_depth_limit,
                policy.instruction_limit,
                policy.memory_limit,
                policy.time_limit,
            )
        };

        // Recording proxy executes closures that would otherwise be skipped
        // (because the parent method returned null). Each DSL block adds ~3-4
        // frames, so deeply nested builds need more headroom. The time limit
        // and instruction limit are the real safety nets here.
        let effective_stack_limit = if self.recording_proxy {
            stack_depth_limit.max(4096)
        } else {
            stack_depth_limit
        };

        let mut thread = Thread::new(effective_stack_limit, Some(instruction_limit));
        let mut heap = Heap::new(memory_limit);
        let mut monitors = MonitorTable::new();
        let start_time = Instant::now();

        let script_obj = heap
            .allocate_object(ClassRef {
                name: main_class.clone(),
                instance_field_count: 0,
                static_field_count: 0,
            })
            .map_err(|e| GradleExecutionError::ExecutionError(format!("{}", e)))?;

        let frame = Frame::with_bytecode(
            main_class.clone(),
            "run".to_string(),
            code_attr.max_locals,
            code_attr.max_stack,
            vec![Value::Reference(ObjectRef::Object(script_obj))],
            code_attr.code.to_vec(),
            "()Ljava/lang/Object;".to_string(),
            exception_table,
        )?;
        thread.push_frame(frame)?;

        debug!("Executing bytecode with Project stub context");

        loop {
            // Keep Gradle execution bounded even for malformed or adversarial scripts.
            if start_time.elapsed() > time_limit {
                return Err(GradleExecutionError::SandboxViolation(
                    "Execution timeout exceeded".to_string(),
                ));
            }

            match executor.step(&mut thread, &mut heap, &mut monitors) {
                Ok(true) => {}
                Ok(false) => {
                    debug!("Bytecode execution completed successfully");
                    break;
                }
                Err(crate::interpreter::VMError::ExecutionLimitExceeded) => {
                    return Err(GradleExecutionError::SandboxViolation(
                        "Instruction limit exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::ExecutionTimeout) => {
                    return Err(GradleExecutionError::SandboxViolation(
                        "Execution timeout exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::MemoryLimitExceeded) => {
                    return Err(GradleExecutionError::SandboxViolation(
                        "Memory limit exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::StackOverflow) => {
                    return Err(GradleExecutionError::SandboxViolation(
                        "Stack depth limit exceeded".to_string(),
                    ));
                }
                Err(e) => {
                    return Err(GradleExecutionError::ExecutionError(format!(
                        "Bytecode execution failed: {}",
                        e
                    )));
                }
            }
        }

        Ok(())
    }

    fn compile_groovy_to_classes(
        &self,
        source: &str,
    ) -> Result<(String, std::collections::HashMap<String, Vec<u8>>), GradleExecutionError> {
        use crate::groovy_producer::GroovyBytecodeProducer;
        use crate::producer::CompilationContext;

        let class_name = "build".to_string();
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("build.gradle".to_string()),
            ..Default::default()
        };

        let producer = GroovyBytecodeProducer::new();
        let classes = producer
            .produce_all_classes(&context)
            .map_err(|e| GradleExecutionError::CompilationError(e.to_string()))?;

        Ok((class_name, classes))
    }

    /// Extract configuration using pattern matching (fallback)
    ///
    /// This provides immediate functionality while we build out the full
    /// bytecode execution path.
    fn extract_via_patterns(&self, source: &str) -> GradleBuildConfig {
        let mut config = GradleBuildConfig::new();

        debug!("Extracting configuration via pattern matching");

        // Extract plugins
        self.extract_plugins(source, &mut config);

        // Extract repositories
        self.extract_repositories(source, &mut config);

        // Extract project properties BEFORE dependencies (for GString interpolation)
        self.extract_properties(source, &mut config);

        // Extract dependencies (may contain ${varName} references)
        self.extract_dependencies(source, &mut config);

        // Interpolate GString expressions in dependency versions
        // Collect interpolated versions first to avoid borrow conflict
        let interpolated_versions: Vec<String> = config
            .dependency_list
            .iter()
            .map(|dep| config.interpolate(&dep.version))
            .collect();
        for (dep, version) in config.dependency_list.iter_mut().zip(interpolated_versions) {
            dep.version = version;
        }

        // Extract tasks
        self.extract_tasks(source, &mut config);

        config
    }

    /// Extract plugins from source
    fn extract_plugins(&self, source: &str, config: &mut GradleBuildConfig) {
        // Match plugins { } block
        if let Some(plugins_block) = Self::extract_block(source, "plugins") {
            debug!("Found plugins block");

            // Match: id 'plugin-id' or id "plugin-id"
            for cap in PLUGIN_ID_RE.captures_iter(&plugins_block) {
                let plugin_id = &cap[1];
                debug!("Found plugin: {}", plugin_id);
                config.add_plugin(Plugin::new(plugin_id));
            }

            // Match: id 'plugin-id' version 'x.y.z'
            for cap in PLUGIN_ID_VERSION_RE.captures_iter(&plugins_block) {
                let plugin_id = &cap[1];
                let version = &cap[2];
                debug!("Found plugin with version: {} v{}", plugin_id, version);
                // Update existing plugin with version
                if let Some(plugin) = config
                    .plugin_definitions
                    .iter_mut()
                    .find(|p| p.id == plugin_id)
                {
                    plugin.version = Some(version.to_string());
                } else {
                    config.add_plugin(Plugin::with_version(plugin_id, version));
                }
            }
        }

        // Match legacy: apply plugin: 'plugin-id'
        for cap in LEGACY_APPLY_PLUGIN_RE.captures_iter(source) {
            let plugin_id = &cap[1];
            debug!("Found legacy plugin application: {}", plugin_id);
            config.add_plugin(Plugin::new(plugin_id));
        }
    }

    /// Extract repositories from source
    fn extract_repositories(&self, source: &str, config: &mut GradleBuildConfig) {
        if let Some(repos_block) = Self::extract_block(source, "repositories") {
            debug!("Found repositories block");

            // Match common repository methods
            if repos_block.contains("mavenCentral()") {
                debug!("Found mavenCentral repository");
                config.add_repository(Repository::maven_central());
            }

            if repos_block.contains("mavenLocal()") {
                debug!("Found mavenLocal repository");
                config.add_repository(Repository::maven_local());
            }

            if repos_block.contains("google()") {
                debug!("Found google repository");
                config.add_repository(Repository::google());
            }

            if repos_block.contains("jcenter()") {
                debug!("Found jcenter repository");
                config.add_repository(Repository {
                    name: "jcenter".to_string(),
                    url: "https://jcenter.bintray.com/".to_string(),
                    repo_type: RepositoryType::Maven,
                });
            }

            if repos_block.contains("gradlePluginPortal()") {
                debug!("Found gradlePluginPortal repository");
                config.add_repository(Repository::gradle_plugin_portal());
            }

            // Match maven { url ... } blocks
            for cap in MAVEN_URL_RE.captures_iter(&repos_block) {
                let url = &cap[1];
                debug!("Found maven repository: {}", url);
                config.add_repository(Repository {
                    name: format!("maven-{}", config.repository_list.len()),
                    url: url.to_string(),
                    repo_type: RepositoryType::Maven,
                });
            }
        }
    }

    /// Extract dependencies from source
    fn extract_dependencies(&self, source: &str, config: &mut GradleBuildConfig) {
        if let Some(deps_block) = Self::extract_block(source, "dependencies") {
            debug!("Found dependencies block");

            // Match: configuration 'group:artifact:version'
            for cap in DEPENDENCY_NOTATION_RE.captures_iter(&deps_block) {
                let config_name = &cap[1];
                if !is_supported_dependency_config(config_name) {
                    continue;
                }

                let group = &cap[2];
                let artifact = &cap[3];
                let version = &cap[4];
                debug!(
                    "Found dependency: {} {} : {} : {}",
                    config_name, group, artifact, version
                );
                config.add_dependency(Dependency::new(config_name, group, artifact, version));
            }

            // Match: configuration('group:artifact:version')
            for cap in DEPENDENCY_PAREN_NOTATION_RE.captures_iter(&deps_block) {
                let config_name = &cap[1];
                if !is_supported_dependency_config(config_name) {
                    continue;
                }

                let group = &cap[2];
                let artifact = &cap[3];
                let version = &cap[4];
                debug!(
                    "Found dependency (paren): {} {} : {} : {}",
                    config_name, group, artifact, version
                );
                // Avoid duplicates
                let exists = config
                    .dependency_list
                    .iter()
                    .any(|d| d.group == group && d.artifact == artifact);
                if !exists {
                    config.add_dependency(Dependency::new(config_name, group, artifact, version));
                }
            }

            // Match project dependencies: implementation project(':module')
            for cap in PROJECT_DEPENDENCY_RE.captures_iter(&deps_block) {
                let config_name = &cap[1];
                if !is_supported_dependency_config(config_name) {
                    continue;
                }
                let project_path = &cap[2];
                debug!(
                    "Found project dependency: {} -> {}",
                    config_name, project_path
                );
                let mut dep = Dependency::new(config_name, "", project_path, "");
                dep.is_project = true;
                config.add_dependency(dep);
            }
        }
    }

    /// Extract project properties from source
    fn extract_properties(&self, source: &str, config: &mut GradleBuildConfig) {
        // Match: group = 'value' or group 'value' (but not inside plugins block)
        // Look for group at the start of a line (with optional whitespace)
        if let Some(cap) = GROUP_RE.captures(source) {
            config.group = Some(cap[1].to_string());
            debug!("Found group: {}", &cap[1]);
        }

        // Match: version = 'value' at the start of a line (not 'id ... version' in plugins)
        if let Some(cap) = VERSION_RE.captures(source) {
            config.version = Some(cap[1].to_string());
            debug!("Found version: {}", &cap[1]);
        }

        // Match: description = 'value'
        if let Some(cap) = DESCRIPTION_RE.captures(source) {
            config.description = Some(cap[1].to_string());
            debug!("Found description: {}", &cap[1]);
        }

        // Extract ext { } block properties
        if let Some(ext_block) = Self::extract_block(source, "ext") {
            for cap in EXT_PROPERTY_RE.captures_iter(&ext_block) {
                let key = &cap[1];
                let value = &cap[2];
                config.set_extra_property(key, value);
                debug!("Found ext property: {} = {}", key, value);
            }
        }

        // Extract def declarations: def varName = 'value'
        for cap in DEF_PROPERTY_RE.captures_iter(source) {
            let var_name = &cap[1];
            let value = &cap[2];
            config.set_extra_property(var_name, value);
            debug!("Found def variable: {} = {}", var_name, value);
        }
    }

    /// Extract tasks from source
    fn extract_tasks(&self, source: &str, config: &mut GradleBuildConfig) {
        // Match: task taskName { }
        for cap in TASK_RE.captures_iter(source) {
            let task_name = &cap[1];
            debug!("Found task: {}", task_name);
            config.add_task(crate::gradle::Task::new(task_name));
        }

        // Match: task taskName(type: TypeName) { }
        for cap in TYPED_TASK_RE.captures_iter(source) {
            let task_name = &cap[1];
            let task_type = &cap[2];
            debug!("Found typed task: {} (type: {})", task_name, task_type);
            // Update existing task or add new one
            if let Some(task) = config
                .task_definitions
                .iter_mut()
                .find(|t| t.name == task_name)
            {
                task.task_type = Some(task_type.to_string());
            } else {
                config.add_task(crate::gradle::Task::typed(task_name, task_type));
            }
        }

        // Match: tasks.register('taskName') { }
        for cap in REGISTER_TASK_RE.captures_iter(source) {
            let task_name = &cap[1];
            debug!("Found registered task: {}", task_name);
            if !config.tasks.contains(&task_name.to_string()) {
                config.add_task(crate::gradle::Task::new(task_name));
            }
        }
    }

    /// Extract a block by name (e.g., "dependencies", "plugins")
    fn extract_block(source: &str, block_name: &str) -> Option<String> {
        fn is_ident_byte(byte: u8) -> bool {
            byte.is_ascii_alphanumeric() || byte == b'_'
        }

        let bytes = source.as_bytes();
        let mut search_start = 0usize;

        while let Some(relative_idx) = source[search_start..].find(block_name) {
            let name_start = search_start + relative_idx;
            let name_end = name_start + block_name.len();

            let has_ident_before = name_start > 0 && is_ident_byte(bytes[name_start - 1]);
            let has_ident_after = name_end < bytes.len() && is_ident_byte(bytes[name_end]);
            if has_ident_before || has_ident_after {
                search_start = name_end;
                continue;
            }

            let mut brace_idx = name_end;
            while brace_idx < bytes.len() && bytes[brace_idx].is_ascii_whitespace() {
                brace_idx += 1;
            }
            if brace_idx >= bytes.len() || bytes[brace_idx] != b'{' {
                search_start = name_end;
                continue;
            }

            let content_start = brace_idx + 1;
            let mut depth = 1usize;
            let mut i = content_start;
            while i < bytes.len() {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(source[content_start..i].to_string());
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            return None;
        }

        None
    }

    /// Merge extracted config into existing config
    fn merge_config(target: &mut GradleBuildConfig, source: GradleBuildConfig) {
        // Merge plugins
        for plugin in source.plugin_definitions {
            if !target.has_plugin(&plugin.id) {
                target.add_plugin(plugin);
            }
        }

        // Merge repositories
        for repo in source.repository_list {
            if !target.repositories.contains(&repo.name) {
                target.add_repository(repo);
            }
        }

        // Merge dependencies
        for dep in source.dependency_list {
            let exists = target.dependency_list.iter().any(|d| {
                d.group == dep.group
                    && d.artifact == dep.artifact
                    && d.configuration == dep.configuration
            });
            if !exists {
                target.add_dependency(dep);
            }
        }

        // Merge tasks
        for task in source.task_definitions {
            if !target.tasks.contains(&task.name) {
                target.add_task(task);
            }
        }

        // Merge properties
        if target.group.is_none() {
            target.group = source.group;
        }
        if target.version.is_none() {
            target.version = source.version;
        }
        if target.description.is_none() {
            target.description = source.description;
        }

        // Merge extra properties
        for (key, value) in source.extra_properties {
            target.extra_properties.entry(key).or_insert(value);
        }
    }

    /// Get the stub registry for direct API access
    pub fn stub_registry(&self) -> &StubRegistry {
        &self.stub_registry
    }

    /// Get the sandbox for inspection
    pub fn sandbox(&self) -> &GradleSandbox {
        &self.sandbox
    }
}

fn read_gradle_script(path: &Path) -> Result<String, GradleExecutionError> {
    let file = std::fs::File::open(path)?;
    let declared_size = file.metadata()?.len();
    if declared_size > MAX_GRADLE_SCRIPT_BYTES as u64 {
        return Err(GradleExecutionError::SourceTooLarge {
            size: declared_size,
            maximum: MAX_GRADLE_SCRIPT_BYTES,
        });
    }

    let mut bytes = Vec::with_capacity(declared_size as usize);
    file.take(MAX_GRADLE_SCRIPT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_GRADLE_SCRIPT_BYTES {
        return Err(GradleExecutionError::SourceTooLarge {
            size: bytes.len() as u64,
            maximum: MAX_GRADLE_SCRIPT_BYTES,
        });
    }
    String::from_utf8(bytes).map_err(|error| {
        GradleExecutionError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

impl Default for GradleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_gradle_files_before_reading_them() {
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file()
            .set_len(MAX_GRADLE_SCRIPT_BYTES as u64 + 1)
            .unwrap();

        assert!(matches!(
            read_gradle_script(file.path()),
            Err(GradleExecutionError::SourceTooLarge { .. })
        ));
    }

    #[test]
    fn rejects_oversized_in_memory_gradle_sources() {
        let source = " ".repeat(MAX_GRADLE_SCRIPT_BYTES + 1);
        let mut executor = GradleExecutor::new();

        assert!(matches!(
            executor.execute_source(&source),
            Err(GradleExecutionError::SourceTooLarge { .. })
        ));
    }

    #[test]
    fn test_execute_simple_gradle() {
        let source = r#"
            plugins {
                id 'java'
            }

            repositories {
                mavenCentral()
            }

            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert!(config.has_plugin("java"));
        assert_eq!(config.repository_list.len(), 1);
        assert_eq!(config.get_dependencies("implementation").len(), 1);

        let dep = &config.get_dependencies("implementation")[0];
        assert_eq!(dep.group, "com.google.guava");
        assert_eq!(dep.artifact, "guava");
        assert_eq!(dep.version, "32.1.2-jre");
    }

    #[test]
    fn test_extract_plugins() {
        let source = r#"
            plugins {
                id 'java'
                id 'org.jetbrains.kotlin.jvm' version '1.9.0'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("org.jetbrains.kotlin.jvm"));

        let kotlin_plugin = config
            .plugin_definitions
            .iter()
            .find(|p| p.id == "org.jetbrains.kotlin.jvm")
            .unwrap();
        assert_eq!(kotlin_plugin.version, Some("1.9.0".to_string()));
    }

    #[test]
    fn test_extract_legacy_plugin() {
        let source = r#"
            apply plugin: 'java'
            apply plugin: 'maven-publish'
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("maven-publish"));
    }

    #[test]
    fn test_extract_repositories() {
        let source = r#"
            repositories {
                mavenCentral()
                mavenLocal()
                google()
                maven { url 'https://jitpack.io' }
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert_eq!(config.repository_list.len(), 4);
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
        assert!(config.repositories.contains(&"mavenLocal".to_string()));
        assert!(config.repositories.contains(&"google".to_string()));

        // Check custom maven repo
        let jitpack = config
            .repository_list
            .iter()
            .find(|r| r.url == "https://jitpack.io");
        assert!(jitpack.is_some());
    }

    #[test]
    fn test_extract_dependencies() {
        let source = r#"
            dependencies {
                implementation 'org.springframework.boot:spring-boot-starter:3.0.0'
                api 'com.google.guava:guava:31.1-jre'
                testImplementation 'org.junit.jupiter:junit-jupiter:5.9.0'
                runtimeOnly 'org.postgresql:postgresql:42.5.0'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("api").len(), 1);
        assert_eq!(config.get_dependencies("testImplementation").len(), 1);
        assert_eq!(config.get_dependencies("runtimeOnly").len(), 1);
    }

    #[test]
    fn test_extract_dependencies_with_parens() {
        let source = r#"
            dependencies {
                implementation('com.example:lib:1.0')
                testImplementation("org.junit:junit:4.13")
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("testImplementation").len(), 1);
    }

    #[test]
    fn test_extract_project_dependencies() {
        let source = r#"
            dependencies {
                implementation project(':core')
                api project(':common')
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        let impl_deps = config.get_dependencies("implementation");
        assert_eq!(impl_deps.len(), 1);
        assert!(impl_deps[0].is_project);
        assert_eq!(impl_deps[0].artifact, ":core");
    }

    #[test]
    fn test_extract_properties() {
        let source = r#"
            group = 'com.example'
            version = '1.0.0'
            description = 'My awesome project'
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert_eq!(config.group, Some("com.example".to_string()));
        assert_eq!(config.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_extract_ext_properties() {
        let source = r#"
            ext {
                kotlinVersion = '1.9.0'
                springVersion = '3.0.0'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert_eq!(
            config.extra_properties.get("kotlinVersion"),
            Some(&"1.9.0".to_string())
        );
        assert_eq!(
            config.extra_properties.get("springVersion"),
            Some(&"3.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_tasks() {
        let source = r#"
            task clean {
                doLast {
                    delete buildDir
                }
            }

            task copyDocs(type: Copy) {
                from 'docs'
                into 'build/docs'
            }

            tasks.register('myTask') {
                doLast { }
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert!(config.tasks.contains(&"clean".to_string()));
        assert!(config.tasks.contains(&"copyDocs".to_string()));
        assert!(config.tasks.contains(&"myTask".to_string()));

        let copy_task = config
            .task_definitions
            .iter()
            .find(|t| t.name == "copyDocs")
            .unwrap();
        assert_eq!(copy_task.task_type, Some("Copy".to_string()));
    }

    #[test]
    fn test_complex_build_file() {
        let source = r#"
            plugins {
                id 'java'
                id 'org.springframework.boot' version '3.0.0'
                id 'io.spring.dependency-management' version '1.1.0'
            }

            group = 'com.example'
            version = '0.0.1-SNAPSHOT'

            repositories {
                mavenCentral()
            }

            ext {
                lombokVersion = '1.18.24'
            }

            dependencies {
                implementation 'org.springframework.boot:spring-boot-starter-web'
                implementation 'org.springframework.boot:spring-boot-starter-data-jpa'
                compileOnly 'org.projectlombok:lombok:1.18.24'
                annotationProcessor 'org.projectlombok:lombok:1.18.24'
                runtimeOnly 'org.postgresql:postgresql:42.5.0'
                testImplementation 'org.springframework.boot:spring-boot-starter-test'
            }

            task bootJar {
                enabled = true
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        // Verify plugins
        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("org.springframework.boot"));
        assert!(config.has_plugin("io.spring.dependency-management"));

        // Verify properties
        assert_eq!(config.group, Some("com.example".to_string()));
        assert_eq!(config.version, Some("0.0.1-SNAPSHOT".to_string()));

        // Verify repositories
        assert!(config.repositories.contains(&"mavenCentral".to_string()));

        // Verify ext properties
        assert_eq!(
            config.extra_properties.get("lombokVersion"),
            Some(&"1.18.24".to_string())
        );

        // Verify tasks
        assert!(config.tasks.contains(&"bootJar".to_string()));
    }

    #[test]
    fn test_invalid_syntax_graceful_handling() {
        // Malformed source should not panic, just extract what it can
        let source = r#"
            plugins {
                id 'java'

            // Missing closing brace intentionally

            dependencies {
                implementation 'com.example:lib:1.0'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let result = executor.execute_source(source);

        // Should still succeed with partial extraction
        assert!(result.is_ok());
        let config = result.unwrap();

        // Should still extract the dependency
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_bytecode_handles_dependencies_constraints_and_because() {
        let source = r#"
            dependencies {
                implementation 'com.google.inject:guice:6.0.0'
                constraints {
                    api('com.squareup.okhttp3:okhttp:4.12.0') {
                        because 'Enforce safe okio version'
                    }
                }
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source_bytecode_only(source).unwrap();

        assert!(
            config
                .dependency_list
                .iter()
                .any(|d| d.configuration == "implementation"
                    && d.to_notation() == "com.google.inject:guice:6.0.0"),
            "Expected implementation dependency to be recorded"
        );
        assert!(
            config
                .dependency_list
                .iter()
                .any(|d| d.configuration == "api"
                    && d.to_notation()
                        .starts_with("com.squareup.okhttp3:okhttp:4.12.0")),
            "Expected constraint dependency to be recorded"
        );
    }

    #[test]
    fn test_bytecode_handles_tasks_with_type_closure() {
        let source = r#"
            tasks.withType(org.gradle.jvm.tasks.Jar) {
                dependsOn("enhance")
            }
        "#;

        let mut executor = GradleExecutor::new();
        let result = executor.execute_source_bytecode_only(source);
        assert!(
            result.is_ok(),
            "Expected bytecode execution to succeed, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_bytecode_handles_custom_extension_blocks() {
        let source = r#"
            forkedRun {
                httpPortOverride = 19080
                appName = "rest-api-core"
            }

            resolve {
                outputFileName = 'swagger'
                resourcePackages = [
                    'com.example.one',
                    'com.example.two'
                ]
            }

            openjpa {
                metamodel {
                    metamodelOutputFolder = "build/generated"
                }
            }
        "#;

        let mut executor = GradleExecutor::new();
        let result = executor.execute_source_bytecode_only(source);
        assert!(
            result.is_ok(),
            "Expected bytecode execution to succeed, got: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_sandbox_limits() {
        let strict_policy = SecurityPolicy::strict();
        let mut executor = GradleExecutor::with_policy(strict_policy);

        let source = r#"
            plugins {
                id 'java'
            }
        "#;

        let config = executor.execute_source(source).unwrap();
        assert!(config.has_plugin("java"));

        // Verify strict policy is applied
        assert_eq!(executor.sandbox().security().instruction_limit, 1_000_000);
    }

    #[test]
    fn test_extract_block() {
        let source = r#"
            dependencies {
                implementation 'a:b:1'
                api 'c:d:2'
            }
        "#;

        let block = GradleExecutor::extract_block(source, "dependencies");
        assert!(block.is_some());

        let content = block.unwrap();
        assert!(content.contains("implementation"));
        assert!(content.contains("api"));
    }

    #[test]
    fn test_extract_block_requires_word_boundary() {
        let source = r#"
            mydependencies {
                implementation 'wrong:block:0'
            }
            dependencies {
                implementation 'right:block:1'
            }
        "#;

        let block = GradleExecutor::extract_block(source, "dependencies").expect("block present");
        assert!(block.contains("right:block:1"));
        assert!(!block.contains("wrong:block:0"));
    }

    #[test]
    fn test_nested_blocks() {
        let source = r#"
            dependencies {
                implementation('com.example:lib:1.0') {
                    exclude group: 'org.unwanted'
                }
            }
        "#;

        let block = GradleExecutor::extract_block(source, "dependencies");
        assert!(block.is_some());

        let content = block.unwrap();
        assert!(content.contains("exclude"));
    }

    #[test]
    fn test_kotlin_android_project() {
        let source = r#"
            plugins {
                id 'com.android.application'
                id 'org.jetbrains.kotlin.android'
            }

            repositories {
                google()
                mavenCentral()
            }

            dependencies {
                implementation 'androidx.core:core-ktx:1.9.0'
                implementation 'org.jetbrains.kotlin:kotlin-stdlib:1.8.0'
            }
        "#;

        let mut executor = GradleExecutor::new();
        let config = executor.execute_source(source).unwrap();

        assert!(config.is_android_project());
        assert!(config.is_kotlin_project());
        assert_eq!(config.repository_list.len(), 2);
    }

    #[test]
    fn test_default_executor() {
        let executor = GradleExecutor::default();
        let gradle_home = executor.sandbox().gradle_home();
        assert!(
            gradle_home.is_absolute(),
            "Default Gradle home should resolve to an absolute path"
        );
        assert!(!gradle_home.as_os_str().is_empty());
    }
}
