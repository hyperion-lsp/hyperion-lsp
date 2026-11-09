// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for the full compilation and execution pipeline
//!
//! These tests verify that we can compile Java source and execute the resulting bytecode.

use hyperion_bytecode::java_compiler::JavaType;
use hyperion_bytecode::java_producer::JavaBytecodeProducer;
use hyperion_bytecode::producer::{BytecodeProducer, JvmLanguage};

#[test]
fn test_compile_simple_integer_literals() {
    // Test that we can compile integer literal expressions
    let producer = JavaBytecodeProducer::new();

    // Simple case: just the infrastructure (we'll add real compilation tests later)
    assert_eq!(producer.language(), JvmLanguage::Java);

    let features = producer.supported_features();
    assert!(!features.is_empty());
}

#[test]
fn test_java_type_descriptors() {
    // Verify that JavaType produces correct JVM descriptors
    assert_eq!(JavaType::Int.descriptor(), "I");
    assert_eq!(JavaType::Long.descriptor(), "J");
    assert_eq!(JavaType::Float.descriptor(), "F");
    assert_eq!(JavaType::Double.descriptor(), "D");
    assert_eq!(JavaType::Void.descriptor(), "V");
    assert_eq!(JavaType::Boolean.descriptor(), "Z");
    assert_eq!(JavaType::Byte.descriptor(), "B");
    assert_eq!(JavaType::Char.descriptor(), "C");
    assert_eq!(JavaType::Short.descriptor(), "S");

    // Object types
    assert_eq!(
        JavaType::Object("java.lang.String".to_string()).descriptor(),
        "Ljava/lang/String;"
    );
    assert_eq!(
        JavaType::Object("java.lang.Object".to_string()).descriptor(),
        "Ljava/lang/Object;"
    );

    // Array types
    let int_array = JavaType::Array {
        element_type: Box::new(JavaType::Int),
        dimensions: 1,
    };
    assert_eq!(int_array.descriptor(), "[I");

    let string_array_2d = JavaType::Array {
        element_type: Box::new(JavaType::Object("java.lang.String".to_string())),
        dimensions: 2,
    };
    assert_eq!(string_array_2d.descriptor(), "[[Ljava/lang/String;");
}

#[test]
fn test_java_type_properties() {
    // Test stack width calculations
    assert_eq!(JavaType::Int.stack_width(), 1);
    assert_eq!(JavaType::Long.stack_width(), 2);
    assert_eq!(JavaType::Double.stack_width(), 2);
    assert_eq!(JavaType::Float.stack_width(), 1);
    assert_eq!(JavaType::Void.stack_width(), 0);

    // Test category2 detection
    assert!(!JavaType::Int.is_category2());
    assert!(JavaType::Long.is_category2());
    assert!(JavaType::Double.is_category2());
    assert!(!JavaType::Float.is_category2());

    // Test numeric type detection
    assert!(JavaType::Int.is_numeric());
    assert!(JavaType::Long.is_numeric());
    assert!(JavaType::Float.is_numeric());
    assert!(JavaType::Double.is_numeric());
    assert!(!JavaType::Void.is_numeric());
    assert!(!JavaType::Object("java.lang.String".to_string()).is_numeric());
}

#[test]
fn test_java_type_assignability() {
    // Primitives
    assert!(JavaType::Int.is_assignable_from(&JavaType::Int));
    assert!(!JavaType::Int.is_assignable_from(&JavaType::Long));

    // Object assignability
    let obj_type = JavaType::Object("java.lang.Object".to_string());
    let string_type = JavaType::Object("java.lang.String".to_string());

    // Object is assignable from everything
    assert!(obj_type.is_assignable_from(&string_type));

    // Same type is assignable
    assert!(string_type.is_assignable_from(&string_type));
}

#[test]
fn test_java_type_array_properties() {
    let int_array = JavaType::Array {
        element_type: Box::new(JavaType::Int),
        dimensions: 1,
    };

    let long_array = JavaType::Array {
        element_type: Box::new(JavaType::Long),
        dimensions: 1,
    };

    // Arrays are not numeric
    assert!(!int_array.is_numeric());

    // Different arrays aren't assignable
    assert!(!int_array.is_assignable_from(&long_array));
}

// TODO: Add real compilation integration tests once we have working class file generation
// This requires finishing Phase 2.4 lambda/invokedynamic support and method type resolution
