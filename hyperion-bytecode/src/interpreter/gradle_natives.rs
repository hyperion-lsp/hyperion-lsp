// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle Native Bridge
//!
//! Routes bytecode method invocations to Gradle stub implementations.
//! This enables build.gradle execution by intercepting gradle/* class method calls
//! and delegating to the StubRegistry.

use super::errors::{VMError, VMResult};
use super::heap::{CollectionData, Heap, HeapEntry};
use super::value::{ObjectRef, Value};
use crate::gradle::Task;
use crate::gradle::{GradleBuildConfig, Plugin, Repository, RepositoryType};
use crate::gradle_stubs::StubMethodRegistry;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Bridge between JVM bytecode execution and Gradle stub implementations
///
/// Architecture:
/// ```text
/// Bytecode: invokestatic gradle/Project.dependencies(Closure)DependencyHandler
///     ↓
/// GradleNativeBridge.invoke_gradle_method()
///     ↓
/// StubMethodRegistry.invoke("Project.dependencies", args)
///     ↓
/// GradleBuildConfig (shared state)
/// ```
pub struct GradleNativeBridge {
    /// Shared configuration written by stubs
    config: Arc<Mutex<GradleBuildConfig>>,
    /// Stub method registry for Gradle API interception
    stub_registry: Arc<StubMethodRegistry>,
    /// When true, unknown methods create proxy stubs instead of returning null.
    /// This enables best-effort extraction from DSL blocks that use custom
    /// configurations (e.g. Android's `debugImplementation`, test suite configs).
    pub(crate) recording_proxy: bool,
    /// Closures that have already been executed (prevents infinite recursion).
    /// A closure should only be executed once — re-execution indicates a dispatch
    /// loop rather than legitimate Gradle behavior.
    executed_closures: Mutex<std::collections::HashSet<super::value::HeapId>>,
    /// When true, execute `subprojects {}` blocks by invoking the provided closure/action.
    /// This is required for internal plugins that apply dependencies to subprojects.
    execute_subprojects: bool,
    /// Optional version-catalog versions for sniffer intrinsics (e.g. TomlUtil.readVersion).
    version_catalog_versions: Option<HashMap<String, String>>,
}

impl GradleNativeBridge {
    /// Create a new Gradle native bridge with shared configuration
    pub fn new(config: Arc<Mutex<GradleBuildConfig>>) -> Self {
        let stub_registry = Arc::new(StubMethodRegistry::new(config.clone()));
        Self {
            config,
            stub_registry,
            recording_proxy: false,
            executed_closures: Mutex::new(std::collections::HashSet::new()),
            execute_subprojects: false,
            version_catalog_versions: None,
        }
    }

    /// Enable recording proxy mode: unknown methods create proxy stubs and
    /// attempt closure execution instead of returning null.
    pub fn with_recording_proxy(mut self, enabled: bool) -> Self {
        self.recording_proxy = enabled;
        self
    }

    pub fn with_execute_subprojects(mut self, enabled: bool) -> Self {
        self.execute_subprojects = enabled;
        self
    }

    pub fn with_version_catalog_versions(mut self, versions: HashMap<String, String>) -> Self {
        self.version_catalog_versions = Some(versions);
        self
    }

    /// Check if a closure has been executed. If not, mark it as executed and return true.
    /// Returns false if the closure was already executed (prevents re-execution loops).
    fn try_mark_closure_executed(&self, closure_id: super::value::HeapId) -> bool {
        let mut executed = self.executed_closures.lock().unwrap();
        executed.insert(closure_id)
    }

    /// Route gradle/* method invocation to appropriate stub
    ///
    /// Supported method signatures:
    /// - `gradle/Project.dependencies(Closure)DependencyHandler`
    /// - `gradle/Project.repositories(Closure)RepositoryHandler`
    /// - `gradle/Project.plugins(Closure)PluginManager`
    /// - `gradle/DependencyHandler.implementation(String)Dependency`
    /// - `gradle/DependencyHandler.testImplementation(String)Dependency`
    /// - `gradle/RepositoryHandler.mavenCentral()Repository`
    /// - `gradle/RepositoryHandler.mavenLocal()Repository`
    /// - `gradle/PluginManager.id(String)PluginSpec`
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::only_used_in_recursion)]
    pub fn invoke_gradle_method(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if let Some(extension) = Self::extension_name_from_class(class_name) {
            return self.handle_extension_method(
                heap,
                method_registry,
                thread,
                &extension,
                method_name,
                args,
            );
        }

        // Exact-match static classes first, then dynamic class prefixes with
        // trailing slash to prevent prefix shadowing (e.g. "gradle/Task" vs
        // "gradle/TaskContainer").
        match class_name {
            "gradle/TaskContainer" | "gradle/Task" => {
                return self.handle_task_method(
                    heap,
                    method_registry,
                    thread,
                    class_name,
                    method_name,
                    args,
                );
            }
            "gradle/SourceSetContainer" => {
                return self.handle_source_set_container(heap, method_name);
            }
            "gradle/FileCollection" => {
                return self.handle_file_collection(heap, method_name, args);
            }
            "gradle/Layout" => {
                return self.handle_layout(heap, method_name);
            }
            _ => {
                // Dynamic class names with instance-specific suffixes
                if class_name.starts_with("gradle/SourceSet/")
                    || class_name.starts_with("gradle/SourceDirectorySet/")
                {
                    return self.handle_source_set_method(heap, class_name, method_name, args);
                }
            }
        }

        // Match class and method to route to appropriate stub handler
        match (class_name, method_name) {
            // Project stub methods
            ("gradle/Project", "dependencies") => {
                self.handle_project_dependencies(heap, method_registry, thread, args)
            }
            ("gradle/Project", "getDependencies") => self.handle_project_get_dependencies(heap),
            ("gradle/Project", "repositories") => {
                self.handle_project_repositories(heap, method_registry, thread, args)
            }
            ("gradle/Project", "plugins") => {
                self.handle_project_plugins(heap, method_registry, thread, args)
            }
            ("gradle/Project", "getPlugins") => self.handle_project_get_plugins(heap),
            ("gradle/Project", "compileThrift") => {
                self.handle_project_extension(heap, method_registry, thread, "compileThrift", args)
            }
            ("gradle/Project", "thrift") => {
                self.handle_project_extension(heap, method_registry, thread, "thrift", args)
            }
            ("gradle/Project", "forkedRun") => {
                self.handle_project_extension(heap, method_registry, thread, "forkedRun", args)
            }
            ("gradle/Project", "resolve") => {
                self.handle_project_extension(heap, method_registry, thread, "resolve", args)
            }
            ("gradle/Project", "openjpa") => {
                self.handle_project_extension(heap, method_registry, thread, "openjpa", args)
            }
            ("gradle/Project", "tasks") | ("gradle/Project", "getTasks") => {
                self.handle_project_tasks(heap)
            }
            ("gradle/Project", "sourceSets") | ("gradle/Project", "getSourceSets") => {
                self.handle_project_source_sets(heap)
            }
            ("gradle/Project", "getLayout") => self.handle_project_layout(heap),
            ("gradle/Project", "file") => self.handle_project_file(heap, args),
            ("gradle/Project", "uri") => self.handle_project_uri(heap, args),
            // `url = uri(...)` inside a maven { } block assigns the delegate's URL, which
            // reaches the owner as setUrl.
            ("gradle/Project", "setUrl") => self.handle_maven_set_url(heap, args),
            ("gradle/Project", "files") => self.handle_project_files(heap, args),
            ("gradle/Project", "getProjectDir") | ("gradle/Project", "getProjectDirectory") => {
                self.handle_project_project_dir(heap)
            }
            ("gradle/Project", "setGroup") => self.handle_project_set_group(heap, args),
            ("gradle/Project", "setVersion") => self.handle_project_set_version(heap, args),
            ("gradle/Project", "setDescription") => self.handle_project_set_description(heap, args),
            ("gradle/Project", "ext") => {
                self.handle_project_ext(heap, method_registry, thread, args)
            }
            ("gradle/Project", "getExtensions") => self.handle_project_extensions(heap),
            ("gradle/Project", "configure") => {
                self.handle_project_configure(heap, method_registry, thread, args)
            }
            ("gradle/Project", "subprojects") => {
                self.handle_project_subprojects(heap, method_registry, thread, args)
            }
            ("gradle/Project", "allprojects") => {
                self.handle_project_allprojects(heap, method_registry, thread, args)
            }
            ("gradle/Project", "buildscript") => {
                self.handle_project_buildscript(heap, method_registry, thread, args)
            }
            ("gradle/Project", "apply") => self.handle_project_apply(heap, args),
            ("gradle/Project", "task") => {
                self.handle_project_task(heap, method_registry, thread, args)
            }
            ("gradle/Project", "project") => self.handle_project_get_project(heap, args),
            ("gradle/Project", "rootProject") | ("gradle/Project", "getRootProject") => {
                self.handle_project_root_project(heap)
            }
            ("gradle/Project", "getRootDir") => self.handle_project_root_dir(heap),
            ("gradle/Project", "java") => {
                self.handle_project_java(heap, method_registry, thread, args)
            }
            ("gradle/Project", "kotlin") => {
                self.handle_project_kotlin(heap, method_registry, thread, args)
            }
            ("gradle/Project", "android") => {
                self.handle_project_android(heap, method_registry, thread, args)
            }
            ("gradle/Project", "configurations") => {
                self.handle_project_configurations(heap, method_registry, thread, args)
            }
            ("gradle/Project", "publishing") => {
                self.handle_project_publishing(heap, method_registry, thread, args)
            }
            ("gradle/Project", "test") => {
                self.handle_project_test(heap, method_registry, thread, args)
            }
            ("gradle/Project", "jar") => {
                self.handle_project_jar(heap, method_registry, thread, args)
            }
            ("gradle/Project", "bootJar") | ("gradle/Project", "bootRun") => {
                self.handle_project_spring_boot_task(heap, method_registry, thread, args)
            }
            ("gradle/Project", "springBoot") => {
                self.handle_project_extension(heap, method_registry, thread, "springBoot", args)
            }
            ("gradle/Project", "application") => {
                self.handle_project_extension(heap, method_registry, thread, "application", args)
            }
            ("gradle/Project", "processResources") => self.handle_project_task_config(
                heap,
                method_registry,
                thread,
                "processResources",
                args,
            ),
            ("gradle/Project", "compileJava") => {
                self.handle_project_task_config(heap, method_registry, thread, "compileJava", args)
            }
            ("gradle/Project", "compileKotlin") => self.handle_project_task_config(
                heap,
                method_registry,
                thread,
                "compileKotlin",
                args,
            ),
            ("gradle/Project", "compileTestJava") => self.handle_project_task_config(
                heap,
                method_registry,
                thread,
                "compileTestJava",
                args,
            ),
            ("gradle/Project", "compileTestKotlin") => self.handle_project_task_config(
                heap,
                method_registry,
                thread,
                "compileTestKotlin",
                args,
            ),
            ("gradle/Project", "getProperty") => {
                // getProperty("propName") - dispatch to the named property's handler
                if let Ok(prop_name) = self.extract_string_arg(heap, args, 0) {
                    return self.invoke_gradle_method(
                        heap,
                        method_registry,
                        thread,
                        "gradle/Project",
                        &prop_name,
                        descriptor,
                        &[],
                    );
                }
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/Project", method) if method.starts_with("get") => {
                self.handle_project_getter(heap, method)
            }

            // DependencyHandler stub methods
            ("gradle/DependencyHandler", "implementation") => {
                self.handle_dependency_configuration(heap, "implementation", args)
            }
            ("gradle/DependencyHandler", "testImplementation") => {
                self.handle_dependency_configuration(heap, "testImplementation", args)
            }
            ("gradle/DependencyHandler", "api") => {
                self.handle_dependency_configuration(heap, "api", args)
            }
            ("gradle/DependencyHandler", "runtimeOnly") => {
                self.handle_dependency_configuration(heap, "runtimeOnly", args)
            }
            ("gradle/DependencyHandler", "compileOnly") => {
                self.handle_dependency_configuration(heap, "compileOnly", args)
            }
            ("gradle/DependencyHandler", "annotationProcessor") => {
                self.handle_dependency_configuration(heap, "annotationProcessor", args)
            }
            ("gradle/DependencyHandler", "testCompileOnly") => {
                self.handle_dependency_configuration(heap, "testCompileOnly", args)
            }
            ("gradle/DependencyHandler", "testRuntimeOnly") => {
                self.handle_dependency_configuration(heap, "testRuntimeOnly", args)
            }
            ("gradle/DependencyHandler", "developmentOnly") => {
                self.handle_dependency_configuration(heap, "developmentOnly", args)
            }
            ("gradle/DependencyHandler", "kapt") => {
                self.handle_dependency_configuration(heap, "kapt", args)
            }
            ("gradle/DependencyHandler", "classpath") => {
                self.handle_dependency_configuration(heap, "classpath", args)
            }
            ("gradle/DependencyHandler", "constraints") => {
                self.handle_dependency_constraints(heap, method_registry, thread, args)
            }
            ("gradle/DependencyHandler", "add") => self.handle_dependency_add(heap, args),
            ("gradle/DependencyHandler", "project") => self.handle_dependency_project(heap, args),
            // Local jars and directories are classpath entries, not coordinates. The model does
            // not record them, but the call succeeds, so the other declarations are kept.
            ("gradle/DependencyHandler", "files") | ("gradle/DependencyHandler", "fileTree") => {
                let marker = heap.intern_string("file|")?;
                Ok(Some(Value::Reference(marker)))
            }
            ("gradle/DependencyHandler", "platform") => self.handle_dependency_platform(heap, args),
            ("gradle/DependencyHandler", "enforcedPlatform") => {
                self.handle_dependency_enforced_platform(heap, args)
            }
            ("gradle/DependencyConstraintHandler", method) => self
                .handle_dependency_constraint_method(heap, method_registry, thread, method, args),
            ("gradle/DependencyConstraint", "because") => {
                self.handle_dependency_constraint_reason(heap, args)
            }

            // RepositoryHandler stub methods
            ("gradle/RepositoryHandler", "mavenCentral") => self.handle_maven_central(),
            ("gradle/RepositoryHandler", "mavenLocal") => self.handle_maven_local(),
            ("gradle/RepositoryHandler", "google") => self.handle_google_repository(),
            ("gradle/RepositoryHandler", "maven") => {
                self.handle_maven_repository(heap, method_registry, thread, args)
            }
            ("gradle/RepositoryHandler", "ivy") => {
                self.handle_ivy_repository(heap, method_registry, thread, args)
            }
            ("gradle/RepositoryHandler", "jcenter") => self.handle_jcenter(),
            ("gradle/RepositoryHandler", "gradlePluginPortal") => {
                self.handle_gradle_plugin_portal()
            }
            ("gradle/RepositoryHandler", "flatDir") => {
                self.handle_flat_dir_repository(heap, method_registry, thread, args)
            }

            // MavenArtifactRepository stub methods (for maven { url '...' } blocks)
            ("gradle/MavenArtifactRepository", "url")
            | ("gradle/MavenArtifactRepository", "setUrl") => self.handle_maven_set_url(heap, args),
            ("gradle/MavenArtifactRepository", "name")
            | ("gradle/MavenArtifactRepository", "setName") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/MavenArtifactRepository", "credentials") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/MavenArtifactRepository", "authentication") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/MavenArtifactRepository", "mavenContent") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/MavenArtifactRepository", "metadataSources") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/MavenArtifactRepository", "content") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // PluginManager stub methods
            ("gradle/PluginManager", "id") => self.handle_plugin_id(heap, args),
            // version() is called on the result of id() but Groovy MOP may still use PluginManager as receiver
            ("gradle/PluginManager", "version") => self.handle_plugin_version(heap, args),
            ("gradle/PluginManager", "apply") => self.handle_plugin_apply(heap, args),

            // PluginSpec stub methods (for proper method chaining)
            ("gradle/PluginSpec", "version") => self.handle_plugin_version(heap, args),
            ("gradle/PluginSpec", "apply") => self.handle_plugin_apply(heap, args),

            // ScriptHandler stub methods (buildscript block)
            ("gradle/ScriptHandler", "dependencies") => {
                self.handle_project_dependencies(heap, method_registry, thread, args)
            }
            ("gradle/ScriptHandler", "repositories") => {
                self.handle_project_repositories(heap, method_registry, thread, args)
            }

            // ExtraPropertiesExtension stub methods (ext block)
            ("gradle/ExtraPropertiesExtension", "set") => self.handle_ext_set(heap, args),
            ("gradle/ExtraPropertiesExtension", "get") => self.handle_ext_get(heap, args),
            ("gradle/ExtraPropertiesExtension", method) => {
                // Dynamic property access - treat unknown methods as property setters
                self.handle_ext_dynamic_property(heap, method, args)
            }

            // ExtensionContainer stub methods
            ("gradle/ExtensionContainer", "getByName") => {
                self.handle_extension_get_by_name(heap, args)
            }
            ("gradle/ExtensionContainer", "findByName") => {
                self.handle_extension_get_by_name(heap, args)
            }
            ("gradle/ExtensionContainer", "configure") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // JavaPluginExtension stub methods
            ("gradle/JavaPluginExtension", "sourceCompatibility")
            | ("gradle/JavaPluginExtension", "setSourceCompatibility")
            | ("gradle/JavaPluginExtension", "targetCompatibility")
            | ("gradle/JavaPluginExtension", "setTargetCompatibility")
            | ("gradle/JavaPluginExtension", "toolchain")
            | ("gradle/JavaPluginExtension", "withJavadocJar")
            | ("gradle/JavaPluginExtension", "withSourcesJar") => {
                self.handle_java_extension_method(heap, method_name, args)
            }

            // JavaToolchainSpec stub methods
            ("gradle/JavaToolchainSpec", "languageVersion")
            | ("gradle/JavaToolchainSpec", "vendor")
            | ("gradle/JavaToolchainSpec", "implementation") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // ConfigurationContainer stub methods
            ("gradle/ConfigurationContainer", "create")
            | ("gradle/ConfigurationContainer", "register")
            | ("gradle/ConfigurationContainer", "getByName")
            | ("gradle/ConfigurationContainer", "named")
            | ("gradle/ConfigurationContainer", "maybeCreate") => {
                self.handle_configuration_container_method(heap, method_name, args)
            }
            ("gradle/ConfigurationContainer", method) => {
                // Dynamic configuration access like: configurations.compileOnly
                self.handle_configuration_access(heap, method)
            }

            // Configuration stub methods
            ("gradle/Configuration", "extendsFrom")
            | ("gradle/Configuration", "setTransitive")
            | ("gradle/Configuration", "setVisible")
            | ("gradle/Configuration", "setCanBeConsumed")
            | ("gradle/Configuration", "setCanBeResolved")
            | ("gradle/Configuration", "exclude")
            | ("gradle/Configuration", "resolutionStrategy") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // PublishingExtension stub methods
            ("gradle/PublishingExtension", "publications")
            | ("gradle/PublishingExtension", "repositories") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // Test task stub methods
            ("gradle/Test", "useJUnitPlatform")
            | ("gradle/Test", "useJUnit")
            | ("gradle/Test", "useTestNG")
            | ("gradle/Test", "testLogging")
            | ("gradle/Test", "maxParallelForks")
            | ("gradle/Test", "forkEvery")
            | ("gradle/Test", "jvmArgs")
            | ("gradle/Test", "systemProperty")
            | ("gradle/Test", "environment")
            | ("gradle/Test", "failFast") => Ok(Some(Value::Reference(ObjectRef::Null))),

            // Jar task stub methods
            ("gradle/Jar", "archiveBaseName")
            | ("gradle/Jar", "archiveVersion")
            | ("gradle/Jar", "archiveClassifier")
            | ("gradle/Jar", "manifest")
            | ("gradle/Jar", "from")
            | ("gradle/Jar", "into")
            | ("gradle/Jar", "exclude")
            | ("gradle/Jar", "include")
            | ("gradle/Jar", "duplicatesStrategy") => Ok(Some(Value::Reference(ObjectRef::Null))),

            // ProcessResources task stub methods
            ("gradle/ProcessResources", "filesMatching")
            | ("gradle/ProcessResources", "expand")
            | ("gradle/ProcessResources", "filter") => Ok(Some(Value::Reference(ObjectRef::Null))),

            // Compile task stub methods (Java/Kotlin)
            ("gradle/JavaCompile", "options")
            | ("gradle/JavaCompile", "sourceCompatibility")
            | ("gradle/JavaCompile", "targetCompatibility") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
            ("gradle/KotlinCompile", "kotlinOptions")
            | ("gradle/KotlinCompile", "compilerOptions") => {
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // File stub methods (for file() calls in build scripts)
            // The file system answers for the path the build script gave, so these are
            // not constants. A File whose path is unknown is an error, not a non-file.
            ("java/io/File", "exists") => {
                let path = self.known_file_path(args)?;
                Ok(Some(Value::Int(i32::from(
                    std::path::Path::new(&path).exists(),
                ))))
            }
            ("java/io/File", "isFile") => {
                let path = self.known_file_path(args)?;
                Ok(Some(Value::Int(i32::from(
                    std::path::Path::new(&path).is_file(),
                ))))
            }
            ("java/io/File", "isDirectory") => {
                let path = self.known_file_path(args)?;
                Ok(Some(Value::Int(i32::from(
                    std::path::Path::new(&path).is_dir(),
                ))))
            }
            ("java/io/File", "toString") => {
                let path = self
                    .file_path_from_args(args)
                    .unwrap_or_else(|| "/unknown".to_string());
                let path = heap.intern_string(&path)?;
                Ok(Some(Value::Reference(path)))
            }
            // Groovy's File.getText() / File.text read the whole file. A file that cannot
            // be read fails the build here, as it would in Groovy.
            ("java/io/File", "getText") | ("java/io/File", "text") => {
                let path = self.known_file_path(args)?;
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    VMError::internal(format!("cannot read {path} as text: {error}"))
                })?;
                Ok(Some(Value::Reference(heap.new_string(&text)?)))
            }
            ("java/io/File", "getAbsolutePath") | ("java/io/File", "getPath") => {
                let path = self
                    .file_path_from_args(args)
                    .unwrap_or_else(|| "/unknown".to_string());
                let path = heap.intern_string(&path)?;
                Ok(Some(Value::Reference(path)))
            }
            ("java/io/File", "getName") => {
                let name = self
                    .file_path_from_args(args)
                    .and_then(|path| {
                        std::path::Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                let name = heap.intern_string(&name)?;
                Ok(Some(Value::Reference(name)))
            }
            ("java/io/File", _) => {
                // Other File methods - return null for objects, 0 for primitives
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }

            // Unknown setter on gradle/Project - store as property, don't crash
            ("gradle/Project", method) if method.starts_with("set") && method.len() > 3 => {
                self.handle_project_set_property(heap, method, args)
            }

            // Unknown method on gradle/Project - treat as extension/task config
            ("gradle/Project", method) => {
                self.handle_project_extension(heap, method_registry, thread, method, args)
            }

            // Recording proxy: treat unknown DependencyHandler methods as custom
            // configurations (integrationTestImplementation, debugCompileOnly, etc.)
            ("gradle/DependencyHandler", method) if self.recording_proxy => {
                tracing::debug!(
                    "Recording proxy: treating DependencyHandler.{} as custom configuration",
                    method
                );
                self.handle_dependency_configuration(heap, method, args)
            }

            // Recording proxy: create proxy stub for unknown methods, execute closures
            _ if self.recording_proxy => self.handle_recording_proxy(
                heap,
                method_registry,
                thread,
                class_name,
                method_name,
                args,
            ),

            // Unknown Gradle method on other classes - return null stub
            _ => {
                tracing::debug!(
                    "Gradle stub not implemented: {}.{} - returning null",
                    class_name,
                    method_name
                );
                Ok(Some(Value::Reference(ObjectRef::Null)))
            }
        }
    }

    /// Recording proxy handler: creates a proxy stub for unknown class/method
    /// combinations and attempts to execute any closure arguments against it.
    ///
    /// This enables best-effort extraction from DSL blocks that we don't have
    /// explicit stubs for — custom plugins, Android build types, test suites, etc.
    fn handle_recording_proxy(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        class_name: &str,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        // Check if there's a closure argument we can execute
        if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
            let proxy_class = format!("gradle/Proxy/{}/{}", class_name, method_name);

            let proxy_id = heap.allocate_object(ClassRef {
                name: proxy_class,
                instance_field_count: 0,
                static_field_count: 0,
            })?;

            // Only execute each closure once — re-execution indicates a dispatch loop
            if self.try_mark_closure_executed(closure_id) {
                tracing::debug!(
                    "Recording proxy: creating stub for '{}' and executing closure",
                    class_name
                );
                ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(proxy_id))?;
                ClosureRuntime::set_resolve_strategy(
                    heap,
                    closure_id,
                    ResolveStrategy::DelegateFirst,
                )?;
                ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
            } else {
                tracing::debug!(
                    "Recording proxy: skipping re-execution of closure for '{}'",
                    class_name
                );
            }

            return Ok(Some(Value::Reference(ObjectRef::Object(proxy_id))));
        }

        // No closure — check if there's a string arg on a DependencyHandler-like class
        // that we might be able to extract as a dependency notation
        if class_name.starts_with("gradle/Dependency")
            || class_name.starts_with("gradle/Proxy/gradle/DependencyHandler")
        {
            if let Ok(notation) = self.extract_string_arg(heap, args, 0) {
                tracing::debug!(
                    "Recording proxy: extracted dependency notation '{}' from {}.{}",
                    notation,
                    class_name,
                    method_name
                );
                let handler = self.stub_registry.stubs().project().dependency_handler();
                handler.add(method_name, &notation);
            }
        }

        // Return a proxy stub (not null) so method chains don't NPE.
        // In recording proxy mode we never want to return null from a method call
        // because Groovy scripts frequently chain calls: `foo().bar().baz()`.
        let proxy_class = format!("gradle/Proxy/{}/{}", class_name, method_name);
        tracing::debug!(
            "Recording proxy: {}.{} - returning proxy stub",
            class_name,
            method_name
        );
        let proxy_id = heap.allocate_object(ClassRef {
            name: proxy_class,
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(proxy_id))))
    }

    /// Handle Project.dependencies(Closure) call
    ///
    /// DSL Pattern:
    /// ```groovy
    /// dependencies {
    ///     implementation "com.google.guava:guava:32.1.2"
    /// }
    /// ```
    ///
    /// Implementation:
    /// 1. Extract closure from args
    /// 2. Create DependencyHandler stub object on heap
    /// 3. Set closure.delegate = DependencyHandler
    /// 4. Set closure.resolveStrategy = DELEGATE_FIRST (Gradle default)
    /// 5. Invoke closure.doCall() by pushing frame onto thread
    /// 6. Configuration captured via DependencyHandler stub calls during execution
    fn handle_project_dependencies(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        // Extract closure argument
        let closure_id = match self.extract_closure_arg(heap, args)? {
            Some(id) => id,
            None => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        // Create DependencyHandler stub object on heap
        let dep_handler_class = ClassRef {
            name: "gradle/DependencyHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let dep_handler_id = heap.allocate_object(dep_handler_class)?;

        // Configure closure for Gradle DSL
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(dep_handler_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Invoke closure (pushes frame, executor continues)
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        // Return DependencyHandler reference (for chaining)
        Ok(Some(Value::Reference(ObjectRef::Object(dep_handler_id))))
    }

    fn handle_project_get_dependencies(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;
        let handler_id = heap.allocate_object(ClassRef {
            name: "org/gradle/api/artifacts/dsl/DependencyHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(handler_id))))
    }

    fn handle_project_get_plugins(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;
        let container_id = heap.allocate_object(ClassRef {
            name: "org/gradle/api/plugins/PluginContainer".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(container_id))))
    }

    fn handle_project_root_dir(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        let path = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
            .project_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let file_id = heap.allocate_object(super::heap::ClassRef {
            name: "java/io/File".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        })?;

        let key = format!("file.{}.path", file_id);
        if let Ok(mut config) = self.config.lock() {
            config.set_property(&key, &path.display().to_string());
        }

        Ok(Some(Value::Reference(ObjectRef::Object(file_id))))
    }

    fn handle_dependency_add(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        let mut strings = Vec::new();
        for i in 0..args.len() {
            if let Ok(s) = self.extract_string_arg(heap, args, i) {
                strings.push(s);
            }
        }

        let (configuration, notation) = if strings.len() >= 2 {
            (strings[0].clone(), strings[1].clone())
        } else if args.len() >= 3 {
            (
                self.extract_string_arg(heap, args, 1)
                    .unwrap_or_else(|_| "implementation".to_string()),
                self.value_to_string_lossy(heap, &args[2])?,
            )
        } else {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        };

        if notation.is_empty() {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        let handler = self.stub_registry.stubs().project().dependency_handler();
        if notation.starts_with(':') {
            handler.project(&configuration, &notation);
        } else {
            handler.add(&configuration, &notation);
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn invoke_action_execute(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        action_id: super::value::HeapId,
        arg: Value,
    ) -> VMResult<()> {
        let class_name = heap.get_object_class_name(action_id)?;
        let candidates = ["(Lorg/gradle/api/Project;)V", "(Ljava/lang/Object;)V"];
        let mut chosen = None;
        for desc in candidates {
            let key = (class_name.clone(), "execute".to_string(), desc.to_string());
            if let Some(bytecode) = method_registry.get(&key) {
                chosen = Some((desc.to_string(), bytecode.clone()));
                break;
            }
        }

        let Some((descriptor, bytecode)) = chosen else {
            return Ok(());
        };

        let frame = super::frame::Frame::with_bytecode(
            class_name,
            "execute".to_string(),
            bytecode.max_locals,
            bytecode.max_stack,
            vec![Value::Reference(ObjectRef::Object(action_id)), arg],
            bytecode.code,
            descriptor,
            bytecode.exception_table,
        )?;
        thread.push_frame(frame)?;
        Ok(())
    }

    fn handle_project_extension(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        extension_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        let extension_id = heap.allocate_object(ClassRef {
            name: Self::extension_class_name(extension_name),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
            ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(extension_id))?;
            ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
            ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
        }

        Ok(Some(Value::Reference(ObjectRef::Object(extension_id))))
    }

    /// Handle Project.repositories(Closure) call
    fn handle_project_repositories(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Err(VMError::internal("Expected closure argument")),
        };

        // Create RepositoryHandler stub object
        let repo_handler_class = ClassRef {
            name: "gradle/RepositoryHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let repo_handler_id = heap.allocate_object(repo_handler_class)?;

        // Configure closure
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(repo_handler_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Invoke closure
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(repo_handler_id))))
    }

    /// Handle Project.plugins(Closure) call
    fn handle_project_plugins(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        if let Ok(plugin_id) = self.extract_string_arg(heap, args, 0) {
            let mut config = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;
            config.plugin_definitions.push(Plugin {
                id: plugin_id,
                version: None,
                apply: true,
            });
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Err(VMError::internal("Expected closure argument")),
        };

        // Create PluginManager stub object
        let plugin_mgr_class = ClassRef {
            name: "gradle/PluginManager".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let plugin_mgr_id = heap.allocate_object(plugin_mgr_class)?;

        // Configure closure
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(plugin_mgr_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Invoke closure
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(plugin_mgr_id))))
    }

    fn handle_project_tasks(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;

        let tasks_id = heap.allocate_object(ClassRef {
            name: "gradle/TaskContainer".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        Ok(Some(Value::Reference(ObjectRef::Object(tasks_id))))
    }

    fn handle_project_source_sets(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;

        let source_sets_id = heap.allocate_object(ClassRef {
            name: "gradle/SourceSetContainer".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        Ok(Some(Value::Reference(ObjectRef::Object(source_sets_id))))
    }

    fn handle_project_layout(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;

        let layout_id = heap.allocate_object(ClassRef {
            name: "gradle/Layout".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        Ok(Some(Value::Reference(ObjectRef::Object(layout_id))))
    }

    fn handle_project_project_dir(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        let path = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
            .project_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let path = heap.intern_string(&path.display().to_string())?;
        Ok(Some(Value::Reference(path)))
    }

    /// Project.uri(Object): the text of the argument, which repository URL setters take.
    fn handle_project_uri(&self, heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
        let text = self.extract_string_arg(heap, args, 0)?;
        Ok(Some(Value::Reference(heap.new_string(&text)?)))
    }

    fn handle_project_file(&self, heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
        let path = if args.is_empty() {
            "unknown".to_string()
        } else if let Ok(path) = self.extract_string_arg(heap, args, 0) {
            // Project.file resolves a relative path against the project directory.
            let project_dir = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .project_dir
                .clone();
            match project_dir {
                Some(dir) if std::path::Path::new(&path).is_relative() => {
                    dir.join(&path).display().to_string()
                }
                _ => path,
            }
        } else if let Some(Value::Reference(ObjectRef::Object(id))) = args.first() {
            self.file_path_for_id(*id)
                .unwrap_or_else(|| "unknown".to_string())
        } else {
            "unknown".to_string()
        };

        // Create a File stub object that can handle exists(), getText(), etc.
        let file_id = heap.allocate_object(super::heap::ClassRef {
            name: "java/io/File".to_string(),
            instance_field_count: 1,
            static_field_count: 0,
        })?;

        // Store the path in config for potential later use
        let key = format!("file.{}.path", file_id);
        if let Ok(mut config) = self.config.lock() {
            config.set_property(&key, &path);
        }

        Ok(Some(Value::Reference(ObjectRef::Object(file_id))))
    }

    fn handle_project_files(&self, heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
        use super::heap::ClassRef;

        let mut items = Vec::new();
        for value in args {
            items.push(self.value_to_string_lossy(heap, value)?);
        }
        let collection_id = heap.allocate_object(ClassRef {
            name: "gradle/FileCollection".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        if !items.is_empty() {
            let key = format!("fileCollection.{}", collection_id);
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .set_property(&key, &items.join(","));
        }

        Ok(Some(Value::Reference(ObjectRef::Object(collection_id))))
    }

    fn handle_project_set_group(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if let Some(value) = args.first() {
            let group = self.value_to_string_lossy(heap, value)?;
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .group = Some(group);
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_project_set_description(
        &self,
        heap: &Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if let Some(value) = args.first() {
            let description = self.value_to_string_lossy(heap, value)?;
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .description = Some(description);
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Generic handler for unknown `set*` property calls on the project.
    /// Stores the value as an extra property rather than crashing.
    fn handle_project_set_property(
        &self,
        heap: &Heap,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if let Some(value) = args.first() {
            let prop_value = self.value_to_string_lossy(heap, value)?;
            let prop_name = Self::decapitalize(&method_name[3..])
                .unwrap_or_else(|| method_name[3..].to_string());
            tracing::debug!(
                "Project.{}({}) - storing as extra property '{}'",
                method_name,
                prop_value,
                prop_name
            );
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .extra_properties
                .insert(prop_name, prop_value);
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_project_set_version(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if let Some(value) = args.first() {
            let version = self.value_to_string_lossy(heap, value)?;
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .version = Some(version);
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_project_getter(&self, heap: &mut Heap, method_name: &str) -> VMResult<Option<Value>> {
        let Some(name) = Self::decapitalize(&method_name[3..]) else {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        };

        self.config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
            .add_task(Task::new(&name));

        let task_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Task".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        Ok(Some(Value::Reference(ObjectRef::Object(task_id))))
    }

    /// Handle Project.ext(Closure) call - extra properties block
    ///
    /// DSL Pattern:
    /// ```groovy
    /// ext {
    ///     springBootVersion = '3.0.0'
    ///     kotlinVersion = '1.9.0'
    /// }
    /// ```
    fn handle_project_ext(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        // If no closure provided, return the ExtraPropertiesExtension object
        if args.is_empty() {
            let ext_id = heap.allocate_object(ClassRef {
                name: "gradle/ExtraPropertiesExtension".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(ext_id))));
        }

        // Extract closure
        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        // Create ExtraPropertiesExtension stub object
        let ext_class = ClassRef {
            name: "gradle/ExtraPropertiesExtension".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let ext_id = heap.allocate_object(ext_class)?;

        // Set up closure delegation
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(ext_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Execute closure
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(ext_id))))
    }

    /// Handle Project.getExtensions() call
    fn handle_project_extensions(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        let ext_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/ExtensionContainer".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(ext_id))))
    }

    /// Handle Project.configure(Closure) call
    fn handle_project_configure(
        &self,
        _heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Configure blocks are used for custom configuration - we just skip them
        // as they don't affect dependency/plugin extraction
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle Project.subprojects(Closure) call
    ///
    /// DSL Pattern:
    /// ```groovy
    /// subprojects {
    ///     apply plugin: 'java'
    /// }
    /// ```
    fn handle_project_subprojects(
        &self,
        heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if !self.execute_subprojects {
            // Default behavior for build script execution: skip cross-project execution and rely on
            // parsing each subproject build file separately.
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        if args.len() < 2 {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        let receiver_project = args[0].clone();

        // Closure-based form: subprojects { ... }
        if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
            use super::closure_runtime::{ClosureRuntime, ResolveStrategy};

            if !self.try_mark_closure_executed(closure_id) {
                return Ok(Some(Value::Reference(ObjectRef::Null)));
            }

            let delegate = match receiver_project {
                Value::Reference(ObjectRef::Object(id)) => ObjectRef::Object(id),
                _ => ObjectRef::Null,
            };
            ClosureRuntime::set_delegate(heap, closure_id, delegate)?;
            ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
            ClosureRuntime::invoke_do_call(heap, _method_registry, _thread, closure_id, vec![])?;
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        // Action-based form (Kotlin): subprojects { } compiles to Action<Project>
        if let Some(Value::Reference(ObjectRef::Object(action_id))) = args.last() {
            self.invoke_action_execute(
                heap,
                _method_registry,
                _thread,
                *action_id,
                receiver_project,
            )?;
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle Project.allprojects(Closure) call
    ///
    /// DSL Pattern:
    /// ```groovy
    /// allprojects {
    ///     repositories {
    ///         mavenCentral()
    ///     }
    /// }
    /// ```
    fn handle_project_allprojects(
        &self,
        _heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Allprojects closures configure all projects including root
        // We skip execution since we parse each project's build.gradle separately
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle Project.buildscript(Closure) call
    ///
    /// DSL Pattern:
    /// ```groovy
    /// buildscript {
    ///     repositories { mavenCentral() }
    ///     dependencies {
    ///         classpath 'org.springframework.boot:spring-boot-gradle-plugin:3.0.0'
    ///     }
    /// }
    /// ```
    fn handle_project_buildscript(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        // Extract closure
        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        // Create ScriptHandler stub object (buildscript delegate)
        let script_handler_class = ClassRef {
            name: "gradle/ScriptHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let script_handler_id = heap.allocate_object(script_handler_class)?;

        // Set up closure delegation
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(script_handler_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Execute closure to capture buildscript dependencies
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(script_handler_id))))
    }

    /// Handle DependencyHandler.{configuration}(String) call
    ///
    /// Parses Maven coordinate string "group:artifact:version" and records dependency
    fn handle_dependency_configuration(
        &self,
        heap: &Heap,
        configuration: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let notations = self.extract_dependency_notations(heap, args, 0)?;
        let handler = self.stub_registry.stubs().project().dependency_handler();

        for notation in notations {
            if notation.starts_with("file|") {
                continue;
            }
            if let Some(stripped) = notation.strip_prefix("platform|") {
                handler.platform(configuration, stripped);
                continue;
            }
            if let Some(stripped) = notation.strip_prefix("enforcedPlatform|") {
                handler.enforced_platform(configuration, stripped);
                continue;
            }
            if notation.starts_with(':') {
                handler.project(configuration, &notation);
                continue;
            }

            handler.add(configuration, &notation);
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_dependency_constraints(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        let handler_id = heap.allocate_object(ClassRef {
            name: "gradle/DependencyConstraintHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Err(VMError::internal("Expected closure argument")),
        };

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(handler_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(handler_id))))
    }

    fn handle_dependency_constraint_method(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        method: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let notations = self.extract_dependency_notations(heap, args, 0)?;
        let handler = self.stub_registry.stubs().project().dependency_handler();
        for notation in notations {
            handler.add(method, &notation);
            let key = "constraint.last".to_string();
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .set_property(&key, &notation);
        }

        if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
            use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
            let constraint_id = heap.allocate_object(super::heap::ClassRef {
                name: "gradle/DependencyConstraint".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(constraint_id))?;
            ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
            ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
            return Ok(Some(Value::Reference(ObjectRef::Object(constraint_id))));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_dependency_constraint_reason(
        &self,
        heap: &Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let reason = self.extract_string_arg(heap, args, 0)?;
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;
        let key = if let Some(last) = config.get_property("constraint.last").cloned() {
            format!("constraint.reason.{}", last)
        } else {
            "constraint.reason".to_string()
        };
        config.set_property(&key, &reason);
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_dependency_project(
        &self,
        heap: &mut Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let project_path = self.extract_string_arg(heap, args, 0)?;
        let obj = heap.intern_string(&project_path)?;
        Ok(Some(Value::Reference(obj)))
    }

    fn handle_dependency_platform(
        &self,
        heap: &mut Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let notation = self.extract_string_arg(heap, args, 0)?;
        let obj = heap.intern_string(&format!("platform|{}", notation))?;
        Ok(Some(Value::Reference(obj)))
    }

    fn handle_dependency_enforced_platform(
        &self,
        heap: &mut Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let notation = self.extract_string_arg(heap, args, 0)?;
        let obj = heap.intern_string(&format!("enforcedPlatform|{}", notation))?;
        Ok(Some(Value::Reference(obj)))
    }

    /// Handle RepositoryHandler.mavenCentral() call
    fn handle_maven_central(&self) -> VMResult<Option<Value>> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        config.add_repository(Repository::maven_central());

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.mavenLocal() call
    fn handle_maven_local(&self) -> VMResult<Option<Value>> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        config.add_repository(Repository::maven_local());

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.google() call
    fn handle_google_repository(&self) -> VMResult<Option<Value>> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        config.add_repository(Repository::google());

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.jcenter() - deprecated but still used
    fn handle_jcenter(&self) -> VMResult<Option<Value>> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        config.add_repository(Repository {
            name: "jcenter".to_string(),
            url: "https://jcenter.bintray.com/".to_string(),
            repo_type: RepositoryType::Maven,
        });

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.gradlePluginPortal()
    fn handle_gradle_plugin_portal(&self) -> VMResult<Option<Value>> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        config.add_repository(Repository::gradle_plugin_portal());

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.maven(Closure) - custom Maven repository
    fn handle_maven_repository(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        // Handle maven { url '...' } pattern
        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            // If it's a string argument, treat as URL directly
            Some(Value::Reference(ObjectRef::Null)) => {
                return Ok(Some(Value::Reference(ObjectRef::Null)));
            }
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        // Create MavenArtifactRepository stub
        let maven_repo_class = ClassRef {
            name: "gradle/MavenArtifactRepository".to_string(),
            instance_field_count: 1, // Store URL
            static_field_count: 0,
        };
        let maven_repo_id = heap.allocate_object(maven_repo_class)?;

        // Configure closure
        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(maven_repo_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;

        // Execute closure
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(maven_repo_id))))
    }

    /// Handle RepositoryHandler.ivy(Closure) - Ivy repository
    fn handle_ivy_repository(
        &self,
        _heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Ivy repos are less common, just acknowledge
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle RepositoryHandler.flatDir(Closure) - local directory repository
    fn handle_flat_dir_repository(
        &self,
        _heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        // flatDir repos are for local JARs
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle MavenArtifactRepository.setUrl(String) - set repository URL
    fn handle_maven_set_url(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if let Ok(url) = self.extract_string_arg(heap, args, 0) {
            let mut config = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

            let name = format!("maven-{}", config.repository_list.len());
            config.add_repository(Repository {
                name,
                url,
                repo_type: RepositoryType::Maven,
            });
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle Project.apply(Map) - legacy plugin application
    /// e.g., apply plugin: 'java'
    fn handle_project_apply(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if args.is_empty() {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        // Gradle legacy plugin application is typically called with named args:
        //   apply plugin: 'java'
        // which Groovy lowers to apply([plugin: 'java']).
        //
        // Try to extract plugin id from either:
        // - apply('java') (string argument)
        // - apply([plugin: 'java']) (map argument)
        let plugin_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => {
                if let Some(HeapEntry::Collection(collection)) = heap.get_entry(*id) {
                    if let CollectionData::HashMap(map) = &collection.data {
                        let mut found: Option<String> = None;
                        for (key, value) in map {
                            let Ok(key_str) = self.value_to_string(heap, key) else {
                                continue;
                            };
                            if key_str == "plugin" {
                                if let Ok(value_str) = self.value_to_string(heap, value) {
                                    found = Some(value_str);
                                }
                                break;
                            }
                        }
                        if found.is_some() {
                            found
                        } else {
                            // Not a plugin application map; fall back below.
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    // Not a collection; try treating it as a String object.
                    self.extract_string_arg(heap, args, 0).ok()
                }
            }
            _ => self.extract_string_arg(heap, args, 0).ok(),
        };

        if let Some(plugin_id) = plugin_id {
            let mut config = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;
            config.add_plugin(Plugin {
                id: plugin_id,
                version: None,
                apply: true,
            });
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle Project.task(String/Map/Closure) - task definition
    fn handle_project_task(
        &self,
        heap: &mut Heap,
        _method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        _thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        // The name is the first string argument: `task('name')`, or `task(type: X, 'name')`
        // with a map first. A call with no string name is not a task we can register.
        let task_name = (0..args.len())
            .find_map(|index| self.extract_string_arg(heap, args, index).ok())
            .ok_or_else(|| VMError::internal("Project.task() has no string name argument"))?;

        // Register the task
        if let Ok(mut config) = self.config.lock() {
            config.add_task(Task::new(&task_name));
        }

        // Return a Task stub
        let task_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Task".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        Ok(Some(Value::Reference(ObjectRef::Object(task_id))))
    }

    /// Handle Project.project(String) - get subproject
    fn handle_project_get_project(
        &self,
        heap: &mut Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Just return a Project stub - we don't need to actually resolve the project
        let project_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Project".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        // Register the subproject name if provided
        if let Ok(subproject_name) = self.extract_string_arg(heap, args, 0) {
            if let Ok(mut config) = self.config.lock() {
                if !config.subprojects.contains(&subproject_name) {
                    config.subprojects.push(subproject_name);
                }
            }
        }

        Ok(Some(Value::Reference(ObjectRef::Object(project_id))))
    }

    /// Handle Project.rootProject / getRootProject() - get root project
    fn handle_project_root_project(&self, heap: &mut Heap) -> VMResult<Option<Value>> {
        // Return a Project stub representing the root project
        let project_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Project".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(project_id))))
    }

    /// Handle Project.java(Closure) - Java plugin extension
    fn handle_project_java(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let java_ext_id = heap.allocate_object(ClassRef {
                name: "gradle/JavaPluginExtension".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(java_ext_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let java_ext_id = heap.allocate_object(ClassRef {
            name: "gradle/JavaPluginExtension".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(java_ext_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(java_ext_id))))
    }

    /// Handle Project.kotlin(Closure) - Kotlin plugin extension
    fn handle_project_kotlin(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let kotlin_ext_id = heap.allocate_object(ClassRef {
                name: "gradle/KotlinProjectExtension".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(kotlin_ext_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let kotlin_ext_id = heap.allocate_object(ClassRef {
            name: "gradle/KotlinProjectExtension".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(kotlin_ext_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(kotlin_ext_id))))
    }

    /// Handle Project.android(Closure) - Android plugin extension
    fn handle_project_android(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let android_ext_id = heap.allocate_object(ClassRef {
                name: "gradle/AndroidExtension".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(android_ext_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let android_ext_id = heap.allocate_object(ClassRef {
            name: "gradle/AndroidExtension".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(android_ext_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(android_ext_id))))
    }

    /// Handle Project.configurations(Closure) - configuration container
    fn handle_project_configurations(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let config_container_id = heap.allocate_object(ClassRef {
                name: "gradle/ConfigurationContainer".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(
                config_container_id,
            ))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let config_container_id = heap.allocate_object(ClassRef {
            name: "gradle/ConfigurationContainer".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(config_container_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(
            config_container_id,
        ))))
    }

    /// Handle Project.publishing(Closure) - Maven publishing extension
    fn handle_project_publishing(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let pub_ext_id = heap.allocate_object(ClassRef {
                name: "gradle/PublishingExtension".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(pub_ext_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let pub_ext_id = heap.allocate_object(ClassRef {
            name: "gradle/PublishingExtension".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(pub_ext_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(pub_ext_id))))
    }

    /// Handle Project.test(Closure) - Test task configuration
    fn handle_project_test(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let test_task_id = heap.allocate_object(ClassRef {
                name: "gradle/Test".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(test_task_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let test_task_id = heap.allocate_object(ClassRef {
            name: "gradle/Test".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(test_task_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(test_task_id))))
    }

    /// Handle Project.jar(Closure) - Jar task configuration
    fn handle_project_jar(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            let jar_task_id = heap.allocate_object(ClassRef {
                name: "gradle/Jar".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(jar_task_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let jar_task_id = heap.allocate_object(ClassRef {
            name: "gradle/Jar".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(jar_task_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(jar_task_id))))
    }

    /// Handle Spring Boot task configurations (bootJar, bootRun)
    fn handle_project_spring_boot_task(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        if args.is_empty() {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let task_id = heap.allocate_object(ClassRef {
            name: "gradle/BootTask".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(task_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(task_id))))
    }

    /// Handle generic task configuration (processResources, compileJava, etc.)
    fn handle_project_task_config(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        task_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::heap::ClassRef;

        // Determine task type based on name
        let task_class = match task_name {
            "processResources" => "gradle/ProcessResources",
            "compileJava" | "compileTestJava" => "gradle/JavaCompile",
            "compileKotlin" | "compileTestKotlin" => "gradle/KotlinCompile",
            _ => "gradle/Task",
        };

        if args.is_empty() {
            let task_id = heap.allocate_object(ClassRef {
                name: task_class.to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(task_id))));
        }

        let closure_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Ok(Some(Value::Reference(ObjectRef::Null))),
        };

        let task_id = heap.allocate_object(ClassRef {
            name: task_class.to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(task_id))?;
        ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
        ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;

        Ok(Some(Value::Reference(ObjectRef::Object(task_id))))
    }

    /// Handle JavaPluginExtension methods
    fn handle_java_extension_method(
        &self,
        _heap: &Heap,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Extract Java version if provided
        if (method_name == "sourceCompatibility"
            || method_name == "setSourceCompatibility"
            || method_name == "targetCompatibility"
            || method_name == "setTargetCompatibility")
            && !args.is_empty()
        {
            if let Some(Value::Reference(ObjectRef::Object(_))) = args.first() {
                // Could extract version here if needed
            }
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle ConfigurationContainer methods
    fn handle_configuration_container_method(
        &self,
        heap: &mut Heap,
        _method_name: &str,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        // Return a Configuration object
        let config_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Configuration".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(config_id))))
    }

    /// Handle dynamic configuration access (e.g., configurations.compileOnly)
    fn handle_configuration_access(
        &self,
        heap: &mut Heap,
        _config_name: &str,
    ) -> VMResult<Option<Value>> {
        let config_id = heap.allocate_object(super::heap::ClassRef {
            name: "gradle/Configuration".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;
        Ok(Some(Value::Reference(ObjectRef::Object(config_id))))
    }

    fn handle_extension_method(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        extension: &str,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if extension == "openjpa" && method_name == "metamodel" {
            return self.handle_nested_extension(
                heap,
                method_registry,
                thread,
                extension,
                "metamodel",
                args,
            );
        }

        if let Some(stripped) = method_name.strip_prefix("set") {
            let Some(prop) = Self::decapitalize(stripped) else {
                return Ok(Some(Value::Reference(ObjectRef::Null)));
            };
            let value = args
                .first()
                .map(|v| self.value_to_string_lossy(heap, v))
                .transpose()?
                .unwrap_or_default();
            self.set_extension_property(extension, &prop, &value)?;
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        if let Some(stripped) = method_name.strip_prefix("get") {
            let Some(prop) = Self::decapitalize(stripped) else {
                return Ok(Some(Value::Reference(ObjectRef::Null)));
            };
            let value = self.get_extension_property(extension, &prop);
            let obj = heap.intern_string(value.as_deref().unwrap_or(""))?;
            return Ok(Some(Value::Reference(obj)));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_nested_extension(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        parent: &str,
        nested: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let name = format!("{}.{}", parent, nested);
        let extension_id = heap.allocate_object(super::heap::ClassRef {
            name: Self::extension_class_name(&name),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
            use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
            ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(extension_id))?;
            ClosureRuntime::set_resolve_strategy(heap, closure_id, ResolveStrategy::DelegateFirst)?;
            ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
        }

        Ok(Some(Value::Reference(ObjectRef::Object(extension_id))))
    }

    fn handle_task_method(
        &self,
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        class_name: &str,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if class_name == "gradle/TaskContainer" && method_name == "withType" {
            let task_type = args
                .first()
                .map(|v| self.value_to_string_lossy(heap, v))
                .transpose()?
                .unwrap_or_else(|| "Task".to_string());
            let task_name = format!("withType:{}", task_type);
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .add_task(Task::typed(&task_name, &task_type));
            let task_id = heap.allocate_object(super::heap::ClassRef {
                name: "gradle/Task".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
                use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
                ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(task_id))?;
                ClosureRuntime::set_resolve_strategy(
                    heap,
                    closure_id,
                    ResolveStrategy::DelegateFirst,
                )?;
                ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
            }
            return Ok(Some(Value::Reference(ObjectRef::Object(task_id))));
        }

        if class_name == "gradle/TaskContainer" && method_name == "named" {
            let task_name = self
                .extract_string_arg(heap, args, 0)
                .unwrap_or_else(|_| "named".to_string());
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .add_task(Task::new(&task_name));
            let task_id = heap.allocate_object(super::heap::ClassRef {
                name: "gradle/Task".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            if let Some(closure_id) = self.extract_closure_arg(heap, args)? {
                use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
                ClosureRuntime::set_delegate(heap, closure_id, ObjectRef::Object(task_id))?;
                ClosureRuntime::set_resolve_strategy(
                    heap,
                    closure_id,
                    ResolveStrategy::DelegateFirst,
                )?;
                ClosureRuntime::invoke_do_call(heap, method_registry, thread, closure_id, vec![])?;
            }
            return Ok(Some(Value::Reference(ObjectRef::Object(task_id))));
        }

        if class_name == "gradle/Task" && (method_name == "doFirst" || method_name == "doLast") {
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        if class_name == "gradle/Task" && method_name == "dependsOn" {
            let deps = self.extract_dependency_notations(heap, args, 0)?;
            let key = "task.dependsOn".to_string();
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .set_property(&key, &deps.join(","));
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_source_set_container(
        &self,
        heap: &mut Heap,
        method_name: &str,
    ) -> VMResult<Option<Value>> {
        if let Some(stripped) = method_name.strip_prefix("get") {
            let Some(name) = Self::decapitalize(stripped) else {
                return Ok(Some(Value::Reference(ObjectRef::Null)));
            };
            let class_name = format!("gradle/SourceSet/{}", name);
            let source_set_id = heap.allocate_object(super::heap::ClassRef {
                name: class_name,
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(source_set_id))));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_source_set_method(
        &self,
        heap: &mut Heap,
        class_name: &str,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if class_name.starts_with("gradle/SourceSet/") && method_name == "getJava" {
            let source_set = class_name.trim_start_matches("gradle/SourceSet/");
            let class_name = format!("gradle/SourceDirectorySet/{}/java", source_set);
            let dir_set_id = heap.allocate_object(super::heap::ClassRef {
                name: class_name,
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(dir_set_id))));
        }

        if class_name.starts_with("gradle/SourceSet/") && method_name == "getRuntimeClasspath" {
            let class_name = "gradle/FileCollection".to_string();
            let file_collection_id = heap.allocate_object(super::heap::ClassRef {
                name: class_name,
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(
                file_collection_id,
            ))));
        }

        if class_name.starts_with("gradle/SourceDirectorySet/") && method_name == "srcDirs" {
            let key = format!("sourceSet.{}.srcDirs", class_name);
            let mut dirs = Vec::new();
            for value in args {
                if let Value::Reference(ObjectRef::Object(id)) = value {
                    let class_name = heap.get_object_class_name(*id)?;
                    if class_name == "gradle/FileCollection" {
                        if let Some(items) = self.file_collection_items(*id) {
                            dirs.extend(items);
                            continue;
                        }
                    }
                }
                dirs.push(self.value_to_string_lossy(heap, value)?);
            }
            self.config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .set_property(&key, &dirs.join(","));
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_file_collection(
        &self,
        heap: &mut Heap,
        method_name: &str,
        _args: &[Value],
    ) -> VMResult<Option<Value>> {
        if method_name == "builtBy" {
            let file_collection_id = heap.allocate_object(super::heap::ClassRef {
                name: "gradle/FileCollection".to_string(),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(
                file_collection_id,
            ))));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    fn handle_layout(&self, heap: &mut Heap, method_name: &str) -> VMResult<Option<Value>> {
        if method_name == "getBuildDirectory" {
            let build_dir = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
                .build_dir
                .clone()
                .unwrap_or_else(|| std::path::PathBuf::from("build"));
            let obj = heap.intern_string(&build_dir.display().to_string())?;
            return Ok(Some(Value::Reference(obj)));
        }

        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle PluginManager.id(String) call
    ///
    /// Returns a reference to self to enable method chaining (e.g., id("x").version("1.0"))
    fn handle_plugin_id(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        let plugin_id = self.extract_string_arg(heap, args, 0)?;

        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        // Add the plugin using add_plugin to update both plugins and plugin_definitions
        // version() will update the last added plugin if needed
        config.add_plugin(Plugin {
            id: plugin_id,
            version: None,
            apply: true,
        });

        // Return self reference to enable method chaining
        // The next call (like version()) will operate on the last added plugin
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle PluginSpec.version(String) call
    ///
    /// Updates the version of the most recently added plugin
    fn handle_plugin_version(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        let version = self.extract_string_arg(heap, args, 0)?;

        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        // Update the version of the most recently added plugin
        if let Some(last_plugin) = config.plugin_definitions.last_mut() {
            last_plugin.version = Some(version);
            tracing::debug!(
                "Set plugin version: {} -> {:?}",
                last_plugin.id,
                last_plugin.version
            );
        }

        // Return self reference to enable method chaining
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle PluginSpec.apply(boolean) call
    ///
    /// Sets whether the most recently added plugin should be applied
    fn handle_plugin_apply(&self, _heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        // JVM represents booleans as integers (0 = false, non-zero = true)
        let apply = match args.first() {
            Some(Value::Int(i)) => *i != 0,
            _ => true,
        };

        let mut config = self
            .config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

        // Update the apply flag of the most recently added plugin
        if let Some(last_plugin) = config.plugin_definitions.last_mut() {
            last_plugin.apply = apply;
            tracing::debug!("Set plugin apply: {} -> {}", last_plugin.id, apply);
        }

        // Return self reference to enable method chaining
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle ExtraPropertiesExtension.set(String, Object) call
    fn handle_ext_set(&self, heap: &Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if args.len() >= 2 {
            if let Ok(key) = self.extract_string_arg(heap, args, 0) {
                let value = match &args[1] {
                    Value::Reference(ObjectRef::Object(id)) => heap.get_string(*id)?,
                    Value::Int(i) => i.to_string(),
                    Value::Long(l) => l.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Double(d) => d.to_string(),
                    _ => String::new(),
                };

                if !value.is_empty() {
                    let mut config = self
                        .config
                        .lock()
                        .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;
                    config.extra_properties.insert(key, value);
                }
            }
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle ExtraPropertiesExtension.get(String) call
    fn handle_ext_get(&self, heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
        if let Ok(key) = self.extract_string_arg(heap, args, 0) {
            let config = self
                .config
                .lock()
                .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;

            if let Some(value) = config.extra_properties.get(&key) {
                let string_ref = heap.intern_string(value)?;
                return Ok(Some(Value::Reference(string_ref)));
            }
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle dynamic property access on ExtraPropertiesExtension
    /// This handles patterns like: ext.springBootVersion = '3.0.0'
    fn handle_ext_dynamic_property(
        &self,
        heap: &Heap,
        method_name: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        // If method name starts with "set" and has args, it's a property setter
        if method_name.starts_with("set") && !args.is_empty() {
            let property_name = Self::decapitalize(&method_name[3..]).unwrap_or_default();
            if !property_name.is_empty() {
                let value = match &args[0] {
                    Value::Reference(ObjectRef::Object(id)) => heap.get_string(*id)?,
                    Value::Int(i) => i.to_string(),
                    Value::Long(l) => l.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Double(d) => d.to_string(),
                    _ => String::new(),
                };

                if !value.is_empty() {
                    let mut config = self
                        .config
                        .lock()
                        .map_err(|_| VMError::internal("Failed to lock config".to_string()))?;
                    config.extra_properties.insert(property_name, value);
                }
            }
        }
        // For any other method, just return null (property getter or unknown)
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Handle ExtensionContainer.getByName(String) call
    fn handle_extension_get_by_name(
        &self,
        heap: &mut Heap,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if let Ok(name) = self.extract_string_arg(heap, args, 0) {
            // Return a generic extension object
            let ext_id = heap.allocate_object(super::heap::ClassRef {
                name: format!("gradle/Extension${}", name),
                instance_field_count: 0,
                static_field_count: 0,
            })?;
            return Ok(Some(Value::Reference(ObjectRef::Object(ext_id))));
        }
        Ok(Some(Value::Reference(ObjectRef::Null)))
    }

    /// Extract string argument
    ///
    /// Resolves a String object reference from args and retrieves its value from the heap
    fn extract_string_arg(&self, heap: &Heap, args: &[Value], index: usize) -> VMResult<String> {
        match args.get(index) {
            Some(Value::Reference(ObjectRef::Object(heap_id))) => {
                // Resolve string from heap
                heap.get_string(*heap_id)
            }
            Some(Value::Reference(ObjectRef::Null)) => Err(VMError::NullPointerException),
            Some(value) => Err(VMError::internal(format!(
                "Expected string reference at argument {}, got: {:?}",
                index, value
            ))),
            None => Err(VMError::internal(format!(
                "Missing argument at index {}",
                index
            ))),
        }
    }

    fn extract_dependency_notations(
        &self,
        heap: &Heap,
        args: &[Value],
        index: usize,
    ) -> VMResult<Vec<String>> {
        match args.get(index) {
            Some(Value::Reference(ObjectRef::Object(heap_id))) => {
                if let Some(HeapEntry::Collection(collection)) = heap.get_entry(*heap_id) {
                    let mut values = Vec::new();
                    match &collection.data {
                        CollectionData::ArrayList(list) | CollectionData::LinkedList(list) => {
                            for value in list {
                                values.push(self.value_to_string(heap, value)?);
                            }
                        }
                        _ => {
                            return Err(VMError::internal(format!(
                                "Unsupported collection type for dependency list: {:?}",
                                collection.data
                            )));
                        }
                    }
                    return Ok(values);
                }

                let value = heap.get_string(*heap_id)?;
                Ok(vec![value])
            }
            Some(Value::Reference(ObjectRef::Null)) => Err(VMError::NullPointerException),
            Some(value) => Err(VMError::internal(format!(
                "Expected dependency argument at {}, got: {:?}",
                index, value
            ))),
            None => Err(VMError::internal(format!(
                "Missing argument at index {}",
                index
            ))),
        }
    }

    fn value_to_string(&self, heap: &Heap, value: &Value) -> VMResult<String> {
        match value {
            Value::Reference(ObjectRef::Object(heap_id)) => heap.get_string(*heap_id),
            Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
            other => Err(VMError::internal(format!(
                "Expected string element in dependency list, got: {:?}",
                other
            ))),
        }
    }

    fn value_to_string_lossy(&self, heap: &Heap, value: &Value) -> VMResult<String> {
        match value {
            Value::Reference(ObjectRef::Object(heap_id)) => {
                if let Ok(string) = heap.get_string(*heap_id) {
                    return Ok(string);
                }
                if let Ok(class_name) = heap.get_object_class_name(*heap_id) {
                    if class_name == "java/io/File" {
                        if let Some(path) = self.file_path_for_id(*heap_id) {
                            return Ok(path);
                        }
                    }
                    if class_name == "gradle/FileCollection" {
                        if let Some(items) = self.file_collection_items(*heap_id) {
                            return Ok(items.join(","));
                        }
                    }
                    return Ok(class_name);
                }
                Ok("Object".to_string())
            }
            Value::Reference(ObjectRef::Null) => Ok("null".to_string()),
            Value::Int(value) => Ok(value.to_string()),
            Value::Long(value) => Ok(value.to_string()),
            Value::Float(value) => Ok(value.to_string()),
            Value::Double(value) => Ok(value.to_string()),
            _ => Ok("Object".to_string()),
        }
    }

    fn extract_closure_arg(
        &self,
        heap: &Heap,
        args: &[Value],
    ) -> VMResult<Option<super::value::HeapId>> {
        if let Some(Value::Reference(ObjectRef::Object(id))) = args.last() {
            let class_name = heap.get_object_class_name(*id)?;
            if class_name.contains("$_closure_")
                || class_name.contains("$_nested_")
                || class_name == "groovy/lang/Closure"
            {
                return Ok(Some(*id));
            }
        }
        Ok(None)
    }

    fn set_extension_property(&self, extension: &str, property: &str, value: &str) -> VMResult<()> {
        let key = format!("extension.{}.{}", extension, property);
        self.config
            .lock()
            .map_err(|_| VMError::internal("Failed to lock config".to_string()))?
            .set_property(&key, value);
        Ok(())
    }

    fn get_extension_property(&self, extension: &str, property: &str) -> Option<String> {
        let key = format!("extension.{}.{}", extension, property);
        self.config
            .lock()
            .ok()
            .and_then(|config| config.get_property(&key).cloned())
    }

    fn decapitalize(input: &str) -> Option<String> {
        let mut chars = input.chars();
        let first = chars.next()?;
        let rest: String = chars.collect();
        Some(format!("{}{}", first.to_ascii_lowercase(), rest))
    }

    fn extension_class_name(name: &str) -> String {
        let slug = name.replace('.', "/");
        format!("gradle/Extension/{}", slug)
    }

    fn extension_name_from_class(class_name: &str) -> Option<String> {
        let name = class_name.strip_prefix("gradle/Extension/")?;
        Some(name.replace('/', "."))
    }

    fn file_path_for_id(&self, heap_id: super::value::HeapId) -> Option<String> {
        let key = format!("file.{}.path", heap_id);
        self.config
            .lock()
            .ok()
            .and_then(|config| config.get_property(&key).cloned())
    }

    fn file_collection_items(&self, heap_id: super::value::HeapId) -> Option<Vec<String>> {
        let key = format!("fileCollection.{}", heap_id);
        let value = self
            .config
            .lock()
            .ok()
            .and_then(|config| config.get_property(&key).cloned())?;
        let items: Vec<String> = value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
        if items.is_empty() {
            None
        } else {
            Some(items)
        }
    }

    /// The path of the File receiver, or an error when the receiver's path is unknown.
    fn known_file_path(&self, args: &[Value]) -> VMResult<String> {
        self.file_path_from_args(args)
            .ok_or_else(|| VMError::internal("File receiver has no known path"))
    }

    fn file_path_from_args(&self, args: &[Value]) -> Option<String> {
        match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => self.file_path_for_id(*id),
            Some(Value::Reference(ObjectRef::Null)) => None,
            _ => None,
        }
    }

    /// Sniffer intrinsics for common build-logic helpers.
    ///
    /// These are intentionally narrow and offline-only: they allow version resolution
    /// without executing arbitrary file I/O or requiring Kotlin stdlib support.
    pub fn try_invoke_sniffer_intrinsic(
        &self,
        heap: &mut Heap,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        if method_name == "resolve"
            && descriptor == "(Ljava/io/File;Ljava/lang/String;)Ljava/io/File;"
            && class_name.starts_with("kotlin/io/FilesKt")
            && args.len() >= 2
        {
            let base = match &args[0] {
                Value::Reference(ObjectRef::Object(id)) => self
                    .file_path_for_id(*id)
                    .unwrap_or_else(|| ".".to_string()),
                _ => ".".to_string(),
            };
            let child = self.extract_string_arg(heap, args, 1)?;
            let joined = std::path::Path::new(&base).join(child);

            let file_id = heap.allocate_object(super::heap::ClassRef {
                name: "java/io/File".to_string(),
                instance_field_count: 1,
                static_field_count: 0,
            })?;
            let key = format!("file.{}.path", file_id);
            if let Ok(mut config) = self.config.lock() {
                config.set_property(&key, &joined.display().to_string());
            }
            return Ok(Some(Value::Reference(ObjectRef::Object(file_id))));
        }

        if method_name == "readVersion"
            && descriptor == "(Ljava/io/File;Ljava/lang/String;)Ljava/lang/String;"
            && class_name.ends_with("TomlUtil")
        {
            let Some(versions) = &self.version_catalog_versions else {
                return Ok(None);
            };
            // args: [receiver?, file, key] for static call: [file, key]
            let key_index = if args.len() >= 2 { args.len() - 1 } else { 0 };
            let key = self.extract_string_arg(heap, args, key_index)?;
            if let Some(version) = versions.get(&key) {
                let s = heap.intern_string(version)?;
                return Ok(Some(Value::Reference(s)));
            }
            return Ok(Some(Value::Reference(ObjectRef::Null)));
        }

        Ok(None)
    }

    /// Get the shared configuration (for inspection after execution)
    pub fn config(&self) -> Arc<Mutex<GradleBuildConfig>> {
        self.config.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::heap::Heap;
    use super::*;
    use crate::interpreter::MemoryConfig;

    #[test]
    fn test_gradle_bridge_creation() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let _bridge = GradleNativeBridge::new(config);
    }

    #[test]
    fn test_maven_central_invocation() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let bridge = GradleNativeBridge::new(config.clone());

        let result = bridge.handle_maven_central();
        assert!(result.is_ok());

        let config_data = config.lock().unwrap();
        assert_eq!(config_data.repository_list.len(), 1);
        assert_eq!(config_data.repository_list[0].name, "mavenCentral");
    }

    #[test]
    fn test_plugin_id_invocation() {
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let bridge = GradleNativeBridge::new(config.clone());

        // Create heap and intern a string for plugin ID
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);
        let plugin_id_ref = heap.intern_string("java").unwrap();

        let args = vec![Value::Reference(plugin_id_ref)];
        let result = bridge.handle_plugin_id(&heap, &args);
        assert!(result.is_ok());

        let config_data = config.lock().unwrap();
        assert_eq!(config_data.plugin_definitions.len(), 1);
        assert_eq!(config_data.plugin_definitions[0].id, "java");
    }

    #[test]
    fn task_and_task_container_dispatch_independently() {
        // Verify that gradle/Task and gradle/TaskContainer route to the
        // correct handler independently (no prefix shadowing).
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let bridge = GradleNativeBridge::new(config);
        let mut heap = Heap::new(64 * 1024 * 1024);
        let method_registry = std::collections::HashMap::new();
        let mut thread = super::super::thread::Thread::new(1024, None);

        // gradle/TaskContainer.withType should create a task
        let result = bridge.invoke_gradle_method(
            &mut heap,
            &method_registry,
            &mut thread,
            "gradle/TaskContainer",
            "withType",
            "",
            &[],
        );
        assert!(result.is_ok(), "TaskContainer.withType should succeed");

        // gradle/Task.doFirst should succeed (returns null)
        let result = bridge.invoke_gradle_method(
            &mut heap,
            &method_registry,
            &mut thread,
            "gradle/Task",
            "doFirst",
            "",
            &[],
        );
        assert!(result.is_ok(), "Task.doFirst should succeed");
    }

    #[test]
    fn source_set_container_not_shadowed_by_source_set() {
        // Verify that gradle/SourceSetContainer dispatches to its own
        // handler, not to handle_source_set_method.
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        let bridge = GradleNativeBridge::new(config);
        let mut heap = Heap::new(64 * 1024 * 1024);
        let method_registry = std::collections::HashMap::new();
        let mut thread = super::super::thread::Thread::new(1024, None);

        // gradle/SourceSetContainer.getMain should allocate a SourceSet/main object
        let result = bridge.invoke_gradle_method(
            &mut heap,
            &method_registry,
            &mut thread,
            "gradle/SourceSetContainer",
            "getMain",
            "",
            &[],
        );
        assert!(result.is_ok(), "SourceSetContainer.getMain should succeed");
        if let Ok(Some(Value::Reference(ObjectRef::Object(id)))) = result {
            let class = heap.get_object_class_name(id).unwrap();
            assert_eq!(class, "gradle/SourceSet/main",
                "SourceSetContainer.getMain should create a SourceSet/main, not route through SourceSet handler");
        }
    }

    #[test]
    fn project_file_with_a_relative_path_is_read_from_the_project_directory() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("version.txt"), "1.2.3").unwrap();
        let mut build_config = GradleBuildConfig::new();
        build_config.project_dir = Some(project.path().to_path_buf());
        let bridge = GradleNativeBridge::new(Arc::new(Mutex::new(build_config)));
        let mut heap = Heap::new(64 * 1024 * 1024);
        let method_registry = std::collections::HashMap::new();
        let mut thread = super::super::thread::Thread::new(1024, None);
        let mut call = |heap: &mut Heap, class: &str, method: &str, args: &[Value]| {
            bridge
                .invoke_gradle_method(heap, &method_registry, &mut thread, class, method, "", args)
                .unwrap()
                .unwrap()
        };

        let name = Value::Reference(heap.intern_string("version.txt").unwrap());
        let file = call(&mut heap, "gradle/Project", "file", &[name]);
        let exists = call(
            &mut heap,
            "java/io/File",
            "exists",
            std::slice::from_ref(&file),
        );
        let text = call(&mut heap, "java/io/File", "getText", &[file]);

        assert_eq!(exists, Value::Int(1));
        let Value::Reference(ObjectRef::Object(text_id)) = text else {
            panic!("getText should return a string, got {text:?}");
        };
        assert_eq!(heap.get_string(text_id).unwrap(), "1.2.3");
    }
}
