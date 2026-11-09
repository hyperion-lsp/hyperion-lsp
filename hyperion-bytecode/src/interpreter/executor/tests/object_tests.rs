// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_instanceof_null() -> VMResult<()> {
    // instanceof with null - should push 0
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0xc1, 0x00, 0x01]) // instanceof, index = 1
        .with_heap_size(1024 * 1024)
        .build()?;

    // Push null reference
    vm.push(Value::Reference(ObjectRef::Null))?;

    // Execute instanceof
    vm.step()?;

    // null instanceof any_type should be 0
    let result = vm.stack_top()?.as_int()?;
    assert_eq!(result, 0);

    Ok(())
}

#[test]
fn test_checkcast_null() -> VMResult<()> {
    // checkcast with null - null can be cast to any type
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0xc0, 0x00, 0x01]) // checkcast, index = 1
        .with_heap_size(1024 * 1024)
        .build()?;

    // Push null reference
    vm.push(Value::Reference(ObjectRef::Null))?;

    // Execute checkcast
    vm.step()?;

    // null can be cast to any type - should leave null on stack
    let result = vm.stack_top()?.as_reference()?;
    assert!(result.is_null());

    Ok(())
}

// ============================================================
// Control Flow Branch Tests
// ============================================================

#[test]
fn test_getfield_putfield() -> VMResult<()> {
    use crate::interpreter::heap::ClassRef;

    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x2a, // aload_0 - push reference from local 0
            0xb4, 0x00, 0x07, // getfield, CP index = 7 (FieldRef)
        ])
        .build()?;

    // Create object and set field
    let class = ClassRef {
        name: "java/lang/Object".into(),
        instance_field_count: 1,
        static_field_count: 0,
    };
    let obj_id = vm.heap_mut().allocate_object(class)?;

    let field_offset = vm.field_offset("java/lang/Object", "test")?;
    vm.heap_mut()
        .put_field(obj_id, field_offset, Value::Int(42))?;

    // Store object reference in local 0
    vm.set_local(0, Value::Reference(ObjectRef::Object(obj_id)))?;

    // Execute aload_0, getfield
    vm.steps(2)?;

    // Verify the field value was pushed to stack
    let result = vm.stack_top()?.as_int()?;
    assert_eq!(result, 42);

    Ok(())
}

#[test]
fn test_putfield_writes_field() -> VMResult<()> {
    use crate::interpreter::heap::ClassRef;

    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x2a, // aload_0 - push reference from local 0
            0x08, // iconst_5 - push value 5
            0xb5, 0x00, 0x07, // putfield, CP index = 7 (FieldRef)
        ])
        .build()?;

    // Create object
    let class = ClassRef {
        name: "java/lang/Object".into(),
        instance_field_count: 1,
        static_field_count: 0,
    };
    let obj_id = vm.heap_mut().allocate_object(class)?;

    let field_offset = vm.field_offset("java/lang/Object", "test")?;

    // Store object reference in local 0
    vm.set_local(0, Value::Reference(ObjectRef::Object(obj_id)))?;

    // Execute aload_0, iconst_5, putfield
    vm.steps(3)?;

    // Verify the field was written at the correct offset
    let field_value = vm.heap_mut().get_field(obj_id, field_offset)?;
    assert!(field_value.is_some());
    assert_eq!(field_value.unwrap().as_int()?, 5);

    Ok(())
}

#[test]
fn test_getfield_null_reference() -> VMResult<()> {
    // Bytecode: aconst_null, getfield index=7
    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x01, // aconst_null
            0xb4, 0x00, 0x07, // getfield, index = 7
        ])
        .build()?;

    // Execute aconst_null
    vm.step()?;

    // Execute getfield - should throw NullPointerException
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/NullPointerException");

    Ok(())
}

#[test]
fn test_putfield_null_reference() -> VMResult<()> {
    // Bytecode: aconst_null, iconst_1, putfield index=7
    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x01, // aconst_null
            0x04, // iconst_1
            0xb5, 0x00, 0x07, // putfield, index = 7
        ])
        .build()?;

    // Execute aconst_null
    vm.step()?;

    // Execute iconst_1
    vm.step()?;

    // Execute putfield - should throw NullPointerException
    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/NullPointerException");

    Ok(())
}

// ============================================================
// Tableswitch Tests
// ============================================================

#[test]
fn test_new_object() -> VMResult<()> {
    // Bytecode: new (index = 1), return
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new, index = 1
            0xb1, // return
        ])
        .build()?;

    vm.step()?; // new

    // Should have object reference on stack
    let obj_ref = vm.stack_top()?.as_reference()?;
    assert!(!obj_ref.is_null());

    Ok(())
}

#[test]
fn test_checkcast_same_class_integration() -> VMResult<()> {
    // Test checkcast Object as Object - should pass
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (Object)
            0xc0, 0x00, 0x01, // checkcast #1 (Object) - same class
        ])
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        ])
        .build()?;

    // Execute: new, checkcast
    vm.steps(2)?;

    // Stack should still have the object
    assert_eq!(vm.stack_depth(), 1);
    Ok(())
}
