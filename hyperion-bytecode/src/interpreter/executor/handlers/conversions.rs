// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type conversion opcode handlers
//!
//! Handles JVM bytecode opcodes for type conversions:
//! - Integer to other types: i2l, i2f, i2d, i2b, i2c, i2s
//! - Long to other types: l2i, l2f, l2d
//! - Float to other types: f2i, f2l, f2d
//! - Double to other types: d2i, d2l, d2f

use crate::interpreter::errors::VMResult;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;

// =============================================================================
// JVM-Compliant Float-to-Integer Conversion Helpers
// =============================================================================
// Per JVMS §2.8.3, float/double to int/long conversions must:
// - NaN → 0
// - +Infinity → MAX_VALUE
// - -Infinity → MIN_VALUE
// - Out of range → saturate to MAX/MIN
// - In range → round toward zero (truncate)
//
// While Rust 1.45+ provides saturating `as` casts, we use explicit checks
// for clarity, self-documentation, and guaranteed JVM compliance regardless
// of Rust version or future changes.
// =============================================================================

/// Convert f32 to i32 per JVM spec (JVMS §2.8.3)
#[inline]
fn jvm_f32_to_i32(f: f32) -> i32 {
    if f.is_nan() {
        0
    } else if f >= i32::MAX as f32 {
        i32::MAX
    } else if f <= i32::MIN as f32 {
        i32::MIN
    } else {
        f as i32 // Safe: value is in range
    }
}

/// Convert f32 to i64 per JVM spec (JVMS §2.8.3)
#[inline]
fn jvm_f32_to_i64(f: f32) -> i64 {
    if f.is_nan() {
        0
    } else if f >= i64::MAX as f32 {
        i64::MAX
    } else if f <= i64::MIN as f32 {
        i64::MIN
    } else {
        f as i64 // Safe: value is in range
    }
}

/// Convert f64 to i32 per JVM spec (JVMS §2.8.3)
#[inline]
fn jvm_f64_to_i32(d: f64) -> i32 {
    if d.is_nan() {
        0
    } else if d >= i32::MAX as f64 {
        i32::MAX
    } else if d <= i32::MIN as f64 {
        i32::MIN
    } else {
        d as i32 // Safe: value is in range
    }
}

/// Convert f64 to i64 per JVM spec (JVMS §2.8.3)
#[inline]
fn jvm_f64_to_i64(d: f64) -> i64 {
    if d.is_nan() {
        0
    } else if d >= i64::MAX as f64 {
        i64::MAX
    } else if d <= i64::MIN as f64 {
        i64::MIN
    } else {
        d as i64 // Safe: value is in range
    }
}

// =============================================================================
// Integer Conversions
// =============================================================================

/// Handle i2l (0x85) - Convert int to long
#[inline]
pub fn handle_i2l(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    frame.push(Value::Long(i as i64))
}

/// Handle i2f (0x86) - Convert int to float
#[inline]
pub fn handle_i2f(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    frame.push(Value::Float(i as f32))
}

/// Handle i2d (0x87) - Convert int to double
#[inline]
pub fn handle_i2d(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    frame.push(Value::Double(i as f64))
}

/// Handle i2b (0x91) - Convert int to byte (with sign extension)
#[inline]
pub fn handle_i2b(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    frame.push(Value::Int((i as i8) as i32))
}

/// Handle i2c (0x92) - Convert int to char (unsigned 16-bit)
#[inline]
pub fn handle_i2c(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    // Java chars are unsigned 16-bit. Mask to prevent sign extension.
    // Example: 65000 & 0xFFFF = 65000 (not -536)
    frame.push(Value::Int(i & 0xFFFF))
}

/// Handle i2s (0x93) - Convert int to short (with sign extension)
#[inline]
pub fn handle_i2s(frame: &mut Frame) -> VMResult<()> {
    let i = frame.pop()?.as_int()?;
    frame.push(Value::Int((i as i16) as i32))
}

// =============================================================================
// Long Conversions
// =============================================================================

/// Handle l2i (0x88) - Convert long to int (truncation)
#[inline]
pub fn handle_l2i(frame: &mut Frame) -> VMResult<()> {
    let l = frame.pop()?.as_long()?;
    frame.push(Value::Int(l as i32))
}

/// Handle l2f (0x89) - Convert long to float
#[inline]
pub fn handle_l2f(frame: &mut Frame) -> VMResult<()> {
    let l = frame.pop()?.as_long()?;
    frame.push(Value::Float(l as f32))
}

/// Handle l2d (0x8a) - Convert long to double
#[inline]
pub fn handle_l2d(frame: &mut Frame) -> VMResult<()> {
    let l = frame.pop()?.as_long()?;
    frame.push(Value::Double(l as f64))
}

// =============================================================================
// Float Conversions
// =============================================================================

/// Handle f2i (0x8b) - Convert float to int
///
/// Per JVM spec (JVMS §2.8.3): NaN→0, +/-Infinity clamps to MAX/MIN_VALUE
#[inline]
pub fn handle_f2i(frame: &mut Frame) -> VMResult<()> {
    let f = frame.pop()?.as_float()?;
    frame.push(Value::Int(jvm_f32_to_i32(f)))
}

/// Handle f2l (0x8c) - Convert float to long
///
/// Per JVM spec (JVMS §2.8.3): NaN→0, +/-Infinity clamps to MAX/MIN_VALUE
#[inline]
pub fn handle_f2l(frame: &mut Frame) -> VMResult<()> {
    let f = frame.pop()?.as_float()?;
    frame.push(Value::Long(jvm_f32_to_i64(f)))
}

/// Handle f2d (0x8d) - Convert float to double
#[inline]
pub fn handle_f2d(frame: &mut Frame) -> VMResult<()> {
    let f = frame.pop()?.as_float()?;
    frame.push(Value::Double(f as f64))
}

// =============================================================================
// Double Conversions
// =============================================================================

/// Handle d2i (0x8e) - Convert double to int
///
/// Per JVM spec (JVMS §2.8.3): NaN→0, +/-Infinity clamps to MAX/MIN_VALUE
#[inline]
pub fn handle_d2i(frame: &mut Frame) -> VMResult<()> {
    let d = frame.pop()?.as_double()?;
    frame.push(Value::Int(jvm_f64_to_i32(d)))
}

/// Handle d2l (0x8f) - Convert double to long
///
/// Per JVM spec (JVMS §2.8.3): NaN→0, +/-Infinity clamps to MAX/MIN_VALUE
#[inline]
pub fn handle_d2l(frame: &mut Frame) -> VMResult<()> {
    let d = frame.pop()?.as_double()?;
    frame.push(Value::Long(jvm_f64_to_i64(d)))
}

/// Handle d2f (0x90) - Convert double to float
#[inline]
pub fn handle_d2f(frame: &mut Frame) -> VMResult<()> {
    let d = frame.pop()?.as_double()?;
    frame.push(Value::Float(d as f32))
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

    // Integer conversions
    #[test]
    fn test_i2l() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_i2l(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 42i64);
    }

    #[test]
    fn test_i2l_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-100)).unwrap();
        handle_i2l(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), -100i64);
    }

    #[test]
    fn test_i2f() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_i2f(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 42.0f32);
    }

    #[test]
    fn test_i2d() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(42)).unwrap();
        handle_i2d(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 42.0f64);
    }

    #[test]
    fn test_i2b() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(200)).unwrap(); // Exceeds byte range
        handle_i2b(&mut frame).unwrap();
        // 200 as i8 wraps to -56
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -56);
    }

    #[test]
    fn test_i2c() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(65000)).unwrap();
        handle_i2c(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 65000);
    }

    #[test]
    fn test_i2c_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(-1)).unwrap();
        handle_i2c(&mut frame).unwrap();
        // -1 & 0xFFFF = 65535
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 65535);
    }

    #[test]
    fn test_i2s() {
        let mut frame = create_test_frame();
        frame.push(Value::Int(40000)).unwrap(); // Exceeds short range
        handle_i2s(&mut frame).unwrap();
        // 40000 as i16 wraps to -25536
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), -25536);
    }

    // Long conversions
    #[test]
    fn test_l2i() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(0x1_0000_0042)).unwrap();
        handle_l2i(&mut frame).unwrap();
        // Truncates to lower 32 bits
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 0x42);
    }

    #[test]
    fn test_l2f() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(1000)).unwrap();
        handle_l2f(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_float().unwrap(), 1000.0f32);
    }

    #[test]
    fn test_l2d() {
        let mut frame = create_test_frame();
        frame.push(Value::Long(1000)).unwrap();
        handle_l2d(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_double().unwrap(), 1000.0f64);
    }

    // Float conversions
    #[test]
    fn test_f2i() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(3.7)).unwrap();
        handle_f2i(&mut frame).unwrap();
        // Truncates towards zero
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 3);
    }

    #[test]
    fn test_f2l() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(1000.5)).unwrap();
        handle_f2l(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 1000i64);
    }

    #[test]
    fn test_f2d() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(3.14)).unwrap();
        handle_f2d(&mut frame).unwrap();
        let result = frame.pop().unwrap().as_double().unwrap();
        // Float to double can introduce small precision differences
        assert!((result - 3.14f32 as f64).abs() < 0.0001);
    }

    // Double conversions
    #[test]
    fn test_d2i() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(3.9)).unwrap();
        handle_d2i(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 3);
    }

    #[test]
    fn test_d2l() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1000.9)).unwrap();
        handle_d2l(&mut frame).unwrap();
        assert_eq!(frame.pop().unwrap().as_long().unwrap(), 1000i64);
    }

    #[test]
    fn test_d2f() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(3.14159265359)).unwrap();
        handle_d2f(&mut frame).unwrap();
        let result = frame.pop().unwrap().as_float().unwrap();
        // Double to float loses precision
        assert!((result - 3.141_592_7_f32).abs() < 0.0001);
    }

    // =========================================================================
    // NaN and Infinity Edge Cases (JVMS §2.8.3)
    // Per JVM spec: NaN → 0, +Infinity → MAX_VALUE, -Infinity → MIN_VALUE
    // We use explicit jvm_fXX_to_iXX helpers to guarantee compliance.
    // =========================================================================

    // F2I edge cases
    #[test]
    fn test_f2i_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NAN)).unwrap();
        handle_f2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            0,
            "NaN should convert to 0"
        );
    }

    #[test]
    fn test_f2i_positive_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::INFINITY)).unwrap();
        handle_f2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MAX,
            "+Infinity should convert to MAX_VALUE"
        );
    }

    #[test]
    fn test_f2i_negative_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NEG_INFINITY)).unwrap();
        handle_f2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MIN,
            "-Infinity should convert to MIN_VALUE"
        );
    }

    #[test]
    fn test_f2i_overflow_positive() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(3.0e10)).unwrap(); // Larger than i32::MAX
        handle_f2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MAX,
            "Overflow should saturate to MAX_VALUE"
        );
    }

    #[test]
    fn test_f2i_overflow_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(-3.0e10)).unwrap(); // Smaller than i32::MIN
        handle_f2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MIN,
            "Underflow should saturate to MIN_VALUE"
        );
    }

    // D2I edge cases
    #[test]
    fn test_d2i_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::NAN)).unwrap();
        handle_d2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            0,
            "NaN should convert to 0"
        );
    }

    #[test]
    fn test_d2i_positive_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::INFINITY)).unwrap();
        handle_d2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MAX,
            "+Infinity should convert to MAX_VALUE"
        );
    }

    #[test]
    fn test_d2i_negative_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::NEG_INFINITY)).unwrap();
        handle_d2i(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_int().unwrap(),
            i32::MIN,
            "-Infinity should convert to MIN_VALUE"
        );
    }

    // F2L edge cases
    #[test]
    fn test_f2l_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NAN)).unwrap();
        handle_f2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            0,
            "NaN should convert to 0"
        );
    }

    #[test]
    fn test_f2l_positive_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::INFINITY)).unwrap();
        handle_f2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MAX,
            "+Infinity should convert to MAX_VALUE"
        );
    }

    #[test]
    fn test_f2l_negative_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Float(f32::NEG_INFINITY)).unwrap();
        handle_f2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MIN,
            "-Infinity should convert to MIN_VALUE"
        );
    }

    // D2L edge cases
    #[test]
    fn test_d2l_nan() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::NAN)).unwrap();
        handle_d2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            0,
            "NaN should convert to 0"
        );
    }

    #[test]
    fn test_d2l_positive_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::INFINITY)).unwrap();
        handle_d2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MAX,
            "+Infinity should convert to MAX_VALUE"
        );
    }

    #[test]
    fn test_d2l_negative_infinity() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(f64::NEG_INFINITY)).unwrap();
        handle_d2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MIN,
            "-Infinity should convert to MIN_VALUE"
        );
    }

    #[test]
    fn test_d2l_overflow_positive() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(1.0e20)).unwrap(); // Larger than i64::MAX
        handle_d2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MAX,
            "Overflow should saturate to MAX_VALUE"
        );
    }

    #[test]
    fn test_d2l_overflow_negative() {
        let mut frame = create_test_frame();
        frame.push(Value::Double(-1.0e20)).unwrap(); // Smaller than i64::MIN
        handle_d2l(&mut frame).unwrap();
        assert_eq!(
            frame.pop().unwrap().as_long().unwrap(),
            i64::MIN,
            "Underflow should saturate to MIN_VALUE"
        );
    }
}
