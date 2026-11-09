// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bitwise opcode handlers
//!
//! Handles JVM bytecode opcodes for bitwise operations:
//! - Integer shifts: ishl, ishr, iushr
//! - Integer bitwise: iand, ior, ixor
//! - Long shifts: lshl, lshr, lushr
//! - Long bitwise: land, lor, lxor

use crate::interpreter::errors::VMResult;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;

// =============================================================================
// Integer Shift Operations
// =============================================================================

/// Handle ishl (0x78) - Shift int left
/// Per JVM spec: Uses only low 5 bits of shift amount (0x1f mask)
#[inline]
pub fn handle_ishl(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x1f;
    let value = frame.pop()?.as_int()?;
    frame.push(Value::Int(value.wrapping_shl(shift as u32)))
}

/// Handle ishr (0x7a) - Arithmetic shift int right
/// Per JVM spec: Uses only low 5 bits of shift amount (0x1f mask)
#[inline]
pub fn handle_ishr(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x1f;
    let value = frame.pop()?.as_int()?;
    frame.push(Value::Int(value.wrapping_shr(shift as u32)))
}

/// Handle iushr (0x7c) - Logical shift int right (unsigned)
/// Per JVM spec: Uses only low 5 bits of shift amount (0x1f mask)
/// Treats the value as unsigned for the shift
#[inline]
pub fn handle_iushr(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x1f;
    let value = frame.pop()?.as_int()?;
    frame.push(Value::Int(
        ((value as u32).wrapping_shr(shift as u32)) as i32,
    ))
}

// =============================================================================
// Integer Bitwise Operations
// =============================================================================

/// Handle iand (0x7e) - Bitwise AND of two ints
#[inline]
pub fn handle_iand(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a & b))
}

/// Handle ior (0x80) - Bitwise OR of two ints
#[inline]
pub fn handle_ior(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a | b))
}

/// Handle ixor (0x82) - Bitwise XOR of two ints
#[inline]
pub fn handle_ixor(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a ^ b))
}

// =============================================================================
// Long Shift Operations
// =============================================================================

/// Handle lshl (0x79) - Shift long left
/// Per JVM spec: Uses only low 6 bits of shift amount (0x3f mask)
#[inline]
pub fn handle_lshl(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x3f;
    let value = frame.pop()?.as_long()?;
    frame.push(Value::Long(value.wrapping_shl(shift as u32)))
}

/// Handle lshr (0x7b) - Arithmetic shift long right
/// Per JVM spec: Uses only low 6 bits of shift amount (0x3f mask)
#[inline]
pub fn handle_lshr(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x3f;
    let value = frame.pop()?.as_long()?;
    frame.push(Value::Long(value.wrapping_shr(shift as u32)))
}

/// Handle lushr (0x7d) - Logical shift long right (unsigned)
/// Per JVM spec: Uses only low 6 bits of shift amount (0x3f mask)
/// Treats the value as unsigned for the shift
#[inline]
pub fn handle_lushr(frame: &mut Frame) -> VMResult<()> {
    let shift = frame.pop()?.as_int()? & 0x3f;
    let value = frame.pop()?.as_long()?;
    frame.push(Value::Long(
        ((value as u64).wrapping_shr(shift as u32)) as i64,
    ))
}

// =============================================================================
// Long Bitwise Operations
// =============================================================================

/// Handle land (0x7f) - Bitwise AND of two longs
#[inline]
pub fn handle_land(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a & b))
}

/// Handle lor (0x81) - Bitwise OR of two longs
#[inline]
pub fn handle_lor(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a | b))
}

/// Handle lxor (0x83) - Bitwise XOR of two longs
#[inline]
pub fn handle_lxor(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a ^ b))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Integer shift tests
    #[test]
    fn test_ishl() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Int(4)).unwrap();
        handle_ishl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 16); // 1 << 4 = 16
    }

    #[test]
    fn test_ishl_mask() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Int(36)).unwrap(); // 36 & 0x1f = 4
        handle_ishl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 16); // 1 << 4 = 16
    }

    #[test]
    fn test_ishr() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(16)).unwrap();
        frame.push(Value::Int(2)).unwrap();
        handle_ishr(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 4); // 16 >> 2 = 4
    }

    #[test]
    fn test_ishr_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-16)).unwrap();
        frame.push(Value::Int(2)).unwrap();
        handle_ishr(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -4); // Sign preserved
    }

    #[test]
    fn test_iushr_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-1)).unwrap();
        frame.push(Value::Int(1)).unwrap();
        handle_iushr(&mut frame).unwrap();
        // -1 as unsigned = 0xFFFFFFFF, >> 1 = 0x7FFFFFFF = 2147483647
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), i32::MAX);
    }

    // Integer bitwise tests
    #[test]
    fn test_iand() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0b1100)).unwrap();
        frame.push(Value::Int(0b1010)).unwrap();
        handle_iand(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0b1000);
    }

    #[test]
    fn test_ior() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0b1100)).unwrap();
        frame.push(Value::Int(0b1010)).unwrap();
        handle_ior(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0b1110);
    }

    #[test]
    fn test_ixor() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(0b1100)).unwrap();
        frame.push(Value::Int(0b1010)).unwrap();
        handle_ixor(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0b0110);
    }

    // Long shift tests
    #[test]
    fn test_lshl() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(1)).unwrap();
        frame.push(Value::Int(40)).unwrap();
        handle_lshl(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            1i64 << 40 // 1099511627776
        );
    }

    #[test]
    fn test_lshl_mask() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(1)).unwrap();
        frame.push(Value::Int(68)).unwrap(); // 68 & 0x3f = 4
        handle_lshl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 16); // 1 << 4 = 16
    }

    // Long bitwise tests
    #[test]
    fn test_land() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(0xFF00)).unwrap();
        frame.push(Value::Long(0x0FF0)).unwrap();
        handle_land(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 0x0F00);
    }

    #[test]
    fn test_lor() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(0xFF00)).unwrap();
        frame.push(Value::Long(0x00FF)).unwrap();
        handle_lor(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 0xFFFF);
    }

    #[test]
    fn test_lxor() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(0xFFFF)).unwrap();
        frame.push(Value::Long(0xF0F0)).unwrap();
        handle_lxor(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 0x0F0F);
    }
}
