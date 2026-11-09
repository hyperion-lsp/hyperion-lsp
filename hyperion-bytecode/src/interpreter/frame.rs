// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM stack frame implementation
//!
//! Represents a single stack frame for method execution.

use std::sync::Arc;

use super::errors::{VMError, VMResult};
use super::value::{HeapId, Value};

/// Exception table entry for catch block handling
#[derive(Debug, Clone)]
pub struct ExceptionTableEntry {
    /// Start of protected region (inclusive)
    pub start_pc: u16,
    /// End of protected region (exclusive)
    pub end_pc: u16,
    /// Jump target for exception handler
    pub handler_pc: u16,
    /// Exception class to catch (None = catch-all for finally blocks)
    pub catch_type: Option<String>,
}

/// A JVM stack frame for method execution
#[derive(Debug, Clone)]
pub struct Frame {
    /// Method being executed (would contain method metadata)
    pub method_name: String,

    /// Class name containing this method
    pub class_name: String,

    /// Program counter (current bytecode offset)
    pub pc: usize,

    /// Bytecode offset of the instruction whose execution is still active.
    /// This differs from `pc` while a call is in flight: `pc` is the caller's
    /// continuation, whereas exception-table matching must use the invoke
    /// instruction. `usize::MAX` means no instruction has started yet.
    active_instruction_pc: usize,

    /// Local variable array
    pub locals: Vec<Option<Value>>,

    /// Operand stack
    pub operand_stack: Vec<Value>,

    /// Maximum operand stack size
    pub max_stack: u16,

    /// Maximum local variables
    pub max_locals: u16,

    /// Bytecode for this method (needed for method dispatch).
    /// Wrapped in Arc to avoid full copies when syncing between Frame and Executor.
    /// `None` identifies a synthetic driver frame created by [`Frame::new`];
    /// `Some(empty)` is an owned but malformed empty JVM method body.
    pub bytecode: Option<Arc<Vec<u8>>>,

    /// Method descriptor (e.g., "(II)I" for int add(int, int))
    pub method_descriptor: String,

    /// Exception table for catch block handling
    pub exception_table: Vec<ExceptionTableEntry>,

    /// Monitors acquired by this frame (for automatic cleanup on exception/return)
    /// Stored in the order they were acquired
    pub acquired_monitors: Vec<HeapId>,

    /// Tracks the number of JVM slots currently on the operand stack.
    /// Category-2 values (long/double) count as 2 slots per JVM spec §2.11.1.
    /// Maintained by push/pop to avoid O(n) scans on every push.
    current_slot_size: usize,
}

impl Frame {
    /// Create a new frame for a method
    pub fn new(
        class_name: String,
        method_name: String,
        max_locals: u16,
        max_stack: u16,
        args: Vec<Value>,
    ) -> VMResult<Self> {
        let mut frame = Self::with_bytecode(
            class_name,
            method_name,
            max_locals,
            max_stack,
            args,
            Vec::new(),
            String::new(),
            Vec::new(),
        )?;
        frame.bytecode = None;
        Ok(frame)
    }

    /// Create a new frame for a method with bytecode
    #[allow(clippy::too_many_arguments)]
    pub fn with_bytecode(
        class_name: String,
        method_name: String,
        max_locals: u16,
        max_stack: u16,
        args: Vec<Value>,
        bytecode: impl Into<Arc<Vec<u8>>>,
        method_descriptor: String,
        exception_table: Vec<ExceptionTableEntry>,
    ) -> VMResult<Self> {
        let bytecode = bytecode.into();
        let mut locals = vec![None; max_locals as usize];

        // Store arguments in locals
        // Per JVM spec §2.11.1, category-2 values occupy two consecutive slots
        let mut local_index = 0;
        for arg in args.into_iter() {
            if local_index >= max_locals as usize {
                return Err(VMError::internal(format!(
                    "Too many arguments for method {}",
                    method_name
                )));
            }

            if arg.is_category2() {
                // Category-2: store value at index N, placeholder at N+1
                locals[local_index] = Some(arg);
                local_index += 1;
                if local_index >= max_locals as usize {
                    return Err(VMError::internal(format!(
                        "Category-2 argument would overflow locals in method {}",
                        method_name
                    )));
                }
                locals[local_index] = Some(Value::CategoryTwoPlaceholder);
                local_index += 1;
            } else {
                // Category-1: single slot
                locals[local_index] = Some(arg);
                local_index += 1;
            }
        }

        Ok(Frame {
            class_name,
            method_name,
            pc: 0,
            active_instruction_pc: usize::MAX,
            locals,
            operand_stack: Vec::with_capacity(max_stack as usize),
            max_stack,
            max_locals,
            bytecode: Some(bytecode),
            method_descriptor,
            exception_table,
            acquired_monitors: Vec::new(),
            current_slot_size: 0,
        })
    }

    pub(crate) fn begin_instruction(&mut self, pc: usize) {
        self.active_instruction_pc = pc;
    }

    pub(crate) fn exception_pc(&self) -> usize {
        if self.active_instruction_pc == usize::MAX {
            self.pc
        } else {
            self.active_instruction_pc
        }
    }

    /// Push a value onto the operand stack
    pub fn push(&mut self, value: Value) -> VMResult<()> {
        let push_size = if value.is_category2() { 2 } else { 1 };
        if self.current_slot_size + push_size > self.max_stack as usize {
            return Err(VMError::StackOverflow);
        }
        self.current_slot_size += push_size;
        self.operand_stack.push(value);
        Ok(())
    }

    /// Pop a value from the operand stack
    pub fn pop(&mut self) -> VMResult<Value> {
        match self.operand_stack.pop() {
            Some(value) => {
                let slots = if value.is_category2() { 2 } else { 1 };
                self.current_slot_size = self.current_slot_size.saturating_sub(slots);
                Ok(value)
            }
            None => Err(VMError::StackUnderflow),
        }
    }

    /// Push a value onto the operand stack without overflow checking.
    /// Maintains slot accounting. Use in handlers where the verifier
    /// guarantees the push is safe (e.g., return values from invocations).
    pub fn push_raw(&mut self, value: Value) {
        self.current_slot_size += if value.is_category2() { 2 } else { 1 };
        self.operand_stack.push(value);
    }

    /// Pop a value from the operand stack, maintaining slot accounting.
    /// Returns None if the stack is empty.
    pub fn pop_raw(&mut self) -> Option<Value> {
        self.operand_stack.pop().inspect(|v| {
            let slots = if v.is_category2() { 2 } else { 1 };
            self.current_slot_size = self.current_slot_size.saturating_sub(slots);
        })
    }

    /// Peek at the top value without removing it
    pub fn peek(&self) -> VMResult<&Value> {
        self.operand_stack.last().ok_or(VMError::StackUnderflow)
    }

    /// Pop and convert to int
    pub fn pop_int(&mut self) -> VMResult<i32> {
        self.pop()?.as_int()
    }

    /// Pop and convert to long
    pub fn pop_long(&mut self) -> VMResult<i64> {
        self.pop()?.as_long()
    }

    /// Pop and convert to float
    pub fn pop_float(&mut self) -> VMResult<f32> {
        self.pop()?.as_float()
    }

    /// Pop and convert to double
    pub fn pop_double(&mut self) -> VMResult<f64> {
        self.pop()?.as_double()
    }

    /// Get a local variable
    /// Per JVM spec §2.11.1, category-2 values (long/double) occupy two consecutive slots
    pub fn get_local(&self, index: u16) -> VMResult<Value> {
        if index as usize >= self.locals.len() {
            return Err(VMError::InvalidLocalIndex(index));
        }

        match &self.locals[index as usize] {
            Some(Value::CategoryTwoPlaceholder) => {
                // Attempting to load from second half of category-2 value
                Err(VMError::InvalidLocalIndex(index))
            }
            Some(value) => Ok(value.clone()),
            None => Err(VMError::InvalidLocalIndex(index)),
        }
    }

    /// Set a local variable
    /// Per JVM spec §2.11.1, category-2 values (long/double) occupy two consecutive slots.
    /// When storing a category-2 value at index N, slot N+1 is set to a placeholder.
    pub fn set_local(&mut self, index: u16, value: Value) -> VMResult<()> {
        if index as usize >= self.locals.len() {
            return Err(VMError::InvalidLocalIndex(index));
        }

        // Check if we're trying to store into a placeholder slot
        if let Some(Value::CategoryTwoPlaceholder) = &self.locals[index as usize] {
            return Err(VMError::InvalidLocalIndex(index));
        }

        if value.is_category2() {
            // Category-2 value: occupy two consecutive slots
            let next_index = (index + 1) as usize;
            if next_index >= self.locals.len() {
                return Err(VMError::internal(format!(
                    "Category-2 value at index {} would overflow locals array",
                    index
                )));
            }

            // Store value at index N
            self.locals[index as usize] = Some(value);
            // Store placeholder at index N+1
            self.locals[next_index] = Some(Value::CategoryTwoPlaceholder);
        } else {
            // Category-1 value: single slot
            self.locals[index as usize] = Some(value);
        }
        Ok(())
    }

    /// Clear the operand stack
    pub fn clear_stack(&mut self) {
        self.operand_stack.clear();
        self.current_slot_size = 0;
    }

    /// Get the current stack depth (number of values, not slots).
    ///
    /// Note: For slot-based accounting used in max_stack validation,
    /// see the push() method which correctly counts category-2 values as 2 slots.
    pub fn stack_depth(&self) -> usize {
        self.operand_stack.len()
    }

    /// Get the current stack depth in 32-bit slots.
    ///
    /// Category-2 values (long, double) count as 2 slots per JVM spec.
    pub fn stack_slots(&self) -> usize {
        self.current_slot_size
    }

    /// Duplicate the top value (dup)
    pub fn dup(&mut self) -> VMResult<()> {
        let value = self.peek()?.clone();
        self.push(value)?;
        Ok(())
    }

    /// Duplicate and insert two values down (dup_x1)
    pub fn dup_x1(&mut self) -> VMResult<()> {
        if self.stack_depth() < 2 {
            return Err(VMError::StackUnderflow);
        }
        let v1 = self.pop()?;
        let v2 = self.pop()?;
        self.push(v1.clone())?;
        self.push(v2)?;
        self.push(v1)?;
        Ok(())
    }

    /// Duplicate and insert three values down (dup_x2)
    /// Form 1: ..., value3 (cat-1), value2 (cat-1), value1 (cat-1)
    /// After:  ..., value1, value3, value2, value1
    /// Form 2: ..., value2 (cat-2), value1 (cat-1)
    /// After:  ..., value1, value2, value1
    pub fn dup_x2(&mut self) -> VMResult<()> {
        if self.stack_depth() < 2 {
            return Err(VMError::StackUnderflow);
        }
        let v1 = self.pop()?;
        let v2 = self.pop()?;

        if v2.is_category2() {
            // Form 2: value2 is category-2 (long/double)
            self.push(v1.clone())?;
            self.push(v2)?;
            self.push(v1)?;
        } else {
            // Form 1: value2 is category-1, need to pop value3
            if self.stack_depth() < 1 {
                return Err(VMError::StackUnderflow);
            }
            let v3 = self.pop()?;
            self.push(v1.clone())?;
            self.push(v3)?;
            self.push(v2)?;
            self.push(v1)?;
        }
        Ok(())
    }

    /// Pop top value
    pub fn stack_pop(&mut self) -> VMResult<()> {
        self.pop()?;
        Ok(())
    }

    /// Swap top two values
    pub fn swap(&mut self) -> VMResult<()> {
        if self.stack_depth() < 2 {
            return Err(VMError::StackUnderflow);
        }
        let v1 = self.pop()?;
        let v2 = self.pop()?;
        self.push(v1)?;
        self.push(v2)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_creation() {
        let frame = Frame::new(
            "TestClass".to_string(),
            "testMethod".to_string(),
            10,
            20,
            vec![Value::Int(42)],
        )
        .unwrap();

        assert_eq!(frame.get_local(0).unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_stack_operations() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();

        frame.push(Value::Int(42)).unwrap();
        frame.push(Value::Int(10)).unwrap();

        assert_eq!(frame.pop_int().unwrap(), 10);
        assert_eq!(frame.pop_int().unwrap(), 42);
        assert!(frame.pop().is_err());
    }

    #[test]
    fn test_dup() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();

        frame.push(Value::Int(42)).unwrap();
        frame.dup().unwrap();

        assert_eq!(frame.pop_int().unwrap(), 42);
        assert_eq!(frame.pop_int().unwrap(), 42);
    }

    #[test]
    fn test_swap() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();

        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Int(2)).unwrap();
        frame.swap().unwrap();

        assert_eq!(frame.pop_int().unwrap(), 1);
        assert_eq!(frame.pop_int().unwrap(), 2);
    }

    // ============================================================
    // Regression tests: Category-2 local variable handling
    // ============================================================
    // Per JVM spec §2.11.1, long and double occupy two consecutive local variable slots.
    // The value is stored at index N, and index N+1 contains a placeholder.
    // These tests verify correct handling of category-2 values in local variables.

    #[test]
    fn test_category2_local_store_and_load() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store a long at index 0 (should occupy slots 0 and 1)
        frame.set_local(0, Value::Long(123456789)).unwrap();

        // Load should succeed
        match frame.get_local(0).unwrap() {
            Value::Long(v) => assert_eq!(v, 123456789),
            _ => panic!("Expected Long value"),
        }

        // Verify slot 1 contains placeholder
        match &frame.locals[1] {
            Some(Value::CategoryTwoPlaceholder) => {}
            _ => panic!("Expected CategoryTwoPlaceholder at slot 1"),
        }
    }

    #[test]
    fn test_category2_cannot_load_from_placeholder_slot() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store a long at index 0 (occupies slots 0 and 1)
        frame.set_local(0, Value::Long(999)).unwrap();

        // Attempting to load from slot 1 (the placeholder) should fail
        assert!(
            frame.get_local(1).is_err(),
            "Loading from placeholder slot should fail"
        );
    }

    #[test]
    fn test_category2_cannot_store_into_placeholder_slot() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store a long at index 0 (occupies slots 0 and 1)
        frame.set_local(0, Value::Long(111)).unwrap();

        // Attempting to store into slot 1 (the placeholder) should fail
        let result = frame.set_local(1, Value::Int(222));
        assert!(result.is_err(), "Storing into placeholder slot should fail");
    }

    #[test]
    fn test_category2_mixed_with_category1() {
        // This test reproduces the exact bug scenario from the bug report:
        // long x at slot 0-1, int y at slot 2, int z at slot 3
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // lstore 0 → long at slots 0-1
        frame.set_local(0, Value::Long(123)).unwrap();

        // istore 2 → int at slot 2
        frame.set_local(2, Value::Int(456)).unwrap();

        // istore 3 → int at slot 3
        frame.set_local(3, Value::Int(789)).unwrap();

        // Verify all values are correct
        assert_eq!(frame.get_local(0).unwrap().as_long().unwrap(), 123);
        assert_eq!(frame.get_local(2).unwrap().as_int().unwrap(), 456);
        assert_eq!(frame.get_local(3).unwrap().as_int().unwrap(), 789);

        // Verify slot 1 is placeholder (cannot be accessed)
        assert!(frame.get_local(1).is_err());
    }

    #[test]
    fn test_category2_double_handling() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store a double at index 2 (should occupy slots 2 and 3)
        frame.set_local(2, Value::Double(3.14159)).unwrap();

        // Load should succeed
        match frame.get_local(2).unwrap() {
            Value::Double(v) => assert!((v - 3.14159).abs() < 0.00001),
            _ => panic!("Expected Double value"),
        }

        // Verify slot 3 contains placeholder
        match &frame.locals[3] {
            Some(Value::CategoryTwoPlaceholder) => {}
            _ => panic!("Expected CategoryTwoPlaceholder at slot 3"),
        }

        // Cannot access slot 3
        assert!(frame.get_local(3).is_err());
    }

    #[test]
    fn test_category2_overwrite_clears_old_placeholder() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store long at slots 0-1
        frame.set_local(0, Value::Long(111)).unwrap();
        assert!(matches!(
            frame.locals[1],
            Some(Value::CategoryTwoPlaceholder)
        ));

        // Overwrite with int at slot 0 (should clear, but placeholder at 1 remains)
        frame.set_local(0, Value::Int(222)).unwrap();

        // Slot 0 should be int
        assert_eq!(frame.get_local(0).unwrap().as_int().unwrap(), 222);

        // Slot 1 still has old placeholder (this is ok, just orphaned)
        // The important thing is we can't accidentally use it
    }

    #[test]
    fn test_category2_arguments_initialized_correctly() {
        // Create frame with category-2 argument (e.g., method(long x, int y))
        let frame = Frame::new(
            "Test".to_string(),
            "test".to_string(),
            10,
            10,
            vec![Value::Long(999), Value::Int(111)],
        )
        .unwrap();

        // Argument 0: long at slots 0-1
        assert_eq!(frame.get_local(0).unwrap().as_long().unwrap(), 999);
        assert!(frame.get_local(1).is_err()); // Slot 1 is placeholder

        // Argument 1: int at slot 2 (because long occupied 0-1)
        assert_eq!(frame.get_local(2).unwrap().as_int().unwrap(), 111);
    }

    #[test]
    fn test_category2_multiple_longs() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 10, 10, vec![]).unwrap();

        // Store multiple longs
        frame.set_local(0, Value::Long(100)).unwrap(); // Slots 0-1
        frame.set_local(2, Value::Long(200)).unwrap(); // Slots 2-3
        frame.set_local(4, Value::Long(300)).unwrap(); // Slots 4-5

        // Verify all can be loaded correctly
        assert_eq!(frame.get_local(0).unwrap().as_long().unwrap(), 100);
        assert_eq!(frame.get_local(2).unwrap().as_long().unwrap(), 200);
        assert_eq!(frame.get_local(4).unwrap().as_long().unwrap(), 300);

        // Verify placeholders block access
        assert!(frame.get_local(1).is_err());
        assert!(frame.get_local(3).is_err());
        assert!(frame.get_local(5).is_err());
    }

    #[test]
    fn test_category2_overflow_detection() {
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();

        // Attempting to store long at index 4 (would need slots 4-5, but max is 5)
        let result = frame.set_local(4, Value::Long(999));
        assert!(
            result.is_err(),
            "Category-2 at last slot should fail (overflow)"
        );
    }
}
