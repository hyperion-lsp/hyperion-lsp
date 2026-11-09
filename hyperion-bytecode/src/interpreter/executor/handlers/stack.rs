// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stack manipulation opcode handlers
//!
//! Handles JVM bytecode opcodes for stack manipulation:
//! - pop (0x57): Pop top value
//! - pop2 (0x58): Pop top one or two values (depending on category)
//! - dup (0x59): Duplicate top value
//! - dup_x1 (0x5a): Duplicate and insert one down
//! - dup_x2 (0x5b): Duplicate and insert two or three down
//! - dup2 (0x5c): Duplicate top one or two values
//! - dup2_x1 (0x5d): Duplicate top one or two values and insert down
//! - dup2_x2 (0x5e): Most complex duplication
//! - swap (0x5f): Swap top two values

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::frame::Frame;

/// Handle pop (0x57) - Pop the top operand stack value
#[inline]
pub fn handle_pop(frame: &mut Frame) -> VMResult<()> {
    frame.pop()?;
    Ok(())
}

/// Handle pop2 (0x58) - Pop the top one or two operand stack values
///
/// If top value is category-2 (long/double), pop one value.
/// If top value is category-1, pop two values.
#[inline]
pub fn handle_pop2(frame: &mut Frame) -> VMResult<()> {
    let val = frame.pop()?;
    if !val.is_category2() {
        // Two category-1 values
        frame.pop()?;
    }
    Ok(())
}

/// Handle dup (0x59) - Duplicate the top operand stack value
#[inline]
pub fn handle_dup(frame: &mut Frame) -> VMResult<()> {
    let val = frame.peek()?.clone();
    frame.push(val)
}

/// Handle dup_x1 (0x5a) - Duplicate value1 (cat-1), insert under value2 (cat-1)
///
/// Before: ..., value2, value1
/// After:  ..., value1, value2, value1
#[inline]
pub fn handle_dup_x1(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    let val2 = frame.pop()?;
    frame.push(val1.clone())?;
    frame.push(val2)?;
    frame.push(val1)
}

/// Handle dup_x2 (0x5b) - Duplicate value1, insert 2 or 3 positions down
///
/// Form 1: ..., value3 (cat-1), value2 (cat-1), value1 (cat-1)
/// After:  ..., value1, value3, value2, value1
///
/// Form 2: ..., value2 (cat-2), value1 (cat-1)
/// After:  ..., value1, value2, value1
#[inline]
pub fn handle_dup_x2(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    let val2 = frame.pop()?;

    if val2.is_category2() {
        // Form 2: value2 is category-2
        frame.push(val1.clone())?;
        frame.push(val2)?;
        frame.push(val1)
    } else {
        // Form 1: value2 is category-1, need to pop value3
        let val3 = frame.pop()?;
        frame.push(val1.clone())?;
        frame.push(val3)?;
        frame.push(val2)?;
        frame.push(val1)
    }
}

/// Handle dup2 (0x5c) - Duplicate top 2 category-1 values OR top 1 category-2 value
///
/// Form 1: ..., value2 (cat-1), value1 (cat-1)
/// After:  ..., value2, value1, value2, value1
///
/// Form 2: ..., value (cat-2)
/// After:  ..., value, value
#[inline]
pub fn handle_dup2(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    if val1.is_category2() {
        // Form 2: Single category-2 value
        frame.push(val1.clone())?;
        frame.push(val1)
    } else {
        // Form 1: Two category-1 values
        let val2 = frame.pop()?;
        frame.push(val2.clone())?;
        frame.push(val1.clone())?;
        frame.push(val2)?;
        frame.push(val1)
    }
}

/// Handle dup2_x1 (0x5d) - Duplicate value1 & value2, insert 3 or 2 positions down
///
/// Form 1: ..., value3 (cat-1), value2 (cat-1), value1 (cat-1)
/// After:  ..., value2, value1, value3, value2, value1
///
/// Form 2: ..., value2 (cat-1), value1 (cat-2)
/// After:  ..., value1, value2, value1
#[inline]
pub fn handle_dup2_x1(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    if val1.is_category2() {
        // Form 2: value1 is category-2, value2 is category-1
        let val2 = frame.pop()?;
        frame.push(val1.clone())?;
        frame.push(val2)?;
        frame.push(val1)
    } else {
        // Form 1: Both are category-1
        let val2 = frame.pop()?;
        let val3 = frame.pop()?;
        frame.push(val2.clone())?;
        frame.push(val1.clone())?;
        frame.push(val3)?;
        frame.push(val2)?;
        frame.push(val1)
    }
}

/// Handle dup2_x2 (0x5e) - Most complex - 4 different forms based on categories
///
/// Form 1: ..., value4, value3, value2, value1 -> ..., value2, value1, value4, value3, value2, value1
///         (all category-1)
///
/// Form 2: ..., value3, value2, value1 -> ..., value1, value3, value2, value1
///         (value1 cat-2, value2 and value3 cat-1)
///
/// Form 3: ..., value3, value2, value1 -> ..., value2, value1, value3, value2, value1
///         (value1 and value2 cat-1, value3 cat-2)
///
/// Form 4: ..., value2, value1 -> ..., value1, value2, value1
///         (both cat-2)
#[inline]
pub fn handle_dup2_x2(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    let is_val1_cat2 = val1.is_category2();

    if is_val1_cat2 {
        // Could be form 2 or 4
        let val2 = frame.pop()?;
        if val2.is_category2() {
            // Form 4: Both category-2
            // ..., value2, value1 -> ..., value1, value2, value1
            frame.push(val1.clone())?;
            frame.push(val2)?;
            frame.push(val1)
        } else {
            // Form 2: value1 cat-2, value2 cat-1, value3 cat-1
            // ..., value3, value2, value1 -> ..., value1, value3, value2, value1
            let val3 = frame.pop()?;
            frame.push(val1.clone())?;
            frame.push(val3)?;
            frame.push(val2)?;
            frame.push(val1)
        }
    } else {
        // val1 is category-1, could be form 1 or 3
        let val2 = frame.pop()?;
        if val2.is_category2() {
            // This case (val1 cat-1, val2 cat-2) is INVALID according to JVM spec!
            // No valid dup2_x2 form has this configuration.
            return Err(VMError::InternalError(
                "Invalid operand types for dup2_x2: value1 is category-1, value2 is category-2"
                    .to_string(),
            ));
        }
        // Both val1 and val2 are category-1
        // Could be Form 1 (all cat-1) or Form 3 (value3 is cat-2)
        let val3 = frame.pop()?;
        if val3.is_category2() {
            // Form 3: value1 cat-1, value2 cat-1, value3 cat-2
            // ..., value3, value2, value1 -> ..., value2, value1, value3, value2, value1
            frame.push(val2.clone())?;
            frame.push(val1.clone())?;
            frame.push(val3)?;
            frame.push(val2)?;
            frame.push(val1)
        } else {
            // Form 1: All category-1 (need val4 too)
            // ..., value4, value3, value2, value1 -> ..., value2, value1, value4, value3, value2, value1
            let val4 = frame.pop()?;
            frame.push(val2.clone())?;
            frame.push(val1.clone())?;
            frame.push(val4)?;
            frame.push(val3)?;
            frame.push(val2)?;
            frame.push(val1)
        }
    }
}

/// Handle swap (0x5f) - Exchange top two stack values
///
/// Per JVM spec, both values must be category-1 computational types
/// (int, float, reference, returnAddress). Category-2 values (long, double) are invalid.
#[inline]
pub fn handle_swap(frame: &mut Frame) -> VMResult<()> {
    let val1 = frame.pop()?;
    let val2 = frame.pop()?;

    // Validate both values are category-1
    if val1.is_category2() || val2.is_category2() {
        return Err(VMError::InvalidBytecode);
    }

    frame.push(val1)?;
    frame.push(val2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::value::Value;

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
    fn test_pop() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_pop(&mut frame).unwrap();
        assert!(frame.operand_stack.is_empty());
    }

    #[test]
    fn test_pop2_category1() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Int(2)).unwrap();
        handle_pop2(&mut frame).unwrap();
        assert!(frame.operand_stack.is_empty());
    }

    #[test]
    fn test_pop2_category2() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(42)).unwrap();
        handle_pop2(&mut frame).unwrap();
        assert!(frame.operand_stack.is_empty());
    }

    #[test]
    fn test_dup() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_dup(&mut frame).unwrap();
        assert_eq!(frame.operand_stack.len(), 2);
        let val1 = frame.pop().unwrap();
        let val2 = frame.pop().unwrap();
        assert!(matches!(val1, Value::Int(42)));
        assert!(matches!(val2, Value::Int(42)));
    }

    #[test]
    fn test_dup_x1() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap(); // value2
        frame.push(Value::Int(2)).unwrap(); // value1
        handle_dup_x1(&mut frame).unwrap();
        // After: value1, value2, value1
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 2);
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 2);
    }

    #[test]
    fn test_swap() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Int(2)).unwrap();
        handle_swap(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 1);
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 2);
    }

    #[test]
    fn test_swap_rejects_category2() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(1)).unwrap();
        frame.push(Value::Long(2)).unwrap();
        let result = handle_swap(&mut frame);
        assert!(result.is_err());
    }
}
