// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::test_vm_builder::TestVMBuilder;
use super::*;

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
    assert!(vm.frame().is_none(), "an uncaught frame must be terminated");

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
fn test_athrow_throwable_validation() {
    // Test that is_instance_of correctly identifies Throwable hierarchy
    let vm = TestVMBuilder::new().build().unwrap();
    let executor = vm.executor();

    // Known Throwable subclasses should match
    assert!(
        executor.is_instance_of("java/lang/Exception", "java/lang/Throwable"),
        "Exception should be instanceof Throwable"
    );
    assert!(
        executor.is_instance_of("java/lang/RuntimeException", "java/lang/Throwable"),
        "RuntimeException should be instanceof Throwable"
    );
    assert!(
        executor.is_instance_of("java/lang/Error", "java/lang/Throwable"),
        "Error should be instanceof Throwable"
    );

    // Throwable itself should match
    assert!(
        executor.is_instance_of("java/lang/Throwable", "java/lang/Throwable"),
        "Throwable should be instanceof Throwable"
    );

    // Non-Throwable classes should NOT match
    // Note: Without MethodArea loaded, is_instance_of falls through to false
    // for classes it can't resolve, which is the correct conservative behavior
    assert!(
        !executor.is_instance_of("java/lang/String", "java/lang/Throwable"),
        "String should NOT be instanceof Throwable"
    );
    assert!(
        !executor.is_instance_of("java/lang/Object", "java/lang/Throwable"),
        "Object should NOT be instanceof Throwable"
    );

    assert!(executor.is_instance_of(
        "java/lang/ArrayIndexOutOfBoundsException",
        "java/lang/RuntimeException"
    ));
    assert!(executor.is_instance_of(
        "java/lang/ArrayIndexOutOfBoundsException",
        "java/lang/Throwable"
    ));
    assert!(!executor.is_instance_of(
        "java/lang/ArrayIndexOutOfBoundsException",
        "java/lang/Error"
    ));

    assert!(executor.is_instance_of(
        "java/lang/NoSuchMethodError",
        "java/lang/IncompatibleClassChangeError"
    ));
    assert!(executor.is_instance_of("java/lang/NoSuchMethodError", "java/lang/LinkageError"));
    assert!(executor.is_instance_of("java/lang/NoSuchMethodError", "java/lang/Throwable"));
    assert!(!executor.is_instance_of("java/lang/NoSuchMethodError", "java/lang/Exception"));
}

#[test]
fn athrow_accepts_authoritative_deep_runtime_and_linkage_hierarchies() -> VMResult<()> {
    for exception_class in [
        "java/lang/ArrayIndexOutOfBoundsException",
        "java/lang/NoSuchMethodError",
    ] {
        let constant_pool = vec![
            ConstantPoolItem::Integer(0),
            ConstantPoolItem::Class { name_index: 2 },
            ConstantPoolItem::Utf8(exception_class.to_string()),
        ];
        let mut vm = TestVMBuilder::new()
            .with_constant_pool(constant_pool)
            .with_bytecode(vec![0xbb, 0x00, 0x01, 0xbf])
            .build()?;

        vm.step()?;
        let result = vm.step();
        match result {
            Err(VMError::UncaughtException {
                exception_class: actual,
                ..
            }) => assert_eq!(actual, exception_class),
            other => panic!("expected uncaught {exception_class}, got {other:?}"),
        }
    }
    Ok(())
}

#[test]
fn emergency_vm_errors_are_real_permanently_rooted_throwables() {
    let executor = Executor::new(Vec::new());
    let mut heap = Heap::new(0);
    let oome = executor
        .materialize_exception(&VMError::MemoryLimitExceeded, &mut heap)
        .unwrap();
    let soe = executor
        .materialize_exception(&VMError::StackOverflow, &mut heap)
        .unwrap();
    assert!(!oome.is_null());
    assert!(!soe.is_null());
    assert_ne!(oome, soe);

    GarbageCollector::collect(&mut heap, &GCRoots::new());

    assert_eq!(
        heap.get_class(oome.heap_id().unwrap()).unwrap().name,
        "java/lang/OutOfMemoryError"
    );
    assert_eq!(
        heap.get_class(soe.heap_id().unwrap()).unwrap().name,
        "java/lang/StackOverflowError"
    );
    assert_eq!(
        executor
            .materialize_exception(&VMError::MemoryLimitExceeded, &mut heap)
            .unwrap(),
        oome
    );
    assert_eq!(
        executor
            .materialize_exception(&VMError::StackOverflow, &mut heap)
            .unwrap(),
        soe
    );
}

// =========================================================================
// Ifnull / Ifnonnull Tests (JVMS 6.5.ifnull, 6.5.ifnonnull)
// =========================================================================
