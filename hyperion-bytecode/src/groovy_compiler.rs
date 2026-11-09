// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy compilation infrastructure - expression and statement compilation
//!
//! This module provides Groovy-specific compilation logic for converting
//! Groovy AST to JVM bytecode, extending the base Java compilation infrastructure.

use crate::constant_pool::ConstantPoolBuilder;
use crate::java_compiler::{JavaType, StackDepthTracker};
use std::collections::HashMap;

/// Groovy dynamic call sites encode the receiver as their first descriptor
/// parameter. JVM call-site descriptors permit at most 255 parameter slots.
pub(crate) const MAX_DYNAMIC_CALL_ARGUMENTS: usize = 254;

pub(crate) fn validate_dynamic_call_arity(arg_count: usize) -> Result<(), String> {
    if arg_count > MAX_DYNAMIC_CALL_ARGUMENTS {
        return Err(format!(
            "Groovy dynamic call has {arg_count} arguments; JVM call-site descriptors support at most {MAX_DYNAMIC_CALL_ARGUMENTS} arguments plus the receiver"
        ));
    }
    Ok(())
}

/// Groovy expression compiler for bytecode generation
pub struct GroovyExpressionCompiler {
    /// Stack depth tracker
    stack_depth: StackDepthTracker,

    /// Constant pool builder
    constant_pool: ConstantPoolBuilder,

    /// Bytecode output
    bytecode: Vec<u8>,

    /// Label counter for unique labels
    label_counter: u32,

    /// Closure counter for synthetic inner class names
    closure_counter: u32,

    /// Cached bootstrap method index for Groovy MOP (Meta-Object Protocol)
    /// Lazily initialized on first invokedynamic emission
    groovy_mop_bootstrap_index: Option<u16>,
}

impl GroovyExpressionCompiler {
    /// Create a new Groovy expression compiler
    pub fn new(constant_pool: ConstantPoolBuilder) -> Self {
        Self {
            stack_depth: StackDepthTracker::new(),
            constant_pool,
            bytecode: Vec::new(),
            label_counter: 0,
            closure_counter: 0,
            groovy_mop_bootstrap_index: None,
        }
    }

    /// Get or create the Groovy MOP bootstrap method
    ///
    /// This ensures a bootstrap method is registered for invokedynamic instructions.
    /// For Groovy, this would typically point to IndyInterface.bootstrap, but since
    /// we simulate the runtime, we just need a valid entry in BootstrapMethods.
    pub fn ensure_groovy_bootstrap(&mut self) -> u16 {
        if let Some(idx) = self.groovy_mop_bootstrap_index {
            return idx;
        }

        // Create a bootstrap method entry
        // In real Groovy, this would be: org/codehaus/groovy/vmplugin/v8/IndyInterface.bootstrap
        // We create a minimal valid bootstrap method for our interpreter

        // 1. Add the bootstrap class to constant pool
        let bootstrap_class = self
            .constant_pool
            .add_class("org/codehaus/groovy/vmplugin/v8/IndyInterface");

        // 2. Add bootstrap method name and descriptor
        let bootstrap_method_nat = self.constant_pool.add_name_and_type(
            "bootstrap",
            "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;",
        );

        // 3. Create MethodRef for the bootstrap method
        let bootstrap_method_ref = self
            .constant_pool
            .add_method_ref_by_indices(bootstrap_class, bootstrap_method_nat);

        // 4. Create MethodHandle (kind 6 = REF_invokeStatic)
        let method_handle = self
            .constant_pool
            .add_method_handle(6, bootstrap_method_ref);

        // 5. Register the bootstrap method in the BootstrapMethods attribute
        let bootstrap_index = self.constant_pool.register_bootstrap_method(method_handle);

        self.groovy_mop_bootstrap_index = Some(bootstrap_index);
        bootstrap_index
    }

    /// Emit a byte to the bytecode
    pub fn emit_byte(&mut self, byte: u8) {
        self.bytecode.push(byte);
    }

    /// Emit two bytes (short) in big-endian
    pub fn emit_short(&mut self, value: u16) {
        self.bytecode.push((value >> 8) as u8);
        self.bytecode.push(value as u8);
    }

    /// Allocate a new label
    pub fn new_label(&mut self) -> u32 {
        let label = self.label_counter;
        self.label_counter += 1;
        label
    }

    /// Allocate a new closure ID for synthetic inner class generation
    pub fn new_closure_id(&mut self) -> u32 {
        let id = self.closure_counter;
        self.closure_counter += 1;
        id
    }

    /// Get current bytecode
    pub fn bytecode(&self) -> &[u8] {
        &self.bytecode
    }

    /// Get mutable bytecode
    pub fn bytecode_mut(&mut self) -> &mut Vec<u8> {
        &mut self.bytecode
    }

    /// Get constant pool builder
    pub fn constant_pool(&mut self) -> &mut ConstantPoolBuilder {
        &mut self.constant_pool
    }

    /// Get stack depth tracker
    pub fn stack_depth(&mut self) -> &mut StackDepthTracker {
        &mut self.stack_depth
    }

    /// Get current bytecode length (PC)
    pub fn pc(&self) -> usize {
        self.bytecode.len()
    }

    /// Compile an integer literal to bytecode
    ///
    /// Optimization hierarchy:
    /// - Constants -1 to 5: ICONST_x (1 byte)
    /// - Byte range (-128 to 127): BIPUSH (2 bytes)
    /// - Short range (-32768 to 32767): SIPUSH (3 bytes)
    /// - Larger: LDC with constant pool (2+ bytes)
    pub fn compile_int_literal(&mut self, value: i32) -> JavaType {
        match value {
            -1 => self.emit_byte(0x02), // ICONST_M1
            0 => self.emit_byte(0x03),  // ICONST_0
            1 => self.emit_byte(0x04),  // ICONST_1
            2 => self.emit_byte(0x05),  // ICONST_2
            3 => self.emit_byte(0x06),  // ICONST_3
            4 => self.emit_byte(0x07),  // ICONST_4
            5 => self.emit_byte(0x08),  // ICONST_5
            -128..=127 => {
                // BIPUSH
                self.emit_byte(0x10);
                self.emit_byte(value as u8);
            }
            -32768..=32767 => {
                // SIPUSH
                self.emit_byte(0x11);
                self.emit_short(value as u16);
            }
            _ => {
                // LDC - add to constant pool and load
                let cp_index = self.constant_pool.add_integer(value);
                if cp_index < 256 {
                    self.emit_byte(0x12); // LDC
                    self.emit_byte(cp_index as u8);
                } else {
                    self.emit_byte(0x13); // LDC_W (wide index)
                    self.emit_short(cp_index);
                }
            }
        }

        self.stack_depth.push1(); // int takes 1 slot
        JavaType::Int
    }

    /// Compile a string literal to bytecode
    ///
    /// Pushes a String reference onto the stack via LDC
    pub fn compile_string_literal(&mut self, value: &str) -> JavaType {
        let cp_index = self.constant_pool.add_string(value);

        if cp_index < 256 {
            self.emit_byte(0x12); // LDC
            self.emit_byte(cp_index as u8);
        } else {
            self.emit_byte(0x13); // LDC_W
            self.emit_short(cp_index);
        }

        self.stack_depth.push1(); // String reference takes 1 slot
        JavaType::Object("java/lang/String".to_string())
    }

    /// Compile a boolean literal to bytecode
    ///
    /// In JVM, booleans are represented as int (0 = false, 1 = true)
    /// But for Groovy, we wrap in Boolean objects for proper MOP handling
    pub fn compile_boolean_literal(&mut self, value: bool) -> JavaType {
        // Push the primitive boolean value (0 or 1)
        if value {
            self.emit_byte(0x04); // ICONST_1
        } else {
            self.emit_byte(0x03); // ICONST_0
        }
        self.stack_depth.push1(); // Push int (0 or 1)

        // Box to Boolean object: Boolean.valueOf(boolean)
        let method_ref = self.constant_pool.add_method_ref(
            "java/lang/Boolean",
            "valueOf",
            "(Z)Ljava/lang/Boolean;",
        );

        self.emit_byte(0xB8); // INVOKESTATIC
        self.emit_short(method_ref);

        // Stack: int (1 slot) -> Boolean reference (1 slot)
        // INVOKESTATIC pops 1 and pushes 1, so no net change
        JavaType::Object("java/lang/Boolean".to_string())
    }

    /// Compile a null literal to bytecode
    pub fn compile_null_literal(&mut self) -> JavaType {
        self.emit_byte(0x01); // ACONST_NULL
        self.stack_depth.push1(); // null reference takes 1 slot
        JavaType::Object("java/lang/Object".to_string()) // null is typed as Object
    }

    /// Load an integer variable from a local slot
    ///
    /// Uses optimized ILOAD_n for slots 0-3, ILOAD for others
    /// Stack: -> value
    pub fn compile_iload(&mut self, slot: u16) -> JavaType {
        match slot {
            0 => self.emit_byte(0x1A), // ILOAD_0
            1 => self.emit_byte(0x1B), // ILOAD_1
            2 => self.emit_byte(0x1C), // ILOAD_2
            3 => self.emit_byte(0x1D), // ILOAD_3
            _ if slot < 256 => {
                self.emit_byte(0x15); // ILOAD
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x15); // ILOAD
                self.emit_short(slot);
            }
        }

        self.stack_depth.push1();
        JavaType::Int
    }

    /// Load a reference variable from a local slot
    ///
    /// Uses optimized ALOAD_n for slots 0-3, ALOAD for others
    /// Stack: -> reference
    pub fn compile_aload(&mut self, slot: u16, type_name: &str) -> JavaType {
        match slot {
            0 => self.emit_byte(0x2A), // ALOAD_0
            1 => self.emit_byte(0x2B), // ALOAD_1
            2 => self.emit_byte(0x2C), // ALOAD_2
            3 => self.emit_byte(0x2D), // ALOAD_3
            _ if slot < 256 => {
                self.emit_byte(0x19); // ALOAD
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x19); // ALOAD
                self.emit_short(slot);
            }
        }

        self.stack_depth.push1();
        JavaType::Object(type_name.to_string())
    }

    /// Store an integer value to a local slot
    ///
    /// Uses optimized ISTORE_n for slots 0-3, ISTORE for others
    /// Stack: value ->
    pub fn compile_istore(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x3B), // ISTORE_0
            1 => self.emit_byte(0x3C), // ISTORE_1
            2 => self.emit_byte(0x3D), // ISTORE_2
            3 => self.emit_byte(0x3E), // ISTORE_3
            _ if slot < 256 => {
                self.emit_byte(0x36); // ISTORE
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x36); // ISTORE
                self.emit_short(slot);
            }
        }

        self.stack_depth.pop1();
    }

    /// Store a reference value to a local slot
    ///
    /// Uses optimized ASTORE_n for slots 0-3, ASTORE for others
    /// Stack: reference ->
    pub fn compile_astore(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x4B), // ASTORE_0
            1 => self.emit_byte(0x4C), // ASTORE_1
            2 => self.emit_byte(0x4D), // ASTORE_2
            3 => self.emit_byte(0x4E), // ASTORE_3
            _ if slot < 256 => {
                self.emit_byte(0x3A); // ASTORE
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x3A); // ASTORE
                self.emit_short(slot);
            }
        }

        self.stack_depth.pop1();
    }

    /// Load a long value from a local slot
    ///
    /// Uses optimized LLOAD_n for slots 0-3, LLOAD for others
    /// Stack: -> long
    pub fn compile_lload(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x1E), // LLOAD_0
            1 => self.emit_byte(0x1F), // LLOAD_1
            2 => self.emit_byte(0x20), // LLOAD_2
            3 => self.emit_byte(0x21), // LLOAD_3
            _ if slot < 256 => {
                self.emit_byte(0x16); // LLOAD
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x16); // LLOAD
                self.emit_short(slot);
            }
        }

        self.stack_depth.push2(); // long takes 2 slots
    }

    /// Store a long value to a local slot
    ///
    /// Uses optimized LSTORE_n for slots 0-3, LSTORE for others
    /// Stack: long ->
    pub fn compile_lstore(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x3F), // LSTORE_0
            1 => self.emit_byte(0x40), // LSTORE_1
            2 => self.emit_byte(0x41), // LSTORE_2
            3 => self.emit_byte(0x42), // LSTORE_3
            _ if slot < 256 => {
                self.emit_byte(0x37); // LSTORE
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x37); // LSTORE
                self.emit_short(slot);
            }
        }

        self.stack_depth.pop2(); // long takes 2 slots
    }

    /// Load a float value from a local slot
    ///
    /// Uses optimized FLOAD_n for slots 0-3, FLOAD for others
    /// Stack: -> float
    pub fn compile_fload(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x22), // FLOAD_0
            1 => self.emit_byte(0x23), // FLOAD_1
            2 => self.emit_byte(0x24), // FLOAD_2
            3 => self.emit_byte(0x25), // FLOAD_3
            _ if slot < 256 => {
                self.emit_byte(0x17); // FLOAD
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x17); // FLOAD
                self.emit_short(slot);
            }
        }

        self.stack_depth.push1();
    }

    /// Store a float value to a local slot
    ///
    /// Uses optimized FSTORE_n for slots 0-3, FSTORE for others
    /// Stack: float ->
    pub fn compile_fstore(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x43), // FSTORE_0
            1 => self.emit_byte(0x44), // FSTORE_1
            2 => self.emit_byte(0x45), // FSTORE_2
            3 => self.emit_byte(0x46), // FSTORE_3
            _ if slot < 256 => {
                self.emit_byte(0x38); // FSTORE
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x38); // FSTORE
                self.emit_short(slot);
            }
        }

        self.stack_depth.pop1();
    }

    /// Load a double value from a local slot
    ///
    /// Uses optimized DLOAD_n for slots 0-3, DLOAD for others
    /// Stack: -> double
    pub fn compile_dload(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x26), // DLOAD_0
            1 => self.emit_byte(0x27), // DLOAD_1
            2 => self.emit_byte(0x28), // DLOAD_2
            3 => self.emit_byte(0x29), // DLOAD_3
            _ if slot < 256 => {
                self.emit_byte(0x18); // DLOAD
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x18); // DLOAD
                self.emit_short(slot);
            }
        }

        self.stack_depth.push2(); // double takes 2 slots
    }

    /// Store a double value to a local slot
    ///
    /// Uses optimized DSTORE_n for slots 0-3, DSTORE for others
    /// Stack: double ->
    pub fn compile_dstore(&mut self, slot: u16) {
        match slot {
            0 => self.emit_byte(0x47), // DSTORE_0
            1 => self.emit_byte(0x48), // DSTORE_1
            2 => self.emit_byte(0x49), // DSTORE_2
            3 => self.emit_byte(0x4A), // DSTORE_3
            _ if slot < 256 => {
                self.emit_byte(0x39); // DSTORE
                self.emit_byte(slot as u8);
            }
            _ => {
                // Wide form for slots >= 256
                self.emit_byte(0xC4); // WIDE
                self.emit_byte(0x39); // DSTORE
                self.emit_short(slot);
            }
        }

        self.stack_depth.pop2(); // double takes 2 slots
    }

    /// Emit an invokedynamic instruction for dynamic method dispatch (Groovy MOP)
    ///
    /// This is the core of Groovy's Meta-Object Protocol (MOP) implementation.
    /// At runtime, the bootstrap method will resolve the actual target method via MetaClass.
    ///
    /// Stack: receiver, arg0, arg1, ... → result
    ///
    /// # Arguments
    ///
    /// * `method_name` - Name of the method to invoke
    /// * `descriptor` - Method descriptor (e.g., "(Ljava/lang/Object;)Ljava/lang/Object;")
    pub fn emit_invokedynamic(&mut self, method_name: &str, descriptor: &str) {
        // Ensure we have a registered bootstrap method for Groovy MOP
        let bootstrap_index = self.ensure_groovy_bootstrap();

        // Add InvokeDynamic entry to constant pool
        // This internally creates the NameAndType entry as well
        let indy_idx =
            self.constant_pool
                .add_invoke_dynamic(bootstrap_index, method_name, descriptor);

        // Emit INVOKEDYNAMIC instruction (0xBA)
        self.emit_byte(0xBA);
        self.emit_short(indy_idx);
        self.emit_short(0); // Reserved bytes (must be zero per JVM spec)

        // Stack depth is adjusted by the caller, which knows the operand count.
    }

    /// Compile a static method call using invokestatic
    ///
    /// This compiles: ClassName.method(args...)
    ///
    /// Stack: arg0, arg1, ... → result
    ///
    /// # Arguments
    ///
    /// * `class_name` - JVM internal class name (e.g., "java/lang/Math")
    /// * `method_name` - Name of the static method
    /// * `descriptor` - Method descriptor (e.g., "(II)I")
    pub fn compile_invokestatic(
        &mut self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
    ) -> JavaType {
        // Add method ref to constant pool
        let method_ref_idx = self
            .constant_pool
            .add_method_ref(class_name, method_name, descriptor);

        // Emit INVOKESTATIC instruction (0xB8)
        self.emit_byte(0xB8);
        self.emit_short(method_ref_idx);

        // Parse descriptor to determine stack effect and return type
        // Count arguments (each 'L...;', 'I', 'J', 'F', 'D', etc.)
        self.stack_depth
            .pop(argument_slots_from_descriptor(descriptor) as i32);

        // Push result (unless void)
        let return_type = parse_return_type(descriptor);
        match return_type.stack_width() {
            1 => self.stack_depth.push1(),
            2 => self.stack_depth.push2(),
            _ => {}
        }

        return_type
    }

    /// Compile a dynamic method call using Groovy MOP (invokedynamic)
    ///
    /// This compiles: receiver.method(args...)
    ///
    /// Stack: receiver, arg0, arg1, ... → result
    ///
    /// # Arguments
    ///
    /// * `method_name` - Name of the method to invoke
    /// * `arg_count` - Number of arguments (excluding receiver)
    pub fn compile_dynamic_method_call(
        &mut self,
        method_name: &str,
        arg_count: usize,
    ) -> Result<JavaType, String> {
        validate_dynamic_call_arity(arg_count)?;

        // The invokedynamic call-site descriptor includes its receiver. The
        // bootstrap adapts that to the underlying virtual member descriptor.
        let mut descriptor = String::from("(Ljava/lang/Object;");
        for _ in 0..arg_count {
            descriptor.push_str("Ljava/lang/Object;");
        }
        descriptor.push_str(")Ljava/lang/Object;");

        // Emit invokedynamic
        self.emit_invokedynamic(method_name, &descriptor);

        // Stack effect: receiver + N args → 1 result
        // Pop receiver + args, push result
        for _ in 0..arg_count {
            self.stack_depth.pop1();
        }
        // Receiver consumed by invokedynamic
        self.stack_depth.pop1();
        // Result is now on stack
        self.stack_depth.push1();

        Ok(JavaType::Object("java/lang/Object".to_string()))
    }

    /// Compile a property access using Groovy MOP (invokedynamic getProperty)
    ///
    /// This compiles: receiver.property
    ///
    /// Stack: receiver → value
    ///
    /// # Arguments
    ///
    /// * `property_name` - Name of the property to access
    pub fn compile_dynamic_property_get(&mut self, property_name: &str) -> JavaType {
        // Property access is implemented as: receiver.getProperty(String propertyName)
        // This is a special case of method call with synthetic name

        // Push property name as String constant
        self.compile_string_literal(property_name);

        // Emit invokedynamic for getProperty
        let descriptor = "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;";
        self.emit_invokedynamic("getProperty", descriptor);

        // Stack effect: receiver, propertyName → value
        self.stack_depth.pop1(); // Pop property name
        self.stack_depth.pop1(); // Pop receiver
                                 // Result now on stack
        self.stack_depth.push1();

        JavaType::Object("java/lang/Object".to_string())
    }

    /// Compile a simple GString (string interpolation) from local variable slots
    ///
    /// Uses StringBuilder pattern for simpler interpreter handling.
    ///
    /// # Arguments
    ///
    /// * `strings` - Static string parts (length = expression_slots.len() + 1)
    /// * `expression_slots` - Local variable slots containing expression values
    pub fn compile_gstring_from_locals(
        &mut self,
        strings: &[&str],
        expression_slots: &[u16],
    ) -> JavaType {
        assert_eq!(
            strings.len(),
            expression_slots.len() + 1,
            "GString must have N+1 strings for N expressions"
        );

        // Create StringBuilder: new StringBuilder()
        let string_builder_idx = self.constant_pool.add_class("java/lang/StringBuilder");
        self.emit_byte(0xBB); // NEW
        self.emit_short(string_builder_idx);
        self.stack_depth.push1();

        self.emit_byte(0x59); // DUP
        self.stack_depth.push1();

        // Call StringBuilder.<init>()
        let sb_init_ref =
            self.constant_pool
                .add_method_ref("java/lang/StringBuilder", "<init>", "()V");
        self.emit_byte(0xB7); // INVOKESPECIAL
        self.emit_short(sb_init_ref);
        self.stack_depth.pop1();

        // Append parts in order: string0, value0, string1, value1, ..., stringN
        for i in 0..=expression_slots.len() {
            // Append static string part
            if !strings[i].is_empty() {
                self.compile_string_literal(strings[i]);
                let append_ref = self.constant_pool.add_method_ref(
                    "java/lang/StringBuilder",
                    "append",
                    "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
                );
                self.emit_byte(0xB6); // INVOKEVIRTUAL
                self.emit_short(append_ref);
                self.stack_depth.pop1(); // string arg consumed, StringBuilder returned
            }

            // Append expression value (if not last iteration)
            if i < expression_slots.len() {
                // Load expression value from local slot
                let slot = expression_slots[i];
                self.compile_aload(slot, "java/lang/Object");

                // Call String.valueOf(Object) to convert to String
                let valueof_ref = self.constant_pool.add_method_ref(
                    "java/lang/String",
                    "valueOf",
                    "(Ljava/lang/Object;)Ljava/lang/String;",
                );
                self.emit_byte(0xB8); // INVOKESTATIC
                self.emit_short(valueof_ref);

                // Append the string value
                let append_ref = self.constant_pool.add_method_ref(
                    "java/lang/StringBuilder",
                    "append",
                    "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
                );
                self.emit_byte(0xB6); // INVOKEVIRTUAL
                self.emit_short(append_ref);
                self.stack_depth.pop1(); // string arg consumed, StringBuilder returned
            }
        }

        // Call toString() to get final String
        let tostring_ref = self.constant_pool.add_method_ref(
            "java/lang/StringBuilder",
            "toString",
            "()Ljava/lang/String;",
        );
        self.emit_byte(0xB6); // INVOKEVIRTUAL
        self.emit_short(tostring_ref);

        JavaType::Object("java/lang/String".to_string())
    }

    /// Consume the compiler and return the constant pool
    pub fn into_constant_pool(self) -> ConstantPoolBuilder {
        self.constant_pool
    }
}

/// Binary operators supported in Groovy expressions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Bitwise
    BitwiseAnd,         // &
    BitwiseOr,          // |
    BitwiseXor,         // ^
    LeftShift,          // <<
    RightShift,         // >>
    UnsignedRightShift, // >>>

    // Comparison
    Equal,              // ==
    NotEqual,           // !=
    LessThan,           // <
    LessThanOrEqual,    // <=
    GreaterThan,        // >
    GreaterThanOrEqual, // >=

    // Logical (short-circuit)
    LogicalAnd, // &&
    LogicalOr,  // ||
}

/// Unary operators supported in Groovy expressions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg, // Negation (-)
    Not, // Logical not (!)
}

/// Label for jump targets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label(u32);

/// Loop context for break/continue label tracking
#[derive(Debug, Clone, Copy)]
struct LoopContext {
    /// Label to jump to for continue (loop start/condition)
    continue_label: Label,
    /// Label to jump to for break (loop end)
    break_label: Label,
}

/// Groovy statement compiler for control flow
pub struct GroovyStatementCompiler {
    expr_compiler: GroovyExpressionCompiler,
    /// Forward references: (bytecode_offset, label_id) - patched when label is bound
    forward_refs: Vec<(usize, u32)>,
    /// Bound labels: label_id -> bytecode_offset
    bound_labels: HashMap<u32, usize>,
    /// Sticky branch-encoding failures discovered while binding labels.
    branch_errors: Vec<String>,
    /// Local variable tracking: name -> slot index (Phase 3 - statement-level locals)
    _locals: HashMap<String, u16>,
    /// Next available local variable slot (Phase 3 - statement-level locals)
    _next_local: u16,
    /// Loop context stack for break/continue
    loop_stack: Vec<LoopContext>,
}

impl GroovyStatementCompiler {
    /// Create a new Groovy statement compiler
    pub fn new(expr_compiler: GroovyExpressionCompiler) -> Self {
        Self {
            expr_compiler,
            forward_refs: Vec::new(),
            bound_labels: HashMap::new(),
            branch_errors: Vec::new(),
            _locals: HashMap::new(),
            _next_local: 0,
            loop_stack: Vec::new(),
        }
    }

    /// Get underlying expression compiler
    pub fn expr_compiler(&mut self) -> &mut GroovyExpressionCompiler {
        &mut self.expr_compiler
    }

    /// Get bytecode
    pub fn bytecode(&self) -> &[u8] {
        self.expr_compiler.bytecode()
    }

    /// Allocate a new label
    pub fn new_label(&mut self) -> Label {
        let id = self.expr_compiler.new_label();
        Label(id)
    }

    /// Get current bytecode position (program counter)
    pub fn pc(&self) -> usize {
        self.expr_compiler.pc()
    }

    /// Get current bytecode position
    fn current_offset(&self) -> usize {
        self.expr_compiler.bytecode().len()
    }

    fn branch_offset(&mut self, instruction_start: usize, target: usize) -> Option<i16> {
        let relative = target as i128 - instruction_start as i128;
        match i16::try_from(relative) {
            Ok(offset) => Some(offset),
            Err(_) => {
                self.branch_errors.push(format!(
                    "Groovy branch from bytecode offset {instruction_start} to {target} exceeds the signed 16-bit JVM branch range"
                ));
                None
            }
        }
    }

    pub(crate) fn validate_control_flow(&self) -> Result<(), String> {
        let mut errors = self.branch_errors.clone();
        if !self.forward_refs.is_empty() {
            let mut labels: Vec<_> = self.forward_refs.iter().map(|(_, label)| *label).collect();
            labels.sort_unstable();
            labels.dedup();
            errors.push(format!("unbound Groovy bytecode labels: {labels:?}"));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Bind a label to the current bytecode position
    pub fn bind_label(&mut self, label: Label) {
        let offset = self.current_offset();
        self.bound_labels.insert(label.0, offset);

        // Patch any forward references to this label
        self.patch_forward_refs(label);
    }

    /// Patch forward references for a newly-bound label
    fn patch_forward_refs(&mut self, label: Label) {
        let target_offset = self.bound_labels[&label.0];

        // Find all forward references to this label
        let refs_to_patch: Vec<_> = self
            .forward_refs
            .iter()
            .filter(|(_, lbl_id)| *lbl_id == label.0)
            .map(|(offset, _)| *offset)
            .collect();

        // Patch each reference. JVM short branches are relative to the opcode
        // address and must fit a signed 16-bit offset.
        for ref_offset in refs_to_patch {
            if let Some(relative_offset) = self.branch_offset(ref_offset, target_offset) {
                let offset_bytes = relative_offset.to_be_bytes();
                let bytecode = self.expr_compiler.bytecode_mut();
                bytecode[ref_offset + 1] = offset_bytes[0];
                bytecode[ref_offset + 2] = offset_bytes[1];
            }
        }

        // Remove patched references
        self.forward_refs.retain(|(_, lbl_id)| *lbl_id != label.0);
    }

    /// Emit a forward jump (GOTO) to a label
    pub fn emit_goto(&mut self, label: Label) {
        let offset = self.current_offset();

        if let Some(&target_offset) = self.bound_labels.get(&label.0) {
            // Label already bound - calculate and emit jump immediately
            self.expr_compiler.emit_byte(0xA7); // GOTO
            let relative_offset = self.branch_offset(offset, target_offset).unwrap_or(0);
            self.expr_compiler
                .emit_short(u16::from_be_bytes(relative_offset.to_be_bytes()));
        } else {
            // Forward reference - emit placeholder and record for patching
            self.expr_compiler.emit_byte(0xA7); // GOTO
            self.expr_compiler.emit_short(0); // Placeholder offset
            self.forward_refs.push((offset, label.0));
        }
    }

    /// Emit a conditional jump: if (int1 == int2) goto label
    pub fn emit_if_icmpeq(&mut self, label: Label) {
        self.emit_conditional_jump(0x9F, label); // IF_ICMPEQ
        self.expr_compiler.stack_depth.pop1(); // Pop both operands
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int1 != int2) goto label
    pub fn emit_if_icmpne(&mut self, label: Label) {
        self.emit_conditional_jump(0xA0, label); // IF_ICMPNE
        self.expr_compiler.stack_depth.pop1();
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int1 < int2) goto label
    pub fn emit_if_icmplt(&mut self, label: Label) {
        self.emit_conditional_jump(0xA1, label); // IF_ICMPLT
        self.expr_compiler.stack_depth.pop1();
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int1 >= int2) goto label
    pub fn emit_if_icmpge(&mut self, label: Label) {
        self.emit_conditional_jump(0xA2, label); // IF_ICMPGE
        self.expr_compiler.stack_depth.pop1();
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int1 > int2) goto label
    pub fn emit_if_icmpgt(&mut self, label: Label) {
        self.emit_conditional_jump(0xA3, label); // IF_ICMPGT
        self.expr_compiler.stack_depth.pop1();
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int1 <= int2) goto label
    pub fn emit_if_icmple(&mut self, label: Label) {
        self.emit_conditional_jump(0xA4, label); // IF_ICMPLE
        self.expr_compiler.stack_depth.pop1();
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int == 0) goto label
    pub fn emit_ifeq(&mut self, label: Label) {
        self.emit_conditional_jump(0x99, label); // IFEQ
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int != 0) goto label
    pub fn emit_ifne(&mut self, label: Label) {
        self.emit_conditional_jump(0x9A, label); // IFNE
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int < 0) goto label
    pub fn emit_iflt(&mut self, label: Label) {
        self.emit_conditional_jump(0x9B, label); // IFLT
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int >= 0) goto label
    pub fn emit_ifge(&mut self, label: Label) {
        self.emit_conditional_jump(0x9C, label); // IFGE
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int > 0) goto label
    pub fn emit_ifgt(&mut self, label: Label) {
        self.emit_conditional_jump(0x9D, label); // IFGT
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (int <= 0) goto label
    pub fn emit_ifle(&mut self, label: Label) {
        self.emit_conditional_jump(0x9E, label); // IFLE
        self.expr_compiler.stack_depth.pop1();
    }

    /// Emit a conditional jump: if (reference == null) goto label
    pub fn emit_ifnull(&mut self, label: Label) {
        self.emit_conditional_jump(0xC6, label); // IFNULL
        self.expr_compiler.stack_depth.pop1();
    }

    /// Helper: emit a conditional jump instruction
    fn emit_conditional_jump(&mut self, opcode: u8, label: Label) {
        let offset = self.current_offset();

        if let Some(&target_offset) = self.bound_labels.get(&label.0) {
            // Label already bound - calculate and emit jump immediately
            self.expr_compiler.emit_byte(opcode);
            let relative_offset = self.branch_offset(offset, target_offset).unwrap_or(0);
            self.expr_compiler
                .emit_short(u16::from_be_bytes(relative_offset.to_be_bytes()));
        } else {
            // Forward reference - emit placeholder and record for patching
            self.expr_compiler.emit_byte(opcode);
            self.expr_compiler.emit_short(0); // Placeholder offset
            self.forward_refs.push((offset, label.0));
        }
    }

    /// Compile a void return (no value)
    pub fn compile_return_void(&mut self) {
        self.expr_compiler.emit_byte(0xB1); // RETURN
    }

    /// Compile a break statement
    ///
    /// Emits a GOTO to the current loop's break label (loop end)
    pub fn compile_break(&mut self) -> bool {
        if let Some(loop_ctx) = self.loop_stack.last() {
            self.emit_goto(loop_ctx.break_label);
            true
        } else {
            false
        }
    }

    /// Compile a continue statement
    ///
    /// Emits a GOTO to the current loop's continue label (loop start)
    pub fn compile_continue(&mut self) -> bool {
        if let Some(loop_ctx) = self.loop_stack.last() {
            self.emit_goto(loop_ctx.continue_label);
            true
        } else {
            false
        }
    }

    /// Push a loop context onto the stack for break/continue tracking
    pub fn push_loop_context(&mut self, continue_label: Label, break_label: Label) {
        self.loop_stack.push(LoopContext {
            continue_label,
            break_label,
        });
    }

    /// Pop a loop context from the stack
    pub fn pop_loop_context(&mut self) {
        self.loop_stack.pop();
    }

    /// Get current loop targets for break/continue (used by finally block handling)
    pub fn current_loop_targets(&self) -> (Option<Label>, Option<Label>) {
        self.loop_stack
            .last()
            .map(|ctx| (Some(ctx.break_label), Some(ctx.continue_label)))
            .unwrap_or((None, None))
    }

    /// Consume the compiler and return the constant pool
    pub fn into_constant_pool(self) -> ConstantPoolBuilder {
        self.expr_compiler.into_constant_pool()
    }
}

/// Count JVM local/operand-stack slots consumed by a method descriptor's
/// arguments. Long and double consume two slots; all other field types consume
/// one.
fn argument_slots_from_descriptor(descriptor: &str) -> usize {
    let mut slots = 0;
    let mut chars = descriptor.chars().peekable();

    // Skip opening '('
    if chars.next() != Some('(') {
        return 0;
    }

    while let Some(c) = chars.next() {
        match c {
            ')' => break, // End of args
            'L' => {
                // Object type - skip until ';'
                for character in chars.by_ref() {
                    if character == ';' {
                        break;
                    }
                }
                slots += 1;
            }
            '[' => {
                // Array type - skip array markers, then count the element
                while chars.peek() == Some(&'[') {
                    chars.next();
                }
                // Now handle the element type
                if let Some(elem) = chars.next() {
                    if elem == 'L' {
                        for character in chars.by_ref() {
                            if character == ';' {
                                break;
                            }
                        }
                    }
                }
                slots += 1;
            }
            'D' | 'J' => slots += 2,
            'B' | 'C' | 'F' | 'I' | 'S' | 'Z' => slots += 1,
            _ => {} // Skip unknown
        }
    }
    slots
}

/// Parse the return type from a method descriptor
/// e.g., "(II)I" -> JavaType::Int, "(II)V" -> JavaType::Void
fn parse_return_type(descriptor: &str) -> JavaType {
    // Find the return type after ')'
    if let Some(pos) = descriptor.rfind(')') {
        let return_desc = &descriptor[pos + 1..];
        match return_desc.chars().next() {
            Some('V') => JavaType::Void,
            Some('I') => JavaType::Int,
            Some('J') => JavaType::Long,
            Some('F') => JavaType::Float,
            Some('D') => JavaType::Double,
            Some('Z') => JavaType::Boolean,
            Some('B') => JavaType::Byte,
            Some('C') => JavaType::Char,
            Some('S') => JavaType::Short,
            Some('L') => {
                // Object type - extract class name
                let class_name = return_desc.trim_start_matches('L').trim_end_matches(';');
                JavaType::Object(class_name.to_string())
            }
            Some('[') => {
                // Array type
                JavaType::Object(return_desc.to_string())
            }
            _ => JavaType::Object("java/lang/Object".to_string()),
        }
    } else {
        JavaType::Object("java/lang/Object".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::producer::ConstantPoolItem;

    fn invokedynamic_descriptors(constant_pool: &ConstantPoolBuilder) -> Vec<String> {
        let entries = constant_pool.get_entries();
        entries
            .iter()
            .filter_map(|entry| {
                let ConstantPoolItem::InvokeDynamic {
                    name_and_type_index,
                    ..
                } = entry
                else {
                    return None;
                };
                let ConstantPoolItem::NameAndType {
                    descriptor_index, ..
                } = &entries[usize::from(*name_and_type_index) - 1]
                else {
                    panic!("invokedynamic must reference NameAndType")
                };
                let ConstantPoolItem::Utf8(descriptor) =
                    &entries[usize::from(*descriptor_index) - 1]
                else {
                    panic!("NameAndType must reference a UTF-8 descriptor")
                };
                Some(descriptor.clone())
            })
            .collect()
    }

    #[test]
    fn test_expression_compiler_creation() {
        let cp = ConstantPoolBuilder::new();
        let compiler = GroovyExpressionCompiler::new(cp);
        assert_eq!(compiler.pc(), 0);
    }

    #[test]
    fn test_emit_byte() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.emit_byte(42);
        assert_eq!(compiler.bytecode(), &[42]);
    }

    #[test]
    fn test_emit_short() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.emit_short(0x1234);
        assert_eq!(compiler.bytecode(), &[0x12, 0x34]);
    }

    #[test]
    fn test_label_allocation() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        let label1 = compiler.new_label();
        let label2 = compiler.new_label();
        assert_eq!(label1, 0);
        assert_eq!(label2, 1);
    }

    #[test]
    fn test_closure_id_allocation() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        let closure1 = compiler.new_closure_id();
        let closure2 = compiler.new_closure_id();
        assert_eq!(closure1, 0);
        assert_eq!(closure2, 1);
    }

    // Literal compilation tests
    #[test]
    fn test_compile_int_iconst() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Test ICONST_M1 through ICONST_5
        compiler.compile_int_literal(-1);
        assert_eq!(compiler.bytecode(), &[0x02]); // ICONST_M1

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_int_literal(0);
        assert_eq!(compiler.bytecode(), &[0x03]); // ICONST_0

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_int_literal(5);
        assert_eq!(compiler.bytecode(), &[0x08]); // ICONST_5
    }

    #[test]
    fn test_compile_int_bipush() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_int_literal(42);
        assert_eq!(compiler.bytecode(), &[0x10, 42]); // BIPUSH 42

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_int_literal(-128);
        assert_eq!(compiler.bytecode(), &[0x10, 0x80]); // BIPUSH -128
    }

    #[test]
    fn test_compile_int_sipush() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_int_literal(1000);
        assert_eq!(compiler.bytecode(), &[0x11, 0x03, 0xE8]); // SIPUSH 1000

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_int_literal(-1000);
        // -1000 in two's complement u16 = 0xFC18
        assert_eq!(compiler.bytecode(), &[0x11, 0xFC, 0x18]); // SIPUSH -1000
    }

    #[test]
    fn test_compile_int_ldc() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_int_literal(100_000);
        // Should use LDC with constant pool
        assert_eq!(compiler.bytecode()[0], 0x12); // LDC or LDC_W
    }

    #[test]
    fn test_compile_string_literal() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let java_type = compiler.compile_string_literal("hello");
        assert_eq!(java_type, JavaType::Object("java/lang/String".to_string()));
        assert_eq!(compiler.bytecode()[0], 0x12); // LDC
    }

    #[test]
    fn test_compile_boolean_literal() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let java_type = compiler.compile_boolean_literal(true);
        assert_eq!(java_type, JavaType::Object("java/lang/Boolean".to_string()));

        // Should start with ICONST_1
        assert_eq!(compiler.bytecode()[0], 0x04); // ICONST_1
                                                  // Followed by INVOKESTATIC Boolean.valueOf
        assert_eq!(compiler.bytecode()[1], 0xB8); // INVOKESTATIC

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_boolean_literal(false);
        assert_eq!(compiler.bytecode()[0], 0x03); // ICONST_0
    }

    #[test]
    fn test_compile_null_literal() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let java_type = compiler.compile_null_literal();
        assert_eq!(java_type, JavaType::Object("java/lang/Object".to_string()));
        assert_eq!(compiler.bytecode(), &[0x01]); // ACONST_NULL
    }

    // Variable operation tests
    #[test]
    fn test_compile_iload_optimized() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Test ILOAD_0 through ILOAD_3
        compiler.compile_iload(0);
        assert_eq!(compiler.bytecode(), &[0x1A]); // ILOAD_0

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_iload(1);
        assert_eq!(compiler.bytecode(), &[0x1B]); // ILOAD_1

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_iload(3);
        assert_eq!(compiler.bytecode(), &[0x1D]); // ILOAD_3
    }

    #[test]
    fn test_compile_iload_regular() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_iload(5);
        assert_eq!(compiler.bytecode(), &[0x15, 5]); // ILOAD 5
    }

    #[test]
    fn test_compile_iload_wide() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_iload(300);
        // WIDE ILOAD 300 (0x012C)
        assert_eq!(compiler.bytecode(), &[0xC4, 0x15, 0x01, 0x2C]);
    }

    #[test]
    fn test_compile_aload_optimized() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_aload(0, "java/lang/String");
        assert_eq!(compiler.bytecode(), &[0x2A]); // ALOAD_0

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.compile_aload(2, "java/lang/Object");
        assert_eq!(compiler.bytecode(), &[0x2C]); // ALOAD_2
    }

    #[test]
    fn test_compile_aload_regular() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let java_type = compiler.compile_aload(7, "com/example/User");
        assert_eq!(compiler.bytecode(), &[0x19, 7]); // ALOAD 7
        assert_eq!(java_type, JavaType::Object("com/example/User".to_string()));
    }

    #[test]
    fn test_compile_istore_optimized() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Need value on stack first
        compiler.compile_int_literal(42);
        compiler.compile_istore(0);
        // BIPUSH 42, ISTORE_0
        assert_eq!(compiler.bytecode(), &[0x10, 42, 0x3B]);
    }

    #[test]
    fn test_compile_istore_regular() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_int_literal(42);
        compiler.compile_istore(5);
        // BIPUSH 42, ISTORE 5
        assert_eq!(compiler.bytecode(), &[0x10, 42, 0x36, 5]);
    }

    #[test]
    fn test_compile_astore_optimized() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_null_literal();
        compiler.compile_astore(1);
        // ACONST_NULL, ASTORE_1
        assert_eq!(compiler.bytecode(), &[0x01, 0x4C]);
    }

    #[test]
    fn test_compile_astore_regular() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        compiler.compile_null_literal();
        compiler.compile_astore(10);
        // ACONST_NULL, ASTORE 10
        assert_eq!(compiler.bytecode(), &[0x01, 0x3A, 10]);
    }

    #[test]
    fn test_variable_round_trip() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Store value to slot 2, then load it back
        compiler.compile_int_literal(42);
        compiler.compile_istore(2);
        compiler.compile_iload(2);

        // BIPUSH 42, ISTORE_2, ILOAD_2
        assert_eq!(compiler.bytecode(), &[0x10, 42, 0x3D, 0x1C]);
    }

    #[test]
    fn test_compile_return_void() {
        let cp = ConstantPoolBuilder::new();
        let expr_compiler = GroovyExpressionCompiler::new(cp);
        let mut stmt_compiler = GroovyStatementCompiler::new(expr_compiler);

        stmt_compiler.compile_return_void();
        assert_eq!(stmt_compiler.bytecode(), &[0xB1]); // RETURN
    }

    #[test]
    fn test_label_forward_reference_patching() {
        // Test that forward references are correctly patched
        let cp = ConstantPoolBuilder::new();
        let expr_compiler = GroovyExpressionCompiler::new(cp);
        let mut stmt_compiler = GroovyStatementCompiler::new(expr_compiler);

        // Create a label
        let target = stmt_compiler.new_label();

        // Emit a forward jump (label not yet bound)
        // GOTO takes 3 bytes: opcode (1) + offset (2)
        stmt_compiler.emit_goto(target);

        // Emit some code (BIPUSH 42 = 2 bytes)
        stmt_compiler.expr_compiler().compile_int_literal(42);

        // Bind the label (now at offset 3 + 2 = 5)
        let bind_offset = stmt_compiler.bytecode().len();
        stmt_compiler.bind_label(target);

        // Check that the GOTO instruction was patched
        let bytecode = stmt_compiler.bytecode();
        assert_eq!(bytecode[0], 0xA7); // GOTO

        // The offset should point to the label binding location
        // Jump offset is relative to the instruction START (not PC after instruction)
        let jump_offset = i16::from_be_bytes([bytecode[1], bytecode[2]]);
        let expected_offset = bind_offset as i16; // relative to instruction start (offset 0)
        assert_eq!(jump_offset, expected_offset);
    }

    #[test]
    fn branch_encoding_rejects_out_of_range_offsets_and_unbound_labels() {
        let cp = ConstantPoolBuilder::new();
        let expr_compiler = GroovyExpressionCompiler::new(cp);
        let mut forward = GroovyStatementCompiler::new(expr_compiler);
        let distant = forward.new_label();
        forward.emit_goto(distant);
        for _ in 0..i16::MAX as usize {
            forward.expr_compiler().emit_byte(0x00);
        }
        forward.bind_label(distant);
        assert!(forward
            .validate_control_flow()
            .expect_err("oversized forward branch must fail")
            .contains("exceeds the signed 16-bit JVM branch range"));

        let cp = ConstantPoolBuilder::new();
        let expr_compiler = GroovyExpressionCompiler::new(cp);
        let mut backward = GroovyStatementCompiler::new(expr_compiler);
        let start = backward.new_label();
        backward.bind_label(start);
        for _ in 0..(i16::MAX as usize + 2) {
            backward.expr_compiler().emit_byte(0x00);
        }
        backward.emit_goto(start);
        assert!(backward
            .validate_control_flow()
            .expect_err("oversized backward branch must fail")
            .contains("exceeds the signed 16-bit JVM branch range"));

        let cp = ConstantPoolBuilder::new();
        let expr_compiler = GroovyExpressionCompiler::new(cp);
        let mut unbound = GroovyStatementCompiler::new(expr_compiler);
        let missing = unbound.new_label();
        unbound.emit_goto(missing);
        assert!(unbound
            .validate_control_flow()
            .expect_err("unbound branch target must fail")
            .contains("unbound Groovy bytecode labels"));
    }

    #[test]
    fn test_emit_invokedynamic() {
        // Test basic invokedynamic instruction emission
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Emit a receiver-only Groovy invokedynamic site.
        compiler.emit_invokedynamic("someMethod", "(Ljava/lang/Object;)Ljava/lang/Object;");

        let bytecode = compiler.bytecode();

        // Should emit: INVOKEDYNAMIC (0xBA) + cp_index (2 bytes) + reserved (2 bytes)
        assert_eq!(bytecode[0], 0xBA); // INVOKEDYNAMIC
        assert_eq!(bytecode.len(), 5); // opcode + index + reserved
        assert_eq!(bytecode[3], 0x00); // Reserved byte 1
        assert_eq!(bytecode[4], 0x00); // Reserved byte 2
    }

    #[test]
    fn test_compile_dynamic_method_call() {
        // Test: receiver.doSomething(arg1, arg2)
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Simulate: receiver and 2 args already on stack
        compiler.stack_depth.push1(); // receiver
        compiler.stack_depth.push1(); // arg1
        compiler.stack_depth.push1(); // arg2

        // Compile dynamic method call
        let result_type = compiler
            .compile_dynamic_method_call("doSomething", 2)
            .expect("two arguments fit the call-site descriptor");

        // Verify return type
        assert_eq!(
            result_type,
            JavaType::Object("java/lang/Object".to_string())
        );

        // Verify bytecode contains INVOKEDYNAMIC
        let bytecode = compiler.bytecode();
        assert!(bytecode.contains(&0xBA)); // INVOKEDYNAMIC

        // Stack should have result after popping receiver + 2 args
        assert_eq!(compiler.stack_depth.current_depth(), 1);
    }

    #[test]
    fn dynamic_method_call_enforces_the_jvm_parameter_slot_limit() {
        assert!(validate_dynamic_call_arity(MAX_DYNAMIC_CALL_ARGUMENTS).is_ok());

        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        let error = compiler
            .compile_dynamic_method_call("oversized", MAX_DYNAMIC_CALL_ARGUMENTS + 1)
            .expect_err("receiver plus 255 arguments exceeds the JVM descriptor limit");

        assert!(error.contains("at most 254 arguments plus the receiver"));
        assert!(compiler.bytecode().is_empty());
        assert_eq!(compiler.stack_depth.current_depth(), 0);
    }

    #[test]
    fn invokestatic_tracks_category_two_argument_and_return_slots() {
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);
        compiler.stack_depth.push2();
        compiler.stack_depth.push2();

        assert_eq!(
            compiler.compile_invokestatic("java/lang/Math", "pow", "(DD)D"),
            JavaType::Double
        );
        assert_eq!(compiler.stack_depth.current_depth(), 2);
        assert_eq!(compiler.stack_depth.max_depth().expect("valid depth"), 4);
    }

    #[test]
    fn test_compile_dynamic_property_get() {
        // Test: receiver.propertyName
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Simulate: receiver on stack
        compiler.stack_depth.push1();

        // Compile property access
        let result_type = compiler.compile_dynamic_property_get("myProperty");

        // Verify return type
        assert_eq!(
            result_type,
            JavaType::Object("java/lang/Object".to_string())
        );

        // Verify bytecode
        let bytecode = compiler.bytecode();
        // Should have: LDC (for property name string) + INVOKEDYNAMIC
        assert!(bytecode.contains(&0x12) || bytecode.contains(&0x13)); // LDC or LDC_W
        assert!(bytecode.contains(&0xBA)); // INVOKEDYNAMIC

        // Stack should have result
        assert_eq!(compiler.stack_depth.current_depth(), 1);
    }

    #[test]
    fn test_invokedynamic_descriptor_generation() {
        // Test descriptor generation for various arg counts
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // 0 args
        compiler.stack_depth.push1(); // receiver
        compiler
            .compile_dynamic_method_call("method0", 0)
            .expect("zero arguments fit the call-site descriptor");
        let descriptors = invokedynamic_descriptors(&compiler.constant_pool);
        assert_eq!(descriptors, ["(Ljava/lang/Object;)Ljava/lang/Object;"]);

        // Reset
        let cp2 = ConstantPoolBuilder::new();
        let mut compiler2 = GroovyExpressionCompiler::new(cp2);

        // 3 args
        compiler2.stack_depth.push1(); // receiver
        compiler2.stack_depth.push1(); // arg1
        compiler2.stack_depth.push1(); // arg2
        compiler2.stack_depth.push1(); // arg3
        compiler2
            .compile_dynamic_method_call("method3", 3)
            .expect("three arguments fit the call-site descriptor");
        let descriptors = invokedynamic_descriptors(&compiler2.constant_pool);
        assert_eq!(
            descriptors,
            ["(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"]
        );

        // Both should emit INVOKEDYNAMIC
        assert!(compiler.bytecode().contains(&0xBA));
        assert!(compiler2.bytecode().contains(&0xBA));
    }

    #[test]
    fn test_compile_gstring_structure() {
        // Test GString compilation: "x=${x}" - compiled via StringBuilder to String
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Use compile_gstring_from_locals which takes expression slots instead of stack values
        let strings = vec!["x=", ""];
        let expression_slots = vec![0]; // Expression value in slot 0
        let result = compiler.compile_gstring_from_locals(&strings, &expression_slots);

        // GString is compiled to java/lang/String via StringBuilder for efficiency
        assert_eq!(result, JavaType::Object("java/lang/String".to_string()));

        // Verify bytecode contains StringBuilder creation and calls
        let bytecode = compiler.bytecode();
        assert!(bytecode.contains(&0xBB)); // NEW (StringBuilder)
        assert!(bytecode.contains(&0xB7)); // INVOKESPECIAL (constructor)
        assert!(bytecode.contains(&0xB6)); // INVOKEVIRTUAL (append, toString)
    }

    #[test]
    fn test_compile_gstring_from_locals() {
        // Test: "Hello ${name}, you are ${age}" - compiled via StringBuilder
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Strings: ["Hello ", ", you are ", ""]
        // Expressions in slots: [0 (name), 1 (age)]
        let strings = vec!["Hello ", ", you are ", ""];
        let expression_slots = vec![0, 1];

        let result = compiler.compile_gstring_from_locals(&strings, &expression_slots);

        // GString is compiled to java/lang/String via StringBuilder for efficiency
        assert_eq!(result, JavaType::Object("java/lang/String".to_string()));

        // Verify bytecode contains StringBuilder pattern
        let bytecode = compiler.bytecode();

        // Should have NEW for StringBuilder
        assert!(bytecode.contains(&0xBB)); // NEW

        // Should have INVOKEVIRTUAL for append() and toString() calls
        let invokevirtual_count = bytecode.iter().filter(|&&b| b == 0xB6).count();
        assert!(
            invokevirtual_count >= 2,
            "Should have multiple INVOKEVIRTUAL for append/toString"
        );
    }

    #[test]
    #[should_panic(expected = "GString must have N+1 strings for N expressions")]
    fn test_compile_gstring_invariant() {
        // Test that GString enforces N+1 strings for N expressions
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        // Invalid: 2 strings for 2 expressions (should be 3 strings)
        let strings = vec!["x=", "y="];
        compiler.compile_gstring_from_locals(&strings, &[0, 1]);
    }

    #[test]
    fn test_compile_simple_gstring() {
        // Test simplest GString: "value: ${x}" - compiled via StringBuilder to String
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let strings = vec!["value: ", ""];
        let expression_slots = vec![0];

        let result = compiler.compile_gstring_from_locals(&strings, &expression_slots);

        // GString is compiled to java/lang/String via StringBuilder for efficiency
        assert_eq!(result, JavaType::Object("java/lang/String".to_string()));

        let bytecode = compiler.bytecode();
        // Should contain NEW for StringBuilder
        assert!(bytecode.contains(&0xBB)); // NEW
    }

    #[test]
    fn test_compile_gstring_multiple_expressions() {
        // Test: "${a}+${b}=${c}" - compiled via StringBuilder to String
        let cp = ConstantPoolBuilder::new();
        let mut compiler = GroovyExpressionCompiler::new(cp);

        let strings = vec!["", "+", "=", ""];
        let expression_slots = vec![0, 1, 2]; // a, b, c

        let result = compiler.compile_gstring_from_locals(&strings, &expression_slots);

        // GString is compiled to java/lang/String via StringBuilder for efficiency
        assert_eq!(result, JavaType::Object("java/lang/String".to_string()));

        let bytecode = compiler.bytecode();

        // Should have multiple INVOKEVIRTUAL for StringBuilder.append calls
        let invokevirtual_count = bytecode.iter().filter(|&&b| b == 0xB6).count();
        assert!(
            invokevirtual_count >= 3,
            "Should have multiple append() calls"
        );
    }
}
