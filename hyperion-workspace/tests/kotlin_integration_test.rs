// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end integration test for Kotlin support
//!
//! This test verifies that:
//! 1. .kt files are detected by the scanner
//! 2. Kotlin files are parsed correctly
//! 3. Symbols are extracted from Kotlin files
//! 4. Symbols are indexed in the workspace

use hyperion_workspace::{Language, WorkspaceIndex};
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_kotlin_file_detection_and_indexing() {
    // Setup test workspace
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create a simple Kotlin file
    let kt_dir = root.join("src/main/kotlin/com/example");
    fs::create_dir_all(&kt_dir).unwrap();

    let kt_file = kt_dir.join("User.kt");
    fs::write(
        &kt_file,
        r#"
package com.example

class User {
    val name: String = ""

    fun getName(): String {
        return name
    }
}

fun topLevelFunction() {
    println("Hello")
}
"#,
    )
    .unwrap();

    // Create a Java file for comparison
    let java_dir = root.join("src/main/java/com/example");
    fs::create_dir_all(&java_dir).unwrap();

    let java_file = java_dir.join("Service.java");
    fs::write(
        &java_file,
        r#"
package com.example;

public class Service {
    public void execute() {}
}
"#,
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.to_path_buf());
    let stats = index.build().unwrap();

    // Verify both files were found
    assert_eq!(
        stats.files_scanned, 2,
        "Should find both .java and .kt files"
    );
    assert_eq!(
        stats.files_indexed, 2,
        "Should successfully index both files"
    );

    // Verify Kotlin symbols were extracted
    // User class + getName method + topLevelFunction = 3 symbols minimum
    assert!(
        stats.symbols_indexed >= 3,
        "Should extract at least 3 symbols from Kotlin file (class, method, function)"
    );

    // Verify we can find the Kotlin class
    let user_locations = index.find_by_simple_name("User");
    assert!(!user_locations.is_empty(), "Should find User class");

    let user_location = &user_locations[0];
    assert_eq!(&*user_location.name, "User");

    // Verify we can find the top-level Kotlin function
    let func_symbols = index.find_by_simple_name("topLevelFunction");
    assert!(
        !func_symbols.is_empty(),
        "Should find top-level Kotlin function"
    );

    // Verify Java class is still indexed correctly
    let service_symbols = index.find_by_simple_name("Service");
    assert!(
        !service_symbols.is_empty(),
        "Should find Java Service class"
    );
}

#[test]
fn test_language_detection() {
    use std::path::Path;

    // Test Kotlin detection
    assert_eq!(
        Language::from_path(Path::new("Foo.kt")),
        Some(Language::Kotlin)
    );
    assert_eq!(
        Language::from_path(Path::new("com/example/Bar.kt")),
        Some(Language::Kotlin)
    );

    // Test Java detection
    assert_eq!(
        Language::from_path(Path::new("Foo.java")),
        Some(Language::Java)
    );

    // Test unknown
    assert_eq!(Language::from_path(Path::new("script.py")), None);
}

#[tokio::test]
async fn test_mixed_workspace_with_kotlin() {
    // Setup test workspace with multiple Kotlin and Java files
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create multiple Kotlin files
    let kt_dir = root.join("src/main/kotlin/com/example");
    fs::create_dir_all(&kt_dir).unwrap();

    fs::write(
        kt_dir.join("Model.kt"),
        "package com.example\ndata class Model(val id: Int)",
    )
    .unwrap();

    fs::write(
        kt_dir.join("Repository.kt"),
        "package com.example\ninterface Repository { fun save() }",
    )
    .unwrap();

    // Create multiple Java files
    let java_dir = root.join("src/main/java/com/example");
    fs::create_dir_all(&java_dir).unwrap();

    fs::write(
        java_dir.join("Controller.java"),
        "package com.example; public class Controller {}",
    )
    .unwrap();

    fs::write(
        java_dir.join("Config.java"),
        "package com.example; public class Config {}",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.to_path_buf());
    let stats = index.build().unwrap();

    // Verify all files were found and indexed
    assert_eq!(
        stats.files_scanned, 4,
        "Should find 2 Kotlin + 2 Java files"
    );
    assert_eq!(stats.files_indexed, 4, "Should index all files");

    // Verify Kotlin symbols
    assert!(
        !index.find_by_simple_name("Model").is_empty(),
        "Should find Kotlin Model class"
    );
    assert!(
        !index.find_by_simple_name("Repository").is_empty(),
        "Should find Kotlin Repository interface"
    );

    // Verify Java symbols
    assert!(
        !index.find_by_simple_name("Controller").is_empty(),
        "Should find Java Controller class"
    );
    assert!(
        !index.find_by_simple_name("Config").is_empty(),
        "Should find Java Config class"
    );
}

#[tokio::test]
async fn test_kotlin_package_extraction() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let kt_dir = root.join("src/main/kotlin/com/example/utils");
    fs::create_dir_all(&kt_dir).unwrap();

    fs::write(
        kt_dir.join("Helper.kt"),
        r#"
package com.example.utils

class Helper {
    fun help() {}
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.to_path_buf());
    index.build().unwrap();

    let locations = index.find_by_simple_name("Helper");
    assert!(!locations.is_empty());

    let location = &locations[0];
    assert_eq!(
        location.name.as_ref(),
        "Helper",
        "Should extract correct symbol name"
    );
}
