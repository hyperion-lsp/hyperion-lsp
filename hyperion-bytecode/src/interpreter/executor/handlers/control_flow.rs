// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Control flow opcode handlers
//!
//! Handles JVM bytecode opcodes for control flow:
//! - Zero comparisons: ifeq, ifne, iflt, ifge, ifgt, ifle
//! - Int comparisons: if_icmpeq, if_icmpne, if_icmplt, if_icmpge, if_icmpgt, if_icmple
//! - Reference comparisons: if_acmpeq, if_acmpne, ifnull, ifnonnull
//! - Unconditional: goto
//! - Switch: lookupswitch (sparse case matching)

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::frame::Frame;
use crate::interpreter::value::ObjectRef;

// =============================================================================
// Zero Comparison Branches
// =============================================================================

/// Handle ifeq (0x99) - Branch if int equals zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifeq(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value == 0)
}

/// Handle ifne (0x9a) - Branch if int not equals zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifne(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value != 0)
}

/// Handle iflt (0x9b) - Branch if int less than zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_iflt(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value < 0)
}

/// Handle ifge (0x9c) - Branch if int greater than or equals zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifge(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value >= 0)
}

/// Handle ifgt (0x9d) - Branch if int greater than zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifgt(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value > 0)
}

/// Handle ifle (0x9e) - Branch if int less than or equals zero
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifle(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?.as_int()?;
    Ok(value <= 0)
}

// =============================================================================
// Int Comparison Branches
// =============================================================================

/// Handle if_icmpeq (0x9f) - Branch if int1 equals int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmpeq(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 == v2)
}

/// Handle if_icmpne (0xa0) - Branch if int1 not equals int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmpne(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 != v2)
}

/// Handle if_icmplt (0xa1) - Branch if int1 less than int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmplt(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 < v2)
}

/// Handle if_icmpge (0xa2) - Branch if int1 greater than or equals int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmpge(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 >= v2)
}

/// Handle if_icmpgt (0xa3) - Branch if int1 greater than int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmpgt(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 > v2)
}

/// Handle if_icmple (0xa4) - Branch if int1 less than or equals int2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_icmple(frame: &mut Frame) -> VMResult<bool> {
    let v2 = frame.pop()?.as_int()?;
    let v1 = frame.pop()?.as_int()?;
    Ok(v1 <= v2)
}

// =============================================================================
// Reference Comparison Branches
// =============================================================================

/// Handle if_acmpeq (0xa5) - Branch if ref1 equals ref2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_acmpeq(frame: &mut Frame) -> VMResult<bool> {
    let ref2 = frame.pop()?.as_reference()?;
    let ref1 = frame.pop()?.as_reference()?;
    Ok(ref1 == ref2)
}

/// Handle if_acmpne (0xa6) - Branch if ref1 not equals ref2
/// Returns true if branch should be taken
#[inline]
pub fn handle_if_acmpne(frame: &mut Frame) -> VMResult<bool> {
    let ref2 = frame.pop()?.as_reference()?;
    let ref1 = frame.pop()?.as_reference()?;
    Ok(ref1 != ref2)
}

/// Handle ifnull (0xc6) - Branch if reference is null
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifnull(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?;
    let is_null = matches!(
        value,
        crate::interpreter::value::Value::Reference(ObjectRef::Null)
    );
    Ok(is_null)
}

/// Handle ifnonnull (0xc7) - Branch if reference is not null
/// Returns true if branch should be taken
#[inline]
pub fn handle_ifnonnull(frame: &mut Frame) -> VMResult<bool> {
    let value = frame.pop()?;
    let is_null = matches!(
        value,
        crate::interpreter::value::Value::Reference(ObjectRef::Null)
    );
    Ok(!is_null)
}

// =============================================================================
// Switch Instructions
// =============================================================================

/// Handle lookupswitch (0xab) - Access jump table by key match and jump
///
/// JVM Spec: lookupswitch performs a key match to determine the target offset.
/// Match-offset pairs MUST be sorted by key in increasing order.
///
/// # Bytecode Format
/// ```text
/// lookupswitch
/// <0-3 byte padding>
/// defaultbyte1 defaultbyte2 defaultbyte3 defaultbyte4
/// npairs1 npairs2 npairs3 npairs4
/// match-offset pairs...
/// ```
///
/// # Arguments
/// * `frame` - Current frame to pop key from
/// * `bytecode` - Full bytecode array
/// * `pc` - Mutable program counter (will be updated to skip padding and data)
/// * `instruction_start` - PC of the lookupswitch opcode (for offset calculation)
///
/// # Returns
/// Target PC to jump to
pub fn handle_lookupswitch(
    frame: &mut Frame,
    bytecode: &[u8],
    pc: &mut usize,
    instruction_start: usize,
) -> VMResult<usize> {
    // Pop the int key to match against
    let key = frame.pop()?.as_int()?;

    // Calculate padding to align to 4-byte boundary after the opcode
    let aligned_pc = (instruction_start + 1 + 3) & !3;
    *pc = aligned_pc;

    // Read default offset (4 bytes, signed)
    if *pc + 3 >= bytecode.len() {
        return Err(VMError::InvalidBytecode);
    }
    let default_offset = i32::from_be_bytes([
        bytecode[*pc],
        bytecode[*pc + 1],
        bytecode[*pc + 2],
        bytecode[*pc + 3],
    ]);
    *pc += 4;

    // Read npairs (4 bytes, unsigned)
    if *pc + 3 >= bytecode.len() {
        return Err(VMError::InvalidBytecode);
    }
    let npairs = u32::from_be_bytes([
        bytecode[*pc],
        bytecode[*pc + 1],
        bytecode[*pc + 2],
        bytecode[*pc + 3],
    ]) as usize;
    *pc += 4;

    // Binary search through sorted match-offset pairs
    // Since pairs are sorted by match value, we can use binary search for O(log n)
    let pairs_start = *pc;
    let mut offset = default_offset;
    let mut left = 0;
    let mut right = npairs;

    while left < right {
        let mid = left + (right - left) / 2;
        let pair_offset = pairs_start + (mid * 8); // Each pair is 8 bytes (4 + 4)

        // Bounds check
        if pair_offset + 7 >= bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }

        // Read match value at mid position
        let match_value = i32::from_be_bytes([
            bytecode[pair_offset],
            bytecode[pair_offset + 1],
            bytecode[pair_offset + 2],
            bytecode[pair_offset + 3],
        ]);

        if match_value == key {
            // Found exact match - read its offset
            offset = i32::from_be_bytes([
                bytecode[pair_offset + 4],
                bytecode[pair_offset + 5],
                bytecode[pair_offset + 6],
                bytecode[pair_offset + 7],
            ]);
            break;
        } else if match_value < key {
            left = mid + 1;
        } else {
            right = mid;
        }
    }

    // Update PC to skip all pairs
    *pc = pairs_start + (npairs * 8);

    // Calculate target PC (offset is relative to instruction start)
    let target = (instruction_start as i32 + offset) as usize;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::value::{HeapId, Value};

    fn create_test_frame() -> Frame {
        Frame::new(
            "TestClass".to_string(),
            "testMethod".to_string(),
            10,
            10,
            vec![],
        )
        .unwrap()
    }

    // Zero comparison tests
    #[test]
    fn test_ifeq_zero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0)).unwrap();
        assert!(handle_ifeq(&mut frame).unwrap());
    }

    #[test]
    fn test_ifeq_nonzero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        assert!(!handle_ifeq(&mut frame).unwrap());
    }

    #[test]
    fn test_ifne_zero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0)).unwrap();
        assert!(!handle_ifne(&mut frame).unwrap());
    }

    #[test]
    fn test_ifne_nonzero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        assert!(handle_ifne(&mut frame).unwrap());
    }

    #[test]
    fn test_iflt_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-5)).unwrap();
        assert!(handle_iflt(&mut frame).unwrap());
    }

    #[test]
    fn test_iflt_positive() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        assert!(!handle_iflt(&mut frame).unwrap());
    }

    #[test]
    fn test_ifge_zero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0)).unwrap();
        assert!(handle_ifge(&mut frame).unwrap());
    }

    #[test]
    fn test_ifge_positive() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        assert!(handle_ifge(&mut frame).unwrap());
    }

    #[test]
    fn test_ifge_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-5)).unwrap();
        assert!(!handle_ifge(&mut frame).unwrap());
    }

    // Int comparison tests
    #[test]
    fn test_if_icmpeq_equal() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        frame.push(Value::Int(42)).unwrap();
        assert!(handle_if_icmpeq(&mut frame).unwrap());
    }

    #[test]
    fn test_if_icmpeq_not_equal() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        frame.push(Value::Int(43)).unwrap();
        assert!(!handle_if_icmpeq(&mut frame).unwrap());
    }

    #[test]
    fn test_if_icmplt() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        frame.push(Value::Int(10)).unwrap();
        assert!(handle_if_icmplt(&mut frame).unwrap());
    }

    // Reference comparison tests
    #[test]
    fn test_ifnull_is_null() {
        let mut frame = create_test_frame();
        frame.push(Value::Reference(ObjectRef::Null)).unwrap();
        assert!(handle_ifnull(&mut frame).unwrap());
    }

    #[test]
    fn test_ifnull_not_null() {
        let mut frame = create_test_frame();
        frame
            .push(Value::Reference(ObjectRef::Object(HeapId(1))))
            .unwrap();
        assert!(!handle_ifnull(&mut frame).unwrap());
    }

    #[test]
    fn test_ifnonnull_is_null() {
        let mut frame = create_test_frame();
        frame.push(Value::Reference(ObjectRef::Null)).unwrap();
        assert!(!handle_ifnonnull(&mut frame).unwrap());
    }

    #[test]
    fn test_ifnonnull_not_null() {
        let mut frame = create_test_frame();
        frame
            .push(Value::Reference(ObjectRef::Object(HeapId(1))))
            .unwrap();
        assert!(handle_ifnonnull(&mut frame).unwrap());
    }

    // Lookupswitch tests
    #[test]
    fn test_lookupswitch_empty_pairs_uses_default() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();

        // Bytecode: lookupswitch at offset 0
        // 0: lookupswitch opcode
        // 1-3: padding to align to 4
        // 4-7: default offset = +10
        // 8-11: npairs = 0
        let bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0x0a, // default = +10
            0x00, 0x00, 0x00, 0x00, // npairs = 0
        ];

        let mut pc = 1; // After reading opcode
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 10); // Should jump to default (0 + 10)
        assert_eq!(pc, 12); // PC should be after all data
    }

    #[test]
    fn test_lookupswitch_binary_search_first_element() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(10)).unwrap();

        let bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0x14, // default = +20
            0x00, 0x00, 0x00, 0x04, // npairs = 4
            // Pair 0: match=10, offset=100
            0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x64, // Pair 1: match=20, offset=200
            0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0xc8, // Pair 2: match=30, offset=300
            0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x01, 0x2c, // Pair 3: match=40, offset=400
            0x00, 0x00, 0x00, 0x28, 0x00, 0x00, 0x01, 0x90,
        ];

        let mut pc = 1;
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 100); // Should match first pair
    }

    #[test]
    fn test_lookupswitch_binary_search_last_element() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(40)).unwrap();

        let bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0x14, // default = +20
            0x00, 0x00, 0x00, 0x04, // npairs = 4
            // Pairs: 10, 20, 30, 40
            0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00,
            0x00, 0xc8, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x01, 0x2c, 0x00, 0x00, 0x00, 0x28,
            0x00, 0x00, 0x01, 0x90,
        ];

        let mut pc = 1;
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 400); // Should match last pair
    }

    #[test]
    fn test_lookupswitch_binary_search_middle_element() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(20)).unwrap();

        let bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0x14, // default = +20
            0x00, 0x00, 0x00, 0x04, // npairs = 4
            // Pairs: 10, 20, 30, 40
            0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00,
            0x00, 0xc8, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x01, 0x2c, 0x00, 0x00, 0x00, 0x28,
            0x00, 0x00, 0x01, 0x90,
        ];

        let mut pc = 1;
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 200); // Should match second pair
    }

    #[test]
    fn test_lookupswitch_binary_search_not_found() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(15)).unwrap(); // Between 10 and 20, won't match

        let bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0x14, // default = +20
            0x00, 0x00, 0x00, 0x04, // npairs = 4
            // Pairs: 10, 20, 30, 40
            0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00,
            0x00, 0xc8, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x00, 0x01, 0x2c, 0x00, 0x00, 0x00, 0x28,
            0x00, 0x00, 0x01, 0x90,
        ];

        let mut pc = 1;
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 20); // Should use default
    }

    #[test]
    fn test_lookupswitch_large_npairs_binary_search() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(500)).unwrap();

        // Build bytecode with 10 pairs to ensure binary search is used
        let mut bytecode = vec![
            0xab, 0x00, 0x00, 0x00, // opcode + padding
            0x00, 0x00, 0x00, 0xff, // default = +255
            0x00, 0x00, 0x00, 0x0a, // npairs = 10
        ];

        // Add 10 sorted pairs: (100, 1000), (200, 2000), ..., (1000, 10000)
        for i in 1..=10 {
            let match_val: i32 = i * 100;
            let offset_val: i32 = i * 1000;
            bytecode.extend_from_slice(&match_val.to_be_bytes());
            bytecode.extend_from_slice(&offset_val.to_be_bytes());
        }

        let mut pc = 1;
        let target = handle_lookupswitch(&mut frame, &bytecode, &mut pc, 0).unwrap();

        assert_eq!(target, 5000); // Should match (500, 5000)
    }
}
