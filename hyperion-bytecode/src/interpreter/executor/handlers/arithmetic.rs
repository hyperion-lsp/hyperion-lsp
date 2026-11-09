// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Arithmetic opcode handlers
//!
//! Handles JVM bytecode opcodes for arithmetic operations:
//! - Integer: iadd, isub, imul, idiv, irem, ineg (0x60-0x64, 0x68-0x6c, 0x70, 0x74)
//! - Long: ladd, lsub, lmul, ldiv, lrem, lneg (0x61-0x65, 0x69-0x6d, 0x71, 0x75)
//! - Float: fadd, fsub, fmul, fdiv, frem, fneg (0x62-0x66, 0x6a-0x6e, 0x72, 0x76)
//! - Double: dadd, dsub, dmul, ddiv, drem, dneg (0x63-0x67, 0x6b-0x6f, 0x73, 0x77)

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;

// =============================================================================
// Integer Arithmetic
// =============================================================================

/// Handle iadd (0x60) - Add two integers
#[inline]
pub fn handle_iadd(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a.wrapping_add(b)))
}

/// Handle isub (0x64) - Subtract two integers
#[inline]
pub fn handle_isub(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a.wrapping_sub(b)))
}

/// Handle imul (0x68) - Multiply two integers
#[inline]
pub fn handle_imul(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a.wrapping_mul(b)))
}

/// Handle idiv (0x6c) - Divide two integers
///
/// Per JVM spec 2.11.1: Integer.MIN_VALUE / -1 = Integer.MIN_VALUE (wrapping)
#[inline]
pub fn handle_idiv(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    if b == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    frame.push(Value::Int(a.wrapping_div(b)))
}

/// Handle irem (0x70) - Remainder of two integers
///
/// Per JVM spec 2.11.1: Integer.MIN_VALUE % -1 = 0 (wrapping)
#[inline]
pub fn handle_irem(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_int()?;
    let a = frame.pop()?.as_int()?;
    if b == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    frame.push(Value::Int(a.wrapping_rem(b)))
}

/// Handle ineg (0x74) - Negate integer
#[inline]
pub fn handle_ineg(frame: &mut Frame) -> VMResult<()> {
    let a = frame.pop()?.as_int()?;
    frame.push(Value::Int(a.wrapping_neg()))
}

// =============================================================================
// Long Arithmetic
// =============================================================================

/// Handle ladd (0x61) - Add two longs
#[inline]
pub fn handle_ladd(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a.wrapping_add(b)))
}

/// Handle lsub (0x65) - Subtract two longs
#[inline]
pub fn handle_lsub(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a.wrapping_sub(b)))
}

/// Handle lmul (0x69) - Multiply two longs
#[inline]
pub fn handle_lmul(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a.wrapping_mul(b)))
}

/// Handle ldiv (0x6d) - Divide two longs
///
/// Per JVM spec 2.11.1: Long.MIN_VALUE / -1 = Long.MIN_VALUE (wrapping)
#[inline]
pub fn handle_ldiv(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    if b == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    frame.push(Value::Long(a.wrapping_div(b)))
}

/// Handle lrem (0x71) - Remainder of two longs
///
/// Per JVM spec 2.11.1: Long.MIN_VALUE % -1 = 0 (wrapping)
#[inline]
pub fn handle_lrem(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_long()?;
    let a = frame.pop()?.as_long()?;
    if b == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    frame.push(Value::Long(a.wrapping_rem(b)))
}

/// Handle lneg (0x75) - Negate long
#[inline]
pub fn handle_lneg(frame: &mut Frame) -> VMResult<()> {
    let a = frame.pop()?.as_long()?;
    frame.push(Value::Long(a.wrapping_neg()))
}

// =============================================================================
// Float Arithmetic
// =============================================================================

/// Handle fadd (0x62) - Add two floats
#[inline]
pub fn handle_fadd(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(a + b))
}

/// Handle fsub (0x66) - Subtract two floats
#[inline]
pub fn handle_fsub(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(a - b))
}

/// Handle fmul (0x6a) - Multiply two floats
#[inline]
pub fn handle_fmul(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(a * b))
}

/// Handle fdiv (0x6e) - Divide two floats
///
/// Per JVM spec: Division by zero produces Infinity or NaN, not an exception.
#[inline]
pub fn handle_fdiv(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(a / b))
}

/// Handle frem (0x72) - Remainder of two floats
///
/// Per JVMS 2.11.1: IEEE 754 remainder operation (fmod semantics).
/// Rust's `%` operator for f32/f64 follows IEEE 754 fmod, which is correct for JVM:
/// - Result = dividend - (trunc(dividend/divisor) * divisor)
/// - Special cases per IEEE 754:
///   - x % 0.0 = NaN
///   - ±INFINITY % y = NaN
///   - x % ±INFINITY = x (when x is finite)
///   - ±0.0 % y = ±0.0 (when y is finite and non-zero)
#[inline]
pub fn handle_frem(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_float()?;
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(a % b))
}

/// Handle fneg (0x76) - Negate float
#[inline]
pub fn handle_fneg(frame: &mut Frame) -> VMResult<()> {
    let a = frame.pop()?.as_float()?;
    frame.push(Value::Float(-a))
}

// =============================================================================
// Double Arithmetic
// =============================================================================

/// Handle dadd (0x63) - Add two doubles
#[inline]
pub fn handle_dadd(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(a + b))
}

/// Handle dsub (0x67) - Subtract two doubles
#[inline]
pub fn handle_dsub(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(a - b))
}

/// Handle dmul (0x6b) - Multiply two doubles
#[inline]
pub fn handle_dmul(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(a * b))
}

/// Handle ddiv (0x6f) - Divide two doubles
///
/// Per JVM spec: Division by zero produces Infinity or NaN, not an exception.
#[inline]
pub fn handle_ddiv(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(a / b))
}

/// Handle drem (0x73) - Remainder of two doubles
///
/// Per JVMS 2.11.1: IEEE 754 remainder operation (fmod semantics).
/// Rust's `%` operator for f32/f64 follows IEEE 754 fmod, which is correct for JVM:
/// - Result = dividend - (trunc(dividend/divisor) * divisor)
/// - Special cases per IEEE 754:
///   - x % 0.0 = NaN
///   - ±INFINITY % y = NaN
///   - x % ±INFINITY = x (when x is finite)
///   - ±0.0 % y = ±0.0 (when y is finite and non-zero)
#[inline]
pub fn handle_drem(frame: &mut Frame) -> VMResult<()> {
    let b = frame.pop()?.as_double()?;
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(a % b))
}

/// Handle dneg (0x77) - Negate double
#[inline]
pub fn handle_dneg(frame: &mut Frame) -> VMResult<()> {
    let a = frame.pop()?.as_double()?;
    frame.push(Value::Double(-a))
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

    // Integer tests
    #[test]
    fn test_iadd() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(10)).unwrap();
        frame.push(Value::Int(20)).unwrap();
        handle_iadd(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 30);
    }

    #[test]
    fn test_isub() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(30)).unwrap();
        frame.push(Value::Int(10)).unwrap();
        handle_isub(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 20);
    }

    #[test]
    fn test_imul() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(5)).unwrap();
        frame.push(Value::Int(7)).unwrap();
        handle_imul(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 35);
    }

    #[test]
    fn test_idiv() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(20)).unwrap();
        frame.push(Value::Int(5)).unwrap();
        handle_idiv(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 4);
    }

    #[test]
    fn test_idiv_by_zero() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(10)).unwrap();
        frame.push(Value::Int(0)).unwrap();
        let result = handle_idiv(&mut frame);
        assert!(matches!(result, Err(VMError::ArithmeticException(_))));
    }

    #[test]
    fn test_irem() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(17)).unwrap();
        frame.push(Value::Int(5)).unwrap();
        handle_irem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 2);
    }

    #[test]
    fn test_ineg() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_ineg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -42);
    }

    // Long tests
    #[test]
    fn test_ladd() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(100)).unwrap();
        frame.push(Value::Long(200)).unwrap();
        handle_ladd(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 300);
    }

    #[test]
    fn test_ldiv_by_zero() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(10)).unwrap();
        frame.push(Value::Long(0)).unwrap();
        let result = handle_ldiv(&mut frame);
        assert!(matches!(result, Err(VMError::ArithmeticException(_))));
    }

    #[test]
    fn test_lneg() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(1000)).unwrap();
        handle_lneg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), -1000);
    }

    // Float tests
    #[test]
    fn test_fadd() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(1.5)).unwrap();
        frame.push(Value::Float(2.5)).unwrap();
        handle_fadd(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 4.0);
    }

    #[test]
    fn test_fdiv_by_zero_positive() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(1.0)).unwrap();
        frame.push(Value::Float(0.0)).unwrap();
        handle_fdiv(&mut frame).unwrap();
        let result = frame.pop().unwrap().as_float().unwrap();
        assert!(result.is_infinite() && result.is_sign_positive());
    }

    #[test]
    fn test_fneg() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(3.14)).unwrap();
        handle_fneg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), -3.14);
    }

    #[test]
    fn test_frem() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(5.5)).unwrap();
        frame.push(Value::Float(2.0)).unwrap();
        handle_frem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 1.5);
    }

    // Double tests
    #[test]
    fn test_dadd() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.5)).unwrap();
        frame.push(Value::Double(2.5)).unwrap();
        handle_dadd(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 4.0);
    }

    #[test]
    fn test_ddiv_by_zero_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(-1.0)).unwrap();
        frame.push(Value::Double(0.0)).unwrap();
        handle_ddiv(&mut frame).unwrap();
        let result = frame.pop().unwrap().as_double().unwrap();
        assert!(result.is_infinite() && result.is_sign_negative());
    }

    #[test]
    fn test_dneg() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(2.718)).unwrap();
        handle_dneg(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), -2.718);
    }

    #[test]
    fn test_drem() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(5.5)).unwrap();
        frame.push(Value::Double(2.0)).unwrap();
        handle_drem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 1.5);
    }

    // Edge case tests
    #[test]
    fn test_integer_overflow() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(i32::MAX)).unwrap();
        frame.push(Value::Int(1)).unwrap();
        handle_iadd(&mut frame).unwrap();
        // Wrapping add: MAX + 1 = MIN
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), i32::MIN);
    }

    #[test]
    fn test_integer_min_div_minus_one() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(i32::MIN)).unwrap();
        frame.push(Value::Int(-1)).unwrap();
        handle_idiv(&mut frame).unwrap();
        // Per JVM spec: MIN_VALUE / -1 = MIN_VALUE (wrapping)
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), i32::MIN);
    }

    #[test]
    fn test_long_overflow() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(i64::MAX)).unwrap();
        frame.push(Value::Long(1)).unwrap();
        handle_ladd(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), i64::MIN);
    }

    // =========================================================================
    // MIN_VALUE Edge Cases (JVMS 2.11.1)
    // Division and remainder of MIN_VALUE by -1 are special cases
    // =========================================================================

    #[test]
    fn test_long_min_div_minus_one() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(i64::MIN)).unwrap();
        frame.push(Value::Long(-1)).unwrap();
        handle_ldiv(&mut frame).unwrap();
        // Per JVM spec: Long.MIN_VALUE / -1 = Long.MIN_VALUE (wrapping)
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), i64::MIN);
    }

    #[test]
    fn test_integer_min_rem_minus_one() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(i32::MIN)).unwrap();
        frame.push(Value::Int(-1)).unwrap();
        handle_irem(&mut frame).unwrap();
        // Per JVM spec: Integer.MIN_VALUE % -1 = 0
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0);
    }

    #[test]
    fn test_long_min_rem_minus_one() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(i64::MIN)).unwrap();
        frame.push(Value::Long(-1)).unwrap();
        handle_lrem(&mut frame).unwrap();
        // Per JVM spec: Long.MIN_VALUE % -1 = 0
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 0);
    }

    // =========================================================================
    // IEEE 754 Special Cases for Floating-Point Remainder (JVMS 2.11.1)
    // These tests verify that Rust's % operator follows IEEE 754 fmod semantics
    // =========================================================================

    #[test]
    fn test_frem_infinity_mod_finite_is_nan() {
        // INFINITY % x = NaN
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::INFINITY)).unwrap();
        frame.push(Value::Float(2.0)).unwrap();
        handle_frem(&mut frame).unwrap();
        assert!(frame.pop().unwrap().as_float().unwrap().is_nan());
    }

    #[test]
    fn test_frem_finite_mod_zero_is_nan() {
        // x % 0.0 = NaN
        let mut frame = create_test_frame();
        frame.push(Value::Float(1.0)).unwrap();
        frame.push(Value::Float(0.0)).unwrap();
        handle_frem(&mut frame).unwrap();
        assert!(frame.pop().unwrap().as_float().unwrap().is_nan());
    }

    #[test]
    fn test_frem_finite_mod_infinity_is_dividend() {
        // x % INFINITY = x (when x is finite)
        let mut frame = create_test_frame();
        frame.push(Value::Float(-1.0)).unwrap();
        frame.push(Value::Float(f32::INFINITY)).unwrap();
        handle_frem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), -1.0);
    }

    #[test]
    fn test_frem_zero_mod_finite_is_zero() {
        // 0.0 % x = 0.0 (when x is finite and non-zero)
        let mut frame = create_test_frame();
        frame.push(Value::Float(0.0)).unwrap();
        frame.push(Value::Float(2.0)).unwrap();
        handle_frem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 0.0);
    }

    #[test]
    fn test_drem_infinity_mod_finite_is_nan() {
        // INFINITY % x = NaN
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::INFINITY)).unwrap();
        frame.push(Value::Double(2.0)).unwrap();
        handle_drem(&mut frame).unwrap();
        assert!(frame.pop().unwrap().as_double().unwrap().is_nan());
    }

    #[test]
    fn test_drem_finite_mod_zero_is_nan() {
        // x % 0.0 = NaN
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.0)).unwrap();
        frame.push(Value::Double(0.0)).unwrap();
        handle_drem(&mut frame).unwrap();
        assert!(frame.pop().unwrap().as_double().unwrap().is_nan());
    }

    #[test]
    fn test_drem_finite_mod_infinity_is_dividend() {
        // x % INFINITY = x (when x is finite)
        let mut frame = create_test_frame();
        frame.push(Value::Double(-1.0)).unwrap();
        frame.push(Value::Double(f64::INFINITY)).unwrap();
        handle_drem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), -1.0);
    }

    #[test]
    fn test_drem_zero_mod_finite_is_zero() {
        // 0.0 % x = 0.0 (when x is finite and non-zero)
        let mut frame = create_test_frame();
        frame.push(Value::Double(0.0)).unwrap();
        frame.push(Value::Double(2.0)).unwrap();
        handle_drem(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 0.0);
    }
}
