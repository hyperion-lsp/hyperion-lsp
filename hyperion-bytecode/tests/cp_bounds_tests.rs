// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Missing Constant Pool Bounds Checks
//!
//! The interpreter must validate that constant pool indices are:
//! 1. Within bounds (1 <= index <= constant_pool.len())
//! 2. Point to the correct entry type (e.g., getstatic expects FieldRef)
//!
//! **Bug**: Interpreter might not validate CP indices, leading to:
//! - Out-of-bounds access (panic or undefined behavior)
//! - Type confusion (reading wrong CP entry type)
//!
//! These tests verify that the interpreter properly validates CP references.

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use test_utils::JavaTestProject;

#[test]
fn test_valid_cp_reference() {
    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class ValidCP {
    public static int VALUE = 42;

    public static int getValue() {
        return VALUE;
    }
}
"#;

    project
        .write_source("", "ValidCP", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("ValidCP")
        .expect("Failed to load class bytes");

    // This should execute successfully
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("ValidCP", &bytes)
        .expect("Failed to load class");

    let result = vm
        .invoke_static("ValidCP", "getValue", "()I", &[])
        .expect("Should execute successfully");

    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_malformed_cp_index_detection() {
    // This test creates a deliberately malformed class file with an out-of-bounds CP index
    // The interpreter should detect this and return an error, not panic

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class TestClass {
    public static int field = 10;

    public static int getField() {
        return field;
    }
}
"#;

    project
        .write_source("", "TestClass", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let mut bytes = project
        .load_class_bytes("TestClass")
        .expect("Failed to load class bytes");

    // Find the getstatic instruction and corrupt its CP index
    // getstatic is 0xB2 followed by 2-byte index
    let mut found = false;
    for i in 0..bytes.len() - 2 {
        if bytes[i] == 0xB2 {
            // Found getstatic - replace with an out-of-bounds index
            // Use 0xFF 0xFF (65535) which is almost certainly out of bounds
            bytes[i + 1] = 0xFF;
            bytes[i + 2] = 0xFF;
            found = true;
            break;
        }
    }

    assert!(found, "Should have found getstatic instruction");

    // Try to execute the corrupted bytecode
    let mut vm = VirtualMachine::new(MemoryConfig::default());

    // Loading might fail or succeed depending on when validation happens
    let load_result = vm.load_class_from_bytes("TestClass", &bytes);

    let error = match load_result {
        Ok(()) => vm
            .invoke_static("TestClass", "getField", "()I", &[])
            .expect_err("execution must reject an out-of-bounds constant-pool index"),
        Err(error) => error,
    };
    let error_msg = format!("{error:?}");

    assert!(
        error_msg.contains("constant")
            || error_msg.contains("index")
            || error_msg.contains("bounds"),
        "Error should be about CP bounds, got: {error_msg}"
    );
}

#[test]
fn test_cp_index_zero() {
    // Index 0 is reserved in the constant pool and should never be referenced
    // This test verifies that the interpreter rejects CP index 0

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class ZeroIndex {
    public static int value = 100;

    public static int getValue() {
        return value;
    }
}
"#;

    project
        .write_source("", "ZeroIndex", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let mut bytes = project
        .load_class_bytes("ZeroIndex")
        .expect("Failed to load class bytes");

    // Find getstatic and set its index to 0
    let mut found = false;
    for i in 0..bytes.len() - 2 {
        if bytes[i] == 0xB2 {
            // getstatic
            bytes[i + 1] = 0x00;
            bytes[i + 2] = 0x00;
            found = true;
            break;
        }
    }

    assert!(found, "Should have found getstatic");

    // This should fail during load or execution
    let mut vm = VirtualMachine::new(MemoryConfig::default());

    let load_result = vm.load_class_from_bytes("ZeroIndex", &bytes);

    if load_result.is_ok() {
        let exec_result = vm.invoke_static("ZeroIndex", "getValue", "()I", &[]);

        assert!(exec_result.is_err(), "Should reject CP index 0");
    }
    // Loading can also fail - that's acceptable
}

#[test]
fn test_multiple_cp_references_all_valid() {
    // Positive test: verify that methods with many CP references work correctly
    // This ensures our bounds checking doesn't have false positives

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class ManyReferences {
    public static int a = 1;
    public static int b = 2;
    public static int c = 3;

    public static int sum() {
        int result = a + b + c;
        return result;
    }
}
"#;

    project
        .write_source("", "ManyReferences", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("ManyReferences")
        .expect("Failed to load class bytes");

    // This should execute successfully - all CP references are valid
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("ManyReferences", &bytes)
        .expect("Failed to load class");

    let result = vm
        .invoke_static("ManyReferences", "sum", "()I", &[])
        .expect("Should execute successfully with all valid CP references");

    assert_eq!(result, Value::Int(6), "1 + 2 + 3 should equal 6");
}

#[test]
fn test_string_constant_references() {
    // Test that string constants in the CP work correctly

    // Skip if javac not available
    if std::process::Command::new("javac")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: javac not available");
        return;
    }

    let project = JavaTestProject::new().expect("Failed to create test project");

    let source = r#"
public class StringTest {
    public static String getMessage() {
        return "Hello, World!";
    }
}
"#;

    project
        .write_source("", "StringTest", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("StringTest")
        .expect("Failed to load class bytes");

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("StringTest", &bytes)
        .expect("Failed to load class");

    let result = vm
        .invoke_static("StringTest", "getMessage", "()Ljava/lang/String;", &[])
        .expect("Should execute successfully");

    // Just verify it returns a reference (string)
    assert!(
        matches!(result, Value::Reference(_)),
        "Should return a reference (String)"
    );
}
