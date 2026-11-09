// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Re-export common test utilities
// tests/mod.rs is included via executor_tests.rs, which is included as a test module in executor/mod.rs
// So super:: goes to executor_tests.rs parent scope, which is executor/mod.rs
pub use super::super::*;
pub use crate::interpreter::Frame;

/// Create a minimal constant pool for testing
/// Provides basic structure with Object class and method references
pub fn create_minimal_cp() -> Vec<ConstantPoolItem> {
    vec![
        // Index 0: unused (JVM spec)
        ConstantPoolItem::Utf8(String::new()),
        // Index 1: Class - java/lang/Object
        ConstantPoolItem::Class { name_index: 2 },
        // Index 2: Utf8 - "java/lang/Object"
        ConstantPoolItem::Utf8("java/lang/Object".to_string()),
        // Index 3: NameAndType for test method
        ConstantPoolItem::NameAndType {
            name_index: 4,
            descriptor_index: 5,
        },
        // Index 4: Utf8 - "test"
        ConstantPoolItem::Utf8("test".to_string()),
        // Index 5: Utf8 - "()V" (void method)
        ConstantPoolItem::Utf8("()V".to_string()),
        // Index 6: MethodRef for invokevirtual/invokespecial
        ConstantPoolItem::MethodRef {
            class_index: 1,
            name_and_type_index: 3,
        },
        // Index 7: FieldRef for getfield/putfield
        ConstantPoolItem::FieldRef {
            class_index: 1,
            name_and_type_index: 8,
        },
        // Index 8: NameAndType for the integer test field
        ConstantPoolItem::NameAndType {
            name_index: 4,
            descriptor_index: 9,
        },
        // Index 9: Utf8 - "I"
        ConstantPoolItem::Utf8("I".to_string()),
    ]
}

/// Create a constant pool for exception testing (with java/lang/Exception)
pub fn create_exception_cp() -> Vec<ConstantPoolItem> {
    vec![
        // Index 0: unused (JVM spec)
        ConstantPoolItem::Utf8(String::new()),
        // Index 1: Class - java/lang/Exception (a Throwable)
        ConstantPoolItem::Class { name_index: 2 },
        // Index 2: Utf8 - "java/lang/Exception"
        ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
    ]
}

mod test_vm_builder;
pub use test_vm_builder::TestVMBuilder;

/// Helper to assert that a result contains an uncaught exception of a specific class.
/// This is needed because catchable exceptions now go through exception handling,
/// and if no handler is found, they become UncaughtException.
pub fn assert_uncaught_exception<T: std::fmt::Debug>(
    result: &Result<T, crate::interpreter::errors::VMError>,
    expected_class: &str,
) {
    match result {
        Err(crate::interpreter::errors::VMError::UncaughtException {
            exception_class, ..
        }) => {
            assert_eq!(
                exception_class, expected_class,
                "Expected {} exception, got {}",
                expected_class, exception_class
            );
        }
        other => panic!(
            "Expected UncaughtException({}), got {:?}",
            expected_class, other
        ),
    }
}

// Test modules
mod access_control_tests;
mod arithmetic_tests;
mod array_tests;
mod control_flow_tests;
mod exception_tests;
mod method_tests;
mod misc_tests;
mod object_tests;
mod sync_tests;
