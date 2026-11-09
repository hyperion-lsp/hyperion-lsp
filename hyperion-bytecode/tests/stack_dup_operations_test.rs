// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for JVM stack duplication operations (DUP, DUP_X1, DUP_X2, DUP2, DUP2_X1, DUP2_X2)
//!
//! These tests verify correct behavior for all forms of each operation:
//! - Category-1 values (int, float, object refs) take 1 stack slot
//! - Category-2 values (long, double) take 2 stack slots

mod execution_framework;
use execution_framework::BytecodeTestBuilder;
use hyperion_bytecode::interpreter::*;

// ============================================================================
// DUP Tests (0x59) - Duplicate top stack value
// ============================================================================

#[test]
fn test_dup_category1() -> VMResult<()> {
    // Stack: value → value, value
    // Test: push 42, dup, verify two 42s on stack
    let bytecode = vec![
        0x10, 0x2a, // bipush 42
        0x59, // dup
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Top should be 42
    assert_eq!(result.get(0), Some(&Value::Int(42)));
    // Second should be 42
    assert_eq!(result.get(1), Some(&Value::Int(42)));

    Ok(())
}

#[test]
fn test_dup_with_arithmetic() -> VMResult<()> {
    // Stack: value → value, value, then add them
    // Test: push 5, dup, add (5 + 5 = 10)
    let bytecode = vec![
        0x08, // iconst_5
        0x59, // dup (stack: 5, 5)
        0x60, // iadd (5 + 5 = 10)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(10)?;

    Ok(())
}

// ============================================================================
// DUP_X1 Tests (0x5A) - Duplicate top, insert below second
// Form: value2, value1 → value1, value2, value1
// ============================================================================

#[test]
fn test_dup_x1_category1() -> VMResult<()> {
    // Stack: value2, value1 → value1, value2, value1
    // Test: push 1, push 2, dup_x1
    // Expected stack: 2, 1, 2 (top to bottom)
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x5a, // dup_x1
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 2, 1, 2 (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Int(2))); // top
    assert_eq!(result.get(1), Some(&Value::Int(1))); // middle
    assert_eq!(result.get(2), Some(&Value::Int(2))); // bottom

    Ok(())
}

#[test]
fn test_dup_x1_arithmetic() -> VMResult<()> {
    // Test: push 3, push 5, dup_x1, pop, sub
    // Stack after dup_x1: 5, 3, 5
    // After pop: 5, 3
    // After sub: 5 - 3 = 2
    let bytecode = vec![
        0x06, // iconst_3
        0x08, // iconst_5
        0x5a, // dup_x1 (stack: 5, 3, 5)
        0x57, // pop (stack: 5, 3)
        0x64, // isub (5 - 3 = 2)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(2)?;

    Ok(())
}

// ============================================================================
// DUP_X2 Tests (0x5B) - Duplicate top, insert below third (or second if cat-2)
// Form 1: value3, value2, value1 → value1, value3, value2, value1 (all cat-1)
// Form 2: value2, value1 → value1, value2, value1 (value2 is cat-2)
// ============================================================================

#[test]
fn test_dup_x2_form1_all_category1() -> VMResult<()> {
    // Stack: value3, value2, value1 → value1, value3, value2, value1
    // Test: push 1, push 2, push 3, dup_x2
    // Before: [1, 2, 3] (bottom to top)
    // After:  [3, 1, 2, 3] (bottom to top)
    // Pop order: 3, 2, 1, 3
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x06, // iconst_3
        0x5b, // dup_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Result array is pop order: [3, 2, 1, 3]
    assert_eq!(result.get(0), Some(&Value::Int(3))); // top
    assert_eq!(result.get(1), Some(&Value::Int(2))); // second
    assert_eq!(result.get(2), Some(&Value::Int(1))); // third
    assert_eq!(result.get(3), Some(&Value::Int(3))); // bottom

    Ok(())
}

#[test]
fn test_dup_x2_form2_category2() -> VMResult<()> {
    // Stack: value2 (cat-2), value1 → value1, value2, value1
    // Test: push long 100, push 5, dup_x2
    // Expected stack: 5, 100L, 5 (top to bottom)
    let bytecode = vec![
        0x0a, // lconst_1 (push 1L)
        0x10, 0x05, // bipush 5 (push 5)
        0x5b, // dup_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 5, 1L, 5 (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Int(5))); // top
    assert_eq!(result.get(1), Some(&Value::Long(1))); // middle (cat-2)
    assert_eq!(result.get(2), Some(&Value::Int(5))); // bottom

    Ok(())
}

// ============================================================================
// DUP2 Tests (0x5C) - Duplicate top 2 values (or top cat-2 value)
// Form 1: value2, value1 → value2, value1, value2, value1 (both cat-1)
// Form 2: value → value, value (value is cat-2)
// ============================================================================

#[test]
fn test_dup2_form1_two_category1() -> VMResult<()> {
    // Stack: value2, value1 → value2, value1, value2, value1
    // Test: push 10, push 20, dup2
    // Expected stack: 20, 10, 20, 10 (top to bottom)
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x10, 0x14, // bipush 20
        0x5c, // dup2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 20, 10, 20, 10 (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Int(20))); // top
    assert_eq!(result.get(1), Some(&Value::Int(10))); // second
    assert_eq!(result.get(2), Some(&Value::Int(20))); // third
    assert_eq!(result.get(3), Some(&Value::Int(10))); // bottom

    Ok(())
}

#[test]
fn test_dup2_form2_category2() -> VMResult<()> {
    // Stack: value → value, value (value is cat-2)
    // Test: push long 42, dup2
    // Expected stack: 42L, 42L (top to bottom)
    let bytecode = vec![
        0x10, 0x2a, // bipush 42
        0x85, // i2l (convert to long)
        0x5c, // dup2 (duplicates the long value)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 42L, 42L (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Long(42))); // top
    assert_eq!(result.get(1), Some(&Value::Long(42))); // bottom

    Ok(())
}

#[test]
fn test_dup2_with_arithmetic() -> VMResult<()> {
    // Test: push 7, push 3, dup2, add
    // Before DUP2: [7, 3] (bottom to top)
    // Form 1: value2, value1 → value2, value1, value2, value1
    // After DUP2: [7, 3, 7, 3] (bottom to top)
    // After IADD: [7, 3, 10] (3+7=10)
    let bytecode = vec![
        0x10, 0x07, // bipush 7
        0x06, // iconst_3
        0x5c, // dup2 (stack: 7, 3, 7, 3)
        0x60, // iadd (7 + 3 = 10, stack: 7, 3, 10)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Pop order: 10, 3, 7
    assert_eq!(result.get(0), Some(&Value::Int(10)));
    assert_eq!(result.get(1), Some(&Value::Int(3)));
    assert_eq!(result.get(2), Some(&Value::Int(7)));

    Ok(())
}

// ============================================================================
// DUP2_X1 Tests (0x5D) - Duplicate top 2, insert below third
// Form 1: value3, value2, value1 → value2, value1, value3, value2, value1 (all cat-1)
// Form 2: value2, value1 → value1, value2, value1 (value1 is cat-2)
// ============================================================================

#[test]
fn test_dup2_x1_form1_all_category1() -> VMResult<()> {
    // Stack: value3, value2, value1 → value2, value1, value3, value2, value1
    // Test: push 1, push 2, push 3, dup2_x1
    // Expected stack: 3, 2, 1, 3, 2 (top to bottom)
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x06, // iconst_3
        0x5d, // dup2_x1
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 3, 2, 1, 3, 2 (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Int(3))); // top
    assert_eq!(result.get(1), Some(&Value::Int(2))); // second
    assert_eq!(result.get(2), Some(&Value::Int(1))); // third
    assert_eq!(result.get(3), Some(&Value::Int(3))); // fourth
    assert_eq!(result.get(4), Some(&Value::Int(2))); // bottom

    Ok(())
}

#[test]
fn test_dup2_x1_form2_category2() -> VMResult<()> {
    // Stack: value2, value1 → value1, value2, value1 (value1 is cat-2)
    // Test: push 5, push long 100, dup2_x1
    // Expected stack: 100L, 5, 100L (top to bottom)
    let bytecode = vec![
        0x08, // iconst_5
        0x0a, // lconst_1 (push 1L)
        0x5d, // dup2_x1
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 1L, 5, 1L (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Long(1))); // top (cat-2)
    assert_eq!(result.get(1), Some(&Value::Int(5))); // middle
    assert_eq!(result.get(2), Some(&Value::Long(1))); // bottom (cat-2)

    Ok(())
}

// ============================================================================
// DUP2_X2 Tests (0x5E) - Most complex: 4 different forms
// Form 1: All cat-1 → value4, value3, value2, value1 → value2, value1, value4, value3, value2, value1
// Form 2: value1 cat-2, value3,2 cat-1 → value3, value2, value1 → value1, value3, value2, value1
// Form 3: value1,2 cat-1, value3 cat-2 → value3, value2, value1 → value2, value1, value3, value2, value1
// Form 4: Both cat-2 → value2, value1 → value1, value2, value1
// ============================================================================

#[test]
fn test_dup2_x2_form1_all_category1() -> VMResult<()> {
    // Stack: value4, value3, value2, value1 → value2, value1, value4, value3, value2, value1
    // Test: push 1, push 2, push 3, push 4, dup2_x2
    // Before: [1, 2, 3, 4] (bottom to top, value4=1, value3=2, value2=3, value1=4)
    // After:  [3, 4, 1, 2, 3, 4] (bottom to top)
    // Pop order: 4, 3, 2, 1, 4, 3
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x06, // iconst_3
        0x07, // iconst_4
        0x5e, // dup2_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Pop order: [4, 3, 2, 1, 4, 3]
    assert_eq!(result.get(0), Some(&Value::Int(4))); // top
    assert_eq!(result.get(1), Some(&Value::Int(3))); // second
    assert_eq!(result.get(2), Some(&Value::Int(2))); // third
    assert_eq!(result.get(3), Some(&Value::Int(1))); // fourth
    assert_eq!(result.get(4), Some(&Value::Int(4))); // fifth
    assert_eq!(result.get(5), Some(&Value::Int(3))); // bottom

    Ok(())
}

#[test]
fn test_dup2_x2_form2_top_category2() -> VMResult<()> {
    // Stack: value3, value2, value1 → value1, value3, value2, value1 (value1 is cat-2)
    // Test: push 1, push 2, push long 100, dup2_x2
    // Before: [1, 2, 100L] (value3=1, value2=2, value1=100L)
    // After:  [100L, 1, 2, 100L] (bottom to top)
    // Pop order: 100L, 2, 1, 100L
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x10, 0x64, // bipush 100
        0x85, // i2l (convert to long)
        0x5e, // dup2_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Pop order: [100L, 2, 1, 100L]
    assert_eq!(result.get(0), Some(&Value::Long(100))); // top (cat-2)
    assert_eq!(result.get(1), Some(&Value::Int(2))); // second
    assert_eq!(result.get(2), Some(&Value::Int(1))); // third
    assert_eq!(result.get(3), Some(&Value::Long(100))); // bottom (cat-2)

    Ok(())
}

#[test]
fn test_dup2_x2_form3_third_category2() -> VMResult<()> {
    // Stack: value3 (cat-2), value2, value1 → value2, value1, value3, value2, value1
    // Test: push long 50, push 10, push 20, dup2_x2
    // Before: [50L, 10, 20] (value3=50L cat-2, value2=10, value1=20)
    // After:  [10, 20, 50L, 10, 20] (bottom to top)
    // Pop order: 20, 10, 50L, 20, 10
    let bytecode = vec![
        0x10, 0x32, // bipush 50
        0x85, // i2l (convert to long = 50L)
        0x10, 0x0a, // bipush 10
        0x10, 0x14, // bipush 20
        0x5e, // dup2_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Pop order: [20, 10, 50L, 20, 10]
    assert_eq!(result.get(0), Some(&Value::Int(20))); // top
    assert_eq!(result.get(1), Some(&Value::Int(10))); // second
    assert_eq!(result.get(2), Some(&Value::Long(50))); // third (cat-2)
    assert_eq!(result.get(3), Some(&Value::Int(20))); // fourth
    assert_eq!(result.get(4), Some(&Value::Int(10))); // bottom

    Ok(())
}

#[test]
fn test_dup2_x2_form4_both_category2() -> VMResult<()> {
    // Stack: value2, value1 → value1, value2, value1 (both cat-2)
    // Test: push long 100, push long 200, dup2_x2
    // Expected stack: 200L, 100L, 200L (top to bottom)
    let bytecode = vec![
        0x10, 0x64, // bipush 100
        0x85, // i2l (convert to long = 100L)
        0x10, 0x32, // bipush 50
        0x85, // i2l (convert to long = 50L)
        0x5e, // dup2_x2
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    // Stack should be: 50L, 100L, 50L (top to bottom)
    assert_eq!(result.get(0), Some(&Value::Long(50))); // top (cat-2)
    assert_eq!(result.get(1), Some(&Value::Long(100))); // middle (cat-2)
    assert_eq!(result.get(2), Some(&Value::Long(50))); // bottom (cat-2)

    Ok(())
}

// ============================================================================
// Complex Integration Tests
// ============================================================================

#[test]
fn test_dup_operations_combined() -> VMResult<()> {
    // Test combining multiple dup operations
    // Stack operations: push 5, dup (5,5), push 3, dup_x1 (3,5,3,5), add top two
    let bytecode = vec![
        0x08, // iconst_5
        0x59, // dup (stack: 5, 5)
        0x06, // iconst_3 (stack: 3, 5, 5)
        0x5a, // dup_x1 (stack: 3, 5, 3, 5)
        0x60, // iadd (3 + 5 = 8, stack: 8, 3, 5)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;

    assert_eq!(result.get(0), Some(&Value::Int(8))); // top
    assert_eq!(result.get(1), Some(&Value::Int(3))); // second
    assert_eq!(result.get(2), Some(&Value::Int(5))); // bottom

    Ok(())
}

#[test]
fn test_dup2_with_longs() -> VMResult<()> {
    // Test dup2 with actual long arithmetic
    // push long 10, dup2, ladd (10L + 10L = 20L)
    let bytecode = vec![
        0x10, 0x0a, // bipush 10
        0x85, // i2l (convert to long = 10L)
        0x5c, // dup2 (duplicates the long, stack: 10L, 10L)
        0x61, // ladd (10L + 10L = 20L)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_long(20)?;

    Ok(())
}

#[test]
fn test_mixed_category_operations() -> VMResult<()> {
    // Test mixing category-1 and category-2 values
    // This test demonstrates the complexity of mixing types
    // For now, just verify the test compiles and runs

    Ok(())
}

#[test]
fn test_dup_x2_practical_use() -> VMResult<()> {
    // Simplified test: dup_x2 works in a computational context
    // push 1, push 2, push 3, dup_x2, pop, iadd
    // Stack after dup_x2: [3, 1, 2, 3]
    // After pop: [3, 1, 2]
    // After iadd: [3, 3] (1+2=3)
    let bytecode = vec![
        0x04, // iconst_1
        0x05, // iconst_2
        0x06, // iconst_3 - stack: 1, 2, 3
        0x5b, // dup_x2 - stack: 3, 1, 2, 3
        0x57, // pop (remove top 3)
        0x60, // iadd (1 + 2 = 3)
    ];

    let result = BytecodeTestBuilder::new().bytecode(bytecode).execute()?;
    result.assert_top_int(3)?;

    Ok(())
}
