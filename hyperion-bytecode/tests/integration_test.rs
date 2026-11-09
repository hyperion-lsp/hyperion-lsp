// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests with real class files

use hyperion_bytecode::extractor::extract_symbols;
use hyperion_bytecode::parser::parse_class_file;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_simple_class_extraction() {
    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Write a simple Java class (no package for simplicity)
    let java_source = r#"
public class SimpleClass {
    private int count;
    public String name;

    public SimpleClass() {
        this.count = 0;
    }

    public int getCount() {
        return count;
    }

    public void setCount(int value) {
        this.count = value;
    }

    public static String staticMethod(int x, String y) {
        return y;
    }
}
"#;

    let java_file = temp_path.join("SimpleClass.java");
    fs::write(&java_file, java_source).unwrap();

    // Compile the Java file
    let output = Command::new("javac")
        .arg(&java_file)
        .current_dir(temp_path)
        .output()
        .expect("Failed to compile Java class");

    assert!(
        output.status.success(),
        "Compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Read the compiled class file
    let class_file = temp_path.join("SimpleClass.class");
    let bytes = fs::read(&class_file).expect("Failed to read class file");

    // Parse the class file
    let class = parse_class_file(&bytes).expect("Failed to parse class file");

    // Extract symbols
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Verify class metadata
    assert_eq!(symbols.fqn, "SimpleClass");
    assert_eq!(symbols.package, None);
    assert_eq!(symbols.super_class, Some("java.lang.Object".into()));
    assert!(symbols.is_public);
    assert!(!symbols.is_interface);
    assert!(!symbols.is_abstract);

    // Verify fields
    assert_eq!(symbols.fields.len(), 2);

    let count_field = symbols
        .fields
        .iter()
        .find(|f| f.name == "count")
        .expect("count field not found");
    assert!(count_field.is_private);
    assert!(!count_field.is_static);

    let name_field = symbols
        .fields
        .iter()
        .find(|f| f.name == "name")
        .expect("name field not found");
    assert!(name_field.is_public);

    // Verify methods (constructor + 3 methods)
    assert!(
        symbols.methods.len() >= 4,
        "Expected at least 4 methods, got {}",
        symbols.methods.len()
    );

    // Find constructor
    let constructor = symbols
        .methods
        .iter()
        .find(|m| m.is_constructor())
        .expect("Constructor not found");
    assert!(constructor.is_public);
    assert_eq!(constructor.parameters.len(), 0);

    // Find getCount method
    let get_count = symbols
        .methods
        .iter()
        .find(|m| m.name == "getCount")
        .expect("getCount method not found");
    assert!(get_count.is_public);
    assert_eq!(get_count.parameters.len(), 0);
    println!("getCount signature: {}", get_count.signature());

    // Find setCount method
    let set_count = symbols
        .methods
        .iter()
        .find(|m| m.name == "setCount")
        .expect("setCount method not found");
    assert!(set_count.is_public);
    assert_eq!(set_count.parameters.len(), 1);
    println!("setCount signature: {}", set_count.signature());

    // Find staticMethod
    let static_method = symbols
        .methods
        .iter()
        .find(|m| m.name == "staticMethod")
        .expect("staticMethod not found");
    assert!(static_method.is_public);
    assert!(static_method.is_static);
    assert_eq!(static_method.parameters.len(), 2);
    println!("staticMethod signature: {}", static_method.signature());
}
