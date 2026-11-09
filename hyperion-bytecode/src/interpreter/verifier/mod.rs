// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bytecode verification
//!
//! This module implements bytecode verification per JVMS Chapter 4.10.
//! It validates that bytecode is structurally correct before execution.
//!
//! The verifier currently performs structural verification:
//!    - Instruction encodings are valid
//!    - Branch targets are within bounds
//!    - Exception table PC ranges are valid
//!
//! It does not verify stack maps or JVM type safety. Passing this verifier is
//! therefore not equivalent to passing a conforming JVM verifier.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::Opcode;
use crate::interpreter::frame::ExceptionTableEntry;

/// Bytecode verifier for JVM method code
pub struct BytecodeVerifier<'a> {
    /// The bytecode to verify
    code: &'a [u8],
    /// Exception table entries
    exception_table: &'a [ExceptionTableEntry],
}

/// Result of bytecode verification
#[derive(Debug)]
pub struct VerificationResult {
    /// Whether verification passed
    pub valid: bool,
    /// List of errors found (empty if valid)
    pub errors: Vec<VerificationError>,
}

/// A single verification error
#[derive(Debug, Clone)]
pub struct VerificationError {
    /// Byte offset where the error was detected
    pub offset: usize,
    /// Description of the error
    pub message: String,
}

impl<'a> BytecodeVerifier<'a> {
    /// Create a new verifier for the given bytecode
    pub fn new(code: &'a [u8], exception_table: &'a [ExceptionTableEntry]) -> Self {
        Self {
            code,
            exception_table,
        }
    }

    /// Verify the bytecode and return the result
    pub fn verify(&self) -> VerificationResult {
        let mut errors = Vec::new();

        // Structural verification is the complete contract of this verifier.
        self.verify_instruction_structure(&mut errors);
        self.verify_exception_table(&mut errors);

        VerificationResult {
            valid: errors.is_empty(),
            errors,
        }
    }

    /// Verify that all instructions are structurally valid
    fn verify_instruction_structure(&self, errors: &mut Vec<VerificationError>) {
        let mut pc = 0;

        while pc < self.code.len() {
            let opcode_byte = self.code[pc];

            // Try to decode the opcode
            let opcode = match Opcode::from_u8(opcode_byte) {
                Some(op) => op,
                None => {
                    errors.push(VerificationError {
                        offset: pc,
                        message: format!("Invalid opcode: 0x{:02x}", opcode_byte),
                    });
                    pc += 1;
                    continue;
                }
            };

            // Get instruction size and validate operands
            match self.validate_instruction(pc, opcode) {
                Ok(size) => {
                    pc += size;
                }
                Err(e) => {
                    errors.push(e);
                    // Skip to next byte and continue checking
                    pc += 1;
                }
            }
        }
    }

    /// Validate a single instruction and return its size
    fn validate_instruction(&self, pc: usize, opcode: Opcode) -> Result<usize, VerificationError> {
        let size = opcode.size();

        // Check instruction doesn't extend past end of bytecode
        if pc + size > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: format!(
                    "Truncated instruction {:?}: needs {} bytes but only {} available",
                    opcode,
                    size,
                    self.code.len() - pc
                ),
            });
        }

        // Validate branch targets for control flow instructions
        match opcode {
            // 2-byte branch offset instructions
            Opcode::Ifeq
            | Opcode::Ifne
            | Opcode::Iflt
            | Opcode::Ifge
            | Opcode::Ifgt
            | Opcode::Ifle
            | Opcode::IfIcmpeq
            | Opcode::IfIcmpne
            | Opcode::IfIcmplt
            | Opcode::IfIcmpge
            | Opcode::IfIcmpgt
            | Opcode::IfIcmple
            | Opcode::IfAcmpeq
            | Opcode::IfAcmpne
            | Opcode::Goto
            | Opcode::Ifnull
            | Opcode::Ifnonnull => {
                self.validate_branch_offset(pc, 2)?;
            }

            // 4-byte branch offset instructions (jsr/jsr_w deprecated since JDK 7)
            Opcode::GotoW => {
                self.validate_branch_offset_wide(pc)?;
            }

            // Tableswitch requires special validation
            Opcode::Tableswitch => {
                self.validate_tableswitch(pc)?;
            }

            // Lookupswitch requires special validation
            Opcode::Lookupswitch => {
                self.validate_lookupswitch(pc)?;
            }

            _ => {}
        }

        Ok(size)
    }

    /// Validate a 2-byte branch offset
    fn validate_branch_offset(
        &self,
        pc: usize,
        offset_size: usize,
    ) -> Result<(), VerificationError> {
        if pc + 1 + offset_size > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: "Truncated branch offset".to_string(),
            });
        }

        let offset = if offset_size == 2 {
            i16::from_be_bytes([self.code[pc + 1], self.code[pc + 2]]) as i32
        } else {
            i32::from_be_bytes([
                self.code[pc + 1],
                self.code[pc + 2],
                self.code[pc + 3],
                self.code[pc + 4],
            ])
        };

        let target = pc as i32 + offset;

        if target < 0 || target as usize >= self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: format!(
                    "Branch target {} out of bounds (code length: {})",
                    target,
                    self.code.len()
                ),
            });
        }

        Ok(())
    }

    /// Validate a 4-byte wide branch offset
    fn validate_branch_offset_wide(&self, pc: usize) -> Result<(), VerificationError> {
        self.validate_branch_offset(pc, 4)
    }

    /// Validate tableswitch instruction
    fn validate_tableswitch(&self, pc: usize) -> Result<(), VerificationError> {
        // Calculate padding to 4-byte boundary
        let padding = (4 - ((pc + 1) % 4)) % 4;
        let base = pc + 1 + padding;

        // Check we have room for default, low, high (3 x 4 bytes = 12)
        if base + 12 > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: "Truncated tableswitch instruction".to_string(),
            });
        }

        // Read default offset
        let default_offset = i32::from_be_bytes([
            self.code[base],
            self.code[base + 1],
            self.code[base + 2],
            self.code[base + 3],
        ]);

        // Validate default target
        let default_target = pc as i32 + default_offset;
        if default_target < 0 || default_target as usize >= self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: format!(
                    "tableswitch default target {} out of bounds",
                    default_target
                ),
            });
        }

        // Read low and high
        let low = i32::from_be_bytes([
            self.code[base + 4],
            self.code[base + 5],
            self.code[base + 6],
            self.code[base + 7],
        ]);
        let high = i32::from_be_bytes([
            self.code[base + 8],
            self.code[base + 9],
            self.code[base + 10],
            self.code[base + 11],
        ]);

        if high < low {
            return Err(VerificationError {
                offset: pc,
                message: format!("tableswitch high ({}) < low ({})", high, low),
            });
        }

        let num_offsets = (high - low + 1) as usize;
        let jump_table_start = base + 12;

        // Check we have room for all jump offsets
        if jump_table_start + num_offsets * 4 > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: "Truncated tableswitch jump table".to_string(),
            });
        }

        // Validate each jump target
        for i in 0..num_offsets {
            let offset_pos = jump_table_start + i * 4;
            let offset = i32::from_be_bytes([
                self.code[offset_pos],
                self.code[offset_pos + 1],
                self.code[offset_pos + 2],
                self.code[offset_pos + 3],
            ]);
            let target = pc as i32 + offset;

            if target < 0 || target as usize >= self.code.len() {
                return Err(VerificationError {
                    offset: pc,
                    message: format!(
                        "tableswitch case {} target {} out of bounds",
                        low + i as i32,
                        target
                    ),
                });
            }
        }

        Ok(())
    }

    /// Validate lookupswitch instruction
    fn validate_lookupswitch(&self, pc: usize) -> Result<(), VerificationError> {
        // Calculate padding to 4-byte boundary
        let padding = (4 - ((pc + 1) % 4)) % 4;
        let base = pc + 1 + padding;

        // Check we have room for default and npairs (2 x 4 bytes = 8)
        if base + 8 > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: "Truncated lookupswitch instruction".to_string(),
            });
        }

        // Read default offset
        let default_offset = i32::from_be_bytes([
            self.code[base],
            self.code[base + 1],
            self.code[base + 2],
            self.code[base + 3],
        ]);

        // Validate default target
        let default_target = pc as i32 + default_offset;
        if default_target < 0 || default_target as usize >= self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: format!(
                    "lookupswitch default target {} out of bounds",
                    default_target
                ),
            });
        }

        // Read npairs
        let npairs = i32::from_be_bytes([
            self.code[base + 4],
            self.code[base + 5],
            self.code[base + 6],
            self.code[base + 7],
        ]) as usize;

        let pairs_start = base + 8;

        // Check we have room for all pairs (8 bytes each: match + offset)
        if pairs_start + npairs * 8 > self.code.len() {
            return Err(VerificationError {
                offset: pc,
                message: "Truncated lookupswitch match-offset pairs".to_string(),
            });
        }

        // Validate each jump target and check match values are sorted
        let mut last_match: Option<i32> = None;
        for i in 0..npairs {
            let pair_pos = pairs_start + i * 8;
            let match_val = i32::from_be_bytes([
                self.code[pair_pos],
                self.code[pair_pos + 1],
                self.code[pair_pos + 2],
                self.code[pair_pos + 3],
            ]);

            // Check match values are sorted (JVMS 4.9.1)
            if let Some(last) = last_match {
                if match_val <= last {
                    return Err(VerificationError {
                        offset: pc,
                        message: format!(
                            "lookupswitch match values not sorted: {} after {}",
                            match_val, last
                        ),
                    });
                }
            }
            last_match = Some(match_val);

            let offset = i32::from_be_bytes([
                self.code[pair_pos + 4],
                self.code[pair_pos + 5],
                self.code[pair_pos + 6],
                self.code[pair_pos + 7],
            ]);
            let target = pc as i32 + offset;

            if target < 0 || target as usize >= self.code.len() {
                return Err(VerificationError {
                    offset: pc,
                    message: format!(
                        "lookupswitch case {} target {} out of bounds",
                        match_val, target
                    ),
                });
            }
        }

        Ok(())
    }

    /// Verify exception table entries
    fn verify_exception_table(&self, errors: &mut Vec<VerificationError>) {
        for (i, entry) in self.exception_table.iter().enumerate() {
            let start_pc = entry.start_pc as usize;
            let end_pc = entry.end_pc as usize;
            let handler_pc = entry.handler_pc as usize;

            // Validate start_pc
            if start_pc >= self.code.len() {
                errors.push(VerificationError {
                    offset: 0,
                    message: format!(
                        "Exception handler {} start_pc {} out of bounds (code length: {})",
                        i,
                        start_pc,
                        self.code.len()
                    ),
                });
            }

            // Validate end_pc (exclusive, so can equal code.len())
            if end_pc > self.code.len() {
                errors.push(VerificationError {
                    offset: 0,
                    message: format!(
                        "Exception handler {} end_pc {} out of bounds (code length: {})",
                        i,
                        end_pc,
                        self.code.len()
                    ),
                });
            }

            // Validate start_pc < end_pc
            if start_pc >= end_pc {
                errors.push(VerificationError {
                    offset: 0,
                    message: format!(
                        "Exception handler {} has invalid range: start_pc {} >= end_pc {}",
                        i, start_pc, end_pc
                    ),
                });
            }

            // Validate handler_pc
            if handler_pc >= self.code.len() {
                errors.push(VerificationError {
                    offset: 0,
                    message: format!(
                        "Exception handler {} handler_pc {} out of bounds (code length: {})",
                        i,
                        handler_pc,
                        self.code.len()
                    ),
                });
            }
        }
    }
}

/// Quick verification function that returns VMResult
pub fn verify_bytecode(code: &[u8], exception_table: &[ExceptionTableEntry]) -> VMResult<()> {
    let verifier = BytecodeVerifier::new(code, exception_table);
    let result = verifier.verify();

    if result.valid {
        Ok(())
    } else {
        let error_msg = result
            .errors
            .iter()
            .map(|e| format!("@{}: {}", e.offset, e.message))
            .collect::<Vec<_>>()
            .join("; ");
        Err(VMError::VerifyError(error_msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_simple_bytecode() {
        // iconst_0, ireturn
        let code = vec![0x03, 0xac];
        let result = BytecodeVerifier::new(&code, &[]).verify();
        assert!(result.valid, "Expected valid: {:?}", result.errors);
    }

    #[test]
    fn test_invalid_opcode() {
        // 0xff is invalid in current JVM
        let code = vec![0xff];
        let result = BytecodeVerifier::new(&code, &[]).verify();
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Invalid opcode"));
    }

    #[test]
    fn test_truncated_instruction() {
        // sipush needs 2 operand bytes
        let code = vec![0x11, 0x00]; // Missing second byte
        let result = BytecodeVerifier::new(&code, &[]).verify();
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("Truncated"));
    }

    #[test]
    fn test_valid_branch() {
        // goto +3 (to position 5), nop, nop, nop, return
        let code = vec![0xa7, 0x00, 0x05, 0x00, 0x00, 0xb1];
        let result = BytecodeVerifier::new(&code, &[]).verify();
        assert!(result.valid, "Expected valid: {:?}", result.errors);
    }

    #[test]
    fn test_invalid_branch_target() {
        // goto +100 (way past end)
        let code = vec![0xa7, 0x00, 0x64, 0xb1];
        let result = BytecodeVerifier::new(&code, &[]).verify();
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("out of bounds"));
    }

    #[test]
    fn test_exception_table_valid() {
        let code = vec![0x03, 0x03, 0x03, 0xac]; // 4 bytes
        let exception_table = vec![ExceptionTableEntry {
            start_pc: 0,
            end_pc: 3,
            handler_pc: 3,
            catch_type: None,
        }];
        let result = BytecodeVerifier::new(&code, &exception_table).verify();
        assert!(result.valid, "Expected valid: {:?}", result.errors);
    }

    #[test]
    fn test_exception_table_invalid_range() {
        let code = vec![0x03, 0xac]; // 2 bytes
        let exception_table = vec![ExceptionTableEntry {
            start_pc: 1,
            end_pc: 1, // Invalid: start >= end
            handler_pc: 0,
            catch_type: None,
        }];
        let result = BytecodeVerifier::new(&code, &exception_table).verify();
        assert!(!result.valid);
        assert!(result.errors[0].message.contains("invalid range"));
    }
}
