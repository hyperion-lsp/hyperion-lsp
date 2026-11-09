// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM Bytecode Interpreter
//!
//! This module provides a JVM bytecode interpreter targeting **JVM SE 8 specification compliance**.
//! It executes compiled .class files (Java, Kotlin, Groovy) with spec-compliant bytecode semantics,
//! enabling Gradle build script execution without requiring the full JVM.
//!
//! # Specification Compliance
//!
//! **Target:** JVM SE 8 ([JVM Spec §6](https://docs.oracle.com/javase/specs/jvms/se8/html/jvms-6.html))
//!
//! **Opcode Coverage:** 164/183 opcodes implemented (89.6%)
//!
//! **Implemented opcodes include:**
//! - Arithmetic: iadd, ladd, fadd, dadd, isub, lsub, fsub, dsub, imul, lmul, fmul, dmul, idiv, ldiv, fdiv, ddiv
//! - Comparison: ifeq, ifne, iflt, ifle, ifgt, ifge, if_icmpeq, if_icmpne, lcmp, fcmpl, fcmpg, dcmpl, dcmpg
//! - Control flow: goto, jsr, ret, tableswitch, lookupswitch
//! - Invocation: invokevirtual, invokespecial, invokestatic, invokeinterface, invokedynamic
//! - Object creation: new, newarray, anewarray, multianewarray
//! - Stack manipulation: pop, pop2, dup, dup_x1, dup_x2, dup2, dup2_x1, dup2_x2, swap
//! - Field access: getfield, putfield, getstatic, putstatic
//! - Array access: iaload, laload, faload, daload, aaload, baload, caload, saload, iastore, etc.
//! - Type checking: checkcast, instanceof
//! - Monitor: monitorenter, monitorexit
//!
//! **invokedynamic support:**
//! - Full bootstrap method resolution (BSM lookup via constant pool)
//! - Lambda factory generation for Java 8+ lambdas
//! - Groovy closure invocation via CallSite and MethodHandle
//! - Kotlin coroutine continuations (experimental)
//!
//! **Deviations from JVM Spec:**
//! - Simplified class verification (no full bytecode verification pass)
//! - Limited native method support (JDK stubs for essential methods only)
//! - No JIT compilation (interpreted only)
//! - Simplified garbage collection (mark-sweep, not generational)
//! - Synchronized methods acquire the receiver (instance) or class (static) monitor
//!
//! # Architecture
//!
//! The interpreter is organized into several key components:
//!
//! - **Value System** (`value.rs`): Type representation for JVM values (primitives, references)
//! - **Frame Management** (`frame.rs`): Stack frames for method execution with local variables and operand stack
//! - **Heap Storage** (`heap.rs`): Object and array allocation with reference-based addressing
//! - **Class System** (`class_loader.rs`, `linker.rs`, `method_area.rs`): Class loading, linking, and initialization
//! - **Instruction Executor** (`executor.rs`): Bytecode instruction interpretation (164 opcodes)
//! - **Method Dispatch** (`dispatch.rs`): Virtual, interface, special, and static method invocation with vtable/itable
//! - **Exception Handling** (`exception_handler.rs`, `exceptions.rs`): Try-catch block handling and stack unwinding
//! - **invokedynamic** (`bootstrap.rs`, `call_site.rs`, `method_handles.rs`): Dynamic dispatch for lambdas and closures
//! - **Garbage Collection** (`gc.rs`): Mark-sweep GC with heap compaction
//! - **Native Methods** (`natives.rs`, `jdk_stubs.rs`): JDK method stubs (System.out.println, String methods, etc.)
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_bytecode::interpreter::*;
//!
//! // Create VM with memory limits
//! let config = MemoryConfig {
//!     max_heap_size: 256 * 1024 * 1024,  // 256MB heap
//!     max_stack_depth: 1024,              // 1024 frames deep
//!     max_instructions: 10_000_000,       // Sandbox: 10M instructions max
//!     enable_gc: true,                    // Enable mark-sweep GC
//! };
//! let mut vm = VirtualMachine::new(config);
//!
//! // Load and execute bytecode
//! vm.load_class_from_bytes("Test", &bytecode)?;
//! let result = vm.invoke_static("Test", "main", "([Ljava/lang/String;)V", &[])?;
//! ```
//!
//! # Use Cases
//!
//! 1. **Gradle Execution:** Run Gradle build scripts without JVM startup overhead
//! 2. **Build System Testing:** Execute Java/Kotlin tests in CI without JVM cold start
//! 3. **Sandboxed Execution:** Run untrusted bytecode with resource limits
//! 4. **JVM Learning:** Study JVM internals through a readable Rust implementation
//!
//! # Performance Characteristics
//!
//! - **Startup:** <10ms (vs. 1-5s for JVM)
//! - **Memory:** 10-50MB baseline (vs. 100-300MB for JVM)
//! - **Throughput:** ~10-20x slower than JIT-compiled JVM (interpreted vs. compiled)
//! - **Best for:** Short-lived tasks, build scripts, sandboxed execution
//! - **Not for:** Long-running servers, CPU-intensive workloads

pub mod bootstrap;
pub mod call_site;
pub mod class_loader;
pub mod closure_invoker;
pub mod closure_runtime;
mod constant_pool_parser;
pub mod coroutine_natives;
pub mod dispatch;
pub mod errors;
pub mod exception_handler;
pub mod exceptions;
pub mod executor;
pub mod frame;
pub mod gc;
pub mod gradle_natives;
pub mod heap;
pub mod jdk_stubs;
pub mod kotlin_intrinsics;
pub mod linker;
pub mod lookup;
pub mod method_area;
pub mod method_handles;
pub mod method_type;
pub mod monitor;
pub mod natives;
pub mod png_encode;
pub mod runtime_mop;
pub mod thread;
pub mod value;
pub mod verifier;

pub use bootstrap::BootstrapContext;
pub use call_site::{CallSite, MethodHandle};
pub use class_loader::ClassLoader;
pub use closure_invoker::ClosureInvoker;
pub use closure_runtime::{ClosureRuntime, ResolveStrategy};
pub use constant_pool_parser::parse_constant_pool;
pub use errors::{VMError, VMResult};
pub use executor::Executor;
pub use frame::Frame;
pub use gc::{GCRoots, GCStats, GarbageCollector};
pub use heap::Heap;
pub use jdk_stubs::{JdkStubRegistry, StubClass, StubMethodDescriptor, StubObject};
pub use lookup::Lookup;
pub use method_area::MethodArea;
pub use method_handles::{
    MethodHandleId, MethodHandleKind, MethodHandleStore, RuntimeMethodHandle,
};
pub use method_type::{MethodTypeRef, TypeDescriptor};
pub use monitor::{MonitorTable, ThreadId};
pub use runtime_mop::RuntimeMOP;
pub use thread::Thread;
pub use value::{HeapId, ObjectRef, Value};
pub use verifier::{verify_bytecode, BytecodeVerifier};

/// Memory configuration for the JVM interpreter
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Maximum heap size in bytes
    pub max_heap_size: usize,

    /// Maximum stack depth (number of frames)
    pub max_stack_depth: usize,

    /// Maximum instruction count (for sandboxing)
    pub max_instructions: u64,

    /// Whether to enable garbage collection
    pub enable_gc: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig {
            max_heap_size: 256 * 1024 * 1024, // 256MB
            max_stack_depth: 1024,
            max_instructions: 10_000_000,
            enable_gc: false,
        }
    }
}

/// One class that could not be defined by [`VirtualMachine::load_classes_from_bytes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassLoadFailure {
    /// Canonical dotted class name, or the normalized requested name when the
    /// class file could not be parsed.
    pub class_name: String,
    /// Parse, verification, or structural-linkage failure.
    pub error: String,
}

/// Result of an order-independent class-definition batch.
///
/// A malformed class does not prevent unrelated classes from being installed.
/// Classes whose superclass or interface failed are themselves reported as
/// failures and are not made visible to the VM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchClassLoadReport {
    pub loaded_classes: Vec<String>,
    pub failures: Vec<ClassLoadFailure>,
}

impl BatchClassLoadReport {
    /// Whether a class was successfully linked and installed.
    pub fn is_loaded(&self, class_name: &str) -> bool {
        let canonical = normalize_vm_class_name(class_name);
        self.loaded_classes.iter().any(|name| name == &canonical)
    }

    /// Find the failure for a requested or declared class name.
    pub fn failure_for(&self, class_name: &str) -> Option<&ClassLoadFailure> {
        let canonical = normalize_vm_class_name(class_name);
        self.failures
            .iter()
            .find(|failure| failure.class_name == canonical)
    }
}

struct StagedClass<'a> {
    class: method_area::LoadedClass,
    requested_name: &'a str,
    bytecode: &'a [u8],
}

#[derive(Debug, Clone)]
enum StagedLinkState {
    Visiting,
    Linked,
    Failed(String),
}

fn normalize_vm_class_name(name: &str) -> String {
    name.replace('/', ".")
}

fn link_staged_class(
    name: &str,
    staged: &HashMap<String, StagedClass<'_>>,
    parse_failures: &HashMap<String, String>,
    staging_loader: &ClassLoader,
    states: &mut HashMap<String, StagedLinkState>,
    linked_order: &mut Vec<String>,
) -> Result<(), String> {
    match states.get(name) {
        Some(StagedLinkState::Linked) => return Ok(()),
        Some(StagedLinkState::Failed(error)) => return Err(error.clone()),
        Some(StagedLinkState::Visiting) => {
            return Err(format!("circular class hierarchy involving {name}"));
        }
        None => {}
    }

    let Some(staged_class) = staged.get(name) else {
        if staging_loader.get_loaded_class(name).is_ok() {
            return Ok(());
        }
        if let Some(error) = parse_failures.get(name) {
            return Err(format!("definition of {name} failed: {error}"));
        }
        return Err(format!("missing structural dependency {name}"));
    };

    states.insert(name.to_string(), StagedLinkState::Visiting);
    let result: Result<(), String> = (|| {
        let mut dependencies = Vec::new();
        if let Some(superclass) = &staged_class.class.superclass {
            let superclass = normalize_vm_class_name(superclass);
            if superclass != "java.lang.Object" {
                dependencies.push(superclass);
            }
        }
        dependencies.extend(
            staged_class
                .class
                .interfaces
                .iter()
                .map(|interface| normalize_vm_class_name(interface)),
        );
        dependencies.sort();
        dependencies.dedup();

        for dependency in dependencies {
            link_staged_class(
                &dependency,
                staged,
                parse_failures,
                staging_loader,
                states,
                linked_order,
            )
            .map_err(|error| format!("{name} requires {dependency}: {error}"))?;
        }

        let mut linked_class = staged_class.class.clone();
        linker::ClassLinker::link_class(&mut linked_class, staging_loader)
            .map_err(|error| error.to_string())?;
        staging_loader
            .register_loaded_class(linked_class)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            states.insert(name.to_string(), StagedLinkState::Linked);
            linked_order.push(name.to_string());
            Ok(())
        }
        Err(error) => {
            states.insert(name.to_string(), StagedLinkState::Failed(error.clone()));
            Err(error)
        }
    }
}

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Main JVM interpreter
pub struct VirtualMachine {
    config: MemoryConfig,
    class_loader: ClassLoader,
    method_area: Arc<RwLock<MethodArea>>,
    heap: Heap,
    /// Store raw class bytecode for method execution
    class_bytecode: HashMap<String, Vec<u8>>,
    /// Map of caller-provided names to canonical class names.
    ///
    /// Callers sometimes load classes under an arbitrary label (e.g. "GroovyScript")
    /// while the class file itself declares a different name (e.g. "Test").
    /// MethodArea uses the declared name (dot notation), so keep an alias map to
    /// resolve these consistently in convenience APIs like `invoke_static`.
    class_aliases: HashMap<String, String>,
    /// Runtime Meta-Object Protocol for Groovy dynamic dispatch
    runtime_mop: Arc<RwLock<RuntimeMOP>>,
}

impl VirtualMachine {
    /// Create a new JVM interpreter
    ///
    /// # Bootstrap Classes
    ///
    /// The VM automatically loads these classes during initialization:
    /// - `java/lang/Object` - Required for all class loading (superclass chain)
    ///
    /// This matches real JVM behavior where core classes are part of the bootstrap
    /// classloader and must exist before any user classes can be loaded.
    pub fn new(config: MemoryConfig) -> Self {
        let max_heap_size = config.max_heap_size;
        let shared_method_area = MethodArea::new();
        let class_loader = ClassLoader::with_method_area(shared_method_area.clone());
        let method_area = Arc::new(RwLock::new(shared_method_area));

        // Bootstrap: Pre-load core JDK classes
        // Per JVM Spec, java.lang.Object must exist before any user class can be loaded
        // All classes (except Object itself) extend Object, so the superclass resolution
        // during class loading requires Object to be present in the MethodArea.
        //
        // L1: Pre-create java.lang.Object stub OUTSIDE the lock to prevent any potential
        // deadlock if stub loading becomes more complex in the future. Currently safe
        // (different lock objects), but this makes the code more obviously correct.
        {
            let area = method_area
                .write()
                .expect("Failed to acquire MethodArea lock during VM initialization");

            // Pre-load java.lang.Object from JdkStubRegistry
            // This will auto-register the stub class in the MethodArea
            // Note: MethodArea internally uses dot notation (java.lang.Object)
            //
            // The call to get_class is safe because:
            // 1. We hold write lock on MethodArea (outer)
            // 2. get_class acquires read lock on loaded_classes (inner, different lock)
            // 3. If not found, get_class may call register_class which acquires
            //    write lock on loaded_classes (still inner, still different lock)
            // 4. No re-entrancy on same lock → no deadlock
            area.get_class("java.lang.Object").expect(
                "Failed to load java.lang.Object stub. This is a critical bug in JdkStubRegistry.",
            );
        }

        VirtualMachine {
            config,
            class_loader,
            method_area,
            heap: Heap::new(max_heap_size),
            class_bytecode: HashMap::new(),
            class_aliases: HashMap::new(),
            runtime_mop: Arc::new(RwLock::new(RuntimeMOP::new())),
        }
    }

    /// Get access to the runtime MOP (shared with executors).
    pub fn runtime_mop(&self) -> Arc<RwLock<RuntimeMOP>> {
        Arc::clone(&self.runtime_mop)
    }

    /// Read a `java.lang.String` value out of the heap by its object id.
    /// Useful for inspecting `String`-returning methods after execution.
    pub fn heap_string(&self, id: crate::interpreter::HeapId) -> VMResult<String> {
        self.heap.get_string(id)
    }

    /// Render a value the way Java's `String.valueOf` / `toString` would, for
    /// the common built-in types. Used to display the result of an evaluated
    /// snippet so that a `String`, boxed wrapper, collection, or array prints
    /// its contents rather than an opaque `Object@<id>` handle.
    ///
    /// Delegates to the shared `Heap` renderer so display and the runtime
    /// `append` / `String.valueOf` path never diverge. User-defined objects
    /// without a synthesised `toString` fall back to `internal/class/Name@<id>`.
    pub fn display_value(&self, value: &Value) -> String {
        self.heap.render_value(value)
    }

    /// The internal class name (e.g. `java/lang/String`) of a heap object, if known.
    pub fn object_class_name(&self, id: HeapId) -> Option<String> {
        self.heap.get_object_class_name(id).ok()
    }

    /// Everything the executed program wrote to `System.out` so far.
    pub fn captured_stdout(&self) -> &str {
        self.heap.captured_stdout()
    }

    /// Everything the executed program wrote to `System.err` so far.
    pub fn captured_stderr(&self) -> &str {
        self.heap.captured_stderr()
    }

    /// Buffer program output instead of echoing it to the process console. Call
    /// before executing to capture output without polluting stdout/stderr.
    pub fn set_suppress_console(&mut self, suppress: bool) {
        self.heap.set_suppress_console(suppress);
    }

    /// Build a `java.lang.String[]` on the heap from plain Rust strings.
    ///
    /// Convenience for callers that need to pass a real `String[] args` to a
    /// `main` entry point invoked via [`Self::invoke_static`] (e.g. a CLI
    /// forwarding its own argv). Not exercised by the VM itself internally;
    /// existing callers of `invoke_static("...", "main", "([Ljava/lang/String;)V", ...)`
    /// pass a null reference instead.
    pub fn make_string_array(&mut self, values: &[String]) -> VMResult<Value> {
        use crate::interpreter::heap::ArrayElementType;

        let id = self.heap.allocate_array(
            ArrayElementType::Reference("java/lang/String".to_string()),
            values.len() as i32,
        )?;
        for (i, s) in values.iter().enumerate() {
            let string_ref = self.heap.intern_string(s)?;
            self.heap
                .array_store(id, i as i32, Value::Reference(string_ref))?;
        }
        Ok(Value::Reference(ObjectRef::Array(id)))
    }

    /// Load a class from raw bytecode
    pub fn load_class_from_bytes(&mut self, class_name: &str, bytecode: &[u8]) -> VMResult<()> {
        use crate::interpreter::linker::ClassLinker;

        // Parse the class file
        let mut class = ClassLinker::parse_class(bytecode, &self.class_loader)?;
        let canonical_name = class.name.clone();

        // Link the class (resolve superclass vtable, build itable with default methods)
        ClassLinker::link_class(&mut class, &self.class_loader)?;

        // The class loader and executor share one MethodArea. Publish only
        // after parsing and linking have both succeeded, so failed definitions
        // are never visible to later classes or execution.
        self.class_loader.register_loaded_class(class)?;

        // Store the raw bytecode for method execution
        self.class_bytecode
            .insert(canonical_name.clone(), bytecode.to_vec());
        if canonical_name != class_name {
            self.class_aliases
                .insert(class_name.to_string(), canonical_name);
        }

        Ok(())
    }

    /// Parse, link, and install a collection of class files without depending
    /// on its input order.
    ///
    /// Definitions remain private to a staging loader until their complete
    /// superclass and interface closure has linked successfully. Structural
    /// dependencies are linked depth-first, so an alphabetically earlier child
    /// can safely precede its parent. A failed class only poisons classes that
    /// structurally depend on it; unrelated valid definitions are still
    /// installed and listed in the returned report.
    pub fn load_classes_from_bytes(
        &mut self,
        classes: &[(String, Vec<u8>)],
    ) -> BatchClassLoadReport {
        use crate::interpreter::linker::ClassLinker;

        // The staging registry is private, with an immutable shared fallback
        // to earlier definitions. This keeps publication isolated without an
        // O(already-loaded classes) metadata clone for every subsequent batch.
        let staging_loader =
            ClassLoader::staging_with_fallback(self.class_loader.method_area().clone());

        let mut staged = HashMap::new();
        let mut failures_by_name = HashMap::new();
        let mut seen_definitions = HashSet::new();
        for (requested_name, bytecode) in classes {
            let requested_label = requested_name.as_str();
            let requested_name = normalize_vm_class_name(requested_name);
            match ClassLinker::parse_class(bytecode, &staging_loader) {
                Ok(class) if class.name != requested_name => {
                    failures_by_name.insert(
                        requested_name.clone(),
                        format!(
                            "class file for {requested_name} declares the incompatible name {}",
                            class.name
                        ),
                    );
                }
                Ok(class) if self.class_loader.is_class_loaded(&class.name) => {
                    failures_by_name.insert(
                        requested_name.clone(),
                        format!("duplicate definition of {}", class.name),
                    );
                }
                Ok(class) => {
                    if !seen_definitions.insert(class.name.clone()) {
                        failures_by_name.insert(
                            requested_name.clone(),
                            format!("duplicate definition of {requested_name}"),
                        );
                        staged.remove(&requested_name);
                    } else {
                        staged.insert(
                            class.name.clone(),
                            StagedClass {
                                class,
                                requested_name: requested_label,
                                bytecode: bytecode.as_slice(),
                            },
                        );
                    }
                }
                Err(error) => {
                    failures_by_name.insert(requested_name, error.to_string());
                }
            }
        }

        let mut states = HashMap::new();
        let mut linked_order = Vec::new();
        let mut names: Vec<_> = staged.keys().cloned().collect();
        names.sort();
        for name in &names {
            if let Err(error) = link_staged_class(
                name,
                &staged,
                &failures_by_name,
                &staging_loader,
                &mut states,
                &mut linked_order,
            ) {
                failures_by_name.insert(name.clone(), error);
            }
        }

        let mut definitions = Vec::new();
        let mut installations = Vec::new();
        for name in linked_order {
            let Some(staged_class) = staged.get(&name) else {
                continue;
            };
            match staging_loader.get_loaded_class(&name) {
                Ok(class) => definitions.push(class),
                Err(error) => {
                    failures_by_name.insert(name.clone(), error.to_string());
                    continue;
                }
            }
            installations.push((name, staged_class.requested_name, staged_class.bytecode));
        }

        let mut loaded_classes = Vec::new();
        if !definitions.is_empty() {
            match self.class_loader.register_loaded_classes(definitions) {
                Ok(()) => {
                    for (canonical_name, requested_name, bytecode) in installations {
                        self.class_bytecode
                            .insert(canonical_name.clone(), bytecode.to_vec());
                        if canonical_name != requested_name {
                            self.class_aliases
                                .insert(requested_name.to_string(), canonical_name.clone());
                        }
                        loaded_classes.push(canonical_name);
                    }
                }
                Err(error) => {
                    let error = format!("atomic batch publication failed: {error}");
                    for (canonical_name, _, _) in installations {
                        failures_by_name.insert(canonical_name, error.clone());
                    }
                }
            }
        }
        loaded_classes.sort();
        let mut failures: Vec<_> = failures_by_name
            .into_iter()
            .map(|(class_name, error)| ClassLoadFailure { class_name, error })
            .collect();
        failures.sort_by(|left, right| left.class_name.cmp(&right.class_name));
        BatchClassLoadReport {
            loaded_classes,
            failures,
        }
    }

    /// Invoke a static method using the full Executor with 199+ opcode support.
    ///
    /// This method parses the class file, extracts the constant pool, method bytecode,
    /// and executes using the full Executor which supports all JVM opcodes including:
    /// - ldc (0x12) - load constants
    /// - new (0xBB) - object creation
    /// - invokestatic (0xB8) - static method calls
    /// - areturn (0xB0) - return references
    pub fn invoke_static(
        &mut self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<Value> {
        use cafebabe::parse_class;
        use executor::MethodBytecode;
        use heap::ClassRef;

        let canonical_name = self
            .class_aliases
            .get(class_name)
            .cloned()
            .unwrap_or_else(|| class_name.to_string());

        // Get the class bytecode
        let bytecode = self
            .class_bytecode
            .get(&canonical_name)
            .ok_or_else(|| VMError::ClassNotFound(canonical_name.clone()))?
            .clone();

        // Parse the constant pool from raw bytecode
        let constant_pool = parse_constant_pool(&bytecode)?;

        // Parse the class file to find the method
        let class_file =
            parse_class(&bytecode).map_err(|e| VMError::ClassFormatError(format!("{:?}", e)))?;
        let declared_name = class_file.this_class.to_string().replace('/', ".");
        let canonical_name = if canonical_name == class_name && declared_name != canonical_name {
            // If the caller didn't provide an alias mapping (e.g. class was loaded under the
            // declared name) keep the declared name as canonical.
            declared_name
        } else {
            canonical_name
        };

        // Find the method
        let method = class_file
            .methods
            .iter()
            .find(|m| m.name == method_name && m.descriptor.to_string() == descriptor)
            .ok_or_else(|| VMError::NoSuchMethod(format!("{}:{}", method_name, descriptor)))?;

        // Extract the Code attribute
        use cafebabe::attributes::AttributeData;

        let code_data = method
            .attributes
            .iter()
            .find_map(|attr| {
                if let AttributeData::Code(code) = &attr.data {
                    Some(code)
                } else {
                    None
                }
            })
            .ok_or_else(|| VMError::NoSuchMethod(format!("No code for {}", method_name)))?;

        let max_stack = code_data.max_stack;
        let max_locals = code_data.max_locals;
        let code = code_data.code.to_vec();
        let access_flags = method.access_flags.bits();

        // Convert exception table
        // cafebabe resolves catch_type to the class name string (None for catch-all)
        let exception_table: Vec<frame::ExceptionTableEntry> = code_data
            .exception_table
            .iter()
            .map(|e| frame::ExceptionTableEntry {
                start_pc: e.start_pc,
                end_pc: e.end_pc,
                handler_pc: e.handler_pc,
                catch_type: e.catch_type.as_ref().map(|s| s.to_string()),
            })
            .collect();

        // Build method registry from ALL loaded classes (not just the main class)
        // This includes closure classes and any other dependencies
        let mut method_registry = HashMap::new();
        for (loaded_class_name, loaded_bytecode) in &self.class_bytecode {
            if let Ok(loaded_class_file) = parse_class(loaded_bytecode) {
                for m in &loaded_class_file.methods {
                    let m_name = m.name.to_string();
                    let m_desc = m.descriptor.to_string();

                    if let Some(code_attr) = m.attributes.iter().find_map(|attr| {
                        if let AttributeData::Code(code) = &attr.data {
                            Some(code)
                        } else {
                            None
                        }
                    }) {
                        let m_exception_table: Vec<frame::ExceptionTableEntry> = code_attr
                            .exception_table
                            .iter()
                            .map(|e| frame::ExceptionTableEntry {
                                start_pc: e.start_pc,
                                end_pc: e.end_pc,
                                handler_pc: e.handler_pc,
                                catch_type: e.catch_type.as_ref().map(|s| s.to_string()),
                            })
                            .collect();

                        method_registry.insert(
                            (loaded_class_name.clone(), m_name, m_desc),
                            MethodBytecode {
                                max_locals: code_attr.max_locals,
                                max_stack: code_attr.max_stack,
                                code: code_attr.code.to_vec(),
                                exception_table: m_exception_table,
                                access_flags: m.access_flags.bits(),
                            },
                        );
                    }
                }
            }
        }

        // Create the Executor with constant pool, method registry, method area, and native registry
        let mut executor = Executor::new(code.clone())
            .with_constant_pool(constant_pool)
            .with_methods(method_registry)
            .with_method_area(self.method_area.clone())
            .with_native_registry(natives::NativeRegistry::new())
            .with_runtime_mop(Arc::clone(&self.runtime_mop));

        // Add all class bytecode so executor can switch constant pools per class
        for (loaded_class_name, loaded_bytecode) in &self.class_bytecode {
            executor.add_class_bytecode(loaded_class_name.clone(), loaded_bytecode.clone());
        }

        // Create execution environment
        let mut thread = Thread::new(self.config.max_stack_depth, None);
        // Use VM's heap so we can access objects after execution
        //
        // MonitorTable is created here and passed as &mut to executor.step(). Each
        // frame records the monitors it acquired and releases them when it returns or
        // unwinds, so synchronized methods and MONITORENTER/MONITOREXIT share one path.
        let mut monitors = MonitorTable::new();

        // Best-effort: provide an implicit receiver for "script-like" entrypoints.
        //
        // Groovy scripts commonly compile to a `run()Ljava/lang/Object;` method that expects a
        // script instance in local 0, even when the method is marked static in our generated
        // bytecode. GradleExecutor already executes scripts with an explicit receiver in slot 0,
        // so this keeps VirtualMachine's convenience API consistent with the Gradle path.
        let is_static = (access_flags & 0x0008) != 0; // ACC_STATIC
        let wants_implicit_receiver =
            method_name == "run" && descriptor == "()Ljava/lang/Object;" && args.is_empty();
        let needs_receiver = !is_static || wants_implicit_receiver;
        let effective_args: Vec<Value> = if needs_receiver {
            // Reserve local 0 now so the target frame can act as the synchronous
            // class-initialization boundary. The actual construction happens
            // only after initialization completes.
            let mut out = Vec::with_capacity(args.len() + 1);
            out.push(Value::Reference(ObjectRef::Null));
            out.extend_from_slice(args);
            out
        } else {
            args.to_vec()
        };
        if std::env::var("HYPERION_DEBUG_VM_ENTRY").is_ok() {
            eprintln!(
                "DEBUG: invoke_static {}.{}{} access_flags=0x{:04X} is_static={} effective_args={}",
                canonical_name,
                method_name,
                descriptor,
                access_flags,
                is_static,
                effective_args.len()
            );
        }

        // Create initial frame with args
        let mut initial_frame = Frame::with_bytecode(
            canonical_name.clone(),
            method_name.to_string(),
            max_locals,
            max_stack,
            effective_args,
            code,
            descriptor.to_string(),
            exception_table,
        )?;

        // A static synchronized method locks its class object (JVMS §2.11.10).
        monitor::enter_static_synchronized(
            access_flags,
            &canonical_name,
            &mut initial_frame,
            &thread,
            &mut self.heap,
            &mut monitors,
        )?;

        thread.push_frame(initial_frame)?;

        if let Err(error) =
            executor.ensure_initialized(&canonical_name, &mut thread, &mut self.heap, &mut monitors)
        {
            if error.is_catchable() || matches!(error, VMError::ThrownException(_)) {
                let exception = executor.materialize_exception(&error, &mut self.heap)?;
                let exception_class = exception
                    .heap_id()
                    .ok_or_else(VMError::invalid_object_ref)
                    .and_then(|id| self.heap.get_object_class_name(id))?;
                return Err(VMError::UncaughtException {
                    exception_class,
                    stack_trace: format!("  at {canonical_name}.{method_name}"),
                    exception,
                });
            }
            return Err(error);
        }
        if needs_receiver {
            let instance_field_count = self
                .method_area
                .read()
                .map_err(|_| VMError::internal("Failed to acquire method_area lock"))?
                .get_class(&canonical_name)?
                .field_layout
                .field_count;
            let obj_id = self.heap.allocate_object(ClassRef {
                name: canonical_name.clone(),
                instance_field_count,
                static_field_count: 0,
            })?;
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            let slot = frame
                .locals
                .first_mut()
                .ok_or_else(|| VMError::internal("instance entrypoint has no receiver local"))?;
            *slot = Some(Value::Reference(ObjectRef::Object(obj_id)));
        }

        // Execute until completion
        loop {
            match executor.step(&mut thread, &mut self.heap, &mut monitors) {
                Ok(true) => {
                    // Check if GC should run (heap usage > 70%)
                    if self.config.enable_gc && self.heap.should_collect() {
                        tracing::debug!(
                            "Heap usage high ({} / {} bytes, {:.1}%), triggering GC",
                            self.heap.memory_used(),
                            self.heap.memory_limit(),
                            (self.heap.memory_used() as f64 / self.heap.memory_limit() as f64)
                                * 100.0
                        );

                        // Collect GC roots from all live references
                        let mut roots = gc::GCRoots::new();

                        // Add stack frames (locals + operand stack)
                        thread.add_gc_roots(&mut roots);

                        // Add monitored objects
                        monitors.add_gc_roots(&mut roots);

                        // Heap-owned pools are intrinsic roots. Runtime static
                        // fields remain external roots owned by MethodArea.
                        if let Ok(area) = self.method_area.read() {
                            for value in area.static_field_values() {
                                roots.add_value(&value);
                            }
                        }

                        // Run GC
                        let stats = gc::GarbageCollector::collect(&mut self.heap, &roots);
                        tracing::info!(
                            "GC collected {} objects, freed {} bytes, {} objects remaining (took {}ms)",
                            stats.objects_freed,
                            stats.bytes_freed,
                            stats.objects_remaining,
                            stats.duration_ms
                        );
                    }

                    // Continue execution
                }
                Ok(false) => {
                    // Execution complete - return top stack value from final frame
                    if let Some(frame) = thread.current_frame_mut() {
                        if let Some(value) = frame.pop_raw() {
                            return Ok(value);
                        }
                    }
                    // Void return or empty stack
                    return Ok(Value::Reference(ObjectRef::Null));
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    /// Unbox an Integer object reference to a primitive int
    ///
    /// Returns the int value if the reference points to a valid Integer object,
    /// or None if the reference is null or not an Integer.
    pub fn unbox_int(&self, value: &Value) -> Option<i32> {
        if let Value::Reference(ObjectRef::Object(heap_id)) = value {
            // Use existing heap method to unbox
            self.heap.get_boxed_int(*heap_id).ok()
        } else if let Value::Int(i) = value {
            // Already an int
            Some(*i)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod staged_link_tests {
    use super::*;

    #[test]
    fn staged_linking_rejects_a_superclass_cycle() {
        let mut left = method_area::LoadedClass::new("cycle.Left".to_string());
        left.set_superclass("cycle.Right".to_string());
        let mut right = method_area::LoadedClass::new("cycle.Right".to_string());
        right.set_superclass("cycle.Left".to_string());
        let staged = HashMap::from([
            (
                left.name.clone(),
                StagedClass {
                    class: left,
                    requested_name: "cycle/Left",
                    bytecode: &[],
                },
            ),
            (
                right.name.clone(),
                StagedClass {
                    class: right,
                    requested_name: "cycle/Right",
                    bytecode: &[],
                },
            ),
        ]);
        let loader = ClassLoader::new();
        let mut states = HashMap::new();
        let mut linked_order = Vec::new();

        let error = link_staged_class(
            "cycle.Left",
            &staged,
            &HashMap::new(),
            &loader,
            &mut states,
            &mut linked_order,
        )
        .unwrap_err();
        assert!(error.contains("circular class hierarchy"));
        assert!(linked_order.is_empty());
    }
}
