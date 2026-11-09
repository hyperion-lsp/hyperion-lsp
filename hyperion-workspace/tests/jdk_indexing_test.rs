// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for JDK class indexing
//!
//! These tests verify that JDK classes are correctly indexed and that
//! inheritance chains work properly for method resolution.

use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::jdk_integration::index_jdk_classes;
use hyperion_workspace::{ImportMap, WorkspaceIndex};
use std::path::PathBuf;

#[test]
fn test_jdk_classes_indexed_with_inheritance() {
    // Create workspace index
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

    // Index JDK classes
    let count = index_jdk_classes(&index).expect("Failed to index JDK");
    println!("Indexed {} JDK classes", count);
    assert!(count > 1000, "Should index many JDK classes");

    // Test 1: String class exists
    let string_class = index.get_symbol("java.lang.String");
    assert!(string_class.is_some(), "String class should be indexed");
    let string_class = string_class.unwrap();
    assert_eq!(string_class.fqn.as_ref(), "java.lang.String");
    assert_eq!(string_class.simple_name.as_ref(), "String");
    println!("✅ String class: parent = {:?}", string_class.parent_class);

    // Test 2: String extends Object
    assert_eq!(
        string_class.parent_class.as_ref().map(|s| s.as_ref()),
        Some("java.lang.Object"),
        "String should extend Object"
    );

    // Test 3: Object class exists and has no parent
    let object_class = index.get_symbol("java.lang.Object");
    assert!(object_class.is_some(), "Object class should be indexed");
    let object_class = object_class.unwrap();
    assert_eq!(
        object_class.parent_class, None,
        "Object should have no parent"
    );
    println!("✅ Object class: parent = None");

    // Test 4: Throwable class exists
    let throwable_class = index.get_symbol("java.lang.Throwable");
    assert!(
        throwable_class.is_some(),
        "Throwable class should be indexed"
    );
    let throwable_class = throwable_class.unwrap();
    assert_eq!(
        throwable_class.parent_class.as_ref().map(|s| s.as_ref()),
        Some("java.lang.Object"),
        "Throwable should extend Object"
    );
    println!(
        "✅ Throwable class: parent = {:?}",
        throwable_class.parent_class
    );

    // Test 5: Exception class exists and extends Throwable
    let exception_class = index.get_symbol("java.lang.Exception");
    assert!(
        exception_class.is_some(),
        "Exception class should be indexed"
    );
    let exception_class = exception_class.unwrap();
    assert_eq!(
        exception_class.parent_class.as_ref().map(|s| s.as_ref()),
        Some("java.lang.Throwable"),
        "Exception should extend Throwable"
    );
    println!(
        "✅ Exception class: parent = {:?}",
        exception_class.parent_class
    );

    // Test 6: ExecutionException extends Exception
    let exec_exception = index.get_symbol("java.util.concurrent.ExecutionException");
    assert!(
        exec_exception.is_some(),
        "ExecutionException class should be indexed"
    );
    let exec_exception = exec_exception.unwrap();
    assert_eq!(
        exec_exception.parent_class.as_ref().map(|s| s.as_ref()),
        Some("java.lang.Exception"),
        "ExecutionException should extend Exception"
    );
    println!(
        "✅ ExecutionException class: parent = {:?}",
        exec_exception.parent_class
    );

    println!("\n📊 Inheritance chain verification:");
    println!("  ExecutionException → Exception → Throwable → Object");
}

#[test]
fn test_throwable_getmessage_method() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let _ = index_jdk_classes(&index).expect("Failed to index JDK");

    // Test that Throwable.getMessage() exists
    let get_message_fqn = "java.lang.Throwable.getMessage";
    let get_message = index.find_symbol(get_message_fqn);

    assert!(
        get_message.is_some(),
        "Throwable.getMessage() should be indexed"
    );

    let get_message = get_message.unwrap();
    println!("✅ Found Throwable.getMessage()");
    println!("  FQN: {}", get_message.name);
    println!("  Kind: {:?}", get_message.kind);
    println!("  Container: {:?}", get_message.container_name);
}

#[test]
fn test_execution_exception_methods() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let _ = index_jdk_classes(&index).expect("Failed to index JDK");

    // Test that ExecutionException class exists
    let exec_exception = index.get_symbol("java.util.concurrent.ExecutionException");
    assert!(
        exec_exception.is_some(),
        "ExecutionException should be indexed"
    );

    // Test direct methods (constructors)
    let constructor = index.find_symbol("java.util.concurrent.ExecutionException.<init>");
    println!(
        "ExecutionException constructor: {:?}",
        constructor.is_some()
    );

    // Test inherited method - getMessage() from Throwable
    // Note: This should be found via inheritance, not direct lookup
    let direct_get_message =
        index.find_symbol("java.util.concurrent.ExecutionException.getMessage");
    println!(
        "Direct ExecutionException.getMessage lookup: {:?}",
        direct_get_message.is_some()
    );

    // Verify inheritance chain exists
    println!("\nInheritance chain:");
    let exec = index
        .get_symbol("java.util.concurrent.ExecutionException")
        .unwrap();
    println!(
        "  ExecutionException.parent_class = {:?}",
        exec.parent_class
    );

    if let Some(parent) = &exec.parent_class {
        if let Some(exception) = index.get_symbol(parent) {
            println!("  Exception.parent_class = {:?}", exception.parent_class);

            if let Some(grandparent) = &exception.parent_class {
                if let Some(throwable) = index.get_symbol(grandparent) {
                    println!("  Throwable.parent_class = {:?}", throwable.parent_class);
                }
            }
        }
    }
}

#[test]
fn test_method_resolution_via_inheritance() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let _ = index_jdk_classes(&index).expect("Failed to index JDK");

    // Test that we can find methods in parent classes
    // String.length() - defined in String itself
    let string_length = index.find_symbol("java.lang.String.length");
    assert!(string_length.is_some(), "String.length() should be found");
    println!("✅ String.length() found directly");

    // Throwable.getMessage() - defined in Throwable
    let throwable_get_message = index.find_symbol("java.lang.Throwable.getMessage");
    assert!(
        throwable_get_message.is_some(),
        "Throwable.getMessage() should be found"
    );
    println!("✅ Throwable.getMessage() found directly");

    // Exception doesn't override getMessage, so it should inherit from Throwable
    let exception_get_message = index.find_symbol("java.lang.Exception.getMessage");
    println!(
        "Exception.getMessage() direct lookup: {:?}",
        exception_get_message.is_some()
    );

    // ExecutionException also doesn't override getMessage
    let exec_get_message = index.find_symbol("java.util.concurrent.ExecutionException.getMessage");
    println!(
        "ExecutionException.getMessage() direct lookup: {:?}",
        exec_get_message.is_some()
    );

    // Verify that inheritance resolution should work
    println!("\n🔍 Method should be found via inheritance chain:");
    println!("  ExecutionException → Exception → Throwable.getMessage()");
}

#[test]
fn test_list_interface_methods() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let _ = index_jdk_classes(&index).expect("Failed to index JDK");

    // Test List interface
    let list_interface = index.get_symbol("java.util.List");
    assert!(list_interface.is_some(), "List interface should be indexed");
    let list_interface = list_interface.unwrap();

    println!("List interface:");
    println!("  Kind: {:?}", list_interface.kind);
    println!(
        "  Is interface: {}",
        list_interface.kind == lsp_types::SymbolKind::INTERFACE
    );

    // Test List methods
    let list_add = index.find_symbol("java.util.List.add");
    assert!(list_add.is_some(), "List.add() should be indexed");
    println!("✅ List.add() found");

    let list_get = index.find_symbol("java.util.List.get");
    assert!(list_get.is_some(), "List.get() should be indexed");
    println!("✅ List.get() found");

    let list_size = index.find_symbol("java.util.List.size");
    assert!(list_size.is_some(), "List.size() should be indexed");
    println!("✅ List.size() found");

    assert_eq!(
        index.project_type_from_receiver(
            "java.util.List",
            &["java.lang.String".to_string()],
            "java.util.Collection",
            "E",
        ),
        Some("java.lang.String".to_string()),
        "JDK indexing must preserve generic interface edges"
    );
}

#[test]
fn test_jdk_symbol_counts() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let count = index_jdk_classes(&index).expect("Failed to index JDK");

    println!("Total classes indexed: {}", count);

    // Get index statistics
    let stats = index.stats();
    println!("Workspace stats:");
    println!("  Symbols indexed: {}", stats.symbols_indexed);
    println!("  Files indexed: {}", stats.files_indexed);

    // Verify we have a substantial number of symbols
    // Each class has: 1 class symbol + N methods + M fields
    // With 6352 classes, we should have way more than 10k symbols
    assert!(
        stats.symbols_indexed > 10000,
        "Should have many symbols from JDK classes"
    );
}

#[test]
fn test_dynamic_java_lang_class_resolution() {
    // This test verifies that the dynamic java.lang class lookup works
    // with a real JDK, replacing the hardcoded 40-class list.
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let count = index_jdk_classes(&index).expect("Failed to index JDK");
    println!("Indexed {} JDK classes", count);

    // Query for java.lang classes using the new dynamic API
    let java_lang_classes = index.get_package_class_names("java.lang");

    println!(
        "Found {} java.lang classes via get_package_class_names()",
        java_lang_classes.len()
    );
    println!(
        "Sample classes: {:?}",
        &java_lang_classes[..10.min(java_lang_classes.len())]
    );

    // Verify we found significantly more than the hardcoded 40
    assert!(
        java_lang_classes.len() >= 137,
        "Should find 137+ java.lang classes (found {})",
        java_lang_classes.len()
    );

    // Verify core classes are included
    let core_classes = vec![
        "Object",
        "String",
        "Class",
        "System",
        "Math",
        "Integer",
        "Long",
        "Double",
        "Float",
        "Boolean",
        "Exception",
        "Throwable",
        "Error",
        "Thread",
        "CharSequence",
        "Appendable",
        "StringBuilder",
        "StringBuffer",
        "Comparable",
        "Iterable",
        "AutoCloseable",
        "Enum",
        "Record",
        "Process",
        "ProcessBuilder",
        "Runtime",
    ];

    for class_name in core_classes {
        assert!(
            java_lang_classes.contains(&class_name.to_string()),
            "java.lang.{} should be found via dynamic lookup",
            class_name
        );
    }

    // Test that ImportResolver can use the dynamic lookup
    let imports = ImportMap::new();
    let resolver = ImportResolver::new(&imports, &index);

    // Resolve a java.lang class that wasn't in the old hardcoded list
    let result = resolver.resolve("CharSequence");
    assert!(
        result.contains(&"java.lang.CharSequence".to_string()),
        "CharSequence (not in old hardcoded list) should resolve via dynamic lookup"
    );

    let result = resolver.resolve("Appendable");
    assert!(
        result.contains(&"java.lang.Appendable".to_string()),
        "Appendable (not in old hardcoded list) should resolve via dynamic lookup"
    );

    println!(
        "✅ Dynamic java.lang resolution working: {} classes found",
        java_lang_classes.len()
    );
    println!("✅ ImportResolver successfully uses dynamic lookup");
}
