// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Load and store opcode handlers
//!
//! Handles JVM bytecode opcodes for local variable access:
//! - Indexed loads: iload_0-3, lload_0-3, fload_0-3, dload_0-3, aload_0-3
//! - Indexed stores: istore_0-3, lstore_0-3, fstore_0-3, dstore_0-3, astore_0-3
//!
//! Note: Generic load/store opcodes (iload, aload, etc.) require bytecode access
//! for the index operand, so they remain in the executor.

use crate::interpreter::errors::VMResult;
use crate::interpreter::frame::Frame;

// =============================================================================
// Integer Loads (iload_0 through iload_3)
// =============================================================================

/// Handle iload_0 (0x1a) - Load int from local variable 0
#[inline]
pub fn handle_iload_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(0)?)
}

/// Handle iload_1 (0x1b) - Load int from local variable 1
#[inline]
pub fn handle_iload_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(1)?)
}

/// Handle iload_2 (0x1c) - Load int from local variable 2
#[inline]
pub fn handle_iload_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(2)?)
}

/// Handle iload_3 (0x1d) - Load int from local variable 3
#[inline]
pub fn handle_iload_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(3)?)
}

// =============================================================================
// Long Loads (lload_0 through lload_3)
// =============================================================================

/// Handle lload_0 (0x1e) - Load long from local variable 0
#[inline]
pub fn handle_lload_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(0)?)
}

/// Handle lload_1 (0x1f) - Load long from local variable 1
#[inline]
pub fn handle_lload_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(1)?)
}

/// Handle lload_2 (0x20) - Load long from local variable 2
#[inline]
pub fn handle_lload_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(2)?)
}

/// Handle lload_3 (0x21) - Load long from local variable 3
#[inline]
pub fn handle_lload_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(3)?)
}

// =============================================================================
// Float Loads (fload_0 through fload_3)
// =============================================================================

/// Handle fload_0 (0x22) - Load float from local variable 0
#[inline]
pub fn handle_fload_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(0)?)
}

/// Handle fload_1 (0x23) - Load float from local variable 1
#[inline]
pub fn handle_fload_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(1)?)
}

/// Handle fload_2 (0x24) - Load float from local variable 2
#[inline]
pub fn handle_fload_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(2)?)
}

/// Handle fload_3 (0x25) - Load float from local variable 3
#[inline]
pub fn handle_fload_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(3)?)
}

// =============================================================================
// Double Loads (dload_0 through dload_3)
// =============================================================================

/// Handle dload_0 (0x26) - Load double from local variable 0
#[inline]
pub fn handle_dload_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(0)?)
}

/// Handle dload_1 (0x27) - Load double from local variable 1
#[inline]
pub fn handle_dload_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(1)?)
}

/// Handle dload_2 (0x28) - Load double from local variable 2
#[inline]
pub fn handle_dload_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(2)?)
}

/// Handle dload_3 (0x29) - Load double from local variable 3
#[inline]
pub fn handle_dload_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(3)?)
}

// =============================================================================
// Reference Loads (aload_0 through aload_3)
// =============================================================================

/// Handle aload_0 (0x2a) - Load reference from local variable 0
#[inline]
pub fn handle_aload_0(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(0)?)
}

/// Handle aload_1 (0x2b) - Load reference from local variable 1
#[inline]
pub fn handle_aload_1(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(1)?)
}

/// Handle aload_2 (0x2c) - Load reference from local variable 2
#[inline]
pub fn handle_aload_2(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(2)?)
}

/// Handle aload_3 (0x2d) - Load reference from local variable 3
#[inline]
pub fn handle_aload_3(frame: &mut Frame) -> VMResult<()> {
    frame.push(frame.get_local(3)?)
}

// =============================================================================
// Integer Stores (istore_0 through istore_3)
// =============================================================================

/// Handle istore_0 (0x3b) - Store int to local variable 0
#[inline]
pub fn handle_istore_0(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(0, value)
}

/// Handle istore_1 (0x3c) - Store int to local variable 1
#[inline]
pub fn handle_istore_1(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(1, value)
}

/// Handle istore_2 (0x3d) - Store int to local variable 2
#[inline]
pub fn handle_istore_2(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(2, value)
}

/// Handle istore_3 (0x3e) - Store int to local variable 3
#[inline]
pub fn handle_istore_3(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(3, value)
}

// =============================================================================
// Long Stores (lstore_0 through lstore_3)
// =============================================================================

/// Handle lstore_0 (0x3f) - Store long to local variable 0
#[inline]
pub fn handle_lstore_0(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(0, value)
}

/// Handle lstore_1 (0x40) - Store long to local variable 1
#[inline]
pub fn handle_lstore_1(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(1, value)
}

/// Handle lstore_2 (0x41) - Store long to local variable 2
#[inline]
pub fn handle_lstore_2(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(2, value)
}

/// Handle lstore_3 (0x42) - Store long to local variable 3
#[inline]
pub fn handle_lstore_3(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(3, value)
}

// =============================================================================
// Float Stores (fstore_0 through fstore_3)
// =============================================================================

/// Handle fstore_0 (0x43) - Store float to local variable 0
#[inline]
pub fn handle_fstore_0(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(0, value)
}

/// Handle fstore_1 (0x44) - Store float to local variable 1
#[inline]
pub fn handle_fstore_1(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(1, value)
}

/// Handle fstore_2 (0x45) - Store float to local variable 2
#[inline]
pub fn handle_fstore_2(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(2, value)
}

/// Handle fstore_3 (0x46) - Store float to local variable 3
#[inline]
pub fn handle_fstore_3(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(3, value)
}

// =============================================================================
// Double Stores (dstore_0 through dstore_3)
// =============================================================================

/// Handle dstore_0 (0x47) - Store double to local variable 0
#[inline]
pub fn handle_dstore_0(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(0, value)
}

/// Handle dstore_1 (0x48) - Store double to local variable 1
#[inline]
pub fn handle_dstore_1(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(1, value)
}

/// Handle dstore_2 (0x49) - Store double to local variable 2
#[inline]
pub fn handle_dstore_2(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(2, value)
}

/// Handle dstore_3 (0x4a) - Store double to local variable 3
#[inline]
pub fn handle_dstore_3(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(3, value)
}

// =============================================================================
// Reference Stores (astore_0 through astore_3)
// =============================================================================

/// Handle astore_0 (0x4b) - Store reference to local variable 0
#[inline]
pub fn handle_astore_0(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(0, value)
}

/// Handle astore_1 (0x4c) - Store reference to local variable 1
#[inline]
pub fn handle_astore_1(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(1, value)
}

/// Handle astore_2 (0x4d) - Store reference to local variable 2
#[inline]
pub fn handle_astore_2(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(2, value)
}

/// Handle astore_3 (0x4e) - Store reference to local variable 3
#[inline]
pub fn handle_astore_3(frame: &mut Frame) -> VMResult<()> {
    let value = frame.pop()?;
    frame.set_local(3, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::value::{ObjectRef, Value};

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

    // Load tests
    #[test]
    fn test_iload_0() {
        let mut frame = create_test_frame();
        frame.set_local(0, Value::Int(42)).unwrap();
        handle_iload_0(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_iload_3() {
        let mut frame = create_test_frame();
        frame.set_local(3, Value::Int(99)).unwrap();
        handle_iload_3(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 99);
    }

    #[test]
    fn test_lload_0() {
        let mut frame = create_test_frame();
        frame.set_local(0, Value::Long(1000)).unwrap();
        handle_lload_0(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 1000);
    }

    #[test]
    fn test_fload_1() {
        let mut frame = create_test_frame();
        frame.set_local(1, Value::Float(3.14)).unwrap();
        handle_fload_1(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 3.14);
    }

    #[test]
    fn test_dload_2() {
        let mut frame = create_test_frame();
        frame.set_local(2, Value::Double(2.71828)).unwrap();
        handle_dload_2(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 2.71828);
    }

    #[test]
    fn test_aload_0() {
        let mut frame = create_test_frame();
        frame
            .set_local(0, Value::Reference(ObjectRef::Null))
            .unwrap();
        handle_aload_0(&mut frame).unwrap();
        assert!(matches!(
            frame.pop().unwrap(),
            Value::Reference(ObjectRef::Null)
        ));
    }

    // Store tests
    #[test]
    fn test_istore_0() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_istore_0(&mut frame).unwrap();
        assert_eq!(frame.get_local(0).unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn test_istore_3() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(99)).unwrap();
        handle_istore_3(&mut frame).unwrap();
        assert_eq!(frame.get_local(3).unwrap().as_int().unwrap(), 99);
    }

    #[test]
    fn test_lstore_1() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(5000)).unwrap();
        handle_lstore_1(&mut frame).unwrap();
        assert_eq!(frame.get_local(1).unwrap().as_long().unwrap(), 5000);
    }

    #[test]
    fn test_fstore_2() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(1.5)).unwrap();
        handle_fstore_2(&mut frame).unwrap();
        assert_eq!(frame.get_local(2).unwrap().as_float().unwrap(), 1.5);
    }

    #[test]
    fn test_dstore_3() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(9.99)).unwrap();
        handle_dstore_3(&mut frame).unwrap();
        assert_eq!(frame.get_local(3).unwrap().as_double().unwrap(), 9.99);
    }

    #[test]
    fn test_astore_0() {
        let mut frame = create_test_frame();
        frame.push(Value::Reference(ObjectRef::Null)).unwrap();
        handle_astore_0(&mut frame).unwrap();
        assert!(matches!(
            frame.get_local(0).unwrap(),
            Value::Reference(ObjectRef::Null)
        ));
    }

    // Round-trip tests
    #[test]
    fn test_store_load_roundtrip() {
        let mut frame = create_test_frame();

        // Store value
        frame.push(Value::Int(123)).unwrap();
        handle_istore_2(&mut frame).unwrap();

        // Load it back
        handle_iload_2(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 123);
    }
}
