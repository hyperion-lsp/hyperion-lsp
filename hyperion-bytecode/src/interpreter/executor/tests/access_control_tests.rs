// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::test_vm_builder::TestVMBuilder;
use super::*;

#[test]
fn test_aastore_type_validation() -> VMResult<()> {
    // Test that aastore validates reference type compatibility (JVMS §6.5.aastore)
    use crate::interpreter::heap::{ArrayElementType, ClassRef};
    use crate::interpreter::method_area::LoadedClass;

    // Setup: Create class hierarchy
    // String extends Object
    // Integer extends Object
    // String[] should only accept String or null, not Integer
    let mut class_str = LoadedClass::new("java/lang/String".to_string());
    class_str.set_superclass("java/lang/Object".to_string());

    let mut class_int = LoadedClass::new("java/lang/Integer".to_string());
    class_int.set_superclass("java/lang/Object".to_string());

    let mut vm = TestVMBuilder::new()
        .with_class(LoadedClass::new("java/lang/Object".to_string()))
        .with_class(class_str)
        .with_class(class_int)
        .with_heap_size(10 * 1024)
        .build()?;

    // Allocate String[]
    let string_array_id = vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        5,
    )?;

    // Allocate a String object
    let string_obj_id = vm.heap_mut().allocate_object(ClassRef {
        name: "java/lang/String".into(),
        instance_field_count: 1,
        static_field_count: 0,
    })?;

    // Test 1: Storing String into String[] should succeed
    vm.heap_mut().array_store(
        string_array_id,
        0,
        Value::Reference(ObjectRef::Object(string_obj_id)),
    )?;

    // Test 2: Storing null into String[] should succeed (null is always compatible)
    vm.heap_mut()
        .array_store(string_array_id, 1, Value::Reference(ObjectRef::Null))?;

    // Test 3: Storing Integer into String[] must throw ArrayStoreException.
    // This runs the real aastore instruction, not just the subtype predicate.
    let mut store_vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_bytecode(vec![0x53])
        .with_heap_size(1024 * 1024)
        .build()?;
    let strings = store_vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        5,
    )?;
    let integer = store_vm.heap_mut().allocate_object(ClassRef {
        name: "java/lang/Integer".into(),
        instance_field_count: 1,
        static_field_count: 0,
    })?;
    store_vm.push(Value::Reference(ObjectRef::Array(strings)))?;
    store_vm.push(Value::Int(2))?;
    store_vm.push(Value::Reference(ObjectRef::Object(integer)))?;
    let result = store_vm.step();
    super::assert_uncaught_exception(&result, "java/lang/ArrayStoreException");

    // The predicate itself: Integer is not assignable to String.
    assert!(
        !vm.executor()
            .is_subclass_of("java/lang/Integer", "java/lang/String"),
        "Integer should not be compatible with String"
    );

    // Verify String is compatible with String
    let is_compatible = vm
        .executor()
        .is_subclass_of("java/lang/String", "java/lang/String");
    assert!(is_compatible, "String should be compatible with String");

    // Verify that a subclass would be compatible
    // This tests the principle that aastore should accept subtypes
    let is_compatible = vm
        .executor()
        .is_subclass_of("java/lang/String", "java/lang/Object");
    assert!(
        is_compatible,
        "String should be compatible with Object (superclass)"
    );

    Ok(())
}

#[test]
fn test_check_method_access_private_same_class_allowed() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Private method accessed from same class - should succeed
    let result = vm.executor().check_method_access(
        "com/example/MyClass",
        "com/example/MyClass",
        "privateMethod",
        access_flags::ACC_PRIVATE,
    );
    assert!(
        result.is_ok(),
        "Private method should be accessible from same class"
    );
}

#[test]
fn test_check_method_access_private_different_class_denied() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Private method accessed from different class - should fail
    let result = vm.executor().check_method_access(
        "com/example/Caller",
        "com/example/Target",
        "privateMethod",
        access_flags::ACC_PRIVATE,
    );
    assert!(
        matches!(result, Err(VMError::IllegalAccessError(_))),
        "Private method should NOT be accessible from different class: {:?}",
        result
    );
}

#[test]
fn test_check_method_access_public_always_allowed() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Public method from different class - should succeed
    let result = vm.executor().check_method_access(
        "com/example/Caller",
        "com/example/Target",
        "publicMethod",
        access_flags::ACC_PUBLIC,
    );
    assert!(
        result.is_ok(),
        "Public method should be accessible from any class"
    );
}

#[test]
fn test_check_method_access_package_private_same_package() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Package-private method accessed from same package - should succeed
    let result = vm.executor().check_method_access(
        "com.example.Caller",
        "com.example.Target",
        "packageMethod",
        0, // no access flags = package-private
    );
    assert!(
        result.is_ok(),
        "Package-private method should be accessible from same package"
    );
}

#[test]
fn test_check_method_access_package_private_different_package() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Package-private method accessed from different package - should fail
    let result = vm.executor().check_method_access(
        "com.other.Caller",
        "com.example.Target",
        "packageMethod",
        0, // no access flags = package-private
    );
    assert!(
        matches!(result, Err(VMError::IllegalAccessError(_))),
        "Package-private method should NOT be accessible from different package: {:?}",
        result
    );
}

#[test]
fn test_check_method_access_protected_same_class() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Protected method accessed from same class - should succeed
    let result = vm.executor().check_method_access(
        "com.example.MyClass",
        "com.example.MyClass",
        "protectedMethod",
        access_flags::ACC_PROTECTED,
    );
    assert!(
        result.is_ok(),
        "Protected method should be accessible from same class"
    );
}

#[test]
fn test_check_method_access_protected_same_package() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Protected method accessed from same package - should succeed
    let result = vm.executor().check_method_access(
        "com.example.Caller",
        "com.example.Target",
        "protectedMethod",
        access_flags::ACC_PROTECTED,
    );
    assert!(
        result.is_ok(),
        "Protected method should be accessible from same package"
    );
}

#[test]
fn test_check_method_access_protected_different_package_no_inheritance() {
    let vm = TestVMBuilder::new().build().unwrap();

    // Protected method from different package, no inheritance - should fail
    let result = vm.executor().check_method_access(
        "com.other.Caller",
        "com.example.Target",
        "protectedMethod",
        access_flags::ACC_PROTECTED,
    );
    assert!(
        matches!(result, Err(VMError::IllegalAccessError(_))),
        "Protected method should NOT be accessible from different package without inheritance: {:?}",
        result
    );
}

#[test]
fn test_extract_package() {
    assert_eq!(Executor::extract_package("java.lang.String"), "java.lang");
    assert_eq!(
        Executor::extract_package("com.example.MyClass"),
        "com.example"
    );
    assert_eq!(Executor::extract_package("MyClass"), "");
    assert_eq!(Executor::extract_package("a.b.c.D"), "a.b.c");
}
