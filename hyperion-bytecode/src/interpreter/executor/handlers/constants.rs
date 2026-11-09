// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constant opcode handlers
//!
//! Handles JVM bytecode opcodes that push constant values onto the operand stack:
//! - nop (0x00): No operation
//! - aconst_null (0x01): Push null reference
//! - iconst_m1 through iconst_5 (0x02-0x08): Push integer constants -1 to 5
//! - lconst_0, lconst_1 (0x09-0x0a): Push long constants 0 and 1
//! - fconst_0, fconst_1, fconst_2 (0x0b-0x0d): Push float constants 0.0, 1.0, 2.0
//! - dconst_0, dconst_1 (0x0e-0x0f): Push double constants 0.0 and 1.0

use crate::interpreter::errors::VMResult;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::{ObjectRef, Value};

/// Handle nop (0x00) - No operation
#[inline]
pub fn handle_nop() {
    // No operation - intentionally empty
}

/// Handle aconst_null (0x01) - Push null reference
#[inline]
pub fn handle_aconst_null(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Reference(ObjectRef::Null))
}

/// Handle iconst_m1 (0x02) - Push int constant -1
#[inline]
pub fn handle_iconst_m1(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(-1))
}

/// Handle iconst_0 (0x03) - Push int constant 0
#[inline]
pub fn handle_iconst_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(0))
}

/// Handle iconst_1 (0x04) - Push int constant 1
#[inline]
pub fn handle_iconst_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(1))
}

/// Handle iconst_2 (0x05) - Push int constant 2
#[inline]
pub fn handle_iconst_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(2))
}

/// Handle iconst_3 (0x06) - Push int constant 3
#[inline]
pub fn handle_iconst_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(3))
}

/// Handle iconst_4 (0x07) - Push int constant 4
#[inline]
pub fn handle_iconst_4(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(4))
}

/// Handle iconst_5 (0x08) - Push int constant 5
#[inline]
pub fn handle_iconst_5(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Int(5))
}

/// Handle lconst_0 (0x09) - Push long constant 0
#[inline]
pub fn handle_lconst_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Long(0))
}

/// Handle lconst_1 (0x0a) - Push long constant 1
#[inline]
pub fn handle_lconst_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Long(1))
}

/// Handle fconst_0 (0x0b) - Push float constant 0.0
#[inline]
pub fn handle_fconst_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Float(0.0))
}

/// Handle fconst_1 (0x0c) - Push float constant 1.0
#[inline]
pub fn handle_fconst_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Float(1.0))
}

/// Handle fconst_2 (0x0d) - Push float constant 2.0
#[inline]
pub fn handle_fconst_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Float(2.0))
}

/// Handle dconst_0 (0x0e) - Push double constant 0.0
#[inline]
pub fn handle_dconst_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Double(0.0))
}

/// Handle dconst_1 (0x0f) - Push double constant 1.0
#[inline]
pub fn handle_dconst_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(Value::Double(1.0))
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

    #[test]
    fn test_nop() {
        handle_nop();
        // Just verifies it doesn't panic
    }

    #[test]
    fn test_aconst_null() {
        let mut frame = create_test_frame();
        handle_aconst_null(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Reference(ObjectRef::Null)));
    }

    #[test]
    fn test_iconst_m1() {
        let mut frame = create_test_frame();
        handle_iconst_m1(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Int(-1)));
    }

    #[test]
    fn test_iconst_0() {
        let mut frame = create_test_frame();
        handle_iconst_0(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Int(0)));
    }

    #[test]
    fn test_iconst_5() {
        let mut frame = create_test_frame();
        handle_iconst_5(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Int(5)));
    }

    #[test]
    fn test_lconst_0() {
        let mut frame = create_test_frame();
        handle_lconst_0(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Long(0)));
    }

    #[test]
    fn test_lconst_1() {
        let mut frame = create_test_frame();
        handle_lconst_1(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Long(1)));
    }

    #[test]
    fn test_fconst_0() {
        let mut frame = create_test_frame();
        handle_fconst_0(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Float(f) if f == 0.0));
    }

    #[test]
    fn test_fconst_2() {
        let mut frame = create_test_frame();
        handle_fconst_2(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Float(f) if f == 2.0));
    }

    #[test]
    fn test_dconst_0() {
        let mut frame = create_test_frame();
        handle_dconst_0(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Double(d) if d == 0.0));
    }

    #[test]
    fn test_dconst_1() {
        let mut frame = create_test_frame();
        handle_dconst_1(&mut frame).unwrap();
        let val = frame.pop().unwrap();
        assert!(matches!(val, Value::Double(d) if d == 1.0));
    }
}
