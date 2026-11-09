// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration test framework for bytecode execution
//!
//! Provides helpers for building, executing, and verifying simple Java bytecode
//! without needing to compile actual Java classes.

use hyperion_bytecode::interpreter::executor::Executor;
use hyperion_bytecode::interpreter::natives::NativeRegistry;
use hyperion_bytecode::interpreter::*;
use hyperion_bytecode::producer::ConstantPoolItem;

/// A simple bytecode test builder for integration tests
pub struct BytecodeTestBuilder {
    bytecode: Vec<u8>,
    constant_pool: Option<Vec<ConstantPoolItem>>,
    include_natives: bool,
}

impl BytecodeTestBuilder {
    /// Create a new bytecode test builder
    pub fn new() -> Self {
        BytecodeTestBuilder {
            bytecode: Vec::new(),
            constant_pool: None,
            include_natives: true,
        }
    }

    /// Add raw bytecode bytes
    pub fn bytecode(mut self, bytes: Vec<u8>) -> Self {
        self.bytecode = bytes;
        self
    }

    /// Set the constant pool
    /// Automatically prepends index 0 placeholder (JVM spec requires index 0 unused)
    #[allow(dead_code)]
    pub fn constant_pool(mut self, pool: Vec<ConstantPoolItem>) -> Self {
        // JVM constant pools are 1-indexed, index 0 is reserved
        // Prepend a dummy entry so user-provided items start at index 1
        let mut full_pool = vec![ConstantPoolItem::Utf8(String::new())];
        full_pool.extend(pool);
        self.constant_pool = Some(full_pool);
        self
    }

    /// Build and execute the bytecode
    pub fn execute(self) -> VMResult<ExecutionResult> {
        // Create executor with optional constant pool
        let mut executor = Executor::new(self.bytecode);
        if let Some(cp) = self.constant_pool {
            executor = executor.with_constant_pool(cp);
        }
        if self.include_natives {
            executor = executor.with_native_registry(NativeRegistry::new());
        }

        // Create thread and frame
        let mut thread = Thread::new(10, None);
        let frame = Frame::new("Test".to_string(), "execute".to_string(), 10, 20, vec![])?;
        thread.push_frame(frame)?;

        // Create heap
        let mut heap = Heap::new(1024 * 1024);
        let mut monitors = MonitorTable::new();

        // Execute until completion or error
        let mut result_stack: Vec<Value> = Vec::new();
        loop {
            match executor.step(&mut thread, &mut heap, &mut monitors) {
                Ok(true) => {
                    // Continue execution
                }
                Ok(false) => {
                    // Execution complete
                    // Capture stack values for verification
                    if let Some(frame) = thread.current_frame_mut() {
                        while let Ok(val) = frame.pop() {
                            result_stack.push(val);
                        }
                    }
                    break;
                }
                Err(e) => {
                    // Execution error
                    return Err(e);
                }
            }
        }

        Ok(ExecutionResult {
            stack_values: result_stack,
        })
    }
}

impl Default for BytecodeTestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of bytecode execution
pub struct ExecutionResult {
    pub stack_values: Vec<Value>,
}

impl ExecutionResult {
    /// Get the top stack value
    pub fn top(&self) -> Option<&Value> {
        self.stack_values.first()
    }

    /// Get value at specific stack position (0 = top)
    #[allow(dead_code)]
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.stack_values.get(index)
    }

    /// Verify top value is an integer with expected value
    pub fn assert_top_int(&self, expected: i32) -> VMResult<()> {
        match self.top() {
            Some(Value::Int(v)) if *v == expected => Ok(()),
            Some(Value::Int(v)) => Err(VMError::InternalError(format!(
                "Expected int {}, got {}",
                expected, v
            ))),
            Some(v) => Err(VMError::InternalError(format!("Expected int, got {:?}", v))),
            None => Err(VMError::InternalError("Stack is empty".to_string())),
        }
    }

    /// Verify top value is a long with expected value
    pub fn assert_top_long(&self, expected: i64) -> VMResult<()> {
        match self.top() {
            Some(Value::Long(v)) if *v == expected => Ok(()),
            Some(Value::Long(v)) => Err(VMError::InternalError(format!(
                "Expected long {}, got {}",
                expected, v
            ))),
            Some(v) => Err(VMError::InternalError(format!(
                "Expected long, got {:?}",
                v
            ))),
            None => Err(VMError::InternalError("Stack is empty".to_string())),
        }
    }

    /// Verify top value is a float with expected value (with tolerance)
    pub fn assert_top_float(&self, expected: f32, tolerance: f32) -> VMResult<()> {
        match self.top() {
            Some(Value::Float(v)) if (v - expected).abs() < tolerance => Ok(()),
            Some(Value::Float(v)) => Err(VMError::InternalError(format!(
                "Expected float {} (±{}), got {}",
                expected, tolerance, v
            ))),
            Some(v) => Err(VMError::InternalError(format!(
                "Expected float, got {:?}",
                v
            ))),
            None => Err(VMError::InternalError("Stack is empty".to_string())),
        }
    }

    /// Verify top value is a double with expected value (with tolerance)
    pub fn assert_top_double(&self, expected: f64, tolerance: f64) -> VMResult<()> {
        match self.top() {
            Some(Value::Double(v)) if (v - expected).abs() < tolerance => Ok(()),
            Some(Value::Double(v)) => Err(VMError::InternalError(format!(
                "Expected double {} (±{}), got {}",
                expected, tolerance, v
            ))),
            Some(v) => Err(VMError::InternalError(format!(
                "Expected double, got {:?}",
                v
            ))),
            None => Err(VMError::InternalError("Stack is empty".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_arithmetic() -> VMResult<()> {
        // Test: iconst_1, iconst_2, iadd
        // Expected result: 3 on stack
        let bytecode = vec![
            0x04, // iconst_1
            0x05, // iconst_2
            0x60, // iadd
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(3)?;

        Ok(())
    }

    #[test]
    fn test_load_store_arithmetic() -> VMResult<()> {
        // Test: iconst_5, istore_0, iload_0, iconst_3, isub
        // Expected: 5, then pop and store, then load 5, push 3, subtract = 2
        let bytecode = vec![
            0x08, // iconst_5
            0x3b, // istore_0 (store 5 in local 0)
            0x1a, // iload_0 (load from local 0, pushes 5)
            0x06, // iconst_3
            0x64, // isub (5 - 3 = 2)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(2)?;

        Ok(())
    }

    #[test]
    fn test_multiple_arithmetic_operations() -> VMResult<()> {
        // Test: iconst_2, iconst_3, iadd (= 5), iconst_4, imul (= 20), iconst_2, idiv (= 10)
        let bytecode = vec![
            0x05, // iconst_2
            0x06, // iconst_3
            0x60, // iadd (2 + 3 = 5)
            0x07, // iconst_4
            0x68, // imul (5 * 4 = 20)
            0x05, // iconst_2
            0x6c, // idiv (20 / 2 = 10)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(10)?;

        Ok(())
    }

    #[test]
    fn test_long_arithmetic() -> VMResult<()> {
        // Test: lconst_1, lconst_1, ladd (= 2)
        let bytecode = vec![
            0x0a, // lconst_1
            0x0a, // lconst_1
            0x61, // ladd (1L + 1L = 2L)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_long(2)?;

        Ok(())
    }

    #[test]
    fn test_float_arithmetic() -> VMResult<()> {
        // Test: fconst_1, fconst_2, fadd
        let bytecode = vec![
            0x0b, // fconst_0 (0.0)
            0x0c, // fconst_1 (1.0)
            0x62, // fadd (0.0 + 1.0 = 1.0)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_float(1.0, 0.001)?;

        Ok(())
    }

    #[test]
    fn test_type_conversions() -> VMResult<()> {
        // Test: iconst_5, i2l (convert int to long)
        let bytecode = vec![
            0x08, // iconst_5
            0x85, // i2l
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_long(5)?;

        Ok(())
    }

    #[test]
    fn test_stack_manipulation() -> VMResult<()> {
        // Test: iconst_1, iconst_2, dup, iadd (stack: 2, 1, 2, then 2+2=4 on top, then 1)
        let bytecode = vec![
            0x04, // iconst_1
            0x05, // iconst_2
            0x59, // dup (duplicate top of stack)
            0x60, // iadd (2 + 2 = 4)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(4)?;

        Ok(())
    }

    #[test]
    fn test_pop_operation() -> VMResult<()> {
        // Test: iconst_1, iconst_2, pop (removes 2, leaves 1)
        let bytecode = vec![
            0x04, // iconst_1
            0x05, // iconst_2
            0x57, // pop (removes 2)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(1)?;

        Ok(())
    }

    #[test]
    fn test_swap_operation() -> VMResult<()> {
        // Test: iconst_1, iconst_2, swap (swaps top two values)
        let bytecode = vec![
            0x04, // iconst_1
            0x05, // iconst_2
            0x5f, // swap (exchanges top two)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        // After swap, top should be 1 (was originally second)
        result.assert_top_int(1)?;

        Ok(())
    }

    #[test]
    fn test_negative_numbers() -> VMResult<()> {
        // Test: iconst_m1, iconst_5, iadd
        let bytecode = vec![
            0x02, // iconst_m1 (-1)
            0x08, // iconst_5
            0x60, // iadd (-1 + 5 = 4)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(4)?;

        Ok(())
    }

    #[test]
    fn test_bipush_instruction() -> VMResult<()> {
        // Test: bipush 127 (push single byte)
        let bytecode = vec![
            0x10, // bipush
            0x7f, // 127
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(127)?;

        Ok(())
    }

    #[test]
    fn test_sipush_instruction() -> VMResult<()> {
        // Test: sipush 500 (push short)
        let bytecode = vec![
            0x11, // sipush
            0x01, // high byte
            0xf4, // low byte (0x01f4 = 500)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(500)?;

        Ok(())
    }

    #[test]
    fn test_double_arithmetic() -> VMResult<()> {
        // Test: dconst_0, dconst_1, dadd
        let bytecode = vec![
            0x0e, // dconst_0
            0x0f, // dconst_1
            0x63, // dadd
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_double(1.0, 0.001)?;

        Ok(())
    }

    #[test]
    fn test_lmul_arithmetic() -> VMResult<()> {
        // Test: lconst_1, bipush 5, i2l, lmul
        let bytecode = vec![
            0x0a, // lconst_1
            0x10, // bipush
            0x05, // 5
            0x85, // i2l (convert 5 to long)
            0x69, // lmul (1L * 5L = 5L)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_long(5)?;

        Ok(())
    }

    #[test]
    fn test_ineg_negation() -> VMResult<()> {
        // Test: bipush 42, ineg
        let bytecode = vec![
            0x10, // bipush
            0x2a, // 42
            0x74, // ineg (negate = -42)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(-42)?;

        Ok(())
    }

    #[test]
    fn test_lneg_long_negation() -> VMResult<()> {
        // Test: lconst_1, lneg
        let bytecode = vec![
            0x0a, // lconst_1
            0x75, // lneg (negate = -1L)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_long(-1)?;

        Ok(())
    }

    #[test]
    fn test_lrem_modulo() -> VMResult<()> {
        // Test: bipush 7, i2l, bipush 3, i2l, lrem (7L % 3L = 1L)
        let bytecode = vec![
            0x10, // bipush
            0x07, // 7
            0x85, // i2l (convert 7 to long)
            0x10, // bipush
            0x03, // 3
            0x85, // i2l (convert 3 to long)
            0x71, // lrem (7L % 3L = 1L)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_long(1)?;

        Ok(())
    }

    #[test]
    fn test_aconst_null() -> VMResult<()> {
        // Test: aconst_null pushes null reference
        let bytecode = vec![
            0x01, // aconst_null
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        match result.top() {
            Some(Value::Reference(ObjectRef::Null)) => Ok(()),
            Some(v) => Err(VMError::InternalError(format!(
                "Expected null reference, got {:?}",
                v
            ))),
            None => Err(VMError::InternalError("Stack is empty".to_string())),
        }
    }

    #[test]
    fn test_complex_expression() -> VMResult<()> {
        // Test: (10 + 5) * 2 - 3
        let bytecode = vec![
            0x10, // bipush 10
            0x0a, //
            0x10, // bipush 5
            0x05, //
            0x60, // iadd (10 + 5 = 15)
            0x05, // iconst_2
            0x68, // imul (15 * 2 = 30)
            0x06, // iconst_3
            0x64, // isub (30 - 3 = 27)
        ];

        let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
        result.assert_top_int(27)?;

        Ok(())
    }
}
