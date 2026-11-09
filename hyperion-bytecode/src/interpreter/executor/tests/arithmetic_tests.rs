// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_iconst_execution() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x03]) // iconst_0
        .build()?;

    vm.step()?;

    assert_eq!(vm.stack_top()?.as_int()?, 0);
    Ok(())
}

#[test]
fn test_arithmetic_iadd() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x04, 0x05, 0x60]) // iconst_1, iconst_2, iadd
        .build()?;

    // Execute iconst_1
    vm.step()?;
    // Execute iconst_2
    vm.step()?;
    // Execute iadd
    vm.step()?;

    let result = vm.stack_top()?.as_int()?;
    assert_eq!(result, 3);
    Ok(())
}

#[test]
fn test_load_store() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x03, 0x3b, 0x1a]) // iconst_0, istore_0, iload_0
        .build()?;

    vm.step()?; // Push 0
    vm.step()?; // Store to local 0
    vm.step()?; // Load from local 0

    assert_eq!(vm.stack_top()?.as_int()?, 0);
    Ok(())
}

#[test]
fn test_type_conversion() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x04, 0x86]) // iconst_1, i2f
        .build()?;

    vm.step()?; // Push 1
    vm.step()?; // Convert to float

    assert_eq!(vm.stack_top()?.as_float()?, 1.0);
    Ok(())
}

#[test]
fn test_bitwise_ishl() -> VMResult<()> {
    // Test: 1 << 3 = 8
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x04, 0x06, 0x78]) // iconst_1, iconst_3, ishl
        .build()?;

    vm.step()?; // Push 1
    vm.step()?; // Push 3
    vm.step()?; // Shift left

    assert_eq!(vm.stack_top()?.as_int()?, 8);
    Ok(())
}

#[test]
fn test_bitwise_ishr() -> VMResult<()> {
    // Test: 8 >> 2 = 2
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x10, 0x08, 0x05, 0x7a]) // bipush 8, iconst_2, ishr
        .build()?;

    vm.step()?; // Push 8
    vm.step()?; // Push 2
    vm.step()?; // Shift right

    assert_eq!(vm.stack_top()?.as_int()?, 2);
    Ok(())
}

#[test]
fn test_bitwise_iushr_negative() -> VMResult<()> {
    // Test: -1 >>> 31 = 1 (logical shift, fills with zeros)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x02, 0x10, 0x1f, 0x7c]) // iconst_m1, bipush 31, iushr
        .build()?;

    vm.step()?; // Push -1
    vm.step()?; // Push 31
    vm.step()?; // Logical shift right

    assert_eq!(vm.stack_top()?.as_int()?, 1);
    Ok(())
}

#[test]
fn test_bitwise_iand() -> VMResult<()> {
    // Test: 0xFF & 0x0F = 0x0F (255 & 15 = 15)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x10, 0x0f, 0x04, 0x7e]) // bipush 15, iconst_1, iand
        .build()?;

    vm.step()?; // Push 15
    vm.step()?; // Push 1
    vm.step()?; // AND

    assert_eq!(vm.stack_top()?.as_int()?, 1); // 15 & 1 = 1
    Ok(())
}

#[test]
fn test_bitwise_ior() -> VMResult<()> {
    // Test: 0x0F | 0xF0 = 0xFF
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x10, 0x0f, 0x05, 0x80]) // bipush 15, iconst_2, ior
        .build()?;

    vm.step()?; // Push 15
    vm.step()?; // Push 2
    vm.step()?; // OR

    assert_eq!(vm.stack_top()?.as_int()?, 15); // 15 | 2 = 15 (0b1111 | 0b0010 = 0b1111)
    Ok(())
}

#[test]
fn test_bitwise_ixor() -> VMResult<()> {
    // Test: 0xFF ^ 0xFF = 0x00
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x05, 0x05, 0x82]) // iconst_2, iconst_2, ixor
        .build()?;

    vm.step()?; // Push 2
    vm.step()?; // Push 2
    vm.step()?; // XOR

    assert_eq!(vm.stack_top()?.as_int()?, 0); // 2 ^ 2 = 0
    Ok(())
}

#[test]
fn test_frem_basic() -> VMResult<()> {
    // Test basic float remainder: 4.0 % 3.0 = 1.0
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x12, 0x01, // ldc #1 (CP index 1 = array index 0 = 4.0)
            0x12, 0x02, // ldc #2 (CP index 2 = array index 1 = 3.0)
            0x72, // frem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Float(4.0),          // CP index 1, array index 1
            ConstantPoolItem::Float(3.0),          // CP index 2, array index 2
        ])
        .build()?;

    // Execute ldc (4.0)
    vm.step()?;
    // Execute ldc (3.0)
    vm.step()?;
    // Execute frem
    vm.step()?;

    let result = vm.stack_top()?.as_float()?;
    assert!((result - 1.0).abs() < 0.001);
    Ok(())
}

#[test]
fn test_frem_negative() -> VMResult<()> {
    // Test negative float remainder: -7.5 % 2.5 = -0.0 (IEEE 754 semantics)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x12, 0x01, // ldc #1 (load -7.5)
            0x12, 0x02, // ldc #2 (load 2.5)
            0x72, // frem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Float(-7.5),         // CP index 1
            ConstantPoolItem::Float(2.5),          // CP index 2
        ])
        .build()?;

    // Execute ldc (-7.5)
    vm.step()?;
    // Execute ldc (2.5)
    vm.step()?;
    // Execute frem
    vm.step()?;

    let result = vm.stack_top()?.as_float()?;
    // -7.5 % 2.5 = -0.0 in IEEE 754
    assert!((result + 0.0).abs() < 0.001);
    Ok(())
}

#[test]
fn test_drem_basic() -> VMResult<()> {
    // Test basic double remainder: 10.5 % 3.0 = 1.5
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x14, 0x00, 0x01, // ldc2_w #1 (load 10.5)
            0x14, 0x00, 0x02, // ldc2_w #2 (load 3.0)
            0x73, // drem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Double(10.5),        // CP index 1
            ConstantPoolItem::Double(3.0),         // CP index 2
        ])
        .build()?;

    // Execute ldc2_w (10.5)
    vm.step()?;
    // Execute ldc2_w (3.0)
    vm.step()?;
    // Execute drem
    vm.step()?;

    let result = vm.stack_top()?.as_double()?;
    assert!((result - 1.5).abs() < 0.001);
    Ok(())
}

#[test]
fn test_drem_zero_divisor() -> VMResult<()> {
    // Test double remainder with zero divisor: 5.0 % 0.0 = NaN (IEEE 754)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x14, 0x00, 0x01, // ldc2_w #1 (load 5.0)
            0x0e, // dconst_0 (0.0)
            0x73, // drem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Double(5.0),         // CP index 1
        ])
        .build()?;

    // Execute ldc2_w (5.0)
    vm.step()?;
    // Execute dconst_0
    vm.step()?;
    // Execute drem - should produce NaN per IEEE 754
    vm.step()?;

    let result = vm.stack_top()?.as_double()?;
    assert!(result.is_nan());
    Ok(())
}

#[test]
fn test_monitorenter_valid_object() -> VMResult<()> {
    // Test monitorenter with valid object reference
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (create object)
            0xc2, // monitorenter
        ])
        .build()?;

    // Execute new
    vm.step()?;
    // Execute monitorenter - should succeed for single-threaded MVP
    vm.step()?;

    // Stack should be empty now
    assert_eq!(vm.stack_depth(), 0);

    Ok(())
}

#[test]
fn test_monitorenter_null() -> VMResult<()> {
    // Test monitorenter with null - should throw NullPointerException
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x01, // aconst_null
            0xc2, // monitorenter
        ])
        .build()?;

    // Execute aconst_null
    vm.step()?;

    // Execute monitorenter - should throw NullPointerException
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/NullPointerException");

    Ok(())
}

#[test]
fn test_monitorexit_valid_object() -> VMResult<()> {
    // Test monitorexit with valid object reference (proper acquire -> release)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (create object) - stack: [obj]
            0x59, // dup - stack: [obj, obj]
            0xc2, // monitorenter - acquires lock - stack: [obj]
            0xc3, // monitorexit - releases lock - stack: []
        ])
        .build()?;

    // Execute new - creates object
    vm.step()?;
    // Execute dup - duplicates object for monitorenter
    vm.step()?;
    // Execute monitorenter - acquires lock
    vm.step()?;
    // Execute monitorexit - releases lock
    vm.step()?;

    // Stack should be empty now
    assert_eq!(vm.stack_depth(), 0);

    Ok(())
}

#[test]
fn test_monitorexit_null() -> VMResult<()> {
    // Test monitorexit with null - should throw NullPointerException
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x01, // aconst_null
            0xc3, // monitorexit
        ])
        .build()?;

    // Execute aconst_null
    vm.step()?;

    // Execute monitorexit - should throw NullPointerException
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/NullPointerException");

    Ok(())
}

#[test]
fn test_synchronized_block_simulation() -> VMResult<()> {
    // Simulate a synchronized block: monitorenter, some operations, monitorexit
    // Java: synchronized(obj) { int result = 0 + 1; }
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (create object)
            0x59, // dup (stack: obj, obj)
            0xc2, // monitorenter (pops one obj, stack: obj)
            0x03, // iconst_0 (stack: obj, 0)
            0x04, // iconst_1 (stack: obj, 0, 1)
            0x60, // iadd (stack: obj, 1)
            0x5f, // swap (stack: 1, obj)
            0xc3, // monitorexit (pops obj, stack: 1)
        ])
        .build()?;

    // Execute new
    vm.step()?;
    // Execute dup
    vm.step()?;
    // Execute monitorenter
    vm.step()?;
    // Execute iconst_0
    vm.step()?;
    // Execute iconst_1
    vm.step()?;
    // Execute iadd
    vm.step()?;
    // Execute swap
    vm.step()?;
    // Execute monitorexit
    vm.step()?;

    // Stack should have just the result (1)
    assert_eq!(vm.stack_depth(), 1);
    let result = vm.stack_top()?.as_int()?;
    assert_eq!(result, 1);

    Ok(())
}

#[test]
fn test_frem_with_addition() -> VMResult<()> {
    // Integration test: (5.0 + 2.0) % 2.5 = 7.0 % 2.5 = 2.0
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x12, 0x01, // ldc #1 (5.0)
            0x12, 0x02, // ldc #2 (2.0)
            0x62, // fadd
            0x12, 0x03, // ldc #3 (2.5)
            0x72, // frem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Float(5.0),          // CP index 1
            ConstantPoolItem::Float(2.0),          // CP index 2
            ConstantPoolItem::Float(2.5),          // CP index 3
        ])
        .build()?;

    // Execute all operations (9 bytes total)
    vm.steps(5)?;

    let result = vm.stack_top()?.as_float()?;
    assert!((result - 2.0).abs() < 0.001);
    Ok(())
}

#[test]
fn test_drem_with_multiplication() -> VMResult<()> {
    // Integration test: (3.5 * 2.0) % 2.0 = 7.0 % 2.0 = 1.0
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x14, 0x00, 0x01, // ldc2_w #1 (3.5)
            0x14, 0x00, 0x02, // ldc2_w #2 (2.0)
            0x6b, // dmul
            0x14, 0x00, 0x02, // ldc2_w #2 (2.0)
            0x73, // drem
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0: unused (JVM spec)
            ConstantPoolItem::Double(3.5),         // CP index 1
            ConstantPoolItem::Double(2.0),         // CP index 2
        ])
        .build()?;

    // Execute all operations (11 bytes total: 3+3+1+3+1)
    vm.steps(5)?;

    let result = vm.stack_top()?.as_double()?;
    assert!((result - 1.0).abs() < 0.001);
    Ok(())
}
