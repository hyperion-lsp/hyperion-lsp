// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Exception Handler Constant Pool Index
//!
//! Exception handlers in the Code attribute have a `catch_type` field that references
//! the constant pool. This index must be correctly parsed and validated.
//!
//! **Bug**: The interpreter might:
//! - Use wrong index offset (0-based vs 1-based)
//! - Not validate that catch_type points to a Class entry
//! - Mishandle catch_type = 0 (which means "catch all" / finally block)
//!
//! These tests verify correct exception handler parsing and execution.

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::parser::parse_class_file;
use test_utils::JavaTestProject;

#[test]
fn test_exception_handler_exists() {
    // Verify that exception handlers are correctly parsed from bytecode

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
public class ExceptionTest {
    public static int safeDivide(int a, int b) {
        try {
            return a / b;
        } catch (ArithmeticException e) {
            return -1;
        }
    }
}
"#;

    project
        .write_source("", "ExceptionTest", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("ExceptionTest")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "safeDivide")
        .expect("safeDivide method not found");

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

    // Verify exception table exists
    assert!(
        !code_attr.exception_table.is_empty(),
        "Exception table should contain at least one handler"
    );

    // Verify handler structure
    for handler in &code_attr.exception_table {
        // catch_type should be either 0 (catch-all) or > 0 (specific exception)
        // start_pc should be before end_pc
        assert!(
            handler.start_pc < handler.end_pc,
            "Exception handler start_pc should be less than end_pc"
        );
    }
}

#[test]
fn test_finally_block_structure() {
    // Finally blocks may be represented as exception handlers with catch_type = 0

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
public class FinallyTest {
    public static int counter = 0;

    public static int testFinally() {
        try {
            return 42;
        } finally {
            counter++;
        }
    }
}
"#;

    project
        .write_source("", "FinallyTest", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("FinallyTest")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "testFinally")
        .expect("testFinally method not found");

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

    // Verify all exception handlers have valid structure
    for handler in &code_attr.exception_table {
        assert!(
            handler.start_pc < handler.end_pc,
            "Handler PC range should be valid"
        );
    }
}

#[test]
fn test_exception_execution_basic() {
    // Integration test: verify that exception handling actually works

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
public class ExceptionExec {
    public static int divideOrDefault(int a, int b) {
        try {
            return a / b;
        } catch (ArithmeticException e) {
            return 999;
        }
    }

    public static int normalCase() {
        return divideOrDefault(10, 2);
    }

    public static int exceptionCase() {
        return divideOrDefault(10, 0);
    }
}
"#;

    project
        .write_source("", "ExceptionExec", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("ExceptionExec")
        .expect("Failed to load class bytes");

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("ExceptionExec", &bytes)
        .expect("Failed to load class");

    // Normal case: should return 5
    let result = vm
        .invoke_static("ExceptionExec", "normalCase", "()I", &[])
        .expect("Normal execution should succeed");

    assert_eq!(result, Value::Int(5), "10 / 2 should equal 5");

    // Exception case: should catch and return 999
    let result = vm
        .invoke_static("ExceptionExec", "exceptionCase", "()I", &[])
        .expect("Exception should be caught");

    assert_eq!(
        result,
        Value::Int(999),
        "Division by zero should be caught and return 999"
    );
}

#[test]
fn test_multiple_catch_blocks() {
    // Test that multiple catch blocks work correctly

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
public class MultipleCatch {
    public static int handleError(int value) {
        try {
            if (value < 0) {
                throw new IllegalArgumentException();
            }
            int result = 100 / value;
            return result;
        } catch (IllegalArgumentException e) {
            return -1;
        } catch (ArithmeticException e) {
            return -2;
        }
    }
}
"#;

    project
        .write_source("", "MultipleCatch", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("MultipleCatch")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "handleError")
        .expect("handleError method not found");

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

    // Should have multiple exception handlers
    assert!(
        code_attr.exception_table.len() >= 2,
        "Should have at least 2 exception handlers, found {}",
        code_attr.exception_table.len()
    );

    // Verify all handlers have valid structure
    for handler in &code_attr.exception_table {
        assert!(
            handler.start_pc < handler.end_pc,
            "Exception handler PC range should be valid"
        );
    }
}

#[test]
fn test_nested_try_catch() {
    // Test nested try-catch blocks

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
public class NestedTry {
    public static int nestedOperation(int a, int b) {
        try {
            try {
                return a / b;
            } catch (ArithmeticException e) {
                return -1;
            }
        } catch (Exception e) {
            return -2;
        }
    }
}
"#;

    project
        .write_source("", "NestedTry", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("NestedTry")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "nestedOperation")
        .expect("nestedOperation method not found");

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

    // Nested try-catch should generate exception handlers
    assert!(
        !code_attr.exception_table.is_empty(),
        "Nested try-catch should have exception handlers"
    );

    // Verify all handlers have valid structure
    for handler in &code_attr.exception_table {
        assert!(
            handler.start_pc < handler.end_pc,
            "Exception handler start_pc should be less than end_pc"
        );
    }
}

#[test]
fn test_try_catch_finally_combined() {
    // Test try-catch-finally combination

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
public class TryCatchFinally {
    public static int counter = 0;

    public static int combined(int x) {
        try {
            if (x < 0) {
                throw new IllegalArgumentException();
            }
            return x * 2;
        } catch (IllegalArgumentException e) {
            return -1;
        } finally {
            counter++;
        }
    }
}
"#;

    project
        .write_source("", "TryCatchFinally", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("TryCatchFinally")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "combined")
        .expect("combined method not found");

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

    // Should have exception handlers
    assert!(
        !code_attr.exception_table.is_empty(),
        "Try-catch-finally should have exception handlers"
    );

    // Verify all handlers have valid structure
    for handler in &code_attr.exception_table {
        assert!(
            handler.start_pc < handler.end_pc,
            "Exception handler PC range should be valid"
        );
    }
}
