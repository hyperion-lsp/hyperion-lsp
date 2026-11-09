// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for arithmetic edge cases per JVM spec §2.11.1
//!
//! These tests verify correct handling of edge cases that could cause
//! overflow panics in Rust but should wrap per JVM specification.

mod execution_framework;
use execution_framework::BytecodeTestBuilder;
use hyperion_bytecode::interpreter::*;
use hyperion_bytecode::producer::ConstantPoolItem;

// ============================================================================
// Integer Division Edge Cases (JVM spec §2.11.1)
// ============================================================================

#[test]
fn test_idiv_min_value_by_minus_one() -> VMResult<()> {
    // JVM spec: Integer.MIN_VALUE / -1 = Integer.MIN_VALUE (wrapping behavior)
    // In two's complement, -MIN_VALUE overflows back to MIN_VALUE
    let bytecode = vec![
        0x12, 0x01, // ldc #1 (i32::MIN from constant pool)
        0x02, // iconst_m1 (-1)
        0x6c, // idiv
    ];

    // The framework automatically prepends index 0, so we start at index 1
    let constant_pool = vec![
        ConstantPoolItem::Integer(i32::MIN), // Index 1 (after framework prepend)
    ];

    let result = BytecodeTestBuilder::new()
        .bytecode(bytecode)
        .constant_pool(constant_pool)
        .execute()?;

    result.assert_top_int(i32::MIN)?;

    Ok(())
}

#[test]
fn test_irem_min_value_by_minus_one() -> VMResult<()> {
    // JVM spec: Integer.MIN_VALUE % -1 = 0
    // When -MIN_VALUE wraps to MIN_VALUE, MIN_VALUE - MIN_VALUE * -1 = 0
    let bytecode = vec![
        0x12, 0x01, // ldc #1 (i32::MIN from constant pool)
        0x02, // iconst_m1 (-1)
        0x70, // irem
    ];

    // The framework automatically prepends index 0, so we start at index 1
    let constant_pool = vec![
        ConstantPoolItem::Integer(i32::MIN), // Index 1 (after framework prepend)
    ];

    let result = BytecodeTestBuilder::new()
        .bytecode(bytecode)
        .constant_pool(constant_pool)
        .execute()?;

    result.assert_top_int(0)?;

    Ok(())
}

#[test]
fn test_ldiv_min_value_by_minus_one() -> VMResult<()> {
    // JVM spec: Long.MIN_VALUE / -1 = Long.MIN_VALUE (wrapping behavior)
    let bytecode = vec![
        0x14, 0x00, 0x01, // ldc2_w #1 (i64::MIN from constant pool)
        0x02, // iconst_m1 (-1)
        0x85, // i2l (convert int -1 to long -1L)
        0x6d, // ldiv
    ];

    // The framework automatically prepends index 0, so we start at index 1
    let constant_pool = vec![
        ConstantPoolItem::Long(i64::MIN), // Index 1 (after framework prepend)
    ];

    let result = BytecodeTestBuilder::new()
        .bytecode(bytecode)
        .constant_pool(constant_pool)
        .execute()?;

    result.assert_top_long(i64::MIN)?;

    Ok(())
}

#[test]
fn test_lrem_min_value_by_minus_one() -> VMResult<()> {
    // JVM spec: Long.MIN_VALUE % -1 = 0
    let bytecode = vec![
        0x14, 0x00, 0x01, // ldc2_w #1 (i64::MIN from constant pool)
        0x02, // iconst_m1 (-1)
        0x85, // i2l (convert int -1 to long -1L)
        0x71, // lrem
    ];

    // The framework automatically prepends index 0, so we start at index 1
    let constant_pool = vec![
        ConstantPoolItem::Long(i64::MIN), // Index 1 (after framework prepend)
    ];

    let result = BytecodeTestBuilder::new()
        .bytecode(bytecode)
        .constant_pool(constant_pool)
        .execute()?;

    result.assert_top_long(0)?;

    Ok(())
}

// ============================================================================
// Normal Division Cases (sanity checks)
// ============================================================================

#[test]
fn test_idiv_normal_cases() -> VMResult<()> {
    // Simple division: 10 / 3 = 3
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x06, // iconst_3
        0x6c, // idiv
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(3)?;

    Ok(())
}

#[test]
fn test_idiv_negative() -> VMResult<()> {
    // Negative division: -10 / 3 = -3 (truncates toward zero)
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x74, // ineg (negate to -10)
        0x06, // iconst_3
        0x6c, // idiv
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(-3)?;

    Ok(())
}

#[test]
fn test_irem_normal_cases() -> VMResult<()> {
    // Simple remainder: 10 % 3 = 1
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x06, // iconst_3
        0x70, // irem
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(1)?;

    Ok(())
}

#[test]
fn test_irem_negative() -> VMResult<()> {
    // Negative remainder: -10 % 3 = -1
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x74, // ineg (negate to -10)
        0x06, // iconst_3
        0x70, // irem
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(-1)?;

    Ok(())
}

#[test]
fn test_ldiv_normal_cases() -> VMResult<()> {
    // Simple long division: 100L / 7L = 14L
    let bytecode = vec![
        0x10, 0x64, // bipush 100
        0x85, // i2l (convert to long)
        0x10, 0x07, // bipush 7
        0x85, // i2l (convert to long)
        0x6d, // ldiv
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_long(14)?;

    Ok(())
}

#[test]
fn test_lrem_normal_cases() -> VMResult<()> {
    // Simple long remainder: 100L % 7L = 2L
    let bytecode = vec![
        0x10, 0x64, // bipush 100
        0x85, // i2l (convert to long)
        0x10, 0x07, // bipush 7
        0x85, // i2l (convert to long)
        0x71, // lrem
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_long(2)?;

    Ok(())
}

// ============================================================================
// IINC Instruction Tests (JVM spec §6.5.iinc)
// ============================================================================

#[test]
fn test_iinc_positive_increment() -> VMResult<()> {
    // Store 3 in local 0, increment by 5, result should be 8
    let bytecode = vec![
        0x06, // iconst_3 (push 3)
        0x3b, // istore_0 (store to local 0)
        0x84, 0x00, 0x05, // iinc 0, 5 (increment local 0 by 5)
        0x1a, // iload_0 (push local 0)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(8)?;

    Ok(())
}

#[test]
fn test_iinc_negative_increment() -> VMResult<()> {
    // Store 10 in local 0, decrement by 3 (increment by -3), result should be 7
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x3b, // istore_0 (store to local 0)
        0x84, 0x00, 0xfd, // iinc 0, -3 (0xfd = -3 as signed byte)
        0x1a, // iload_0 (push local 0)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(7)?;

    Ok(())
}

#[test]
fn test_iinc_wrapping_overflow() -> VMResult<()> {
    // JVM spec: iinc uses wrapping arithmetic
    // i32::MAX + 1 should wrap to i32::MIN
    let bytecode = vec![
        0x12, 0x01, // ldc #1 (i32::MAX from constant pool)
        0x3b, // istore_0 (store to local 0)
        0x84, 0x00, 0x01, // iinc 0, 1 (increment by 1)
        0x1a, // iload_0 (push local 0)
    ];

    // The framework automatically prepends index 0, so we start at index 1
    let constant_pool = vec![
        ConstantPoolItem::Integer(i32::MAX), // Index 1 (after framework prepend)
    ];

    let result = BytecodeTestBuilder::new()
        .bytecode(bytecode)
        .constant_pool(constant_pool)
        .execute()?;
    result.assert_top_int(i32::MIN)?;

    Ok(())
}
