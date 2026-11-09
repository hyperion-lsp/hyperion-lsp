// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Wide instruction handler for the Executor
//!
//! Handles the Wide prefix opcode (JVMS 6.5.wide) which modifies another
//! instruction to use a 16-bit index instead of 8-bit for local variable access.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::Executor;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;

impl Executor {
    /// Execute the Wide prefix instruction.
    ///
    /// Format: wide <opcode> indexbyte1 indexbyte2 [constbyte1 constbyte2]
    /// Modifies iload, lload, fload, dload, aload, istore, lstore, fstore,
    /// dstore, astore, iinc, and ret to use 16-bit local variable indices.
    pub(crate) fn execute_wide(&mut self, frame: &mut Frame) -> VMResult<()> {
        if self.pc >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let modified_opcode = self.bytecode[self.pc];
        self.pc += 1;

        // Read wide index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let index = u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]);
        self.pc += 2;

        match modified_opcode {
            // Wide loads
            0x15 => frame.push(frame.get_local(index)?)?, // iload
            0x16 => frame.push(frame.get_local(index)?)?, // lload
            0x17 => frame.push(frame.get_local(index)?)?, // fload
            0x18 => frame.push(frame.get_local(index)?)?, // dload
            0x19 => frame.push(frame.get_local(index)?)?, // aload

            // Wide stores
            0x36 => {
                // istore
                let val = frame.pop()?;
                frame.set_local(index, val)?;
            }
            0x37 => {
                // lstore
                let val = frame.pop()?;
                frame.set_local(index, val)?;
            }
            0x38 => {
                // fstore
                let val = frame.pop()?;
                frame.set_local(index, val)?;
            }
            0x39 => {
                // dstore
                let val = frame.pop()?;
                frame.set_local(index, val)?;
            }
            0x3a => {
                // astore
                let val = frame.pop()?;
                frame.set_local(index, val)?;
            }

            // Wide iinc: index (2 bytes) + const (2 bytes, signed)
            0x84 => {
                if self.pc + 1 >= self.bytecode.len() {
                    return Err(VMError::InvalidBytecode);
                }
                let const_val =
                    i16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as i32;
                self.pc += 2;

                let current = frame.get_local(index)?.as_int()?;
                frame.set_local(index, Value::Int(current.wrapping_add(const_val)))?;
            }

            // Wide ret (for jsr/ret - rarely used, but spec-compliant)
            0xa9 => {
                let target = frame.get_local(index)?.as_int()? as usize;
                self.pc = target;
            }

            _ => {
                return Err(VMError::ClassFormatError(format!(
                    "invalid wide opcode modifier: 0x{:02x}",
                    modified_opcode
                )));
            }
        }
        Ok(())
    }
}
