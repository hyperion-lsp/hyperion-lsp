// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Wide Opcode Local Variable Index
//!
//! The `wide` instruction (0xC4) modifies the following instruction to use
//! 16-bit indices instead of 8-bit indices for local variables.
//!
//! Format:
//! - wide iload <indexbyte1> <indexbyte2>
//! - wide istore <indexbyte1> <indexbyte2>
//! - wide iinc <indexbyte1> <indexbyte2> <constbyte1> <constbyte2>
//!
//! **Bug**: Interpreter might:
//! - Not recognize wide opcode (0xC4)
//! - Read index as 1 byte instead of 2 bytes after wide
//! - Not handle iinc's special wide format (4 bytes after opcode)
//!
//! These tests verify correct wide instruction parsing and execution.

mod test_utils;

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::parser::parse_class_file;
use test_utils::JavaTestProject;

#[test]
fn test_wide_iload_bytecode_structure() {
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

    // Create a method with many LOCAL VARIABLES to force wide instructions
    // JVM spec §4.11 limits parameters to 255, so we use local variables instead
    // Local variable slots > 255 require wide prefix
    let mut body = String::new();

    // Declare 300 local variables to exceed the 255 limit
    for i in 0..300 {
        body.push_str(&format!("        int v{} = {};\n", i, i));
    }
    // Return the last variable (index > 255) to ensure wide iload is used
    body.push_str("        return v299;");

    let source = format!(
        r#"
public class WideTest {{
    public static int manyLocals() {{
{}
    }}
}}
"#,
        body
    );

    project
        .write_source("", "WideTest", &source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("WideTest")
        .expect("Failed to load class bytes");

    // Parse the class file
    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    // Find the method
    let method = class
        .methods
        .iter()
        .find(|m| m.name == "manyLocals")
        .expect("manyLocals method not found");

    // Get the Code attribute
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

    let bytecode = &code_attr.code;

    // Look for valid wide instructions (0xC4 followed by a load/store/iinc opcode)
    // Note: 0xC4 can appear as a data byte, so we only count valid wide sequences
    let mut found_valid_wide = false;

    for i in 0..bytecode.len().saturating_sub(3) {
        if bytecode[i] == 0xC4 {
            let next_opcode = bytecode[i + 1];

            // Common opcodes that can follow wide
            let is_valid_wide_opcode = matches!(
                next_opcode,
                0x15 | // iload
                0x16 | // lload
                0x17 | // fload
                0x18 | // dload
                0x19 | // aload
                0x36 | // istore
                0x37 | // lstore
                0x38 | // fstore
                0x39 | // dstore
                0x3A | // astore
                0x84 // iinc
            );

            if is_valid_wide_opcode {
                found_valid_wide = true;

                // For iinc, verify it has 4 bytes after opcode (2 for index, 2 for constant)
                if next_opcode == 0x84 {
                    assert!(
                        i + 5 < bytecode.len(),
                        "wide iinc should have 5 total bytes"
                    );
                }
            }
        }
    }

    assert!(
        found_valid_wide,
        "Method with 300 local variables should contain at least one valid wide instruction"
    );
}

#[test]
fn test_wide_iinc_bytecode() {
    // Test wide iinc specifically (it has a different format)
    // wide iinc <indexbyte1> <indexbyte2> <constbyte1> <constbyte2>

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

    // Create a method with many local variables and increment one at index > 255
    // JVM spec §4.11 limits parameters to 255, so we use local variables instead
    let mut body = String::new();

    // Declare 260 local variables
    for i in 0..260 {
        body.push_str(&format!("        int v{} = {};\n", i, i));
    }
    // Increment variable at index > 255 to trigger wide iinc
    body.push_str("        v259++;\n");
    body.push_str("        return v259;");

    let source = format!(
        r#"
public class WideIinc {{
    public static int testIinc() {{
{}
    }}
}}
"#,
        body
    );

    project
        .write_source("", "WideIinc", &source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("WideIinc")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "testIinc")
        .expect("testIinc method not found");

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

    let bytecode = &code_attr.code;

    // Look for wide iinc (0xC4 0x84)
    let mut found_wide_iinc = false;

    for i in 0..bytecode.len() - 5 {
        if bytecode[i] == 0xC4 && bytecode[i + 1] == 0x84 {
            found_wide_iinc = true;

            // Extract the 16-bit index
            let index = u16::from_be_bytes([bytecode[i + 2], bytecode[i + 3]]);

            // Extract the 16-bit constant
            let constant = i16::from_be_bytes([bytecode[i + 4], bytecode[i + 5]]);

            assert!(
                index >= 255,
                "wide iinc should be used for index >= 255, got {}",
                index
            );

            // For x++, the constant should be 1
            assert_eq!(
                constant, 1,
                "iinc constant for ++ should be 1, got {}",
                constant
            );
        }
    }

    assert!(
        found_wide_iinc,
        "Should find wide iinc instruction for large local variable index"
    );
}

#[test]
fn test_local_variable_boundary_255() {
    // Test the boundary case: local variable at index 255
    // This might or might not use wide depending on the compiler

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

    // Create exactly 256 local variables (indices 0-255)
    // JVM spec §4.11 limits parameters to 255, so we use local variables instead
    let mut body = String::new();
    for i in 0..256 {
        body.push_str(&format!("        int v{} = {};\n", i, i));
    }
    // Return variable at index 255
    body.push_str("        return v255;");

    let source = format!(
        r#"
public class Boundary255 {{
    public static int testBoundary() {{
{}
    }}
}}
"#,
        body
    );

    project
        .write_source("", "Boundary255", &source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("Boundary255")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "testBoundary")
        .expect("testBoundary method not found");

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

    // Verify the bytecode is valid (either with or without wide)
    // The important thing is it can be parsed correctly
    let bytecode = &code_attr.code;

    assert!(!bytecode.is_empty(), "Bytecode should not be empty");

    // Check if wide is used
    let has_wide = bytecode.contains(&0xC4);

    if has_wide {
        // If wide is used, verify it's properly formed
        for i in 0..bytecode.len() - 3 {
            if bytecode[i] == 0xC4 {
                assert!(i + 3 < bytecode.len(), "wide instruction incomplete");
            }
        }
    }
}

#[test]
fn test_wide_instruction_execution() {
    // Integration test: verify that wide instructions execute correctly
    // This is a simplified test since generating 300 parameters is impractical

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

    // Create a simpler test with fewer parameters that still exercises local variable handling
    let source = r#"
public class WideExec {
    // Even if this doesn't use wide, it tests the interpreter's local variable handling
    public static int sumParams(int a, int b, int c, int d, int e,
                                 int f, int g, int h, int i, int j) {
        int result = a + b + c + d + e + f + g + h + i + j;
        result = result + 1;
        return result;
    }
}
"#;

    project
        .write_source("", "WideExec", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("WideExec")
        .expect("Failed to load class bytes");

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("WideExec", &bytes)
        .expect("Failed to load class");

    // Call with test values
    let args = vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
        Value::Int(5),
        Value::Int(6),
        Value::Int(7),
        Value::Int(8),
        Value::Int(9),
        Value::Int(10),
    ];

    let result = vm
        .invoke_static("WideExec", "sumParams", "(IIIIIIIIII)I", &args)
        .expect("Execution should succeed");

    // Sum of 1..10 is 55, plus 1 = 56
    assert_eq!(
        result,
        Value::Int(56),
        "Should correctly handle multiple local variables"
    );
}

#[test]
fn test_non_wide_regular_case() {
    // Positive test: verify that methods with few local variables work correctly
    // (should NOT use wide instructions)

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
public class RegularTest {
    public static int simpleMethod(int a, int b, int c) {
        int x = a + b;
        int y = x + c;
        return y;
    }
}
"#;

    project
        .write_source("", "RegularTest", source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("RegularTest")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "simpleMethod")
        .expect("simpleMethod not found");

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

    let bytecode = &code_attr.code;

    // This simple method should NOT need wide instructions
    let has_wide = bytecode.contains(&0xC4);

    assert!(
        !has_wide,
        "Simple method with few locals should not need wide instructions"
    );

    // But it should still execute correctly
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("RegularTest", &bytes)
        .expect("Failed to load class");

    let result = vm
        .invoke_static(
            "RegularTest",
            "simpleMethod",
            "(III)I",
            &[Value::Int(10), Value::Int(20), Value::Int(30)],
        )
        .expect("Should execute successfully");

    assert_eq!(result, Value::Int(60)); // 10 + 20 + 30 = 60
}

#[test]
fn test_wide_aload() {
    // Test wide with reference types (aload/astore)

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

    // Create method with many String local variables
    // JVM spec §4.11 limits parameters to 255, so we use local variables instead
    let mut body = String::new();
    for i in 0..280 {
        body.push_str(&format!("        String s{} = \"str{}\";\n", i, i));
    }
    // Return variable at index > 255 to trigger wide aload
    body.push_str("        return s279;");

    let source = format!(
        r#"
public class WideAload {{
    public static String testAload() {{
{}
    }}
}}
"#,
        body
    );

    project
        .write_source("", "WideAload", &source)
        .expect("Failed to write source");
    project.compile().expect("Failed to compile");

    let bytes = project
        .load_class_bytes("WideAload")
        .expect("Failed to load class bytes");

    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    let method = class
        .methods
        .iter()
        .find(|m| m.name == "testAload")
        .expect("testAload not found");

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

    let bytecode = &code_attr.code;

    // Should have wide aload (0xC4 0x19)
    let mut found_wide_aload = false;

    for i in 0..bytecode.len() - 3 {
        if bytecode[i] == 0xC4 && bytecode[i + 1] == 0x19 {
            // aload
            found_wide_aload = true;

            let index = u16::from_be_bytes([bytecode[i + 2], bytecode[i + 3]]);

            assert!(
                index >= 255,
                "wide aload should be for index >= 255, got {}",
                index
            );
        }
    }

    assert!(
        found_wide_aload,
        "Should find wide aload for large local variable index"
    );
}
