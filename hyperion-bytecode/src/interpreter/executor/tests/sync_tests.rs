// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_monitorenter_valid_object() -> VMResult<()> {
    // Test monitorenter with valid object reference
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (create object)
            0xc2, // monitorenter
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0), // dummy
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
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
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0), // dummy
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
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
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0), // dummy
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        ])
        .build()?;

    // Execute all 8 instructions
    vm.steps(8)?;

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

    // Execute all operations
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

    // Execute all operations
    vm.steps(5)?;

    let result = vm.stack_top()?.as_double()?;
    assert!((result - 1.0).abs() < 0.001);
    Ok(())
}

// ========== Comprehensive Synchronization Tests ==========

#[test]
fn test_monitorenter_exit_reentrant() -> VMResult<()> {
    // Test reentrant locking: acquire and release on same object
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 - [obj]
            0x59, // dup - [obj, obj]
            0xc2, // monitorenter - [obj]
            0xc3, // monitorexit - []
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        ])
        .build()?;

    // Execute: new, dup, monitorenter, monitorexit
    vm.steps(4)?;

    // Stack should be empty
    assert_eq!(vm.stack_depth(), 0);
    Ok(())
}

#[test]
fn test_monitorexit_without_enter() -> VMResult<()> {
    // Test IllegalMonitorStateException - exit without enter
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1
            0xc3, // monitorexit (without monitorenter) - should fail
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        ])
        .build()?;

    // Execute new
    vm.step()?;
    // Execute monitorexit - should throw IllegalMonitorStateException
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/IllegalMonitorStateException");

    Ok(())
}

#[test]
fn test_monitorenter_then_exit_different_object() -> VMResult<()> {
    // Test that exiting on a different object fails
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 - obj1
            0xbb, 0x00, 0x01, // new #1 - obj2
            0xc2, // monitorenter (obj2) - stack: [obj1]
            0x57, // pop obj1
            0xbb, 0x00, 0x01, // new #1 - obj3
            0xc3, // monitorexit (obj3 instead of obj2) - should fail
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        ])
        .build()?;

    // Execute: new, new, monitorenter, pop, new
    vm.steps(5)?;

    // This should fail because we're trying to exit a lock we don't own
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/IllegalMonitorStateException");

    Ok(())
}

// ========== Additional Type Checking Tests ==========

#[test]
fn test_swap_category1_int_int() -> VMResult<()> {
    // Swap two ints (valid category-1 values)
    // Stack before: [1, 2] (bottom to top)
    // Stack after:  [2, 1]
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // iconst_1 (push 1)
            0x05, // iconst_2 (push 2)
            0x5f, // swap (exchange top two)
        ])
        .build()?;

    // Execute iconst_1, iconst_2, swap
    vm.steps(3)?;

    // After swap: top=1, second=2
    assert_eq!(vm.pop()?, Value::Int(1)); // top was 2, now 1
    assert_eq!(vm.pop()?, Value::Int(2)); // second was 1, now 2

    Ok(())
}

#[test]
fn test_swap_category1_float_ref() -> VMResult<()> {
    // Swap float and null ref (both valid category-1 values)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x0c, // fconst_1 (push 1.0f)
            0x01, // aconst_null (push null)
            0x5f, // swap
        ])
        .build()?;

    // Execute fconst_1, aconst_null, swap
    vm.steps(3)?;

    // After swap: top=1.0f, second=null
    assert_eq!(vm.pop()?, Value::Float(1.0));
    assert!(matches!(vm.pop()?, Value::Reference(ObjectRef::Null)));

    Ok(())
}

#[test]
fn test_swap_with_long_fails() -> VMResult<()> {
    // Swap with long (category-2) should fail
    // Long takes 2 stack slots, SWAP only works with category-1 values
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x0a, // lconst_1 (push 1L - category-2)
            0x04, // iconst_1 (push 1 - category-1)
            0x5f, // swap - should fail
        ])
        .build()?;

    // Execute lconst_1, iconst_1
    vm.steps(2)?;

    // Execute swap - should return InvalidBytecode error
    let result = vm.step();
    assert!(
        matches!(result, Err(VMError::InvalidBytecode)),
        "SWAP with long should return InvalidBytecode, got {:?}",
        result
    );

    Ok(())
}

#[test]
fn test_swap_with_double_fails() -> VMResult<()> {
    // Swap with double (category-2) should fail
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x0f, // dconst_1 (push 1.0 - category-2)
            0x04, // iconst_1 (push 1 - category-1)
            0x5f, // swap - should fail
        ])
        .build()?;

    // Execute dconst_1, iconst_1
    vm.steps(2)?;

    // Execute swap - should return InvalidBytecode error
    let result = vm.step();
    assert!(
        matches!(result, Err(VMError::InvalidBytecode)),
        "SWAP with double should return InvalidBytecode, got {:?}",
        result
    );

    Ok(())
}

#[test]
fn test_swap_two_longs_fails() -> VMResult<()> {
    // Swap two longs (both category-2) should fail
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x09, // lconst_0 (push 0L - category-2)
            0x0a, // lconst_1 (push 1L - category-2)
            0x5f, // swap - should fail
        ])
        .build()?;

    // Execute lconst_0, lconst_1
    vm.steps(2)?;

    // Execute swap - should return InvalidBytecode error
    let result = vm.step();
    assert!(
        matches!(result, Err(VMError::InvalidBytecode)),
        "SWAP with two longs should return InvalidBytecode, got {:?}",
        result
    );

    Ok(())
}
