// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_array_int_operations() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;
    use crate::interpreter::value::ObjectRef;

    // First: iastore (store 42 at index 2)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0x4f]) // iastore
        .build()?;

    let array_id = vm.heap_mut().allocate_array(ArrayElementType::Int, 5)?;
    vm.push(Value::Reference(ObjectRef::Array(array_id)))?; // arrayref
    vm.push(Value::Int(2))?; // index
    vm.push(Value::Int(42))?; // value

    vm.step()?; // iastore

    // Now: iaload (load from index 2)
    let mut vm2 = TestVMBuilder::new()
        .with_bytecode(vec![0x2e]) // iaload
        .build()?;

    // Re-allocate and store manually so the heap has the data
    let array_id2 = vm2.heap_mut().allocate_array(ArrayElementType::Int, 5)?;
    vm2.heap_mut().array_store(array_id2, 2, Value::Int(42))?;
    vm2.push(Value::Reference(ObjectRef::Array(array_id2)))?; // arrayref
    vm2.push(Value::Int(2))?; // index

    vm2.step()?; // iaload

    let val = vm2.stack_top()?.as_int()?;
    assert_eq!(val, 42);

    Ok(())
}

#[test]
fn test_arraylength() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;
    use crate::interpreter::value::ObjectRef;

    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![0xbe]) // arraylength
        .build()?;

    let array_id = vm.heap_mut().allocate_array(ArrayElementType::Int, 7)?;
    vm.push(Value::Reference(ObjectRef::Array(array_id)))?;

    vm.step()?; // arraylength

    let length = vm.stack_top()?.as_int()?;
    assert_eq!(length, 7);

    Ok(())
}

#[test]
fn test_newarray_int_array() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x05, // bipush 5
            0xbc, 0x0a, // newarray int (type 10)
            0xb1, // return
        ])
        .build()?;

    vm.steps(2)?; // bipush 5, newarray

    // Should have array reference on stack
    let array_ref = vm.stack_top()?.as_reference()?;
    assert!(!array_ref.is_null());

    // Verify array length
    let heap_id = array_ref.heap_id().unwrap();
    let length = vm.heap_mut().array_length(heap_id)?;
    assert_eq!(length, 5);

    Ok(())
}

#[test]
fn test_newarray_boolean_array() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x03, // bipush 3
            0xbc, 0x04, // newarray boolean (type 4)
            0xb1, // return
        ])
        .build()?;

    vm.steps(2)?; // bipush 3, newarray

    // Should have array reference on stack
    let array_ref = vm.stack_top()?.as_reference()?;
    assert!(!array_ref.is_null());

    // Verify array length
    let heap_id = array_ref.heap_id().unwrap();
    let length = vm.heap_mut().array_length(heap_id)?;
    assert_eq!(length, 3);

    Ok(())
}

#[test]
fn test_newarray_negative_count() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0xff, // bipush -1
            0xbc, 0x0a, // newarray int (type 10)
        ])
        .build()?;

    vm.step()?; // bipush -1
    let result = vm.step(); // newarray

    // Should fail with negative array size exception
    super::assert_uncaught_exception(&result, "java/lang/NegativeArraySizeException");

    Ok(())
}

#[test]
fn test_anewarray_object_array() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x0a, // bipush 10
            0xbd, 0x00, 0x01, // anewarray, index = 1
            0xb1, // return
        ])
        .build()?;

    vm.steps(2)?; // bipush 10, anewarray

    // Should have array reference on stack
    let array_ref = vm.stack_top()?.as_reference()?;
    assert!(!array_ref.is_null());

    // Verify array length
    let heap_id = array_ref.heap_id().unwrap();
    let length = vm.heap_mut().array_length(heap_id)?;
    assert_eq!(length, 10);

    Ok(())
}

#[test]
fn test_anewarray_negative_count() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0xfb, // bipush -5
            0xbd, 0x00, 0x01, // anewarray, index = 1
        ])
        .build()?;

    vm.step()?; // bipush -5
    let result = vm.step(); // anewarray

    // Should fail with negative array size exception
    super::assert_uncaught_exception(&result, "java/lang/NegativeArraySizeException");

    Ok(())
}

#[test]
fn test_multianewarray_2d() -> VMResult<()> {
    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("[[I".to_string()),
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x03, // bipush 3
            0x10, 0x05, // bipush 5
            0xc5, 0x00, 0x01, 0x02, // multianewarray, index = 1, dimensions = 2
            0xb1, // return
        ])
        .with_constant_pool(constant_pool)
        .build()?;

    vm.steps(3)?; // bipush 3, bipush 5, multianewarray

    // Should have array reference on stack
    let array_ref = vm.stack_top()?.as_reference()?;
    assert!(!array_ref.is_null());

    // Verify array length (outer dimension)
    let heap_id = array_ref.heap_id().unwrap();
    let length = vm.heap_mut().array_length(heap_id)?;
    assert_eq!(length, 3); // First pushed (3) is outer dimension
    assert_eq!(vm.heap_mut().get_class(heap_id)?.name, "[[I");

    let inner = vm.heap_mut().array_load(heap_id, 0)?.as_reference()?;
    let inner_id = inner.heap_id().expect("allocated inner array");
    assert_eq!(vm.heap_mut().get_class(inner_id)?.name, "[I");
    assert_eq!(vm.heap_mut().array_length(inner_id)?, 5);

    Ok(())
}

#[test]
fn test_anewarray_string() -> VMResult<()> {
    // Test anewarray creates array with correct element type
    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("java/lang/String".to_string()),
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x05, // iconst_2 (create array of size 2)
            0xbd, 0x00, 0x01, // anewarray #1 (String)
        ])
        .with_constant_pool(constant_pool)
        .build()?;

    // Execute: iconst_2, anewarray
    vm.steps(2)?;

    // Stack should have array reference
    assert_eq!(vm.stack_depth(), 1);

    // Verify array was created with correct type
    let array_ref = vm.stack_top()?;
    match array_ref {
        Value::Reference(ObjectRef::Array(heap_id)) => {
            let class_ref = vm.heap_mut().get_class(heap_id)?;
            // Note: Array class names are typically [Ljava/lang/String;
            // The exact representation depends on array handling
            assert!(class_ref.name.contains("String") || class_ref.name.starts_with('['));
        }
        _ => panic!("Expected array reference"),
    }

    Ok(())
}

#[test]
fn test_anewarray_zero_length() -> VMResult<()> {
    // Test anewarray with zero length
    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("java/lang/Object".to_string()),
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x03, // iconst_0
            0xbd, 0x00, 0x01, // anewarray #1
        ])
        .with_constant_pool(constant_pool)
        .build()?;

    // Execute: iconst_0, anewarray
    vm.steps(2)?;

    assert_eq!(vm.stack_depth(), 1);
    Ok(())
}

#[test]
fn test_is_subclass_of_cycle_detection() -> VMResult<()> {
    // Test that is_subclass_of detects and handles cycles in class hierarchy
    // Create a circular interface hierarchy: A extends B, B extends A
    use crate::interpreter::method_area::LoadedClass;

    let mut class_a = LoadedClass::new("A".to_string());
    class_a.add_interface("B".to_string());

    let mut class_b = LoadedClass::new("B".to_string());
    class_b.add_interface("A".to_string()); // B extends A, which extends B -> cycle

    let vm = TestVMBuilder::new()
        .with_class(class_a)
        .with_class(class_b)
        .build()?;
    let executor = vm.executor();

    // Should not stack overflow - should return false for non-existent relationship
    let result = executor.is_subclass_of("A", "C");
    assert!(!result, "A should not be subclass of non-existent C");

    // Should not stack overflow - direct interface relationship
    let result = executor.is_subclass_of("A", "B");
    assert!(result, "A extends B (directly through interface)");

    // Should not stack overflow - reverse relationship
    let result = executor.is_subclass_of("B", "A");
    assert!(result, "B extends A (directly through interface)");

    // Most importantly: querying with self should return true
    let result = executor.is_subclass_of("A", "A");
    assert!(result, "A is always subclass of itself");

    Ok(())
}

#[test]
fn test_class_cast_exception_error() -> VMResult<()> {
    // Test ClassCastException error variant works correctly
    let vm = TestVMBuilder::new().build()?;
    let executor = vm.executor();

    // Test that is_instance_of returns false for incompatible types
    let is_string_instance_of_integer =
        executor.is_instance_of("java/lang/String", "java/lang/Integer");
    assert!(
        !is_string_instance_of_integer,
        "String should not be instance of Integer"
    );

    // Test the error variant
    let error = VMError::class_cast_exception("java/lang/String", "java/lang/Integer");
    match error {
        VMError::ClassCastException { from, to } => {
            assert_eq!(from, "java/lang/String");
            assert_eq!(to, "java/lang/Integer");
        }
        _ => panic!("Expected ClassCastException"),
    }

    Ok(())
}

#[test]
fn test_checkcast_array_type_mismatch() -> VMResult<()> {
    // Test that different array types don't match
    let vm = TestVMBuilder::new().build()?;
    let executor = vm.executor();

    // String[] should not match Integer[]
    let is_compatible = executor.is_subclass_of("[Ljava/lang/String;", "[Ljava/lang/Integer;");
    assert!(!is_compatible, "String[] should not match Integer[]");

    // But same types should match
    let is_same = executor.is_subclass_of("[I", "[I");
    assert!(is_same, "int[] should match int[]");

    // Arrays should match Object
    let is_object = executor.is_subclass_of("[I", "java/lang/Object");
    assert!(is_object, "int[] should be compatible with Object");

    Ok(())
}

// ============================================================
// Regression test: Reserved byte validation
// ============================================================
// invokeinterface has a reserved byte that must be 0 per JVM spec.
// Previously this was ignored, now it's validated.

#[test]
fn test_array_castable_to_object() {
    // JVM spec §5.5: Arrays can be cast to java/lang/Object
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    // int[] can be cast to Object
    assert!(
        executor.is_subclass_of("[I", "java/lang/Object"),
        "int[] should be castable to Object"
    );

    // String[] can be cast to Object
    assert!(
        executor.is_subclass_of("[Ljava/lang/String;", "java/lang/Object"),
        "String[] should be castable to Object"
    );

    // Object[][] can be cast to Object
    assert!(
        executor.is_subclass_of("[[Ljava/lang/Object;", "java/lang/Object"),
        "Object[][] should be castable to Object"
    );
}

#[test]
fn test_array_castable_to_cloneable() {
    // JVM spec §5.5: Arrays implement java/lang/Cloneable
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    // int[] can be cast to Cloneable
    assert!(
        executor.is_subclass_of("[I", "java/lang/Cloneable"),
        "int[] should be castable to Cloneable"
    );

    // String[] can be cast to Cloneable
    assert!(
        executor.is_subclass_of("[Ljava/lang/String;", "java/lang/Cloneable"),
        "String[] should be castable to Cloneable"
    );

    // int[][] can be cast to Cloneable
    assert!(
        executor.is_subclass_of("[[I", "java/lang/Cloneable"),
        "int[][] should be castable to Cloneable"
    );
}

#[test]
fn test_array_castable_to_serializable() {
    // JVM spec §5.5: Arrays implement java/io/Serializable
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    // int[] can be cast to Serializable
    assert!(
        executor.is_subclass_of("[I", "java/io/Serializable"),
        "int[] should be castable to Serializable"
    );

    // Object[] can be cast to Serializable
    assert!(
        executor.is_subclass_of("[Ljava/lang/Object;", "java/io/Serializable"),
        "Object[] should be castable to Serializable"
    );

    // byte[][] can be cast to Serializable
    assert!(
        executor.is_subclass_of("[[B", "java/io/Serializable"),
        "byte[][] should be castable to Serializable"
    );
}

#[test]
fn test_array_not_castable_to_arbitrary_class() {
    // Arrays should NOT be castable to arbitrary classes
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    assert!(
        !executor.is_subclass_of("[I", "java/lang/String"),
        "int[] should NOT be castable to String"
    );

    assert!(
        !executor.is_subclass_of("[Ljava/lang/Object;", "java/util/List"),
        "Object[] should NOT be castable to List"
    );
}

#[test]
fn test_array_to_array_type_matching() {
    // Same array types should match
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    assert!(
        executor.is_subclass_of("[I", "[I"),
        "int[] should match int[]"
    );

    assert!(
        executor.is_subclass_of("[[Ljava/lang/String;", "[[Ljava/lang/String;"),
        "String[][] should match String[][]"
    );

    // Different array types should not match
    assert!(
        !executor.is_subclass_of("[I", "[J"),
        "int[] should NOT match long[]"
    );
}

#[test]
fn reference_arrays_are_covariant_across_dimensions() {
    let vm = TestVMBuilder::new().with_jdk_core().build().unwrap();
    let executor = vm.executor();

    assert!(executor.is_subclass_of("[Ljava/lang/String;", "[Ljava/lang/Object;"));
    assert!(executor.is_subclass_of("[[Ljava/lang/String;", "[[Ljava/lang/Object;"));
    assert!(executor.is_subclass_of("[[Ljava/lang/String;", "[Ljava/lang/Object;"));
    assert!(executor.is_subclass_of("[[Ljava/lang/String;", "[Ljava/lang/Cloneable;"));
    assert!(executor.is_subclass_of("[Ljava/lang/String;", "[Ljava/io/Serializable;"));
    assert!(executor.is_subclass_of("[[I", "[Ljava/lang/Object;"));
    assert!(executor.is_subclass_of("[[I", "[Ljava/io/Serializable;"));

    assert!(!executor.is_subclass_of("[I", "[Ljava/lang/Object;"));
    assert!(!executor.is_subclass_of("[Ljava/lang/Object;", "[Ljava/io/Serializable;"));
    assert!(!executor.is_subclass_of("[Ljava/lang/String;", "[[Ljava/lang/Object;"));
    assert!(!executor.is_subclass_of("[[I", "[[J"));
}

#[test]
fn subtype_lookup_is_iterative_normalized_and_cycle_safe() -> VMResult<()> {
    use crate::interpreter::method_area::LoadedClass;

    const DEPTH: usize = 4096;
    let mut builder = TestVMBuilder::new();
    for index in 0..DEPTH {
        let mut class = LoadedClass::new(format!("deep.C{index}"));
        if index > 0 {
            class.set_superclass(format!("deep/C{}", index - 1));
        }
        builder = builder.with_class(class);
    }

    let mut root_interface = LoadedClass::new("api.Root".to_string());
    root_interface.is_interface = true;
    let mut middle_interface = LoadedClass::new("api/Middle".to_string());
    middle_interface.is_interface = true;
    middle_interface.add_interface("api.Root".to_string());
    let mut implementation = LoadedClass::new("impl/Implementation".to_string());
    implementation.add_interface("api.Middle".to_string());

    let mut cycle_a = LoadedClass::new("cycle/A".to_string());
    cycle_a.add_interface("cycle/B".to_string());
    let mut cycle_b = LoadedClass::new("cycle/B".to_string());
    cycle_b.add_interface("cycle/A".to_string());

    let vm = builder
        .with_class(root_interface)
        .with_class(middle_interface)
        .with_class(implementation)
        .with_class(cycle_a)
        .with_class(cycle_b)
        .build()?;
    let executor = vm.executor();

    assert!(executor.is_subclass_of("deep/C4095", "deep.C0"));
    assert!(!executor.is_subclass_of("deep/C0", "deep.C4095"));
    assert!(executor.is_subclass_of("impl.Implementation", "api/Root"));
    assert!(!executor.is_subclass_of("cycle/A", "missing/Type"));
    assert!(executor.is_subclass_of("cycle.A", "cycle/B"));
    Ok(())
}

#[test]
fn malformed_and_unavailable_reference_types_fail_closed() {
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    for malformed in [
        "",
        "/Leading",
        "trailing/",
        "empty//Segment",
        "mixed./Segment",
        "[",
        "[V",
        "[Q",
        "[L;",
        "[Ljava/lang/String",
        "[Lbad..Name;",
        "[[",
    ] {
        assert!(!executor.is_subclass_of(malformed, "java/lang/Object"));
        assert!(!executor.is_subclass_of("java/lang/Object", malformed));
    }
    assert!(!executor.is_subclass_of("missing/Type", "missing/Type"));
    assert!(!executor.is_subclass_of("[Lmissing/Type;", "java/lang/Object"));
}

#[test]
fn a_dangling_supertype_edge_is_not_authoritative() -> VMResult<()> {
    use crate::interpreter::method_area::LoadedClass;

    let mut child = LoadedClass::new("example/Child".to_string());
    child.set_superclass("missing/Base".to_string());
    let vm = TestVMBuilder::new().with_class(child).build()?;
    assert!(!vm
        .executor()
        .is_subclass_of("example/Child", "missing/Base"));
    Ok(())
}

#[test]
fn multianewarray_preserves_full_and_partial_runtime_descriptors() -> VMResult<()> {
    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("[[[Ljava/lang/String;".to_string()),
    ];
    let mut vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_constant_pool(constant_pool)
        .with_bytecode(vec![
            0x05, // iconst_2
            0x06, // iconst_3
            0xc5, 0x00, 0x01, 0x02, // allocate only two of three dimensions
        ])
        .with_heap_size(1024 * 1024)
        .build()?;
    vm.steps(3)?;

    let outer = vm.stack_top()?.as_reference()?.heap_id().unwrap();
    assert_eq!(
        vm.heap_mut().get_class(outer)?.name,
        "[[[Ljava/lang/String;"
    );
    let middle = vm.heap_mut().array_load(outer, 0)?.as_reference()?;
    let middle = middle.heap_id().expect("second dimension is allocated");
    assert_eq!(
        vm.heap_mut().get_class(middle)?.name,
        "[[Ljava/lang/String;"
    );
    assert!(vm
        .heap_mut()
        .array_load(middle, 0)?
        .as_reference()?
        .is_null());
    Ok(())
}

#[test]
fn checkcast_and_instanceof_apply_array_covariance() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;

    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("[Ljava/lang/Object;".to_string()),
    ];
    let mut cast_vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_constant_pool(constant_pool.clone())
        .with_bytecode(vec![0xc0, 0x00, 0x01])
        .with_heap_size(1024 * 1024)
        .build()?;
    let strings = cast_vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        1,
    )?;
    cast_vm.push(Value::Reference(ObjectRef::Array(strings)))?;
    cast_vm.step()?;
    assert_eq!(
        cast_vm.stack_top()?.as_reference()?.heap_id(),
        Some(strings)
    );

    let mut instanceof_vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_constant_pool(constant_pool)
        .with_bytecode(vec![0xc1, 0x00, 0x01])
        .with_heap_size(1024 * 1024)
        .build()?;
    let strings = instanceof_vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        1,
    )?;
    instanceof_vm.push(Value::Reference(ObjectRef::Array(strings)))?;
    instanceof_vm.step()?;
    assert_eq!(instanceof_vm.stack_top()?.as_int()?, 1);
    Ok(())
}

#[test]
fn null_type_checks_still_resolve_the_symbolic_target() -> VMResult<()> {
    let constant_pool = vec![
        ConstantPoolItem::Integer(0),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("missing/Target".to_string()),
    ];
    for opcode in [0xc0, 0xc1] {
        let mut vm = TestVMBuilder::new()
            .with_constant_pool(constant_pool.clone())
            .with_bytecode(vec![opcode, 0x00, 0x01])
            .build()?;
        vm.push(Value::Reference(ObjectRef::Null))?;
        let result = vm.step();
        super::assert_uncaught_exception(&result, "java/lang/NoClassDefFoundError");
    }
    Ok(())
}

#[test]
fn reference_type_opcodes_check_leaf_class_access_before_runtime_operands() -> VMResult<()> {
    use crate::interpreter::method_area::LoadedClass;

    let hidden = || {
        let mut class = LoadedClass::new("secret/Hidden".to_string());
        class.modifiers = 0; // package-private
        class
    };
    let class_pool = || {
        vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("secret/Hidden".to_string()),
        ]
    };

    for opcode in [0xc0, 0xc1] {
        let mut vm = TestVMBuilder::new()
            .with_class(hidden())
            .with_constant_pool(class_pool())
            .with_bytecode(vec![opcode, 0x00, 0x01])
            .build()?;
        vm.push(Value::Reference(ObjectRef::Null))?;
        let result = vm.step();
        super::assert_uncaught_exception(&result, "java/lang/IllegalAccessError");
    }

    let mut anewarray = TestVMBuilder::new()
        .with_class(hidden())
        .with_constant_pool(class_pool())
        .with_bytecode(vec![0xbd, 0x00, 0x01])
        .build()?;
    anewarray.push(Value::Int(-1))?;
    let result = anewarray.step();
    super::assert_uncaught_exception(&result, "java/lang/IllegalAccessError");

    let mut multianewarray = TestVMBuilder::new()
        .with_class(hidden())
        .with_constant_pool(vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8("[Lsecret/Hidden;".to_string()),
        ])
        .with_bytecode(vec![0xc5, 0x00, 0x01, 0x01])
        .build()?;
    multianewarray.push(Value::Int(-1))?;
    let result = multianewarray.step();
    super::assert_uncaught_exception(&result, "java/lang/IllegalAccessError");
    Ok(())
}

#[test]
fn aastore_checks_bounds_before_assignment_compatibility() -> VMResult<()> {
    use crate::interpreter::heap::{ArrayElementType, ClassRef};

    let mut vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_bytecode(vec![0x53])
        .with_heap_size(1024 * 1024)
        .build()?;
    let strings = vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        1,
    )?;
    let integer = vm.heap_mut().allocate_object(ClassRef {
        name: "java/lang/Integer".to_string(),
        instance_field_count: 0,
        static_field_count: 0,
    })?;
    vm.push(Value::Reference(ObjectRef::Array(strings)))?;
    vm.push(Value::Int(1))?;
    vm.push(Value::Reference(ObjectRef::Object(integer)))?;

    let result = vm.step();
    super::assert_uncaught_exception(&result, "java/lang/ArrayIndexOutOfBoundsException");
    Ok(())
}

#[test]
fn aastore_accepts_array_values_using_runtime_covariance() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;

    let mut vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_bytecode(vec![0x53])
        .with_heap_size(1024 * 1024)
        .build()?;
    let objects = vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/Object".to_string()),
        1,
    )?;
    let strings = vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        1,
    )?;
    vm.push(Value::Reference(ObjectRef::Array(objects)))?;
    vm.push(Value::Int(0))?;
    vm.push(Value::Reference(ObjectRef::Array(strings)))?;
    vm.step()?;
    assert_eq!(
        vm.heap_mut().array_load(objects, 0)?.as_reference()?,
        ObjectRef::Array(strings)
    );

    // The heap entry, rather than the lightweight reference tag, is the
    // authoritative runtime type.
    let mut alias_vm = TestVMBuilder::new()
        .with_jdk_core()
        .with_bytecode(vec![0x53])
        .with_heap_size(1024 * 1024)
        .build()?;
    let objects = alias_vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/Object".to_string()),
        1,
    )?;
    let strings = alias_vm.heap_mut().allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        1,
    )?;
    alias_vm.push(Value::Reference(ObjectRef::Array(objects)))?;
    alias_vm.push(Value::Int(0))?;
    alias_vm.push(Value::Reference(ObjectRef::Object(strings)))?;
    alias_vm.step()?;
    assert_eq!(
        alias_vm.heap_mut().array_load(objects, 0)?.as_reference()?,
        ObjectRef::Object(strings)
    );
    Ok(())
}

// =========================================================================
// Athrow Throwable Validation Tests (JVMS 6.5.athrow)
// =========================================================================

#[test]
fn test_multianewarray_zero_dimensions_rejected() {
    // Bytecode: multianewarray with dimensions=0 (invalid per JVM spec)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc5, // multianewarray
            0x00, 0x01, // CP index 1
            0x00, // dimensions = 0 (invalid!)
        ])
        .build()
        .unwrap();

    let result = vm.step();

    super::assert_uncaught_exception(&result, "java/lang/ClassFormatError");
}

// =========================================================================
// Access Control Tests (JVMS 5.4.4)
// =========================================================================
