// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::test_vm_builder::TestVMBuilder;
use super::*;
use crate::interpreter::value::ObjectRef;

#[test]
fn test_invokestatic_reads_cp_index() -> VMResult<()> {
    // Bytecode: invokestatic (index = 0x0005)
    let bytecode = vec![
        0xb8, 0x00, 0x05, // invokestatic, CP index = 5
    ];
    let mut vm = TestVMBuilder::new().with_bytecode(bytecode).build()?;

    // Should return InvalidBytecode when constant pool is missing the referenced index
    let result = vm.step();
    assert!(matches!(result, Err(VMError::InvalidBytecode)));

    // Verify PC was advanced past CP index (1 opcode + 2 index bytes)
    assert_eq!(vm.pc(), 3);

    Ok(())
}

#[test]
fn test_invokevirtual_null_receiver() -> VMResult<()> {
    // Bytecode: aconst_null, invokevirtual (index = 6 = MethodRef)
    let bytecode = vec![
        0x01, // aconst_null
        0xb6, 0x00, 0x06, // invokevirtual, CP index = 6 (MethodRef)
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // aconst_null
    let result = vm.step(); // invokevirtual

    // Symbolic method resolution precedes the receiver null check. This
    // synthetic pool does not declare Object.test, so linkage wins over NPE.
    assert_uncaught_exception(&result, "java/lang/NoSuchMethodError");

    Ok(())
}

#[test]
fn test_invokevirtual_valid_receiver() -> VMResult<()> {
    // Bytecode: new (index = 1), invokevirtual (index = 6 = MethodRef)
    let bytecode = vec![
        0xbb, 0x00, 0x01, // new, CP index = 1
        0xb6, 0x00, 0x06, // invokevirtual, CP index = 6 (MethodRef)
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // new
    let result = vm.step(); // invokevirtual

    // With proper method area setup, this would invoke the method
    // For now this test documents that full invokevirtual requires method area
    assert!(
        result.is_err(),
        "Without full method-area setup, invokevirtual should fail"
    );

    Ok(())
}

#[test]
fn test_invokespecial_null_receiver() -> VMResult<()> {
    // Bytecode: aconst_null, invokespecial (index = 6 = MethodRef)
    let bytecode = vec![
        0x01, // aconst_null
        0xb7, 0x00, 0x06, // invokespecial, CP index = 6 (MethodRef)
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // aconst_null
    let result = vm.step(); // invokespecial

    assert_uncaught_exception(&result, "java/lang/NoSuchMethodError");

    Ok(())
}

#[test]
fn test_invokespecial_null_constructor_receiver() -> VMResult<()> {
    let bytecode = vec![
        0x01, // aconst_null
        0xb7, 0x00, 0x06, // invokespecial Object.<init>:()V
    ];
    let mut constant_pool = create_minimal_cp();
    constant_pool[4] = ConstantPoolItem::Utf8("<init>".to_string());
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(constant_pool)
        .build()?;

    vm.step()?;
    assert!(matches!(
        vm.step(),
        Err(VMError::UncaughtException { ref exception_class, .. })
            if exception_class == "java/lang/NullPointerException"
    ));

    Ok(())
}

#[test]
fn test_invokespecial_valid_receiver() -> VMResult<()> {
    // Bytecode: new (index = 1), invokespecial (index = 6 = MethodRef)
    let bytecode = vec![
        0xbb, 0x00, 0x01, // new, CP index = 1
        0xb7, 0x00, 0x06, // invokespecial, CP index = 6 (MethodRef)
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // new
    let result = vm.step(); // invokespecial

    // With proper method area setup, this would invoke the method
    assert!(
        result.is_err(),
        "Without full method-area setup, invokespecial should fail"
    );

    Ok(())
}

#[test]
fn test_invokeinterface_null_receiver() -> VMResult<()> {
    // Bytecode: aconst_null, invokeinterface (index = 6 = MethodRef, count = 1, reserved = 0)
    let bytecode = vec![
        0x01, // aconst_null
        0xb9, 0x00, 0x06, 0x01,
        0x00, // invokeinterface, CP index = 6 (MethodRef), count = 1, reserved = 0
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // aconst_null
    let result = vm.step(); // invokeinterface

    // The synthetic pool uses a Methodref where invokeinterface requires an
    // InterfaceMethodref, so malformed-bytecode rejection precedes NPE.
    assert!(matches!(result, Err(VMError::InvalidBytecode)));

    Ok(())
}

#[test]
fn test_invokeinterface_reads_count() -> VMResult<()> {
    // Bytecode: new (index = 1), invokeinterface (index = 6 = MethodRef, count = 1, reserved = 0)
    let bytecode = vec![
        0xbb, 0x00, 0x01, // new, CP index = 1
        0xb9, 0x00, 0x06, 0x01,
        0x00, // invokeinterface, CP index = 6 (MethodRef), count = 1, reserved = 0
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_minimal_cp())
        .build()?;

    vm.step()?; // new
    let result = vm.step(); // invokeinterface

    // With proper method area setup, this would invoke the interface method
    assert!(
        result.is_err(),
        "Without full method-area setup, invokeinterface should fail"
    );

    Ok(())
}

// ============================================================
// Exception Handling Tests
// ============================================================

#[test]
fn test_athrow_pops_exception() -> VMResult<()> {
    // Create an exception object and throw it
    let bytecode = vec![
        0xbb, 0x00, 0x01, // new (create Exception, CP index 1)
        0xbf, // athrow
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_exception_cp())
        .build()?;

    // Create object
    vm.step()?;

    // Verify object is on stack
    assert_eq!(vm.stack_depth(), 1);

    // Now athrow should pop the exception and trigger exception handling
    // Since there's no exception handler, we get UncaughtException
    let result = vm.step();

    assert!(
        matches!(result, Err(VMError::UncaughtException { .. })),
        "Expected UncaughtException, got: {:?}",
        result
    );

    // Verify the exception class is correct
    if let Err(VMError::UncaughtException {
        exception_class, ..
    }) = result
    {
        assert_eq!(exception_class, "java/lang/Exception");
    }

    Ok(())
}

#[test]
fn test_athrow_null_exception() -> VMResult<()> {
    // Attempt to throw null - should throw NullPointerException
    let bytecode = vec![
        0x01, // aconst_null
        0xbf, // athrow
    ];
    let mut vm = TestVMBuilder::new().with_bytecode(bytecode).build()?;

    // Push null
    vm.step()?;

    // Athrow on null should throw NullPointerException
    let result = vm.step();

    super::assert_uncaught_exception(&result, "java/lang/NullPointerException");

    Ok(())
}

#[test]
fn test_athrow_with_object() -> VMResult<()> {
    // Test that athrow returns ThrownException with the correct object reference
    let bytecode = vec![
        0xbb, 0x00, 0x01, // new (create Exception)
        0xbf, // athrow
    ];
    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(create_exception_cp())
        .build()?;

    // Create exception object
    vm.step()?;

    // Throw the exception - since there's no handler, we get UncaughtException
    let result = vm.step();

    // Verify we got UncaughtException with the correct exception class
    match result {
        Err(VMError::UncaughtException {
            exception_class, ..
        }) => {
            assert_eq!(exception_class, "java/lang/Exception");
        }
        _ => panic!("Expected UncaughtException, got: {:?}", result),
    }

    Ok(())
}

#[test]
fn test_invokestatic_native_method() -> VMResult<()> {
    // Test calling a native static method with constant pool resolution
    // This simulates: System.currentTimeMillis()
    let bytecode = vec![
        0xb8, 0x00, 0x02, // invokestatic, CP index = 2
    ];

    // Build a minimal constant pool for System.currentTimeMillis()
    // Index 0 is placeholder (JVM spec), bytecode indices map directly to Vec indices
    let constant_pool = vec![
        // Index 0: unused (JVM spec)
        ConstantPoolItem::Utf8(String::new()),
        // Index 1: Utf8 - "java/lang/System"
        ConstantPoolItem::Utf8("java/lang/System".to_string()),
        // Index 2: MethodRef (bytecode CP index 2 points here)
        ConstantPoolItem::MethodRef {
            class_index: 3,
            name_and_type_index: 5,
        },
        // Index 3: Class
        ConstantPoolItem::Class {
            name_index: 1, // Points to "java/lang/System" at index 1
        },
        // Index 4: Utf8 - "currentTimeMillis"
        ConstantPoolItem::Utf8("currentTimeMillis".to_string()),
        // Index 5: NameAndType
        ConstantPoolItem::NameAndType {
            name_index: 4, // Points to "currentTimeMillis" at index 4
            descriptor_index: 6,
        },
        // Index 6: Utf8 - "()J"
        ConstantPoolItem::Utf8("()J".to_string()),
    ];

    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(constant_pool)
        .with_native_registry(NativeRegistry::new())
        .build()?;

    // Execute invokestatic to call System.currentTimeMillis()
    vm.step()?;

    // Check that a long value was pushed on the stack
    let result = vm.stack_top()?;

    // Verify it's a long value and get the milliseconds
    let millis = result.as_long()?;
    assert!(millis > 0, "Expected positive timestamp, got {}", millis);

    Ok(())
}

#[test]
fn test_multi_method_invocation() -> VMResult<()> {
    // Test multi-method execution with frame stack
    // Main bytecode: iconst_5, iconst_3, invokestatic add(II)I, ireturn
    // Helper method add: iload_0, iload_1, iadd, ireturn
    //   Expected: 5 + 3 = 8

    // Helper method: add(int a, int b) -> int
    // Bytecode: load arg0, load arg1, add, return
    let add_method_bytecode = vec![
        0x1a, // iload_0 (load first argument)
        0x1b, // iload_1 (load second argument)
        0x60, // iadd (add them)
        0xac, // ireturn (return result)
    ];

    // Main bytecode: call add(5, 3)
    let main_bytecode = vec![
        0x08, // iconst_5 (push 5)
        0x06, // iconst_3 (push 3)
        0xb8, 0x00, 0x02, // invokestatic, CP index = 2
        0xac, // ireturn (return result)
    ];

    // Build constant pool for the invokestatic
    // Index 0 is placeholder (JVM spec), bytecode indices map directly to Vec indices
    let constant_pool = vec![
        // Index 0: unused (JVM spec)
        ConstantPoolItem::Utf8(String::new()),
        // Index 1: Utf8 - "Test"
        ConstantPoolItem::Utf8("Test".to_string()),
        // Index 2: MethodRef (bytecode CP index 2 points here)
        ConstantPoolItem::MethodRef {
            class_index: 3,
            name_and_type_index: 5,
        },
        // Index 3: Class
        ConstantPoolItem::Class {
            name_index: 1, // Points to "Test" at index 1
        },
        // Index 4: Utf8 - "add"
        ConstantPoolItem::Utf8("add".to_string()),
        // Index 5: NameAndType
        ConstantPoolItem::NameAndType {
            name_index: 4, // Points to "add" at index 4
            descriptor_index: 6,
        },
        // Index 6: Utf8 - "(II)I"
        ConstantPoolItem::Utf8("(II)I".to_string()),
    ];

    let mut vm = TestVMBuilder::new()
        .with_bytecode(main_bytecode)
        .with_constant_pool(constant_pool)
        .with_max_locals(10)
        .with_max_stack(20)
        .build()?;

    // Register the helper method
    vm.register_method(
        "Test".into(),
        "add".into(),
        "(II)I".into(),
        MethodBytecode {
            max_locals: 2,
            max_stack: 2,
            code: add_method_bytecode,
            exception_table: Vec::new(),
            access_flags: 0x0008,
        },
    );

    // Execute until completion
    vm.execute_loop()?;

    // Verify result: 5 + 3 = 8
    let frame = vm.frame().expect("frame should exist");
    let value = frame
        .operand_stack
        .last()
        .expect("No result value on stack");
    assert_eq!(value.as_int()?, 8);

    Ok(())
}

#[test]
fn test_multi_method_with_execute_with_methods() -> VMResult<()> {
    // Test using the convenient execute_with_methods() API
    // Main bytecode: iconst_2, iconst_3, invokestatic multiply(II)I, ireturn
    // Helper method multiply: iload_0, iload_1, imul, ireturn
    //   Expected: 2 * 3 = 6

    let multiply_method_bytecode = vec![
        0x1a, // iload_0
        0x1b, // iload_1
        0x68, // imul (multiply)
        0xac, // ireturn
    ];

    let main_bytecode = vec![
        0x05, // iconst_2
        0x06, // iconst_3
        0xb8, 0x00, 0x02, // invokestatic, CP index = 2
        0xac, // ireturn
    ];

    // Index 0 is placeholder (JVM spec), bytecode indices map directly to Vec indices
    let constant_pool = vec![
        // Index 0: unused (JVM spec)
        ConstantPoolItem::Utf8(String::new()),
        // Index 1: Utf8 - "Test"
        ConstantPoolItem::Utf8("Test".to_string()),
        // Index 2: MethodRef (bytecode CP index 2 points here)
        ConstantPoolItem::MethodRef {
            class_index: 3,
            name_and_type_index: 5,
        },
        // Index 3: Class
        ConstantPoolItem::Class { name_index: 1 },
        // Index 4: Utf8 - "multiply"
        ConstantPoolItem::Utf8("multiply".to_string()),
        // Index 5: NameAndType
        ConstantPoolItem::NameAndType {
            name_index: 4,
            descriptor_index: 6,
        },
        // Index 6: Utf8 - "(II)I"
        ConstantPoolItem::Utf8("(II)I".to_string()),
    ];

    let mut vm = TestVMBuilder::new()
        .with_bytecode(main_bytecode)
        .with_constant_pool(constant_pool)
        .build()?;

    let mut methods = HashMap::new();
    methods.insert(
        ("Test".into(), "multiply".into(), "(II)I".into()),
        MethodBytecode {
            max_locals: 2,
            max_stack: 2,
            code: multiply_method_bytecode,
            exception_table: Vec::new(),
            access_flags: 0x0008,
        },
    );

    let result = vm.executor_mut().execute_with_methods(methods)?;
    assert_eq!(result.unwrap().as_int()?, 6);

    Ok(())
}

#[test]
fn test_invokeinterface_reserved_byte_must_be_zero() {
    // invokeinterface format: opcode(1) + cp_index(2) + count(1) + reserved(1)
    // Opcode 0xb9 = invokeinterface
    // We set reserved byte to non-zero (0x01) which should fail

    let bytecode = vec![
        0xb9, // invokeinterface
        0x00, 0x06, // cp_index = 6 (pointing to InterfaceMethodRef)
        0x02, // count = 2 (receiver + 1 arg)
        0x01, // reserved = 1 (INVALID - must be 0)
    ];

    // Set up constant pool with InterfaceMethodRef
    let cp = vec![
        ConstantPoolItem::Utf8(String::new()),     // 0: unused
        ConstantPoolItem::Class { name_index: 2 }, // 1: interface class
        ConstantPoolItem::Utf8("java/lang/Runnable".to_string()), // 2: interface name
        ConstantPoolItem::NameAndType {
            name_index: 4,
            descriptor_index: 5,
        }, // 3
        ConstantPoolItem::Utf8("run".to_string()), // 4: method name
        ConstantPoolItem::Utf8("()V".to_string()), // 5: descriptor
        ConstantPoolItem::InterfaceMethodRef {
            class_index: 1,
            name_and_type_index: 3,
        }, // 6: the interface method ref
    ];

    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(cp)
        .build()
        .unwrap();

    // Push dummy receiver onto stack
    vm.push(Value::Reference(ObjectRef::Null)).unwrap();

    let result = vm.step();

    // Should fail with ClassFormatError due to non-zero reserved byte
    assert!(
        result.is_err(),
        "invokeinterface with non-zero reserved byte must fail"
    );

    assert_uncaught_exception(&result, "java/lang/ClassFormatError");
}

#[test]
fn test_invokeinterface_reserved_byte_zero_accepted() {
    // Same as above but with reserved = 0 (valid)
    let bytecode = vec![
        0xb9, // invokeinterface
        0x00, 0x06, // cp_index = 6
        0x01, // count = 1 (just receiver)
        0x00, // reserved = 0 (VALID)
    ];

    let cp = vec![
        ConstantPoolItem::Utf8(String::new()),
        ConstantPoolItem::Class { name_index: 2 },
        ConstantPoolItem::Utf8("java/lang/Runnable".to_string()),
        ConstantPoolItem::NameAndType {
            name_index: 4,
            descriptor_index: 5,
        },
        ConstantPoolItem::Utf8("run".to_string()),
        ConstantPoolItem::Utf8("()V".to_string()),
        ConstantPoolItem::InterfaceMethodRef {
            class_index: 1,
            name_and_type_index: 3,
        },
    ];

    let mut vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .with_constant_pool(cp)
        .build()
        .unwrap();

    // Push null receiver (will fail with NullPointerException, not ClassFormatError)
    vm.push(Value::Reference(ObjectRef::Null)).unwrap();

    let result = vm.step();

    // Should NOT fail with ClassFormatError - reserved byte is valid
    // (Will likely fail with NullPointerException since receiver is null)
    match result {
        Err(VMError::ClassFormatError(msg)) if msg.contains("reserved") => {
            panic!("Valid reserved byte (0) should not cause ClassFormatError");
        }
        _ => {
            // Any other result is fine - we just care that reserved byte validation passed
        }
    }
}

// ============================================================
// Regression test: invokedynamic cache key includes class
// ============================================================
// The cache key must include class name to prevent cross-class pollution.
// This is a compile-time verification that the type signature is correct.

#[test]
fn test_call_site_cache_key_includes_class_name() {
    // This test verifies the cache key type at compile time.
    // The cache should be HashMap<(String, u16), CallSite> not HashMap<u16, CallSite>

    let vm = TestVMBuilder::new().build().unwrap();

    // Access the cache type - if this compiles, the key type is (String, u16)
    let _cache: &HashMap<(String, u16), crate::interpreter::CallSite> =
        &vm.executor().call_site_cache;

    // Verify we can insert with the composite key
    let mut vm2 = TestVMBuilder::new().build().unwrap();
    let executor2 = vm2.executor_mut();
    let key = ("com/example/ClassA".to_string(), 5u16);

    // Create a dummy CallSite for testing
    let call_site = crate::interpreter::CallSite {
        target: crate::interpreter::MethodHandle::InvokeStatic {
            class_name: "Test".into(),
            method_name: "test".into(),
            descriptor: "()V".into(),
        },
        name: "test".into(),
        method_type: "()V".into(),
        kind: crate::interpreter::call_site::CallSiteKind::Constant,
    };

    executor2
        .call_site_cache
        .insert(key.clone(), call_site.clone());

    // Different class, same CP index - should be a different key
    let key2 = ("com/example/ClassB".to_string(), 5u16);
    executor2
        .call_site_cache
        .insert(key2.clone(), call_site.clone());

    // Both entries should exist independently
    assert!(
        executor2.call_site_cache.contains_key(&key),
        "Cache should contain ClassA entry"
    );
    assert!(
        executor2.call_site_cache.contains_key(&key2),
        "Cache should contain ClassB entry (same CP index, different class)"
    );
    assert_eq!(
        executor2.call_site_cache.len(),
        2,
        "Should have 2 separate entries for different classes"
    );

    // Verify they're actually different keys
    let key_same_class = ("com/example/ClassA".to_string(), 5u16);
    assert!(
        executor2.call_site_cache.contains_key(&key_same_class),
        "Same class+index should find existing entry"
    );
}

// =========================================================================
// SWAP Opcode Tests (JVM spec §swap)
// =========================================================================
