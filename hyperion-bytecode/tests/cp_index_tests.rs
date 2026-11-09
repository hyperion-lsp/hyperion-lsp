// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Constant Pool Index Off-by-One Error
//!
//! The constant pool in Java class files is 1-indexed (index 0 is reserved).
//! When bytecode instructions reference the constant pool (e.g., getstatic, putstatic),
//! they use 1-based indices.
//!
//! **Bug**: Interpreter might subtract 1 from CP index when it shouldn't, or fail to
//! subtract 1 when it should, leading to reading the wrong constant pool entry.
//!
//! These tests verify that:
//! - getstatic reads the correct field from the constant pool
//! - putstatic writes to the correct field
//! - Multiple field references work correctly (no off-by-one errors)

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::parser::parse_class_file;
use test_utils::JavaTestProject;

#[test]
fn test_getstatic_execution() {
    // Integration test: verify getstatic works correctly
    // If CP indices are off-by-one, this will read the wrong field

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
public class StaticFields {
    public static int FIRST_FIELD = 42;
    public static String SECOND_FIELD = "hello";
    public static int THIRD_FIELD = 100;

    public static int getFirst() {
        return FIRST_FIELD;
    }

    public static int getThird() {
        return THIRD_FIELD;
    }
}
"#;

    project
        .write_source("", "StaticFields", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("StaticFields")
        .expect("Failed to load class bytes");

    // Execute to verify CP indices are correct
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("StaticFields", &bytes)
        .expect("Failed to load class");

    // If CP index is off-by-one, these will return wrong values
    let result1 = vm
        .invoke_static("StaticFields", "getFirst", "()I", &[])
        .expect("getFirst should succeed");

    assert_eq!(result1, Value::Int(42), "Should read FIRST_FIELD correctly");

    let result2 = vm
        .invoke_static("StaticFields", "getThird", "()I", &[])
        .expect("getThird should succeed");

    assert_eq!(
        result2,
        Value::Int(100),
        "Should read THIRD_FIELD correctly"
    );
}

#[test]
fn test_putstatic_execution() {
    // Integration test: verify putstatic works correctly

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
public class StaticWrites {
    public static int counter = 0;
    public static int value = 5;

    public static void incrementCounter() {
        counter = counter + 1;
    }

    public static int getCounter() {
        return counter;
    }

    public static void setValue(int newValue) {
        value = newValue;
    }

    public static int getValue() {
        return value;
    }
}
"#;

    project
        .write_source("", "StaticWrites", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("StaticWrites")
        .expect("Failed to load class bytes");

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("StaticWrites", &bytes)
        .expect("Failed to load class");

    // Increment counter and verify it works
    vm.invoke_static("StaticWrites", "incrementCounter", "()V", &[])
        .expect("incrementCounter should succeed");

    let counter = vm
        .invoke_static("StaticWrites", "getCounter", "()I", &[])
        .expect("getCounter should succeed");

    assert_eq!(counter, Value::Int(1), "Counter should be incremented to 1");

    // Set value and verify
    vm.invoke_static("StaticWrites", "setValue", "(I)V", &[Value::Int(999)])
        .expect("setValue should succeed");

    let value = vm
        .invoke_static("StaticWrites", "getValue", "()I", &[])
        .expect("getValue should succeed");

    assert_eq!(value, Value::Int(999), "Value should be set to 999");
}

#[test]
fn test_multiple_field_references() {
    // Test with many fields to catch off-by-one errors
    // If indices are wrong, we'll get incorrect field values

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
public class MultiFields {
    public static int A = 1;
    public static int B = 2;
    public static int C = 3;
    public static int D = 4;
    public static int E = 5;

    public static int getA() { return A; }
    public static int getB() { return B; }
    public static int getC() { return C; }
    public static int getD() { return D; }
    public static int getE() { return E; }

    public static int sumAll() {
        return A + B + C + D + E;
    }
}
"#;

    project
        .write_source("", "MultiFields", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("MultiFields")
        .expect("Failed to load class bytes");

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("MultiFields", &bytes)
        .expect("Failed to load class");

    // Verify each field individually
    assert_eq!(
        vm.invoke_static("MultiFields", "getA", "()I", &[]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        vm.invoke_static("MultiFields", "getB", "()I", &[]).unwrap(),
        Value::Int(2)
    );
    assert_eq!(
        vm.invoke_static("MultiFields", "getC", "()I", &[]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        vm.invoke_static("MultiFields", "getD", "()I", &[]).unwrap(),
        Value::Int(4)
    );
    assert_eq!(
        vm.invoke_static("MultiFields", "getE", "()I", &[]).unwrap(),
        Value::Int(5)
    );

    // Verify sum (tests multiple getstatic in one method)
    let sum = vm
        .invoke_static("MultiFields", "sumAll", "()I", &[])
        .expect("sumAll should succeed");

    assert_eq!(sum, Value::Int(15), "Sum should be 1+2+3+4+5=15");
}

#[test]
fn test_bytecode_has_getstatic() {
    // Verify the bytecode structure contains getstatic instructions
    // This test checks the bytecode is generated correctly

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
public class BytecodeTest {
    public static int field = 42;

    public static int getField() {
        return field;
    }
}
"#;

    project
        .write_source("", "BytecodeTest", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("BytecodeTest")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "getField")
        .expect("getField not found");

    let code_attr = method
        .attributes
        .iter()
        .find_map(|attr| {
            if let cafebabe::attributes::AttributeData::Code(code) = &attr.data {
                Some(code)
            } else {
                None
            }
        })
        .expect("Code attribute not found");

    // Verify getstatic instruction (0xB2) is present
    let bytecode = &code_attr.code;
    let has_getstatic = bytecode.contains(&0xB2);

    assert!(
        has_getstatic,
        "Method accessing static field should contain getstatic instruction"
    );

    // Verify the index after getstatic is valid (> 0)
    for i in 0..bytecode.len() - 2 {
        if bytecode[i] == 0xB2 {
            let cp_index = u16::from_be_bytes([bytecode[i + 1], bytecode[i + 2]]);
            assert!(
                cp_index > 0,
                "Constant pool index should be > 0 (1-indexed), got {}",
                cp_index
            );
        }
    }
}
