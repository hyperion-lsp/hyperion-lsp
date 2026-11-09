// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bytecode instruction executor

mod handlers;
mod opcode;

pub use opcode::Opcode;

use super::class_loader::ClassLoader;
use super::errors::{VMError, VMResult};
use super::exception_handler;
use super::frame::{ExceptionTableEntry, Frame};
use super::gc::{GCRoots, GarbageCollector};
use super::heap::{ClassRef, Heap};
use super::method_area::MethodArea;
use super::monitor::MonitorTable;
use super::natives::NativeRegistry;
use super::runtime_mop::RuntimeMOP;
use super::thread::Thread;
use super::value::{ObjectRef, Value};
use crate::producer::ConstantPoolItem;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::debug;

const MAX_ARRAY_DIMENSIONS: usize = 255;

#[derive(Debug, PartialEq, Eq)]
enum RuntimeReferenceType<'a> {
    Class(Cow<'a, str>),
    Array(RuntimeArrayType<'a>),
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeArrayType<'a> {
    dimensions: usize,
    leaf: RuntimeArrayLeaf<'a>,
}

#[derive(Debug, PartialEq, Eq)]
enum RuntimeArrayLeaf<'a> {
    Primitive(u8),
    Class(Cow<'a, str>),
}

fn normalize_binary_name(name: &str) -> Option<Cow<'_, str>> {
    let bytes = name.as_bytes();
    let is_separator = |byte| matches!(byte, b'.' | b'/');
    if bytes.is_empty()
        || bytes.contains(&0)
        || name.contains(['[', ';'])
        || is_separator(bytes[0])
        || is_separator(*bytes.last()?)
        || bytes
            .windows(2)
            .any(|pair| is_separator(pair[0]) && is_separator(pair[1]))
    {
        return None;
    }
    if name.as_bytes().contains(&b'.') {
        Some(Cow::Owned(name.replace('.', "/")))
    } else {
        Some(Cow::Borrowed(name))
    }
}

fn parse_runtime_reference_type(name: &str) -> Option<RuntimeReferenceType<'_>> {
    if !name.starts_with('[') {
        return normalize_binary_name(name).map(RuntimeReferenceType::Class);
    }

    let dimensions = name.bytes().take_while(|byte| *byte == b'[').count();
    if dimensions == 0 || dimensions > MAX_ARRAY_DIMENSIONS {
        return None;
    }
    let leaf = &name[dimensions..];
    let leaf = match leaf.as_bytes() {
        [primitive @ (b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z')] => {
            RuntimeArrayLeaf::Primitive(*primitive)
        }
        [b'L', .., b';'] if leaf.len() > 2 => {
            let class_name = &leaf[1..leaf.len() - 1];
            RuntimeArrayLeaf::Class(normalize_binary_name(class_name)?)
        }
        _ => return None,
    };
    Some(RuntimeReferenceType::Array(RuntimeArrayType {
        dimensions,
        leaf,
    }))
}

/// Methods the executor routes through the gradle native bridge on script/closure objects.
const GRADLE_SCRIPT_METHODS: &[&str] = &[
    "dependencies",
    "repositories",
    "plugins",
    "forkedRun",
    "resolve",
    "openjpa",
    "tasks",
    "getTasks",
    "sourceSets",
    "getSourceSets",
    "getLayout",
    "file",
    "files",
    "getMetamodel",
    "ext",
    "subprojects",
    "allprojects",
    "buildscript",
    "configure",
    "getExtensions",
    "task",
    "apply",
    "project",
    "rootProject",
    "setGroup",
    "setVersion",
    "getProperty",
    "setProperty",
];

/// Subset: void-returning DSL blocks whose return value is NOT pushed onto the operand stack.
const GRADLE_VOID_DSL_METHODS: &[&str] = &[
    "dependencies",
    "repositories",
    "plugins",
    "forkedRun",
    "resolve",
    "openjpa",
    "tasks",
    "sourceSets",
];

pub(super) fn is_gradle_script_method(name: &str) -> bool {
    GRADLE_SCRIPT_METHODS.contains(&name) || name.starts_with("set") || name.starts_with("get")
}

pub(super) fn is_gradle_void_dsl_method(name: &str) -> bool {
    GRADLE_VOID_DSL_METHODS.contains(&name)
}

/// Result of executing a single bytecode instruction
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Continue execution (normal instruction)
    Continue,
    /// Frame completed (method returned or finished)
    FrameComplete(Option<Value>),
}

/// Cursor and stack boundary for a synchronous call driven re-entrantly from a
/// native handler. One owner restores the cursor and unwinds partial frames on
/// every failure path.
pub(crate) struct NestedExecutionState {
    target_depth: usize,
    saved_pc: usize,
    saved_bytecode: Arc<Vec<u8>>,
    budget_subject: &'static str,
}

impl NestedExecutionState {
    pub(crate) fn capture(
        executor: &Executor,
        thread: &Thread,
        budget_subject: &'static str,
    ) -> Self {
        Self {
            target_depth: thread.depth(),
            saved_pc: executor.pc,
            saved_bytecode: executor.bytecode.clone(),
            budget_subject,
        }
    }

    pub(crate) fn target_depth(&self) -> usize {
        self.target_depth
    }

    pub(crate) fn restore(self, executor: &mut Executor) {
        executor.bytecode = self.saved_bytecode;
        executor.pc = self.saved_pc;
    }
}

/// Metadata for a resolved method's bytecode
#[derive(Debug, Clone)]
pub struct MethodBytecode {
    /// Maximum local variables
    pub max_locals: u16,
    /// Maximum operand stack size
    pub max_stack: u16,
    /// Bytecode instructions
    pub code: Vec<u8>,
    /// Exception table for catch block handling
    pub exception_table: Vec<ExceptionTableEntry>,
    /// Method access flags (ACC_PUBLIC, ACC_SYNCHRONIZED, etc.)
    pub access_flags: u16,
}

/// JVM method access flag constants (per JVMS 4.6)
pub mod access_flags {
    /// Method is public - accessible from any class
    pub const ACC_PUBLIC: u16 = 0x0001;
    /// Method is private - accessible only from defining class
    pub const ACC_PRIVATE: u16 = 0x0002;
    /// Method is protected - accessible from same package or subclasses
    pub const ACC_PROTECTED: u16 = 0x0004;
    /// Method is static
    pub const ACC_STATIC: u16 = 0x0008;
    /// Method is synchronized - acquires monitor on entry
    pub const ACC_SYNCHRONIZED: u16 = 0x0020;
    /// Method or class is abstract.
    pub const ACC_ABSTRACT: u16 = 0x0400;
    /// Class metadata denotes an interface.
    pub const ACC_INTERFACE: u16 = 0x0200;
}

/// Bytecode instruction executor
pub struct Executor {
    /// Program counter for bytecode execution
    pc: usize,
    /// Current bytecode being executed.
    /// Wrapped in Arc to avoid full copies when syncing from Frame.
    bytecode: Arc<Vec<u8>>,
    /// Constant pool for resolving method/field references.
    /// Wrapped in Arc so cache-hit clones are O(1) refcount increments.
    constant_pool: Option<Arc<Vec<ConstantPoolItem>>>,
    /// Constant pool cache per class name
    class_constant_pools: HashMap<String, Arc<Vec<ConstantPoolItem>>>,
    /// Raw bytecode per class for constant pool parsing
    class_bytecode: HashMap<String, Vec<u8>>,
    /// Native method registry for calling native methods
    native_registry: Option<NativeRegistry>,
    /// User method registry: (class_name, method_name, descriptor) -> bytecode
    method_registry: HashMap<(String, String, String), MethodBytecode>,
    /// Class loader for dynamic class resolution and loading
    class_loader: Option<Arc<RwLock<ClassLoader>>>,
    /// Method area for storing loaded class information
    method_area: Option<Arc<RwLock<MethodArea>>>,
    /// Call site cache for invokedynamic ((class_name, CP index) -> CallSite)
    /// The key includes class name to prevent cross-class pollution when
    /// different classes have different invokedynamic sites at the same CP index.
    call_site_cache: HashMap<(String, u16), crate::interpreter::CallSite>,
    /// Runtime MOP for Groovy dynamic method dispatch
    runtime_mop: Option<Arc<RwLock<RuntimeMOP>>>,
    /// Bootstrap context for invokedynamic resolution
    bootstrap_context: super::bootstrap::BootstrapContext,
    /// Gradle native bridge for intercepting Gradle stub method calls
    gradle_native_bridge: Option<Arc<super::gradle_natives::GradleNativeBridge>>,
    /// Heap id of the user `Thread` object whose `run()` is currently executing,
    /// so `Thread.currentThread().getName()` can be answered. We execute threads
    /// synchronously (start() runs run() inline), so this is a single slot that
    /// tracks the most recently started thread. `None` means the main thread.
    current_thread: Option<crate::interpreter::HeapId>,
    /// `StringConcatFactory` recipes for invokedynamic string concatenation,
    /// keyed by call site (class name, CP index). `(recipe, constants)` — the
    /// recipe drives `+` desugaring (Java 9+ `makeConcatWithConstants`).
    concat_recipes: HashMap<(String, u16), (String, Vec<String>)>,
    /// Call sites (class name, CP index) bootstrapped by `LambdaMetafactory`, so
    /// the indy creates a lambda *object* capturing its args rather than calling
    /// the implementation method immediately.
    lambda_sites: std::collections::HashSet<(String, u16)>,
    /// Record `ObjectMethods` call sites: `(kind, simpleClassName, components)`
    /// where kind is toString/equals/hashCode. Drives generated record methods.
    record_methods: HashMap<(String, u16), (String, String, Vec<String>)>,
    /// Lowest frame depth visible to exception matching during one synchronous
    /// nested step. Exceptions escaping that frame return to the Rust driver
    /// before handlers in an outer Java caller are considered.
    exception_unwind_floor: Option<usize>,
}

impl Executor {
    /// Create a new executor with bytecode
    pub fn new(bytecode: Vec<u8>) -> Self {
        Executor {
            pc: 0,
            bytecode: Arc::new(bytecode),
            constant_pool: None,
            class_constant_pools: HashMap::new(),
            class_bytecode: HashMap::new(),
            native_registry: None,
            method_registry: HashMap::new(),
            class_loader: None,
            method_area: None,
            call_site_cache: HashMap::new(),
            runtime_mop: None,
            bootstrap_context: super::bootstrap::BootstrapContext::new(),
            gradle_native_bridge: None,
            current_thread: None,
            concat_recipes: HashMap::new(),
            lambda_sites: std::collections::HashSet::new(),
            record_methods: HashMap::new(),
            exception_unwind_floor: None,
        }
    }

    /// Set constant pool for method/field resolution
    pub fn with_constant_pool(mut self, constant_pool: Vec<ConstantPoolItem>) -> Self {
        self.constant_pool = Some(Arc::new(constant_pool));
        self
    }

    /// Set native method registry
    pub fn with_native_registry(mut self, registry: NativeRegistry) -> Self {
        self.native_registry = Some(registry);
        self
    }

    /// Set class loader for dynamic class resolution
    pub fn with_class_loader(mut self, class_loader: Arc<RwLock<ClassLoader>>) -> Self {
        self.class_loader = Some(class_loader);
        self
    }

    /// Set method area for loaded class information
    pub fn with_method_area(mut self, method_area: Arc<RwLock<MethodArea>>) -> Self {
        self.method_area = Some(method_area);
        self
    }

    /// Add class bytecode for constant pool resolution
    pub fn add_class_bytecode(&mut self, class_name: String, bytecode: Vec<u8>) {
        self.class_bytecode.insert(class_name, bytecode);
    }

    /// Switch to the constant pool for a specific class
    /// Parses and caches the constant pool if not already cached
    fn switch_constant_pool_for_class(&mut self, class_name: &str) {
        // Check if already cached — Arc clone is O(1) refcount increment
        if let Some(cp) = self.class_constant_pools.get(class_name) {
            self.constant_pool = Some(Arc::clone(cp));
            return;
        }

        // Parse from class bytecode if available
        if let Some(bytecode) = self.class_bytecode.get(class_name) {
            if let Ok(cp) = super::parse_constant_pool(bytecode) {
                let cp = Arc::new(cp);
                self.class_constant_pools
                    .insert(class_name.to_string(), Arc::clone(&cp));
                self.constant_pool = Some(cp);
            }
        }
    }

    /// Set runtime MOP for Groovy dynamic method dispatch
    pub fn with_runtime_mop(mut self, runtime_mop: Arc<RwLock<RuntimeMOP>>) -> Self {
        self.runtime_mop = Some(runtime_mop);
        self
    }

    /// Set Gradle native bridge for intercepting Gradle stub calls
    pub fn with_gradle_native_bridge(
        mut self,
        bridge: Arc<super::gradle_natives::GradleNativeBridge>,
    ) -> Self {
        self.gradle_native_bridge = Some(bridge);
        self
    }

    /// Check if the Gradle native bridge is in recording proxy mode.
    pub(crate) fn is_recording_proxy(&self) -> bool {
        self.gradle_native_bridge
            .as_ref()
            .is_some_and(|b| b.recording_proxy)
    }

    /// Register a user-defined method bytecode
    pub fn register_method(
        &mut self,
        class_name: String,
        method_name: String,
        descriptor: String,
        bytecode: MethodBytecode,
    ) {
        self.method_registry
            .insert((class_name, method_name, descriptor), bytecode);
    }

    /// Register multiple methods at once
    pub fn with_methods(
        mut self,
        methods: HashMap<(String, String, String), MethodBytecode>,
    ) -> Self {
        self.method_registry = methods;
        self
    }

    /// Check JVM reference assignment compatibility.
    ///
    /// In addition to ordinary class and interface ancestry this handles array
    /// covariance, primitive-array invariance, and the three direct array
    /// supertypes (`Object`, `Cloneable`, and `Serializable`). Malformed or
    /// unavailable types are deliberately not assignment compatible.
    fn is_subclass_of(&self, source_class: &str, target_class: &str) -> bool {
        let Some(source) = parse_runtime_reference_type(source_class) else {
            return false;
        };
        let Some(target) = parse_runtime_reference_type(target_class) else {
            return false;
        };

        match (source, target) {
            (RuntimeReferenceType::Class(source), RuntimeReferenceType::Class(target)) => {
                self.is_named_subclass_of(source.as_ref(), target.as_ref())
            }
            (RuntimeReferenceType::Array(source), RuntimeReferenceType::Class(target)) => {
                matches!(
                    target.as_ref(),
                    "java/lang/Object" | "java/lang/Cloneable" | "java/io/Serializable"
                ) && self.is_array_leaf_available(&source)
            }
            (RuntimeReferenceType::Class(_), RuntimeReferenceType::Array(_)) => false,
            (RuntimeReferenceType::Array(source), RuntimeReferenceType::Array(target)) => {
                self.is_array_assignable_to(&source, &target)
            }
        }
    }

    fn is_array_assignable_to(
        &self,
        source: &RuntimeArrayType<'_>,
        target: &RuntimeArrayType<'_>,
    ) -> bool {
        use std::cmp::Ordering;

        match source.dimensions.cmp(&target.dimensions) {
            Ordering::Less => false,
            Ordering::Greater => matches!(
                &target.leaf,
                RuntimeArrayLeaf::Class(target) if matches!(
                    target.as_ref(),
                    "java/lang/Object" | "java/lang/Cloneable" | "java/io/Serializable"
                ) && self.is_array_leaf_available(source)
            ),
            Ordering::Equal => match (&source.leaf, &target.leaf) {
                (RuntimeArrayLeaf::Primitive(source), RuntimeArrayLeaf::Primitive(target)) => {
                    source == target
                }
                (RuntimeArrayLeaf::Class(source), RuntimeArrayLeaf::Class(target)) => {
                    self.is_named_subclass_of(source.as_ref(), target.as_ref())
                }
                _ => false,
            },
        }
    }

    fn is_array_leaf_available(&self, array: &RuntimeArrayType<'_>) -> bool {
        match &array.leaf {
            RuntimeArrayLeaf::Primitive(_) => true,
            RuntimeArrayLeaf::Class(class_name) => {
                self.is_named_subclass_of(class_name.as_ref(), class_name.as_ref())
            }
        }
    }

    /// Traverse the complete class/interface graph iteratively. One outer
    /// MethodArea guard and one steady-state metadata guard cover the query;
    /// class method tables are never cloned. This VM has one defining-loader
    /// namespace, so canonical binary name is the runtime type identity here.
    fn is_named_subclass_of(&self, source_class: &str, target_class: &str) -> bool {
        let Some(method_area) = &self.method_area else {
            return false;
        };
        let Ok(area) = method_area.read() else {
            return false;
        };
        area.is_subtype(source_class, target_class).unwrap_or(false)
    }

    /// Resolve and access-check the non-array leaf of a reference type. This
    /// gives checkcast, instanceof, anewarray, and multianewarray the same eager
    /// linkage point, including when the value being tested is null.
    fn ensure_reference_type_resolved(&self, caller_class: &str, type_name: &str) -> VMResult<()> {
        parse_runtime_reference_type(type_name).ok_or_else(|| {
            VMError::ClassFormatError(format!("invalid reference type {type_name}"))
        })?;
        self.check_symbolic_class_access(caller_class, type_name)
            .map_err(|error| match error {
                VMError::ClassNotFound(class_name) => VMError::NoClassDefFoundError(class_name),
                error => error,
            })
    }

    fn runtime_reference_type(heap: &Heap, reference: ObjectRef) -> VMResult<String> {
        match reference {
            ObjectRef::Object(heap_id) | ObjectRef::Array(heap_id) => {
                Ok(heap.get_class(heap_id)?.name)
            }
            ObjectRef::MethodHandle(_) => Ok("java/lang/invoke/MethodHandle".to_string()),
            ObjectRef::Null => Err(VMError::NullPointerException),
        }
    }

    /// Allocate a Java Throwable object for an interpreter-generated error.
    /// Explicitly thrown and synchronously transported exceptions bypass this
    /// function so their object identity is never changed.
    pub(crate) fn materialize_exception(
        &self,
        error: &VMError,
        heap: &mut Heap,
    ) -> VMResult<ObjectRef> {
        match error {
            VMError::ThrownException(reference) => return Ok(*reference),
            VMError::UncaughtException { exception, .. } => return Ok(*exception),
            VMError::MemoryLimitExceeded => return Ok(heap.emergency_out_of_memory()),
            VMError::StackOverflow => return Ok(heap.emergency_stack_overflow()),
            _ => {}
        }

        let message = match error.java_message() {
            Some(message) => match heap.intern_string(&message) {
                Ok(reference) => reference,
                Err(VMError::MemoryLimitExceeded) => return Ok(heap.emergency_out_of_memory()),
                Err(error) => return Err(error),
            },
            None => ObjectRef::Null,
        };
        self.allocate_exception(&error.exception_class(), message, None, heap)
    }

    pub(crate) fn allocate_exception(
        &self,
        class_name: &str,
        message: ObjectRef,
        cause: Option<ObjectRef>,
        heap: &mut Heap,
    ) -> VMResult<ObjectRef> {
        let instance_field_count = self
            .method_area
            .as_ref()
            .and_then(|area| area.read().ok())
            .and_then(|area| area.get_class(class_name).ok())
            .map_or(0, |class| class.field_layout.field_count);
        match heap.allocate_throwable(
            ClassRef {
                name: class_name.to_string(),
                instance_field_count,
                static_field_count: 0,
            },
            message,
            cause,
        ) {
            Ok(reference) => Ok(reference),
            Err(VMError::MemoryLimitExceeded) => Ok(heap.emergency_out_of_memory()),
            Err(error) => Err(error),
        }
    }

    /// Check if an object is an instance of a target class
    ///
    /// Returns false if obj is null, true if obj's class matches target_class
    /// (or is a subclass of it).
    fn is_instance_of(&self, obj_class: &str, target_class: &str) -> bool {
        if obj_class == "null" {
            return false;
        }

        self.is_subclass_of(obj_class, target_class)
    }

    /// Collect external GC roots from thread stacks, active monitors, and
    /// static fields. Heap pools are rooted centrally by the collector.
    fn collect_gc_roots(&self, thread: &Thread, monitors: &MonitorTable) -> GCRoots {
        let mut roots = GCRoots::new();

        thread.add_gc_roots(&mut roots);
        monitors.add_gc_roots(&mut roots);

        // Static fields from method_area
        if let Some(method_area) = &self.method_area {
            if let Ok(ma) = method_area.read() {
                for value in ma.static_field_values() {
                    roots.add_value(&value);
                }
            }
        }

        roots
    }

    /// Execute bytecode with registered methods to completion
    ///
    /// Creates a thread and heap, executes the bytecode, and returns the
    /// top stack value (if any) from the final frame.
    pub fn execute_with_methods(
        &mut self,
        methods: HashMap<(String, String, String), MethodBytecode>,
    ) -> VMResult<Option<Value>> {
        // Register the methods
        self.method_registry = methods;

        // Create execution environment
        let mut thread = Thread::new(256, None); // Allow up to 256 nested calls
        let mut heap = Heap::new(1024 * 1024); // 1MB heap
        let mut monitors = MonitorTable::new(); // Monitor table for synchronization

        // Create initial frame for executor's bytecode
        let frame = Frame::new("_main".to_string(), "_run".to_string(), 10, 20, vec![])?;
        thread.push_frame(frame)?;

        // Execute until completion
        loop {
            match self.step(&mut thread, &mut heap, &mut monitors) {
                Ok(true) => {
                    if heap.should_collect() {
                        let roots = self.collect_gc_roots(&thread, &monitors);
                        GarbageCollector::collect(&mut heap, &roots);
                    }
                }
                Ok(false) => {
                    // Execution complete - return top stack value from final frame
                    if let Some(frame) = thread.current_frame_mut() {
                        return Ok(frame.pop_raw());
                    }
                    return Ok(None);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    /// Execute one nested instruction with exception propagation bounded to
    /// the synchronous call's outermost frame.
    pub(crate) fn step_with_unwind_floor(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        unwind_floor: usize,
    ) -> VMResult<bool> {
        let previous = self.exception_unwind_floor.replace(unwind_floor);
        let result = self.step(thread, heap, monitors);
        self.exception_unwind_floor = previous;
        result
    }

    /// Drive a synchronously-entered JVM frame until it returns to the captured
    /// depth. Unexpected completion and all errors unwind partial frames and
    /// release their monitors before restoring the caller's executor cursor.
    pub(crate) fn finish_nested_execution(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        state: NestedExecutionState,
    ) -> VMResult<()> {
        let target_depth = state.target_depth;
        let result = (|| {
            let mut steps: u64 = 0;
            while thread.depth() > target_depth {
                steps += 1;
                if steps > 100_000_000 {
                    return Err(VMError::internal(format!(
                        "{} exceeded step budget",
                        state.budget_subject
                    )));
                }
                if !self.step_with_unwind_floor(thread, heap, monitors, target_depth + 1)? {
                    return Err(VMError::internal(format!(
                        "{} completed without returning to its caller",
                        state.budget_subject
                    )));
                }
            }
            if thread.depth() < target_depth {
                return Err(VMError::internal(format!(
                    "{} unwound below its caller",
                    state.budget_subject
                )));
            }
            Ok(())
        })();

        if result.is_err() {
            // Preserve the execution failure as the primary error. Cleanup is
            // best-effort but drains every tracked monitor and frame.
            thread.unwind_after_failure(target_depth, monitors);
        }
        state.restore(self);
        result
    }

    /// Execute the next instruction
    pub fn step(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<bool> {
        if thread.is_completed() {
            return Ok(false);
        }

        // Check execution limit
        thread.increment_step()?;

        // Sync bytecode and PC from frame if frame has its own bytecode
        // This ensures execute() reads from the correct bytecode
        let initial_depth = thread.depth();
        let has_frame_bytecode = if let Some(frame) = thread.current_frame() {
            if let Some(frame_bytecode) = &frame.bytecode {
                // Sync from frame to executor for execute() to work correctly
                if frame.class_name.contains("Closure") {
                    debug!(
                        "SYNC: syncing {} bytecode (len={}) and pc={} to executor",
                        frame.class_name,
                        frame_bytecode.len(),
                        frame.pc
                    );
                }
                self.bytecode = frame_bytecode.clone();
                self.pc = frame.pc;

                // Also switch constant pool if this is a different class
                let class_name = frame.class_name.clone();
                let _ = frame; // Release borrow before mutable operations
                self.switch_constant_pool_for_class(&class_name);
                true
            } else {
                false
            }
        } else {
            false
        };

        // Check if we're at the end of bytecode
        if self.pc >= self.bytecode.len() {
            return if has_frame_bytecode {
                // Every JVM method must complete through a return or throw.
                // Treat reachable fall-through (including an empty Code body)
                // as malformed bytecode instead of confusing it with thread
                // completion and leaving the frame live.
                Err(VMError::InvalidBytecode)
            } else {
                Ok(false)
            };
        }

        // Get the opcode
        let opcode_byte = self.bytecode[self.pc];
        let opcode = Opcode::from_u8(opcode_byte).ok_or(VMError::InvalidBytecode)?;

        // Debug trace each instruction if we're in a closure method
        if let Some(frame) = thread.current_frame() {
            if frame.class_name.contains("Closure") {
                debug!(
                    "STEP[{}]: pc={}, opcode={:?} (0x{:02X}), stack_size={}",
                    frame.class_name,
                    self.pc,
                    opcode,
                    opcode_byte,
                    frame.operand_stack.len()
                );
            }
        }

        // Save the PC of the current instruction (before incrementing)
        // This is needed for exception table lookup if the instruction throws
        let instruction_pc = self.pc;
        if let Some(frame) = thread.current_frame_mut() {
            frame.begin_instruction(instruction_pc);
        }

        // Increment PC past opcode byte
        self.pc += 1;

        // Execute the instruction
        // Handle catchable exceptions by searching for an exception handler
        match self.execute(opcode, thread, heap, monitors) {
            Ok(()) => {}
            Err(error) if error.is_catchable() || matches!(error, VMError::ThrownException(_)) => {
                // Save the PC of the THROWING instruction to the frame
                // Exception table lookup uses this PC to find matching handlers
                // Note: We use instruction_pc (before increment), not self.pc (after operands)
                if let Some(frame) = thread.current_frame_mut() {
                    frame.pc = instruction_pc;
                }

                // This is the sole materialization and dispatch boundary. A
                // handler therefore always receives a real, typed object.
                let exception_reference = self.materialize_exception(&error, heap)?;
                let heap_id = exception_reference
                    .heap_id()
                    .ok_or_else(VMError::invalid_object_ref)?;
                let exception_class = heap.get_class(heap_id)?.name;

                // Try to find an exception handler. Resolving a catch type can
                // itself fail with a Java linkage error; in that case the new
                // exception replaces the original and resumes dispatch in the
                // invoker after the frame with the bad exception table has
                // been unwound.
                let mut exception_reference = exception_reference;
                let mut exception_class = exception_class;
                loop {
                    match exception_handler::handle_exception_with_unwind_floor(
                        thread,
                        &exception_class,
                        Value::Reference(exception_reference),
                        monitors,
                        self.method_area.as_ref(),
                        self.exception_unwind_floor,
                    ) {
                        Ok(()) => break,
                        Err(error @ VMError::UncaughtException { .. }) => return Err(error),
                        Err(error)
                            if error.is_catchable()
                                || matches!(error, VMError::ThrownException(_)) =>
                        {
                            exception_reference = self.materialize_exception(&error, heap)?;
                            let heap_id = exception_reference
                                .heap_id()
                                .ok_or_else(VMError::invalid_object_ref)?;
                            exception_class = heap.get_class(heap_id)?.name;
                        }
                        Err(error) => return Err(error),
                    }
                }

                // Handler was found and execution state is set up
                // Sync from the handler frame for next instruction
                if let Some(frame) = thread.current_frame() {
                    self.pc = frame.pc;
                    if let Some(frame_bytecode) = &frame.bytecode {
                        self.bytecode = frame_bytecode.clone();
                    }
                }
            }
            Err(error) => return Err(error),
        }

        // Sync PC back to frame if it has its own bytecode
        // BUT only if we're still on the same frame (depth unchanged)
        // If a new frame was pushed/popped, the current frame is different
        if has_frame_bytecode && thread.depth() == initial_depth {
            if let Some(frame) = thread.current_frame_mut() {
                frame.pc = self.pc;
            }
        }

        Ok(true) // Continue execution
    }

    /// Execute a specific opcode.
    ///
    /// Two-tier dispatch:
    /// - Tier 1 (`try_execute_thread_level`): Opcodes that manage their own frame access
    ///   internally (invocations, returns, field ops, monitors, etc.). These need `&mut Thread`
    ///   without a pre-existing frame borrow.
    /// - Tier 2 (below): Simple stack/local/arithmetic ops that only need `&mut Frame`.
    fn execute(
        &mut self,
        opcode: Opcode,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Tier 1: opcodes that acquire frame internally (need &mut Thread without frame borrow)
        if self.try_execute_thread_level(opcode, thread, heap, monitors)? {
            return Ok(());
        }

        // Tier 2: opcodes that only need &mut Frame
        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;

        match opcode {
            // Constants - delegated to handlers::constants
            Opcode::Nop => handlers::handle_nop(),
            Opcode::AconstNull => handlers::handle_aconst_null(frame)?,
            Opcode::IconstM1 => handlers::handle_iconst_m1(frame)?,
            Opcode::Iconst0 => handlers::handle_iconst_0(frame)?,
            Opcode::Iconst1 => handlers::handle_iconst_1(frame)?,
            Opcode::Iconst2 => handlers::handle_iconst_2(frame)?,
            Opcode::Iconst3 => handlers::handle_iconst_3(frame)?,
            Opcode::Iconst4 => handlers::handle_iconst_4(frame)?,
            Opcode::Iconst5 => handlers::handle_iconst_5(frame)?,
            Opcode::Lconst0 => handlers::handle_lconst_0(frame)?,
            Opcode::Lconst1 => handlers::handle_lconst_1(frame)?,
            Opcode::Fconst0 => handlers::handle_fconst_0(frame)?,
            Opcode::Fconst1 => handlers::handle_fconst_1(frame)?,
            Opcode::Fconst2 => handlers::handle_fconst_2(frame)?,
            Opcode::Dconst0 => handlers::handle_dconst_0(frame)?,
            Opcode::Dconst1 => handlers::handle_dconst_1(frame)?,

            // Bipush: push byte constant
            Opcode::Bipush => {
                if self.pc >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let byte = self.bytecode[self.pc] as i8 as i32;
                self.pc += 1;
                frame.push(Value::Int(byte))?;
            }

            // Sipush: push short constant
            Opcode::Sipush => {
                if self.pc + 1 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let short =
                    i16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]);
                self.pc += 2;
                frame.push(Value::Int(short as i32))?;
            }

            // Loads - delegated to handlers::loads_stores
            Opcode::Iload0 => handlers::handle_iload_0(frame)?,
            Opcode::Iload1 => handlers::handle_iload_1(frame)?,
            Opcode::Iload2 => handlers::handle_iload_2(frame)?,
            Opcode::Iload3 => handlers::handle_iload_3(frame)?,
            Opcode::Lload0 => handlers::handle_lload_0(frame)?,
            Opcode::Lload1 => handlers::handle_lload_1(frame)?,
            Opcode::Lload2 => handlers::handle_lload_2(frame)?,
            Opcode::Lload3 => handlers::handle_lload_3(frame)?,
            Opcode::Fload0 => handlers::handle_fload_0(frame)?,
            Opcode::Fload1 => handlers::handle_fload_1(frame)?,
            Opcode::Fload2 => handlers::handle_fload_2(frame)?,
            Opcode::Fload3 => handlers::handle_fload_3(frame)?,
            Opcode::Dload0 => handlers::handle_dload_0(frame)?,
            Opcode::Dload1 => handlers::handle_dload_1(frame)?,
            Opcode::Dload2 => handlers::handle_dload_2(frame)?,
            Opcode::Dload3 => handlers::handle_dload_3(frame)?,
            Opcode::Aload0 => handlers::handle_aload_0(frame)?,
            Opcode::Aload1 => handlers::handle_aload_1(frame)?,
            Opcode::Aload2 => handlers::handle_aload_2(frame)?,
            Opcode::Aload3 => handlers::handle_aload_3(frame)?,

            Opcode::Iload | Opcode::Lload | Opcode::Fload | Opcode::Dload | Opcode::Aload => {
                if self.pc >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let index = self.bytecode[self.pc] as u16;
                self.pc += 1;
                frame.push(frame.get_local(index)?)?;
            }

            // Stores - delegated to handlers::loads_stores
            Opcode::Istore0 => handlers::handle_istore_0(frame)?,
            Opcode::Lstore0 => handlers::handle_lstore_0(frame)?,
            Opcode::Fstore0 => handlers::handle_fstore_0(frame)?,
            Opcode::Dstore0 => handlers::handle_dstore_0(frame)?,
            Opcode::Astore0 => handlers::handle_astore_0(frame)?,
            Opcode::Istore1 => handlers::handle_istore_1(frame)?,
            Opcode::Lstore1 => handlers::handle_lstore_1(frame)?,
            Opcode::Fstore1 => handlers::handle_fstore_1(frame)?,
            Opcode::Dstore1 => handlers::handle_dstore_1(frame)?,
            Opcode::Astore1 => handlers::handle_astore_1(frame)?,
            Opcode::Istore2 => handlers::handle_istore_2(frame)?,
            Opcode::Lstore2 => handlers::handle_lstore_2(frame)?,
            Opcode::Fstore2 => handlers::handle_fstore_2(frame)?,
            Opcode::Dstore2 => handlers::handle_dstore_2(frame)?,
            Opcode::Astore2 => handlers::handle_astore_2(frame)?,
            Opcode::Istore3 => handlers::handle_istore_3(frame)?,
            Opcode::Lstore3 => handlers::handle_lstore_3(frame)?,
            Opcode::Fstore3 => handlers::handle_fstore_3(frame)?,
            Opcode::Dstore3 => handlers::handle_dstore_3(frame)?,
            Opcode::Astore3 => handlers::handle_astore_3(frame)?,

            Opcode::Istore | Opcode::Lstore | Opcode::Fstore | Opcode::Dstore | Opcode::Astore => {
                if self.pc >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let index = self.bytecode[self.pc] as u16;
                self.pc += 1;
                let value = frame.pop()?;
                frame.set_local(index, value)?;
            }

            // Stack manipulation - delegated to handlers::stack
            Opcode::Pop => handlers::handle_pop(frame)?,
            Opcode::Pop2 => handlers::handle_pop2(frame)?,
            Opcode::Dup => handlers::handle_dup(frame)?,
            Opcode::DupX1 => handlers::handle_dup_x1(frame)?,
            Opcode::DupX2 => handlers::handle_dup_x2(frame)?,
            Opcode::Dup2 => handlers::handle_dup2(frame)?,
            Opcode::Dup2X1 => handlers::handle_dup2_x1(frame)?,
            Opcode::Dup2X2 => handlers::handle_dup2_x2(frame)?,
            Opcode::Swap => handlers::handle_swap(frame)?,

            // Arithmetic - delegated to handlers::arithmetic
            Opcode::Iadd => handlers::handle_iadd(frame)?,
            Opcode::Isub => handlers::handle_isub(frame)?,
            Opcode::Imul => handlers::handle_imul(frame)?,
            Opcode::Idiv => handlers::handle_idiv(frame)?,
            Opcode::Irem => handlers::handle_irem(frame)?,
            Opcode::Ineg => handlers::handle_ineg(frame)?,
            Opcode::Ladd => handlers::handle_ladd(frame)?,
            Opcode::Lsub => handlers::handle_lsub(frame)?,
            Opcode::Lmul => handlers::handle_lmul(frame)?,
            Opcode::Ldiv => handlers::handle_ldiv(frame)?,
            Opcode::Lrem => handlers::handle_lrem(frame)?,
            Opcode::Lneg => handlers::handle_lneg(frame)?,
            Opcode::Fadd => handlers::handle_fadd(frame)?,
            Opcode::Fsub => handlers::handle_fsub(frame)?,
            Opcode::Fmul => handlers::handle_fmul(frame)?,
            Opcode::Fdiv => handlers::handle_fdiv(frame)?,
            Opcode::Frem => handlers::handle_frem(frame)?,
            Opcode::Fneg => handlers::handle_fneg(frame)?,
            Opcode::Dadd => handlers::handle_dadd(frame)?,
            Opcode::Dsub => handlers::handle_dsub(frame)?,
            Opcode::Dmul => handlers::handle_dmul(frame)?,
            Opcode::Ddiv => handlers::handle_ddiv(frame)?,
            Opcode::Drem => handlers::handle_drem(frame)?,
            Opcode::Dneg => handlers::handle_dneg(frame)?,

            // Bitwise operations - delegated to handlers::bitwise
            Opcode::Ishl => handlers::handle_ishl(frame)?,
            Opcode::Ishr => handlers::handle_ishr(frame)?,
            Opcode::Iushr => handlers::handle_iushr(frame)?,
            Opcode::Iand => handlers::handle_iand(frame)?,
            Opcode::Ior => handlers::handle_ior(frame)?,
            Opcode::Ixor => handlers::handle_ixor(frame)?,
            Opcode::Lshl => handlers::handle_lshl(frame)?,
            Opcode::Lshr => handlers::handle_lshr(frame)?,
            Opcode::Lushr => handlers::handle_lushr(frame)?,
            Opcode::Land => handlers::handle_land(frame)?,
            Opcode::Lor => handlers::handle_lor(frame)?,
            Opcode::Lxor => handlers::handle_lxor(frame)?,

            // Increment local variable by constant (JVMS §6.5.iinc)
            Opcode::Iinc => {
                if self.pc + 1 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let index = self.bytecode[self.pc] as u16;
                self.pc += 1;
                let const_val = self.bytecode[self.pc] as i8 as i32;
                self.pc += 1;
                let current = frame.get_local(index)?.as_int()?;
                frame.set_local(index, Value::Int(current.wrapping_add(const_val)))?;
            }

            // Type conversions - delegated to handlers::conversions
            Opcode::I2l => handlers::handle_i2l(frame)?,
            Opcode::I2f => handlers::handle_i2f(frame)?,
            Opcode::I2d => handlers::handle_i2d(frame)?,
            Opcode::L2i => handlers::handle_l2i(frame)?,
            Opcode::L2f => handlers::handle_l2f(frame)?,
            Opcode::L2d => handlers::handle_l2d(frame)?,
            Opcode::F2i => handlers::handle_f2i(frame)?,
            Opcode::F2l => handlers::handle_f2l(frame)?,
            Opcode::F2d => handlers::handle_f2d(frame)?,
            Opcode::D2i => handlers::handle_d2i(frame)?,
            Opcode::D2l => handlers::handle_d2l(frame)?,
            Opcode::D2f => handlers::handle_d2f(frame)?,
            Opcode::I2b => handlers::handle_i2b(frame)?,
            Opcode::I2c => handlers::handle_i2c(frame)?,
            Opcode::I2s => handlers::handle_i2s(frame)?,

            // Comparisons - delegated to handlers::comparisons
            Opcode::Lcmp => handlers::handle_lcmp(frame)?,
            Opcode::Fcmpl => handlers::handle_fcmpl(frame)?,
            Opcode::Fcmpg => handlers::handle_fcmpg(frame)?,
            Opcode::Dcmpl => handlers::handle_dcmpl(frame)?,
            Opcode::Dcmpg => handlers::handle_dcmpg(frame)?,

            // Array operations - delegated to handlers::arrays
            Opcode::Iaload => handlers::handle_iaload(frame, heap)?,
            Opcode::Laload => handlers::handle_laload(frame, heap)?,
            Opcode::Faload => handlers::handle_faload(frame, heap)?,
            Opcode::Daload => handlers::handle_daload(frame, heap)?,
            Opcode::Aaload => handlers::handle_aaload(frame, heap)?,
            Opcode::Baload => handlers::handle_baload(frame, heap)?,
            Opcode::Caload => handlers::handle_caload(frame, heap)?,
            Opcode::Saload => handlers::handle_saload(frame, heap)?,
            Opcode::Iastore => handlers::handle_iastore(frame, heap)?,
            Opcode::Lastore => handlers::handle_lastore(frame, heap)?,
            Opcode::Fastore => handlers::handle_fastore(frame, heap)?,
            Opcode::Dastore => handlers::handle_dastore(frame, heap)?,
            Opcode::Aastore => {
                let is_subclass_of = |src: &str, tgt: &str| self.is_subclass_of(src, tgt);
                handlers::handle_aastore(frame, heap, &is_subclass_of)?
            }
            Opcode::Bastore => handlers::handle_bastore(frame, heap)?,
            Opcode::Castore => handlers::handle_castore(frame, heap)?,
            Opcode::Sastore => handlers::handle_sastore(frame, heap)?,
            Opcode::Arraylength => handlers::handle_arraylength(frame, heap)?,

            // Type checking (need frame + heap + CP access)
            Opcode::Checkcast => {
                let index = self.read_u16_operand()? as usize;
                let target_class = self.resolve_class_name(index)?;
                self.ensure_reference_type_resolved(&frame.class_name, &target_class)?;

                let obj_ref = frame.peek()?.as_reference()?;
                if !obj_ref.is_null() {
                    let obj_class = Self::runtime_reference_type(heap, obj_ref)?;
                    if !self.is_instance_of(&obj_class, &target_class) {
                        return Err(VMError::class_cast_exception(&obj_class, &target_class));
                    }
                }
            }
            Opcode::Instanceof => {
                let index = self.read_u16_operand()? as usize;
                let target_class = self.resolve_class_name(index)?;
                self.ensure_reference_type_resolved(&frame.class_name, &target_class)?;

                let obj_ref = frame.pop()?.as_reference()?;
                if obj_ref.is_null() {
                    frame.push(Value::Int(0))?;
                } else {
                    let obj_class = Self::runtime_reference_type(heap, obj_ref)?;
                    let is_instance = self.is_instance_of(&obj_class, &target_class);
                    frame.push(Value::Int(if is_instance { 1 } else { 0 }))?;
                }
            }

            // Object creation (need frame + heap + CP access)
            Opcode::Newarray => {
                let type_code = self.read_u8_operand()?;
                handlers::arrays::handle_newarray(frame, heap, type_code)?;
            }
            Opcode::Anewarray => {
                let index = self.read_u16_operand()?;
                let element_class_name = self.resolve_class_name(index as usize)?;
                self.ensure_reference_type_resolved(&frame.class_name, &element_class_name)?;
                handlers::arrays::handle_anewarray(frame, heap, element_class_name)?;
            }
            Opcode::Multianewarray => {
                let index = self.read_u16_operand()?;
                let dimensions = self.read_u8_operand()? as usize;
                // The CP class is the complete array type (e.g. `[[I`); retain
                // it so every allocated dimension has its exact runtime type.
                let descriptor = self.resolve_class_name(index as usize)?;
                self.ensure_reference_type_resolved(&frame.class_name, &descriptor)?;
                handlers::handle_multianewarray(frame, heap, dimensions, &descriptor)?;
            }

            // Control flow
            Opcode::Ifeq => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifeq(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifne => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifne(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Iflt => {
                let target = self.read_branch_offset()?;
                if handlers::handle_iflt(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifge => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifge(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifgt => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifgt(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifle => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifle(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmpeq => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmpeq(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmpne => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmpne(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmplt => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmplt(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmpge => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmpge(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmpgt => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmpgt(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfIcmple => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_icmple(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfAcmpeq => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_acmpeq(frame)? {
                    self.pc = target;
                }
            }
            Opcode::IfAcmpne => {
                let target = self.read_branch_offset()?;
                if handlers::handle_if_acmpne(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifnull => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifnull(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Ifnonnull => {
                let target = self.read_branch_offset()?;
                if handlers::handle_ifnonnull(frame)? {
                    self.pc = target;
                }
            }
            Opcode::Goto => {
                let target = self.read_branch_offset()?;
                self.pc = target;
            }
            Opcode::GotoW => {
                let target = self.read_branch_offset_wide()?;
                self.pc = target;
            }

            // Tableswitch
            Opcode::Tableswitch => {
                let value = frame.pop()?.as_int()?;
                let instruction_start = self.pc - 1;
                let aligned_pc = (instruction_start + 1 + 3) & !3;
                self.pc = aligned_pc;

                if self.pc + 3 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let default_offset = i32::from_be_bytes([
                    self.bytecode[self.pc],
                    self.bytecode[self.pc + 1],
                    self.bytecode[self.pc + 2],
                    self.bytecode[self.pc + 3],
                ]);
                self.pc += 4;

                if self.pc + 3 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let low = i32::from_be_bytes([
                    self.bytecode[self.pc],
                    self.bytecode[self.pc + 1],
                    self.bytecode[self.pc + 2],
                    self.bytecode[self.pc + 3],
                ]);
                self.pc += 4;

                if self.pc + 3 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let high = i32::from_be_bytes([
                    self.bytecode[self.pc],
                    self.bytecode[self.pc + 1],
                    self.bytecode[self.pc + 2],
                    self.bytecode[self.pc + 3],
                ]);
                self.pc += 4;

                let offset = if value >= low && value <= high {
                    let index = (value - low) as usize;
                    let offset_pos = self.pc + (index * 4);
                    if offset_pos + 3 >= self.bytecode.len() {
                        return Err(VMError::InvalidBytecode);
                    }
                    i32::from_be_bytes([
                        self.bytecode[offset_pos],
                        self.bytecode[offset_pos + 1],
                        self.bytecode[offset_pos + 2],
                        self.bytecode[offset_pos + 3],
                    ])
                } else {
                    default_offset
                };

                let target = (instruction_start as i32 + offset) as usize;
                let num_offsets = (high - low + 1) as usize;
                self.pc += num_offsets * 4;
                self.pc = target;
            }

            // Lookupswitch
            Opcode::Lookupswitch => {
                let instruction_start = self.pc - 1;
                self.pc = handlers::handle_lookupswitch(
                    frame,
                    &self.bytecode,
                    &mut self.pc,
                    instruction_start,
                )?;
            }

            // LDC variants (need frame + CP access)
            Opcode::Ldc => return self.execute_ldc(frame, heap),
            Opcode::LdcW => return self.execute_ldc_w(frame, heap),
            Opcode::Ldc2W => return self.execute_ldc2_w(frame),

            // Wide prefix
            Opcode::Wide => return self.execute_wide(frame),

            // Tier 1 opcodes already handled above — this arm is unreachable
            // but needed for exhaustiveness. The compiler will optimize it away.
            Opcode::Ireturn
            | Opcode::Lreturn
            | Opcode::Freturn
            | Opcode::Dreturn
            | Opcode::Areturn
            | Opcode::Return
            | Opcode::Invokestatic
            | Opcode::Invokevirtual
            | Opcode::Invokespecial
            | Opcode::Invokeinterface
            | Opcode::Invokedynamic
            | Opcode::Getstatic
            | Opcode::Putstatic
            | Opcode::Getfield
            | Opcode::Putfield
            | Opcode::New
            | Opcode::Athrow
            | Opcode::Monitorenter
            | Opcode::Monitorexit => {
                unreachable!("handled by try_execute_thread_level")
            }
        }

        Ok(())
    }

    /// Tier 1: Execute opcodes that need `&mut Thread` without a pre-existing frame borrow.
    ///
    /// These opcodes acquire the frame internally, allowing them to also push/pop frames
    /// on the thread (invocations, returns) or release the frame borrow before calling
    /// methods that need `&mut Thread` (monitors, field ops with class init).
    ///
    /// Returns `Ok(true)` if the opcode was handled, `Ok(false)` if not a tier-1 opcode.
    fn try_execute_thread_level(
        &mut self,
        opcode: Opcode,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<bool> {
        match opcode {
            // Returns: pop return value from frame, then manipulate thread stack
            Opcode::Ireturn
            | Opcode::Lreturn
            | Opcode::Freturn
            | Opcode::Dreturn
            | Opcode::Areturn => {
                let (return_value, class_name, method_name) = {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    let rv = frame.pop()?;
                    let cn = frame.class_name.clone();
                    let mn = frame.method_name.clone();
                    (rv, cn, mn)
                };
                self.execute_return_value(thread, monitors, return_value, class_name, method_name)?;
                Ok(true)
            }
            Opcode::Return => {
                self.execute_return_void(thread, monitors)?;
                Ok(true)
            }

            // Method invocation: acquire frame internally for arg popping
            Opcode::Invokestatic => {
                self.execute_invokestatic(thread, heap, monitors)?;
                Ok(true)
            }
            Opcode::Invokevirtual => {
                self.execute_invokevirtual(thread, heap, monitors)?;
                Ok(true)
            }
            Opcode::Invokespecial => {
                self.execute_invokespecial(thread, heap, monitors)?;
                Ok(true)
            }
            Opcode::Invokeinterface => {
                self.execute_invokeinterface(thread, heap, monitors)?;
                Ok(true)
            }
            Opcode::Invokedynamic => {
                self.execute_invokedynamic(thread, heap, monitors)?;
                Ok(true)
            }

            // Field operations: getfield/putfield take frame, getstatic/putstatic need thread
            Opcode::Getfield => {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                self.execute_getfield(frame, heap)?;
                Ok(true)
            }
            Opcode::Putfield => {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                self.execute_putfield(frame, heap)?;
                Ok(true)
            }
            Opcode::Getstatic => {
                self.execute_getstatic(thread, heap, monitors)?;
                Ok(true)
            }
            Opcode::Putstatic => {
                self.execute_putstatic(thread, heap, monitors)?;
                Ok(true)
            }

            // `new` is an active use. Resolution and the instantiability check
            // precede initialization; allocation happens only after <clinit>
            // and all of its dependencies complete normally.
            Opcode::New => {
                let index = self.read_u16_operand()?;
                let class_name = self.resolve_class_name(index as usize)?;
                let caller = thread
                    .current_frame()
                    .ok_or(VMError::NoActiveFrame)?
                    .class_name
                    .clone();
                self.check_symbolic_class_access(&caller, &class_name)
                    .map_err(|error| match error {
                        VMError::ClassNotFound(name) => VMError::NoClassDefFoundError(name),
                        other => other,
                    })?;
                let class = self
                    .method_area
                    .as_ref()
                    .ok_or_else(|| VMError::internal("MethodArea not available"))?
                    .read()
                    .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
                    .get_class(&class_name)
                    .map_err(|error| match error {
                        VMError::ClassNotFound(name) => VMError::NoClassDefFoundError(name),
                        other => other,
                    })?;
                if class.is_interface || class.modifiers & access_flags::ACC_ABSTRACT != 0 {
                    return Err(VMError::InstantiationError(class.name));
                }
                self.ensure_initialized(&class_name, thread, heap, monitors)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                handlers::handle_new(frame, heap, self.method_area.as_ref(), class_name)?;
                Ok(true)
            }

            // Exception handling: pop from frame, then unwind thread
            Opcode::Athrow => {
                let exception_ref = {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.pop()?.as_reference()?
                };

                if exception_ref.is_null() {
                    return Err(VMError::NullPointerException);
                }

                let heap_id = exception_ref
                    .heap_id()
                    .ok_or_else(VMError::invalid_object_ref)?;
                let exception_class = heap.get_class(heap_id)?.name;

                if !self.is_instance_of(&exception_class, "java/lang/Throwable") {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "athrow: {} is not assignable to Throwable",
                        exception_class
                    )));
                }

                Err(VMError::ThrownException(exception_ref))
            }

            // Monitor operations: pop from frame, then use thread for monitor table
            Opcode::Monitorenter => {
                let obj_ref = {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.pop()?
                };
                handlers::monitors::handle_monitorenter(obj_ref, thread, monitors)?;
                Ok(true)
            }
            Opcode::Monitorexit => {
                let obj_ref = {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.pop()?
                };
                handlers::monitors::handle_monitorexit(obj_ref, thread, monitors)?;
                Ok(true)
            }

            // Not a tier-1 opcode
            _ => Ok(false),
        }
    }

    /// Get current program counter
    pub fn pc(&self) -> usize {
        self.pc
    }

    /// Set program counter
    pub fn set_pc(&mut self, pc: usize) {
        self.pc = pc;
    }

    /// Read a 2-byte signed branch offset and compute the target PC.
    ///
    /// JVM branch offsets are:
    /// - 2-byte big-endian signed integers (i16)
    /// - Relative to the START of the branch instruction (opcode position)
    ///
    /// After this call, self.pc will be advanced past the 2 offset bytes.
    /// Returns the computed branch target PC.
    fn read_branch_offset(&mut self) -> VMResult<usize> {
        // Ensure we have 2 bytes to read
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }

        // Read 2-byte signed offset
        let offset =
            i16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as i32;
        self.pc += 2;

        // Calculate target: offset is relative to the opcode position
        // At this point, self.pc has been incremented:
        //   +1 for opcode (in step())
        //   +2 for offset bytes (just now)
        // So opcode was at (self.pc - 3)
        let opcode_pc = (self.pc as i32) - 3;
        let target = opcode_pc + offset;

        // Validate target is within bounds
        if target < 0 || target as usize >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }

        Ok(target as usize)
    }

    /// Read a 4-byte wide branch offset (for goto_w and jsr_w)
    ///
    /// Branch offsets are:
    /// - 4-byte big-endian signed integers (i32)
    /// - Relative to the START of the branch instruction (opcode position)
    ///
    /// After this call, self.pc will be advanced past the 4 offset bytes.
    /// Returns the computed branch target PC.
    fn read_branch_offset_wide(&mut self) -> VMResult<usize> {
        // Ensure we have 4 bytes to read
        if self.pc + 3 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }

        // Read 4-byte signed offset
        let offset = i32::from_be_bytes([
            self.bytecode[self.pc],
            self.bytecode[self.pc + 1],
            self.bytecode[self.pc + 2],
            self.bytecode[self.pc + 3],
        ]);
        self.pc += 4;

        // Calculate target: offset is relative to the opcode position
        // At this point, self.pc has been incremented:
        //   +1 for opcode (in step())
        //   +4 for offset bytes (just now)
        // So opcode was at (self.pc - 5)
        let opcode_pc = (self.pc as i32) - 5;
        let target = opcode_pc + offset;

        // Validate target is within bounds
        if target < 0 || target as usize >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }

        Ok(target as usize)
    }
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;
