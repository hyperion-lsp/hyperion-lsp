// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comparison opcode handlers
//!
//! Handles JVM bytecode opcodes for numeric comparisons:
//! - lcmp (0x94): Compare two longs
//! - fcmpl (0x95): Compare two floats, -1 on NaN
//! - fcmpg (0x96): Compare two floats, +1 on NaN
//! - dcmpl (0x97): Compare two doubles, -1 on NaN
//! - dcmpg (0x98): Compare two doubles, +1 on NaN

use crate::interpreter::errors::VMResult;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;

/// Handle lcmp (0x94) - Compare two longs
///
/// Pops two longs, pushes:
/// - 1 if a > b
/// - 0 if a == b
/// - -1 if a < b
#[inline]
pub fn handle_lcmp(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    let cmp = if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    };
    frame.push(Value::Int(cmp))
}

/// Handle fcmpl (0x95) - Compare two floats, -1 on NaN
///
/// Pops two floats, pushes:
/// - 1 if a > b
/// - 0 if a == b
/// - -1 if a < b OR either is NaN
#[inline]
pub fn handle_fcmpl(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    let cmp = if a.is_nan() || b.is_nan() || a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    };
    frame.push(Value::Int(cmp))
}

/// Handle fcmpg (0x96) - Compare two floats, +1 on NaN
///
/// Pops two floats, pushes:
/// - 1 if a > b OR either is NaN
/// - 0 if a == b
/// - -1 if a < b
#[inline]
pub fn handle_fcmpg(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    let cmp = if a.is_nan() || b.is_nan() {
        1
    } else if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    };
    frame.push(Value::Int(cmp))
}

/// Handle dcmpl (0x97) - Compare two doubles, -1 on NaN
///
/// Pops two doubles, pushes:
/// - 1 if a > b
/// - 0 if a == b
/// - -1 if a < b OR either is NaN
#[inline]
pub fn handle_dcmpl(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    let cmp = if a.is_nan() || b.is_nan() || a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    };
    frame.push(Value::Int(cmp))
}

/// Handle dcmpg (0x98) - Compare two doubles, +1 on NaN
///
/// Pops two doubles, pushes:
/// - 1 if a > b OR either is NaN
/// - 0 if a == b
/// - -1 if a < b
#[inline]
pub fn handle_dcmpg(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    let cmp = if a.is_nan() || b.is_nan() {
        1
    } else if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    };
    frame.push(Value::Int(cmp))
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

    // Long comparison tests
    #[test]
    fn test_lcmp_greater() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(100)).unwrap();
        frame.push(Value::Long(50)).unwrap();
        handle_lcmp(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_lcmp_less() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(50)).unwrap();
        frame.push(Value::Long(100)).unwrap();
        handle_lcmp(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }

    #[test]
    fn test_lcmp_equal() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(42)).unwrap();
        frame.push(Value::Long(42)).unwrap();
        handle_lcmp(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0);
    }

    // Float comparison tests
    #[test]
    fn test_fcmpl_greater() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(3.14)).unwrap();
        frame.push(Value::Float(2.0)).unwrap();
        handle_fcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_fcmpl_less() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(1.0)).unwrap();
        frame.push(Value::Float(2.0)).unwrap();
        handle_fcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }

    #[test]
    fn test_fcmpl_equal() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(2.5)).unwrap();
        frame.push(Value::Float(2.5)).unwrap();
        handle_fcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0);
    }

    #[test]
    fn test_fcmpl_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NAN)).unwrap();
        frame.push(Value::Float(1.0)).unwrap();
        handle_fcmpl(&mut frame).unwrap();
        // fcmpl returns -1 on NaN
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }

    #[test]
    fn test_fcmpg_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NAN)).unwrap();
        frame.push(Value::Float(1.0)).unwrap();
        handle_fcmpg(&mut frame).unwrap();
        // fcmpg returns +1 on NaN
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_fcmpg_greater() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(5.0)).unwrap();
        frame.push(Value::Float(3.0)).unwrap();
        handle_fcmpg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    // Double comparison tests
    #[test]
    fn test_dcmpl_greater() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(3.14159)).unwrap();
        frame.push(Value::Double(2.71828)).unwrap();
        handle_dcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_dcmpl_less() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.0)).unwrap();
        frame.push(Value::Double(2.0)).unwrap();
        handle_dcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }

    #[test]
    fn test_dcmpl_equal() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(2.5)).unwrap();
        frame.push(Value::Double(2.5)).unwrap();
        handle_dcmpl(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0);
    }

    #[test]
    fn test_dcmpl_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::NAN)).unwrap();
        frame.push(Value::Double(1.0)).unwrap();
        handle_dcmpl(&mut frame).unwrap();
        // dcmpl returns -1 on NaN
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }

    #[test]
    fn test_dcmpg_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.0)).unwrap();
        frame.push(Value::Double(f64::NAN)).unwrap();
        handle_dcmpg(&mut frame).unwrap();
        // dcmpg returns +1 on NaN
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_dcmpg_less() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.0)).unwrap();
        frame.push(Value::Double(5.0)).unwrap();
        handle_dcmpg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -1);
    }
}
