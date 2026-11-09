// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for JDK class extraction and parsing

use hyperion_bytecode::extractor::extract_symbols;
use hyperion_bytecode::jdk_indexer::{
    extract_class, find_java_home, get_modules_file, list_javabase_classes,
};
use hyperion_bytecode::parser::parse_class_file;

#[test]
fn test_extract_and_parse_string_class() {
    // Find JDK
    let java_home = find_java_home().expect("JAVA_HOME not found");
    let modules = get_modules_file(&java_home).expect("Modules file not found");

    // Extract String.class
    let bytes =
        extract_class(&modules, "java/lang/String").expect("Failed to extract String.class");

    println!("Extracted String.class: {} bytes", bytes.len());

    // Parse the class file
    let class = parse_class_file(&bytes).expect("Failed to parse String.class");

    // Extract symbols
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Verify class metadata
    assert_eq!(symbols.fqn, "java.lang.String");
    assert_eq!(symbols.package.as_deref(), Some("java.lang"));
    assert_eq!(symbols.super_class, Some("java.lang.Object".into()));
    assert!(symbols.is_public);
    assert!(symbols.is_final);

    // Verify some well-known methods exist
    let has_length = symbols.methods.iter().any(|m| m.name == "length");
    let has_substring = symbols.methods.iter().any(|m| m.name == "substring");
    let has_to_upper = symbols.methods.iter().any(|m| m.name == "toUpperCase");

    assert!(has_length, "String.length() method not found");
    assert!(has_substring, "String.substring() method not found");
    assert!(has_to_upper, "String.toUpperCase() method not found");

    println!(
        "✅ String.class parsed successfully with {} methods",
        symbols.methods.len()
    );
}

#[test]
fn test_extract_and_parse_list_interface() {
    let java_home = find_java_home().expect("JAVA_HOME not found");
    let modules = get_modules_file(&java_home).expect("Modules file not found");

    // Extract List.class
    let bytes = extract_class(&modules, "java/util/List").expect("Failed to extract List.class");

    println!("Extracted List.class: {} bytes", bytes.len());

    // Parse the class file
    let class = parse_class_file(&bytes).expect("Failed to parse List.class");

    // Extract symbols
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Verify class metadata
    assert_eq!(symbols.fqn, "java.util.List");
    assert_eq!(symbols.package.as_deref(), Some("java.util"));
    assert!(symbols.is_public);
    assert!(symbols.is_interface);

    // Verify some well-known methods
    let has_add = symbols.methods.iter().any(|m| m.name == "add");
    let has_get = symbols.methods.iter().any(|m| m.name == "get");
    let has_size = symbols.methods.iter().any(|m| m.name == "size");

    assert!(has_add, "List.add() method not found");
    assert!(has_get, "List.get() method not found");
    assert!(has_size, "List.size() method not found");

    println!(
        "✅ List.class parsed successfully with {} methods",
        symbols.methods.len()
    );
}

#[test]
fn test_extract_and_parse_throwable_class() {
    let java_home = find_java_home().expect("JAVA_HOME not found");
    let modules = get_modules_file(&java_home).expect("Modules file not found");

    // Extract Throwable.class
    let bytes =
        extract_class(&modules, "java/lang/Throwable").expect("Failed to extract Throwable.class");

    println!("Extracted Throwable.class: {} bytes", bytes.len());

    // Parse the class file
    let class = parse_class_file(&bytes).expect("Failed to parse Throwable.class");

    // Extract symbols
    let symbols = extract_symbols(&class).expect("Failed to extract symbols");

    // Verify class metadata
    assert_eq!(symbols.fqn, "java.lang.Throwable");
    assert_eq!(symbols.package.as_deref(), Some("java.lang"));
    assert_eq!(symbols.super_class, Some("java.lang.Object".into()));
    assert!(symbols.is_public);

    // Verify well-known methods
    let has_get_message = symbols.methods.iter().any(|m| m.name == "getMessage");
    let has_get_cause = symbols.methods.iter().any(|m| m.name == "getCause");
    let has_print_stack = symbols.methods.iter().any(|m| m.name == "printStackTrace");

    assert!(has_get_message, "Throwable.getMessage() method not found");
    assert!(has_get_cause, "Throwable.getCause() method not found");
    assert!(
        has_print_stack,
        "Throwable.printStackTrace() method not found"
    );

    println!(
        "✅ Throwable.class parsed successfully with {} methods",
        symbols.methods.len()
    );
}

#[test]
fn test_list_javabase_contains_core_classes() {
    let java_home = find_java_home().expect("JAVA_HOME not found");
    let modules = get_modules_file(&java_home).expect("Modules file not found");

    let classes = list_javabase_classes(&modules).expect("Failed to list classes");

    println!("Found {} classes in java.base", classes.len());

    // Verify core classes are present
    assert!(classes.iter().any(|c| c == "java/lang/String"));
    assert!(classes.iter().any(|c| c == "java/lang/Object"));
    assert!(classes.iter().any(|c| c == "java/lang/Integer"));
    assert!(classes.iter().any(|c| c == "java/util/List"));
    assert!(classes.iter().any(|c| c == "java/util/ArrayList"));
    assert!(classes.iter().any(|c| c == "java/util/HashMap"));
    assert!(classes.iter().any(|c| c == "java/lang/Throwable"));
    assert!(classes.iter().any(|c| c == "java/lang/Exception"));

    // Should have thousands of classes
    assert!(
        classes.len() > 1000,
        "Expected > 1000 classes, got {}",
        classes.len()
    );

    println!("✅ All core JDK classes found");
}

#[test]
fn test_bulk_parse_sample_jdk_classes() {
    let java_home = find_java_home().expect("JAVA_HOME not found");
    let modules = get_modules_file(&java_home).expect("Modules file not found");

    // Sample a subset of important classes
    let sample_classes = vec![
        "java/lang/String",
        "java/lang/Object",
        "java/lang/Integer",
        "java/lang/Throwable",
        "java/util/List",
        "java/util/ArrayList",
        "java/util/HashMap",
        "java/util/Optional",
    ];

    let mut success_count = 0;
    let mut total_methods = 0;

    for class_name in &sample_classes {
        print!("  Parsing {} ... ", class_name);

        match extract_class(&modules, class_name) {
            Ok(bytes) => match parse_class_file(&bytes) {
                Ok(class) => match extract_symbols(&class) {
                    Ok(symbols) => {
                        success_count += 1;
                        total_methods += symbols.methods.len();
                        println!("✅ ({} methods)", symbols.methods.len());
                    }
                    Err(e) => println!("❌ Failed to extract symbols: {}", e),
                },
                Err(e) => println!("❌ Failed to parse: {}", e),
            },
            Err(e) => println!("❌ Failed to extract: {}", e),
        }
    }

    println!("\n📊 Results:");
    println!(
        "  Successfully parsed: {}/{}",
        success_count,
        sample_classes.len()
    );
    println!("  Total methods found: {}", total_methods);

    assert_eq!(
        success_count,
        sample_classes.len(),
        "Failed to parse some classes"
    );
}
