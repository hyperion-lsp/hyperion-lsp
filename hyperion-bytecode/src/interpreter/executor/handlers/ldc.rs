// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LDC (load constant) handlers for the Executor
//!
//! Handles Ldc, LdcW, and Ldc2W opcodes which load constants from the constant pool.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::Executor;
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::Heap;
use crate::interpreter::value::Value;
use crate::producer::ConstantPoolItem;

impl Executor {
    /// Execute LDC: Load constant from constant pool (1-byte index)
    /// Stack: ... -> ..., value
    /// Can push: int, float, string reference, class reference
    pub(crate) fn execute_ldc(&mut self, frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
        if self.pc >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index = self.bytecode[self.pc] as usize;
        self.pc += 1;

        let value = self.resolve_ldc_value(cp_index, heap)?;
        frame.push(value)?;
        Ok(())
    }

    /// Execute LDC_W: Load constant from constant pool (2-byte index)
    /// Stack: ... -> ..., value
    /// Same as LDC but uses 16-bit index instead of 8-bit
    pub(crate) fn execute_ldc_w(&mut self, frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        let value = self.resolve_ldc_value(cp_index, heap)?;
        frame.push(value)?;
        Ok(())
    }

    /// Execute LDC2_W: Load long or double from constant pool (2-byte index)
    /// Stack: ... -> ..., value (long/double takes 2 stack slots)
    pub(crate) fn execute_ldc2_w(&mut self, frame: &mut Frame) -> VMResult<()> {
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Get constant pool
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or_else(|| VMError::internal("Constant pool not available"))?;

        // Validate index (0 is reserved, indices are 1-based)
        if cp_index == 0 || cp_index > cp.len() {
            return Err(VMError::InvalidConstantPoolIndex(cp_index as u16));
        }

        // LDC2_W can only load Long or Double (category-2 values, parser has placeholder at 0)
        let value = match &cp[cp_index] {
            ConstantPoolItem::Long(l) => Value::Long(*l),
            ConstantPoolItem::Double(d) => Value::Double(*d),
            _ => {
                return Err(VMError::InvalidBytecode);
            }
        };

        frame.push(value)?;
        Ok(())
    }

    /// Shared logic for resolving LDC/LDC_W constant pool entries to values.
    fn resolve_ldc_value(&self, cp_index: usize, heap: &mut Heap) -> VMResult<Value> {
        // Get constant pool
        let cp = self
            .constant_pool
            .as_ref()
            .ok_or_else(|| VMError::internal("Constant pool not available"))?;

        // Validate index (0 is reserved, indices are 1-based)
        if cp_index == 0 || cp_index > cp.len() {
            return Err(VMError::InvalidConstantPoolIndex(cp_index as u16));
        }

        // Convert constant pool entry to value (parser has placeholder at index 0)
        match &cp[cp_index] {
            ConstantPoolItem::Integer(i) => Ok(Value::Int(*i)),
            ConstantPoolItem::Float(f) => Ok(Value::Float(*f)),
            ConstantPoolItem::String { string_index } => {
                // String references need to be resolved and interned on the heap
                let string_value = self.resolve_utf8(*string_index as usize)?;
                let obj_ref = heap.intern_string(&string_value)?;
                Ok(Value::Reference(obj_ref))
            }
            ConstantPoolItem::Class { name_index } => {
                // Resolve the class name from the name_index (UTF-8 entry)
                let class_name = self.resolve_utf8(*name_index as usize)?;
                // Create or retrieve Class object from heap cache
                let obj_ref = heap.intern_class(&class_name)?;
                Ok(Value::Reference(obj_ref))
            }
            _ => Err(VMError::InvalidBytecode),
        }
    }
}
