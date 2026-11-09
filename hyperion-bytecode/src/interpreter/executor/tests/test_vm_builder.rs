// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test VM Builder - Declarative VM setup for executor tests
//!
//! Provides a fluent API for constructing VMs with specific MethodArea configurations.
//! Tests declare what they need, builder provides it.
//!
//! # Design Principles
//!
//! 1. **Declarative**: Tests say "I need a class with these fields", not "create MethodArea, register class, etc."
//! 2. **Composable**: Combine multiple requirements (JDK stubs + custom classes)
//! 3. **Isolated**: Each test gets a fresh VM, no shared state
//! 4. **Fast**: Minimal setup, lazy JDK stub loading
//!
//! # Examples
//!
//! ```rust,ignore
//! // Simple field access test
//! let vm = TestVMBuilder::new()
//!     .with_class_having_fields("java/lang/Object", &[("value", "I")])
//!     .with_bytecode(vec![0xb4, 0x00, 0x07]) // getfield
//!     .build()?;
//!
//! // Test requiring java.lang.Object stub
//! let vm = TestVMBuilder::new()
//!     .with_jdk_core() // Adds Object, String, Class
//!     .with_bytecode(vec![0xbb, 0x00, 0x01]) // new Object
//!     .build()?;
//!
//! // Complex multi-class test
//! let vm = TestVMBuilder::new()
//!     .with_jdk_core()
//!     .with_class_having_fields("com/example/User", &[("name", "Ljava/lang/String;"), ("age", "I")])
//!     .with_class_having_methods("com/example/Service", &[("process", "(Lcom/example/User;)V")])
//!     .with_bytecode(bytecode_vec)
//!     .build()?;
//! ```

use super::*;
use crate::interpreter::method_area::{ClassInitState, FieldDeclaration, FieldLayout, LoadedClass};
use std::sync::{Arc, RwLock};

/// Builder for test VMs with declarative configuration
pub struct TestVMBuilder {
    /// Classes to register in MethodArea
    classes: Vec<LoadedClass>,
    /// JDK stub level to include
    jdk_stubs: JdkStubLevel,
    /// Bytecode to execute
    bytecode: Vec<u8>,
    /// Constant pool items
    constant_pool: Vec<ConstantPoolItem>,
    /// Heap size in bytes
    heap_size: usize,
    /// Max local variables in the test frame
    max_locals: usize,
    /// Max operand stack depth in the test frame
    max_stack: usize,
    /// Max frame stack depth for the thread
    max_frames: usize,
    /// Optional native registry
    native_registry: Option<NativeRegistry>,
}

/// Level of JDK stub completeness
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // API surface for future test migration
pub enum JdkStubLevel {
    /// No JDK stubs (fastest, for pure opcode tests)
    None,
    /// Core classes only (Object, String, Class)
    Core,
    /// Core + collections (ArrayList, HashMap)
    Collections,
    /// Core + collections + I/O
    IO,
    /// Everything in JdkStubRegistry
    Full,
}

impl TestVMBuilder {
    /// Create a new test VM builder with defaults
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            jdk_stubs: JdkStubLevel::None,
            bytecode: Vec::new(),
            constant_pool: create_minimal_cp(), // From mod.rs
            heap_size: 1024,
            max_locals: 10,
            max_stack: 10,
            max_frames: 10,
            native_registry: None,
        }
    }

    /// Set the bytecode to execute
    pub fn with_bytecode(mut self, bytecode: Vec<u8>) -> Self {
        self.bytecode = bytecode;
        self
    }

    /// Set the constant pool
    pub fn with_constant_pool(mut self, cp: Vec<ConstantPoolItem>) -> Self {
        self.constant_pool = cp;
        self
    }

    /// Set heap size
    pub fn with_heap_size(mut self, size: usize) -> Self {
        self.heap_size = size;
        self
    }

    /// Include core JDK stubs (java.lang.Object, String, Class)
    pub fn with_jdk_core(mut self) -> Self {
        self.jdk_stubs = JdkStubLevel::Core;
        self
    }

    /// Include JDK collections stubs (ArrayList, HashMap, etc.)
    #[allow(dead_code)] // API surface for future test migration
    pub fn with_jdk_collections(mut self) -> Self {
        self.jdk_stubs = JdkStubLevel::Collections;
        self
    }

    /// Include JDK I/O stubs (InputStream, OutputStream, etc.)
    #[allow(dead_code)] // API surface for future test migration
    pub fn with_jdk_io(mut self) -> Self {
        self.jdk_stubs = JdkStubLevel::IO;
        self
    }

    /// Include all JDK stubs from registry
    #[allow(dead_code)] // API surface for future test migration
    pub fn with_jdk_full(mut self) -> Self {
        self.jdk_stubs = JdkStubLevel::Full;
        self
    }

    /// Set max local variables for the test frame
    pub fn with_max_locals(mut self, n: usize) -> Self {
        self.max_locals = n;
        self
    }

    /// Set max operand stack depth for the test frame
    pub fn with_max_stack(mut self, n: usize) -> Self {
        self.max_stack = n;
        self
    }

    /// Set max frame stack depth for the thread
    pub fn with_max_frames(mut self, n: usize) -> Self {
        self.max_frames = n;
        self
    }

    /// Set a native registry for native method calls
    pub fn with_native_registry(mut self, registry: NativeRegistry) -> Self {
        self.native_registry = Some(registry);
        self
    }

    /// Add a raw LoadedClass directly
    pub fn with_class(mut self, class: LoadedClass) -> Self {
        self.classes.push(class);
        self
    }

    /// Add a custom class with instance fields
    ///
    /// This is the replacement for `create_test_class_with_fields`.
    ///
    /// # Arguments
    /// * `name` - Fully qualified class name (e.g., "java/lang/Object")
    /// * `fields` - Array of (field_name, descriptor) tuples
    ///
    /// # Returns
    /// Self for chaining, and the class will be available in MethodArea after build()
    pub fn with_class_having_fields(mut self, name: &str, fields: &[(&str, &str)]) -> Self {
        let mut loaded_class = LoadedClass {
            name: name.to_string(),
            superclass: Some("java/lang/Object".into()),
            interfaces: vec![],
            vtable: vec![],
            resolved_virtual_methods: Vec::new(),
            non_virtual_methods: vec![],
            itable: vec![],
            field_layout: FieldLayout::new(),
            init_state: ClassInitState::Initialized, // Tests don't need <clinit> execution
            declared_fields: fields
                .iter()
                .map(|(name, descriptor)| FieldDeclaration::new(*name, *descriptor, 0x0001))
                .collect::<Vec<_>>()
                .into(),
            constant_field_values: Arc::from([]),
            modifiers: 0x0001, // ACC_PUBLIC
            is_interface: false,
            is_native_stub: false,
            default_conflict_count: 0,
            source_file: None,
            nest: None,
            bootstrap_methods: vec![],
        };

        // Build field layout
        for (field_name, descriptor) in fields {
            loaded_class
                .field_layout
                .add_field_with_descriptor(field_name.to_string(), descriptor.to_string());
        }

        self.classes.push(loaded_class);
        self
    }

    /// Add a custom class with methods (vtable entries)
    #[allow(dead_code)] // API surface for future test migration
    pub fn with_class_having_methods(
        mut self,
        name: &str,
        methods: &[(&str, &str)], // (method_name, descriptor)
    ) -> Self {
        let mut loaded_class = LoadedClass {
            name: name.to_string(),
            superclass: Some("java/lang/Object".into()),
            interfaces: vec![],
            vtable: vec![],
            resolved_virtual_methods: Vec::new(),
            non_virtual_methods: vec![],
            itable: vec![],
            field_layout: FieldLayout::new(),
            init_state: ClassInitState::Initialized,
            declared_fields: Arc::from([]),
            constant_field_values: Arc::from([]),
            modifiers: 0x0001,
            is_interface: false,
            is_native_stub: false,
            default_conflict_count: 0,
            source_file: None,
            nest: None,
            bootstrap_methods: vec![],
        };

        for (method_name, descriptor) in methods {
            loaded_class.add_vtable_entry(
                method_name.to_string(),
                descriptor.to_string(),
                name.to_string(),
            );
        }

        self.classes.push(loaded_class);
        self
    }

    /// Build the test VM
    pub fn build(self) -> VMResult<TestVM> {
        let method_area = Arc::new(RwLock::new(crate::interpreter::MethodArea::new()));

        // Register custom classes
        {
            let area = method_area.write().map_err(|_| {
                VMError::InternalError("Failed to acquire MethodArea lock".to_string())
            })?;

            for class in self.classes {
                area.register_class(class)?;
            }
        }

        // Pre-load JDK stubs based on level
        // The MethodArea already has JdkStubRegistry, but we can force registration
        // of commonly-used classes to avoid lazy-loading overhead in tests
        if self.jdk_stubs != JdkStubLevel::None {
            let area = method_area.write().map_err(|_| {
                VMError::InternalError("Failed to acquire MethodArea lock".to_string())
            })?;

            let classes_to_preload = match self.jdk_stubs {
                JdkStubLevel::None => vec![],
                JdkStubLevel::Core => {
                    vec!["java/lang/Object", "java/lang/String", "java/lang/Class"]
                }
                JdkStubLevel::Collections => vec![
                    "java/lang/Object",
                    "java/lang/String",
                    "java/lang/Class",
                    "java/util/ArrayList",
                    "java/util/HashMap",
                    "java/util/List",
                    "java/util/Map",
                ],
                JdkStubLevel::IO => vec![
                    "java/lang/Object",
                    "java/lang/String",
                    "java/lang/Class",
                    "java/util/ArrayList",
                    "java/util/HashMap",
                    "java/io/File",
                    "java/io/InputStream",
                    "java/io/OutputStream",
                ],
                JdkStubLevel::Full => {
                    // For Full, just rely on lazy loading from JdkStubRegistry
                    // No need to preload everything
                    vec![]
                }
            };

            for class_name in classes_to_preload {
                // Force-load from stub registry (get_class auto-registers)
                let _ = area.get_class(class_name)?;
            }
        }

        let mut executor = Executor::new(self.bytecode)
            .with_constant_pool(self.constant_pool)
            .with_method_area(method_area.clone());

        if let Some(registry) = self.native_registry {
            executor = executor.with_native_registry(registry);
        }

        let mut thread = Thread::new(self.max_frames, None);
        let frame = Frame::new(
            "TestClass".to_string(),
            "testMethod".to_string(),
            self.max_locals as u16,
            self.max_stack as u16,
            vec![],
        )?;
        thread.push_frame(frame)?;

        Ok(TestVM {
            executor,
            thread,
            heap: Heap::new(self.heap_size),
            monitors: MonitorTable::new(),
            method_area,
        })
    }
}

impl Default for TestVMBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Test VM with all components
pub struct TestVM {
    executor: Executor,
    thread: Thread,
    heap: Heap,
    monitors: MonitorTable,
    method_area: Arc<RwLock<crate::interpreter::MethodArea>>,
}

impl TestVM {
    /// Execute one instruction
    pub fn step(&mut self) -> VMResult<()> {
        self.executor
            .step(&mut self.thread, &mut self.heap, &mut self.monitors)?;
        Ok(())
    }

    /// Execute N instructions
    pub fn steps(&mut self, n: usize) -> VMResult<()> {
        for _ in 0..n {
            self.step()?;
        }
        Ok(())
    }

    /// Get value from top of stack
    pub fn stack_top(&self) -> VMResult<Value> {
        let frame = self
            .thread
            .current_frame()
            .ok_or_else(|| VMError::InternalError("No current frame".to_string()))?;
        frame.peek().cloned()
    }

    /// Get value from local variable
    pub fn local(&self, index: usize) -> VMResult<Value> {
        let frame = self
            .thread
            .current_frame()
            .ok_or_else(|| VMError::InternalError("No current frame".to_string()))?;
        frame.get_local(index as u16)
    }

    /// Set local variable
    pub fn set_local(&mut self, index: usize, value: Value) -> VMResult<()> {
        let frame = self
            .thread
            .current_frame_mut()
            .ok_or_else(|| VMError::InternalError("No current frame".to_string()))?;
        frame.set_local(index as u16, value)
    }

    /// Push value onto stack
    pub fn push(&mut self, value: Value) -> VMResult<()> {
        let frame = self
            .thread
            .current_frame_mut()
            .ok_or_else(|| VMError::InternalError("No current frame".to_string()))?;
        frame.push(value)
    }

    /// Get mutable heap reference (for manual object allocation in tests)
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// Get MethodArea (for querying field layouts, etc.)
    pub fn method_area(&self) -> &Arc<RwLock<crate::interpreter::MethodArea>> {
        &self.method_area
    }

    /// Get field offset for a class (convenience method for tests)
    pub fn field_offset(&self, class_name: &str, field_name: &str) -> VMResult<usize> {
        let area = self
            .method_area
            .read()
            .map_err(|_| VMError::InternalError("Failed to acquire lock".to_string()))?;
        let class = area.get_class(class_name)?;
        class.field_layout.get_offset(field_name)
    }

    /// Get current program counter
    pub fn pc(&self) -> usize {
        self.executor.pc()
    }

    /// Pop value from top of stack
    pub fn pop(&mut self) -> VMResult<Value> {
        let frame = self
            .thread
            .current_frame_mut()
            .ok_or_else(|| VMError::InternalError("No current frame".to_string()))?;
        frame.pop()
    }

    /// Get current operand stack depth
    pub fn stack_depth(&self) -> usize {
        self.thread
            .current_frame()
            .map(|f| f.stack_depth())
            .unwrap_or(0)
    }

    /// Get immutable reference to current frame
    pub fn frame(&self) -> Option<&crate::interpreter::Frame> {
        self.thread.current_frame()
    }

    /// Register a method for multi-method invocation tests
    pub fn register_method(
        &mut self,
        class: String,
        name: String,
        descriptor: String,
        bytecode: MethodBytecode,
    ) {
        self.executor
            .register_method(class, name, descriptor, bytecode);
    }

    /// Execute until method returns (Ok(false)) or error
    pub fn execute_loop(&mut self) -> VMResult<()> {
        loop {
            match self
                .executor
                .step(&mut self.thread, &mut self.heap, &mut self.monitors)
            {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    /// Get a reference to the executor (for direct API access in tests)
    pub fn executor(&self) -> &Executor {
        &self.executor
    }

    /// Get a mutable reference to the executor
    pub fn executor_mut(&mut self) -> &mut Executor {
        &mut self.executor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() -> VMResult<()> {
        let vm = TestVMBuilder::new()
            .with_bytecode(vec![0x03]) // iconst_0
            .build()?;

        assert!(vm.stack_top().is_err()); // Stack should be empty initially
        Ok(())
    }

    #[test]
    fn test_builder_with_fields() -> VMResult<()> {
        let vm = TestVMBuilder::new()
            .with_class_having_fields(
                "TestClass",
                &[("value", "I"), ("name", "Ljava/lang/String;")],
            )
            .build()?;

        // Verify field layout was created
        let value_offset = vm.field_offset("TestClass", "value")?;
        let name_offset = vm.field_offset("TestClass", "name")?;

        assert_eq!(value_offset, 0);
        assert_eq!(name_offset, 1);

        Ok(())
    }

    #[test]
    fn test_builder_with_jdk_core() -> VMResult<()> {
        let vm = TestVMBuilder::new().with_jdk_core().build()?;

        // Verify Object class is available
        let area = vm.method_area().read().unwrap();
        assert!(area.class_available("java/lang/Object"));
        assert!(area.class_available("java/lang/String"));

        Ok(())
    }
}
