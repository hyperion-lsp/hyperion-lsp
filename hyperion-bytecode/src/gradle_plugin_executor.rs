// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Execute compiled internal Gradle plugins offline (warm-cache mode).
//!
//! This is intentionally conservative and sandboxed:
//! - No Gradle invocation
//! - Bounded execution (time/instructions/memory/stack)
//! - Intercepts Gradle API calls via `GradleNativeBridge`
//! - Provides narrow intrinsics for version resolution

use crate::classpath::{
    load_classpath_classes_with_limits, ClasspathLoadLimits, MAX_CLASSPATH_DIRECTORY_DEPTH,
};
use crate::gradle::{GradleBuildConfig, GradleSandbox, SecurityPolicy};
use crate::interpreter::gradle_natives::GradleNativeBridge;
use crate::interpreter::heap::ClassRef;
use crate::interpreter::natives::NativeRegistry;
use crate::interpreter::runtime_mop::RuntimeMOP;
use crate::interpreter::value::ObjectRef;
use crate::interpreter::{Executor, Frame, Heap, MethodArea, MonitorTable, Thread, VMError, Value};
use cafebabe::attributes::AttributeData;
use cafebabe::parse_class;
use std::collections::HashMap;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Internal build-logic plugins should be small relative to a full application
/// classpath. Retaining at most 64 MiB keeps one cache probe from consuming the
/// generic loader's 512 MiB compatibility budget.
pub const MAX_GRADLE_PLUGIN_CLASSPATH_BYTES: usize = 64 * 1024 * 1024;

const GRADLE_PLUGIN_CLASSPATH_LIMITS: ClasspathLoadLimits = ClasspathLoadLimits {
    maximum_retained_class_bytes: MAX_GRADLE_PLUGIN_CLASSPATH_BYTES,
    maximum_discovered_entries: 100_000,
    maximum_directory_depth: MAX_CLASSPATH_DIRECTORY_DEPTH,
};

/// One deterministic, bounded classpath observation.
///
/// The snapshot owns the exact bytes used by execution. Callers may therefore
/// fingerprint it, consult a cache, and execute a miss without reopening files
/// that could have changed between those operations.
#[derive(Debug)]
pub struct LoadedGradlePluginClasspath {
    classes: Vec<(String, Vec<u8>)>,
}

impl LoadedGradlePluginClasspath {
    pub fn load(classpath_entries: &[PathBuf]) -> Result<Self> {
        let classes =
            load_classpath_classes_with_limits(classpath_entries, GRADLE_PLUGIN_CLASSPATH_LIMITS)
                .map_err(|error| GradlePluginExecutionError::Execution(error.to_string()))?;
        Ok(Self { classes })
    }

    /// Whether the snapshot contains the JVM class selected by a descriptor.
    pub fn contains_class(&self, class_name: &str) -> bool {
        let internal_name = class_name.replace('.', "/");
        self.classes
            .binary_search_by(|(candidate, _)| candidate.as_str().cmp(&internal_name))
            .is_ok()
    }

    pub fn classes(&self) -> impl ExactSizeIterator<Item = (&str, &[u8])> {
        self.classes
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
    }
}

#[derive(Debug, Clone)]
pub struct PluginExecutionStats {
    pub duration: Duration,
    pub instruction_limit_hit: bool,
    pub timed_out: bool,
    pub memory_limit_hit: bool,
    pub stack_limit_hit: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GradlePluginExecutionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Class not found: {0}")]
    ClassNotFound(String),
    #[error("Plugin apply method not found on {0}")]
    ApplyMethodMissing(String),
    #[error("Execution failed: {0}")]
    Execution(String),
    #[error("Sandbox limit exceeded: {0}")]
    SandboxViolation(String),
    #[error("VM error: {0}")]
    Vm(#[from] VMError),
}

pub type Result<T> = std::result::Result<T, GradlePluginExecutionError>;

pub struct GradlePluginExecutor {
    sandbox: GradleSandbox,
}

impl Default for GradlePluginExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl GradlePluginExecutor {
    pub fn new() -> Self {
        let gradle_home = crate::gradle::gradle_user_home();
        Self {
            sandbox: GradleSandbox::new(gradle_home),
        }
    }

    pub fn with_policy(policy: SecurityPolicy) -> Self {
        let gradle_home = crate::gradle::gradle_user_home();
        Self {
            sandbox: GradleSandbox::with_policy(gradle_home, policy),
        }
    }

    /// Execute `Plugin.apply(Project)` for a compiled plugin class.
    ///
    /// `classpath_entries` must contain the compiled plugin bytecode and any nested classes.
    pub fn execute_apply(
        &self,
        implementation_class: &str,
        classpath_entries: &[PathBuf],
        project_dir: &Path,
        subprojects: Vec<String>,
        version_catalog_versions: HashMap<String, String>,
    ) -> Result<(GradleBuildConfig, PluginExecutionStats)> {
        let start_time = Instant::now();
        let loaded = LoadedGradlePluginClasspath::load(classpath_entries)?;
        self.execute_apply_loaded_since(
            implementation_class,
            loaded,
            project_dir,
            subprojects,
            version_catalog_versions,
            start_time,
        )
    }

    /// Execute against an already-loaded classpath snapshot.
    ///
    /// No filesystem access is performed for plugin classes. This is the
    /// cache-safe entry point: the caller can hash `loaded.classes()` and pass
    /// the same owned bytes here on a miss.
    pub fn execute_apply_loaded(
        &self,
        implementation_class: &str,
        loaded: LoadedGradlePluginClasspath,
        project_dir: &Path,
        subprojects: Vec<String>,
        version_catalog_versions: HashMap<String, String>,
    ) -> Result<(GradleBuildConfig, PluginExecutionStats)> {
        self.execute_apply_loaded_since(
            implementation_class,
            loaded,
            project_dir,
            subprojects,
            version_catalog_versions,
            Instant::now(),
        )
    }

    fn execute_apply_loaded_since(
        &self,
        implementation_class: &str,
        loaded: LoadedGradlePluginClasspath,
        project_dir: &Path,
        subprojects: Vec<String>,
        version_catalog_versions: HashMap<String, String>,
        start_time: Instant,
    ) -> Result<(GradleBuildConfig, PluginExecutionStats)> {
        let impl_internal = implementation_class.replace('.', "/");
        if !loaded.contains_class(&impl_internal) {
            return Err(GradlePluginExecutionError::ClassNotFound(
                implementation_class.to_string(),
            ));
        }
        let classes = loaded.classes.into_iter().collect::<HashMap<_, _>>();

        let mut method_registry = HashMap::new();
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

        // Shared extracted config.
        let config = Arc::new(Mutex::new(GradleBuildConfig::new()));
        {
            let mut guard = config.lock().map_err(|_| {
                GradlePluginExecutionError::Execution("Config lock poisoned".to_string())
            })?;
            guard.project_dir = Some(project_dir.to_path_buf());
            guard.build_dir = Some(project_dir.join("build"));
            guard.subprojects = subprojects;
            guard.is_multi_project = !guard.subprojects.is_empty();
        }

        let runtime_mop = Arc::new(RwLock::new(RuntimeMOP::new()));
        let gradle_bridge = Arc::new(
            GradleNativeBridge::new(config.clone())
                .with_recording_proxy(true)
                .with_execute_subprojects(true)
                .with_version_catalog_versions(version_catalog_versions),
        );

        let method_area = Arc::new(RwLock::new(MethodArea::new()));
        register_loaded_classes(&method_area, &classes)?;

        let mut native_registry = NativeRegistry::new();
        crate::interpreter::kotlin_intrinsics::register_kotlin_intrinsics(&mut native_registry);

        let mut executor = Executor::new(Vec::new())
            .with_native_registry(native_registry)
            .with_methods(method_registry.clone())
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

        let mut thread = Thread::new(stack_depth_limit.max(2048), Some(instruction_limit));
        let mut heap = Heap::new(memory_limit);
        let mut monitors = MonitorTable::new();

        let plugin_fields = 0;
        let plugin_obj = heap.allocate_object(ClassRef {
            name: impl_internal.clone(),
            instance_field_count: plugin_fields,
            static_field_count: 0,
        })?;

        let project_obj = heap.allocate_object(ClassRef {
            name: "org/gradle/api/Project".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        })?;

        // Best-effort: run <init>()V if present, then apply(Project).
        run_optional_constructor(
            &mut executor,
            &mut thread,
            &mut heap,
            &mut monitors,
            &method_registry,
            &impl_internal,
            plugin_obj,
            time_limit,
        )?;

        let (apply_desc, apply_bytecode) = find_apply_method(&method_registry, &impl_internal)
            .ok_or_else(|| {
                GradlePluginExecutionError::ApplyMethodMissing(implementation_class.to_string())
            })?;

        let apply_frame = Frame::with_bytecode(
            impl_internal.clone(),
            "apply".to_string(),
            apply_bytecode.max_locals,
            apply_bytecode.max_stack,
            vec![
                Value::Reference(ObjectRef::Object(plugin_obj)),
                Value::Reference(ObjectRef::Object(project_obj)),
            ],
            apply_bytecode.code.clone(),
            apply_desc,
            apply_bytecode.exception_table.clone(),
        )?;
        thread.push_frame(apply_frame)?;

        loop {
            if start_time.elapsed() > time_limit {
                return Err(GradlePluginExecutionError::SandboxViolation(
                    "Execution timeout exceeded".to_string(),
                ));
            }

            match executor.step(&mut thread, &mut heap, &mut monitors) {
                Ok(true) => {}
                Ok(false) => break,
                Err(crate::interpreter::VMError::ExecutionLimitExceeded) => {
                    return Err(GradlePluginExecutionError::SandboxViolation(
                        "Instruction limit exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::ExecutionTimeout) => {
                    return Err(GradlePluginExecutionError::SandboxViolation(
                        "Execution timeout exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::MemoryLimitExceeded) => {
                    return Err(GradlePluginExecutionError::SandboxViolation(
                        "Memory limit exceeded".to_string(),
                    ));
                }
                Err(crate::interpreter::VMError::StackOverflow) => {
                    return Err(GradlePluginExecutionError::SandboxViolation(
                        "Stack depth limit exceeded".to_string(),
                    ));
                }
                Err(e) => {
                    return Err(GradlePluginExecutionError::Execution(format!(
                        "Plugin execution failed: {}",
                        e
                    )));
                }
            }
        }

        let stats = PluginExecutionStats {
            duration: start_time.elapsed(),
            instruction_limit_hit: false,
            timed_out: false,
            memory_limit_hit: false,
            stack_limit_hit: false,
        };

        let final_config = config.lock().map_err(|_| {
            GradlePluginExecutionError::Execution("Config lock poisoned".to_string())
        })?;
        Ok((final_config.clone(), stats))
    }
}

fn register_loaded_classes(
    method_area: &Arc<RwLock<MethodArea>>,
    classes: &HashMap<String, Vec<u8>>,
) -> Result<()> {
    use crate::interpreter::linker::ClassLinker;
    use crate::interpreter::ClassLoader;

    let guard = method_area
        .write()
        .map_err(|_| GradlePluginExecutionError::Execution("MethodArea lock failed".to_string()))?;

    let staging_loader = ClassLoader::staging_with_fallback(guard.clone());
    let mut pending = HashMap::new();
    for (requested_name, bytes) in classes {
        // Keep the same declaration metadata used by normal VM definition.
        // A skeletal class record loses symbolic method resolution and lets
        // native fast paths accidentally bypass linkage checks.
        let class = ClassLinker::parse_class(bytes, &staging_loader)
            .map_err(|error| GradlePluginExecutionError::Execution(error.to_string()))?;
        if requested_name.replace('/', ".") != class.name.replace('/', ".") {
            return Err(GradlePluginExecutionError::Execution(format!(
                "classpath entry {requested_name} declares {}",
                class.name
            )));
        }
        if pending.insert(class.name.clone(), class).is_some() {
            return Err(GradlePluginExecutionError::Execution(format!(
                "duplicate plugin class definition for {requested_name}"
            )));
        }
    }

    let mut linked = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let mut progress = false;
        let names: Vec<_> = pending.keys().cloned().collect();
        for name in names {
            let mut class = pending.remove(&name).expect("pending class disappeared");
            let waits_for_staged_dependency = class
                .superclass
                .iter()
                .chain(class.interfaces.iter())
                .map(|dependency| dependency.replace('/', "."))
                .any(|dependency| pending.contains_key(&dependency));
            if waits_for_staged_dependency {
                pending.insert(name, class);
                continue;
            }
            match ClassLinker::link_class(&mut class, &staging_loader) {
                Ok(()) => {
                    staging_loader
                        .register_loaded_class(class.clone())
                        .map_err(|error| {
                            GradlePluginExecutionError::Execution(error.to_string())
                        })?;
                    linked.push(class);
                    progress = true;
                }
                Err(crate::interpreter::VMError::NoClassDefFoundError(_)) => {
                    pending.insert(name, class);
                }
                Err(error) => {
                    return Err(GradlePluginExecutionError::Execution(error.to_string()));
                }
            }
        }
        if !progress {
            let unresolved = pending.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(GradlePluginExecutionError::Execution(format!(
                "could not link plugin class dependency cycle or missing parent: {unresolved}"
            )));
        }
    }

    guard
        .register_classes(linked)
        .map_err(|error| GradlePluginExecutionError::Execution(error.to_string()))?;

    Ok(())
}

fn find_apply_method(
    method_registry: &HashMap<
        (String, String, String),
        crate::interpreter::executor::MethodBytecode,
    >,
    impl_internal: &str,
) -> Option<(String, crate::interpreter::executor::MethodBytecode)> {
    let descs = [
        "(Lorg/gradle/api/Project;)V".to_string(),
        "(Ljava/lang/Object;)V".to_string(),
    ];
    for desc in descs {
        if let Some(bytecode) =
            method_registry.get(&(impl_internal.to_string(), "apply".to_string(), desc.clone()))
        {
            return Some((desc, bytecode.clone()));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn run_optional_constructor(
    executor: &mut Executor,
    thread: &mut Thread,
    heap: &mut Heap,
    monitors: &mut MonitorTable,
    method_registry: &HashMap<
        (String, String, String),
        crate::interpreter::executor::MethodBytecode,
    >,
    class_name: &str,
    obj_id: crate::interpreter::HeapId,
    time_limit: Duration,
) -> Result<()> {
    let Some(bytecode) = method_registry
        .get(&(
            class_name.to_string(),
            "<init>".to_string(),
            "()V".to_string(),
        ))
        .cloned()
    else {
        return Ok(());
    };

    let frame = Frame::with_bytecode(
        class_name.to_string(),
        "<init>".to_string(),
        bytecode.max_locals,
        bytecode.max_stack,
        vec![Value::Reference(ObjectRef::Object(obj_id))],
        bytecode.code.clone(),
        "()V".to_string(),
        bytecode.exception_table.clone(),
    )?;
    thread.push_frame(frame)?;

    let start = Instant::now();
    loop {
        if thread.depth() == 0 {
            break;
        }
        if start.elapsed() > time_limit {
            return Err(GradlePluginExecutionError::SandboxViolation(
                "Execution timeout exceeded".to_string(),
            ));
        }
        match executor.step(thread, heap, monitors) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                return Err(GradlePluginExecutionError::Execution(format!(
                    "Constructor execution failed: {}",
                    e
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn write_class(root: &Path, rel: &str, b64: &str) {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;
        let bytes = STANDARD.decode(b64).unwrap();
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn plugin_registration_links_parent_and_default_interface_before_child() {
        if !Command::new("javac")
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            return;
        }
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("src/pluginlink");
        let output = tmp.path().join("classes");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(
            source.join("Service.java"),
            "package pluginlink; public interface Service { default int answer() { return 42; } }",
        )
        .unwrap();
        fs::write(
            source.join("Base.java"),
            "package pluginlink; public class Base implements Service {}",
        )
        .unwrap();
        fs::write(
            source.join("Child.java"),
            "package pluginlink; public class Child extends Base {}",
        )
        .unwrap();
        let status = Command::new("javac")
            .args(["-d", output.to_str().unwrap()])
            .arg(source.join("Service.java"))
            .arg(source.join("Base.java"))
            .arg(source.join("Child.java"))
            .status()
            .unwrap();
        assert!(status.success());

        let classes = ["Child", "Base", "Service"]
            .into_iter()
            .map(|name| {
                (
                    format!("pluginlink.{name}"),
                    fs::read(output.join(format!("pluginlink/{name}.class"))).unwrap(),
                )
            })
            .collect();
        let method_area = Arc::new(RwLock::new(MethodArea::new()));
        register_loaded_classes(&method_area, &classes).unwrap();
        let child = method_area
            .read()
            .unwrap()
            .get_class("pluginlink.Child")
            .unwrap();
        let route = child
            .itable
            .iter()
            .find(|entry| entry.method_name.as_ref() == "answer")
            .unwrap();
        assert_eq!(
            child.vtable[route.vtable_index].declaring_class(),
            "pluginlink.Service"
        );
    }

    #[test]
    fn loaded_snapshot_ignores_later_file_mutation_and_records_dependencies() {
        let tmp = TempDir::new().unwrap();
        let classes = tmp.path().join("classes");

        // Precompiled fixture classes (base64-encoded javac output for a toy com.example plugin).
        write_class(&classes, "com/example/InternalPlugin.class", "yv66vgAAADQAIgoAAgADBwAEDAAFAAYBABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWBwAIAQAcY29tL2V4YW1wbGUvSW50ZXJuYWxQbHVnaW4kMQoABwAKDAAFAAsBAB8oTGNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luOylWCwANAA4HAA8MABAAEQEAFm9yZy9ncmFkbGUvYXBpL1Byb2plY3QBAAtzdWJwcm9qZWN0cwEAGihMb3JnL2dyYWRsZS9hcGkvQWN0aW9uOylWCgATABQHABUMABYAFwEAGmNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luAQAFYXBwbHkBABsoTG9yZy9ncmFkbGUvYXBpL1Byb2plY3Q7KVYHABkBABVvcmcvZ3JhZGxlL2FwaS9QbHVnaW4BAARDb2RlAQAPTGluZU51bWJlclRhYmxlAQAVKExqYXZhL2xhbmcvT2JqZWN0OylWAQAJU2lnbmF0dXJlAQBDTGphdmEvbGFuZy9PYmplY3Q7TG9yZy9ncmFkbGUvYXBpL1BsdWdpbjxMb3JnL2dyYWRsZS9hcGkvUHJvamVjdDs+OwEAClNvdXJjZUZpbGUBABNJbnRlcm5hbFBsdWdpbi5qYXZhAQAMSW5uZXJDbGFzc2VzACEAEwACAAEAGAAAAAMAAQAFAAYAAQAaAAAAHQABAAEAAAAFKrcAAbEAAAABABsAAAAGAAEAAAAEAAEAFgAXAAEAGgAAACsABAACAAAADyu7AAdZKrcACbkADAIAsQAAAAEAGwAAAAoAAgAAAAYADgAMEEEAFgAcAAEAGgAAACEAAgACAAAACSorwAANtgASsQAAAAEAGwAAAAYAAQAAAAQAAwAdAAAAAgAeAB8AAAACACAAIQAAAAoAAQAHAAAAAAAA");
        write_class(&classes, "com/example/InternalPlugin$1.class", "yv66vgAAADQAPAkAAgADBwAEDAAFAAYBABxjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbiQxAQAGdGhpcyQwAQAcTGNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luOwoACAAJBwAKDAALAAwBABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWCwAOAA8HABAMABEAEgEAFm9yZy9ncmFkbGUvYXBpL1Byb2plY3QBAApnZXRQbHVnaW5zAQAqKClMb3JnL2dyYWRsZS9hcGkvcGx1Z2lucy9QbHVnaW5Db250YWluZXI7CAAUAQAEamF2YQsAFgAXBwAYDAAZABoBACZvcmcvZ3JhZGxlL2FwaS9wbHVnaW5zL1BsdWdpbkNvbnRhaW5lcgEABWFwcGx5AQAVKExqYXZhL2xhbmcvU3RyaW5nOylWCwAOABwMAB0AHgEAD2dldERlcGVuZGVuY2llcwEAMigpTG9yZy9ncmFkbGUvYXBpL2FydGlmYWN0cy9kc2wvRGVwZW5kZW5jeUhhbmRsZXI7CAAgAQAOaW1wbGVtZW50YXRpb24IACIBACdvcmcuYXBhY2hlLmNvbW1vbnM6Y29tbW9ucy1sYW5nMzozLjEzLjALACQAJQcAJgwAJwAoAQAub3JnL2dyYWRsZS9hcGkvYXJ0aWZhY3RzL2RzbC9EZXBlbmRlbmN5SGFuZGxlcgEAA2FkZAEAOChMamF2YS9sYW5nL1N0cmluZztMamF2YS9sYW5nL09iamVjdDspTGphdmEvbGFuZy9PYmplY3Q7CgACACoMACsALAEAB2V4ZWN1dGUBABsoTG9yZy9ncmFkbGUvYXBpL1Byb2plY3Q7KVYHAC4BABVvcmcvZ3JhZGxlL2FwaS9BY3Rpb24BAB8oTGNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luOylWAQAEQ29kZQEAD0xpbmVOdW1iZXJUYWJsZQEAFShMamF2YS9sYW5nL09iamVjdDspVgEACVNpZ25hdHVyZQEAQ0xqYXZhL2xhbmcvT2JqZWN0O0xvcmcvZ3JhZGxlL2FwaS9BY3Rpb248TG9yZy9ncmFkbGUvYXBpL1Byb2plY3Q7PjsBAApTb3VyY2VGaWxlAQATSW50ZXJuYWxQbHVnaW4uamF2YQEAD0VuY2xvc2luZ01ldGhvZAcAOQEAGmNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luDAAZACwBAAxJbm5lckNsYXNzZXMAIAACAAgAAQAtAAEQEAAFAAYAAAADAAAACwAvAAEAMAAAACIAAgACAAAACiortQABKrcAB7EAAAABADEAAAAGAAEAAAAGAAEAKwAsAAEAMAAAAD4AAwACAAAAHiu5AA0BABITuQAVAgAruQAbAQASHxIhuQAjAwBXsQAAAAEAMQAAAA4AAwAAAAgADQAJAB0AChBBACsAMgABADAAAAAhAAIAAgAAAAkqK8AADrYAKbEAAAABADEAAAAGAAEAAAAGAAQAMwAAAAIANAA1AAAAAgA2ADcAAAAEADgAOgA7AAAACgABAAIAAAAAAAA=");

        // Interface stubs (not strictly required, but keeps the classpath self-contained).
        write_class(&classes, "org/gradle/api/Project.class", "yv66vgAAADQAEQcAAgEAFm9yZy9ncmFkbGUvYXBpL1Byb2plY3QHAAQBABBqYXZhL2xhbmcvT2JqZWN0AQAKZ2V0Um9vdERpcgEAECgpTGphdmEvaW8vRmlsZTsBAApnZXRQbHVnaW5zAQAqKClMb3JnL2dyYWRsZS9hcGkvcGx1Z2lucy9QbHVnaW5Db250YWluZXI7AQAPZ2V0RGVwZW5kZW5jaWVzAQAyKClMb3JnL2dyYWRsZS9hcGkvYXJ0aWZhY3RzL2RzbC9EZXBlbmRlbmN5SGFuZGxlcjsBAAtzdWJwcm9qZWN0cwEAGihMb3JnL2dyYWRsZS9hcGkvQWN0aW9uOylWAQAJU2lnbmF0dXJlAQA0KExvcmcvZ3JhZGxlL2FwaS9BY3Rpb248TG9yZy9ncmFkbGUvYXBpL1Byb2plY3Q7PjspVgEAClNvdXJjZUZpbGUBAAxQcm9qZWN0LmphdmEGAQABAAMAAAAAAAQEAQAFAAYAAAQBAAcACAAABAEACQAKAAAEAQALAAwAAQANAAAAAgAOAAEADwAAAAIAEA==");
        write_class(&classes, "org/gradle/api/Action.class", "yv66vgAAADQADAcAAgEAFW9yZy9ncmFkbGUvYXBpL0FjdGlvbgcABAEAEGphdmEvbGFuZy9PYmplY3QBAAdleGVjdXRlAQAVKExqYXZhL2xhbmcvT2JqZWN0OylWAQAJU2lnbmF0dXJlAQAGKFRUOylWAQAoPFQ6TGphdmEvbGFuZy9PYmplY3Q7PkxqYXZhL2xhbmcvT2JqZWN0OwEAClNvdXJjZUZpbGUBAAtBY3Rpb24uamF2YQYBAAEAAwAAAAAAAQQBAAUABgABAAcAAAACAAgAAgAHAAAAAgAJAAoAAAACAAs=");
        write_class(&classes, "org/gradle/api/Plugin.class", "yv66vgAAADQADAcAAgEAFW9yZy9ncmFkbGUvYXBpL1BsdWdpbgcABAEAEGphdmEvbGFuZy9PYmplY3QBAAVhcHBseQEAFShMamF2YS9sYW5nL09iamVjdDspVgEACVNpZ25hdHVyZQEABihUVDspVgEAKDxUOkxqYXZhL2xhbmcvT2JqZWN0Oz5MamF2YS9sYW5nL09iamVjdDsBAApTb3VyY2VGaWxlAQALUGx1Z2luLmphdmEGAQABAAMAAAAAAAEEAQAFAAYAAQAHAAAAAgAIAAIABwAAAAIACQAKAAAAAgAL");
        write_class(&classes, "org/gradle/api/plugins/PluginContainer.class", "yv66vgAAADQACQcAAgEAJm9yZy9ncmFkbGUvYXBpL3BsdWdpbnMvUGx1Z2luQ29udGFpbmVyBwAEAQAQamF2YS9sYW5nL09iamVjdAEABWFwcGx5AQAVKExqYXZhL2xhbmcvU3RyaW5nOylWAQAKU291cmNlRmlsZQEAFFBsdWdpbkNvbnRhaW5lci5qYXZhBgEAAQADAAAAAAABBAEABQAGAAAAAQAHAAAAAgAI");
        write_class(&classes, "org/gradle/api/artifacts/dsl/DependencyHandler.class", "yv66vgAAADQACQcAAgEALm9yZy9ncmFkbGUvYXBpL2FydGlmYWN0cy9kc2wvRGVwZW5kZW5jeUhhbmRsZXIHAAQBABBqYXZhL2xhbmcvT2JqZWN0AQADYWRkAQA4KExqYXZhL2xhbmcvU3RyaW5nO0xqYXZhL2xhbmcvT2JqZWN0OylMamF2YS9sYW5nL09iamVjdDsBAApTb3VyY2VGaWxlAQAWRGVwZW5kZW5jeUhhbmRsZXIuamF2YQYBAAEAAwAAAAAAAQQBAAUABgAAAAEABwAAAAIACA==");

        let loaded = LoadedGradlePluginClasspath::load(std::slice::from_ref(&classes)).unwrap();
        let staged_classes: HashMap<_, _> = loaded.classes.iter().cloned().collect();
        let linked_area = Arc::new(RwLock::new(MethodArea::new()));
        register_loaded_classes(&linked_area, &staged_classes).unwrap();
        let anonymous_action = linked_area
            .read()
            .unwrap()
            .get_class("com/example/InternalPlugin$1")
            .unwrap();
        assert!(anonymous_action.itable.iter().any(|entry| {
            entry.interface_name.replace('.', "/") == "org/gradle/api/Action"
                && entry.method_name.as_ref() == "execute"
        }));
        fs::write(
            classes.join("com/example/InternalPlugin.class"),
            b"mutated after snapshot",
        )
        .unwrap();
        let exec = GradlePluginExecutor::new();
        let (config, _stats) = exec
            .execute_apply_loaded(
                "com.example.InternalPlugin",
                loaded,
                tmp.path(),
                vec!["app".to_string()],
                HashMap::new(),
            )
            .unwrap();

        assert!(
            config
                .dependency_list
                .iter()
                .any(|d| d.configuration == "implementation"
                    && d.group == "org.apache.commons"
                    && d.artifact == "commons-lang3"
                    && d.version == "3.13.0"),
            "expected dependency to be recorded, got deps={:?} plugins={:?}",
            config
                .dependency_list
                .iter()
                .map(|d| format!(
                    "{}:{}:{}:{}",
                    d.configuration, d.group, d.artifact, d.version
                ))
                .collect::<Vec<_>>(),
            config
                .plugin_definitions
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        );
    }
}
