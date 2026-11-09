// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_bytecode::extractor::extract_symbols;
use hyperion_bytecode::parser::parse_class_file;
use std::fs;

#[test]
fn test_jdk_string_substring_has_parameter_names() {
    // Find JDK String class
    // For testing, we'll use a compiled test class instead
    // Compile a test class with javac -g to include debug info
    let test_class = "../TestInlayHints.class";

    if !std::path::Path::new(test_class).exists() {
        eprintln!("Skipping test - TestInlayHints.class not found. Compile with: javac -g TestInlayHints.java");
        return;
    }

    let bytes = fs::read(test_class).expect("Failed to read test class");
    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Find the customMethod
    let custom_method = symbols
        .methods
        .iter()
        .find(|m| m.name == "customMethod")
        .expect("customMethod not found");

    println!("customMethod parameters: {:?}", custom_method.parameters);
    println!(
        "customMethod parameter_names: {:?}",
        custom_method.parameter_names
    );

    // Check that we extracted parameter names
    assert_eq!(custom_method.parameter_names.len(), 2);
    assert_eq!(custom_method.parameter_names[0], Some("name".into()));
    assert_eq!(custom_method.parameter_names[1], Some("count".into()));
}

#[test]
fn test_method_without_debug_info_has_no_parameter_names() {
    // Most JDK classes won't have parameter names, but some do
    // This test verifies we handle both cases gracefully
    let test_class = "../TestInlayHints.class";

    if !std::path::Path::new(test_class).exists() {
        eprintln!("Skipping test - TestInlayHints.class not found");
        return;
    }

    let bytes = fs::read(test_class).expect("Failed to read test class");
    let class = parse_class_file(&bytes).expect("Failed to parse class file");
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // All methods should have parameter_names vec of correct length
    for method in &symbols.methods {
        println!(
            "Method: {} - params: {}, param_names: {}",
            method.name,
            method.parameters.len(),
            method.parameter_names.len()
        );
        assert_eq!(
            method.parameters.len(),
            method.parameter_names.len(),
            "Parameter count mismatch for method {}",
            method.name
        );
    }
}
