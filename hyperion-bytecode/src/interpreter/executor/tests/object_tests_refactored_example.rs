// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Refactored object tests showing TestVMBuilder pattern
//!
//! Compare this file to object_tests.rs to see the difference.
//! These tests are more declarative and easier to understand.

use super::*;

#[test]
fn test_getfield_putfield_refactored() -> VMResult<()> {
    use crate::interpreter::heap::ClassRef;

    // OLD WAY (from object_tests.rs):
    // let (method_area, field_offsets) = create_test_class_with_fields(...);
    // let test_field_offset = field_offsets["test"];
    // [manual heap allocation]
    // [manual executor setup with .with_method_area()]

    // NEW WAY:
    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x2a, // aload_0 - push reference from local 0
            0xb4, 0x00, 0x07, // getfield, CP index = 7 (FieldRef)
        ])
        .build()?;

    // Create object on heap (test still needs manual object creation)
    let class = ClassRef {
        name: "java/lang/Object".into(),
        instance_field_count: 1,
        static_field_count: 0,
    };
    let obj_id = vm.heap_mut().allocate_object(class)?;

    // Set field value
    let field_offset = vm.field_offset("java/lang/Object", "test")?;
    vm.heap_mut().put_field(obj_id, field_offset, Value::Int(42))?;

    // Store object ref in local 0
    vm.set_local(0, Value::Reference(ObjectRef::Object(obj_id)))?;

    // Execute: aload_0, getfield
    vm.steps(2)?;

    // Verify
    let result = vm.stack_top()?.as_int()?;
    assert_eq!(result, 42);

    Ok(())
}

#[test]
fn test_putfield_refactored() -> VMResult<()> {
    use crate::interpreter::heap::ClassRef;

    let mut vm = TestVMBuilder::new()
        .with_class_having_fields("java/lang/Object", &[("test", "I")])
        .with_bytecode(vec![
            0x2a, // aload_0
            0x08, // iconst_5
            0xb5, 0x00, 0x07, // putfield, CP index = 7
        ])
        .build()?;

    let class = ClassRef {
        name: "java/lang/Object".into(),
        instance_field_count: 1,
        static_field_count: 0,
    };
    let obj_id = vm.heap_mut().allocate_object(class)?;
    vm.set_local(0, Value::Reference(ObjectRef::Object(obj_id)))?;

    // Execute: aload_0, iconst_5, putfield
    vm.steps(3)?;

    // Verify the field was written
    let field_offset = vm.field_offset("java/lang/Object", "test")?;
    let field_value = vm.heap_mut().get_field(obj_id, field_offset)?;
    assert_eq!(field_value.unwrap().as_int()?, 5);

    Ok(())
}

#[test]
fn test_new_object_with_jdk_core() -> VMResult<()> {
    // This test demonstrates using JDK core stubs
    // The `new` instruction requires java/lang/Object to exist

    let mut vm = TestVMBuilder::new()
        .with_jdk_core() // Includes java.lang.Object, String, Class
        .with_bytecode(vec![
            0xbb, 0x00, 0x01, // new #1 (Object)
        ])
        .build()?;

    vm.step()?; // new

    // Should have object reference on stack
    let obj_ref = vm.stack_top()?.as_reference()?;
    assert!(!obj_ref.is_null());

    Ok(())
}

// This shows the pattern for tests that need multiple classes
#[test]
fn test_multiple_classes_interaction() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_class_having_fields(
            "com/example/User",
            &[("name", "Ljava/lang/String;"), ("age", "I")]
        )
        .with_class_having_methods(
            "com/example/Service",
            &[("process", "(Lcom/example/User;)V")]
        )
        .with_bytecode(vec![/* ... */])
        .build()?;

    // Test logic here...
    Ok(())
}
