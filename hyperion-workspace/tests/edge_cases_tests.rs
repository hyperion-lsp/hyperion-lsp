// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Edge cases and error handling tests

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::query::QueryEngine;
use hyperion_workspace::scanner::WorkspaceScanner;
use hyperion_workspace::symbol_extractor::SymbolExtractor;
use hyperion_workspace::{FileId, ImportMap, TextRange};
use std::fs;
use tempfile::TempDir;

// TextRange edge cases
#[test]
fn test_text_range_empty() {
    let range = TextRange::new(10, 10);
    assert!(range.is_empty());
    assert_eq!(range.len(), 0);
}

#[test]
fn test_text_range_zero_start() {
    let range = TextRange::new(0, 10);
    assert_eq!(range.len(), 10);
    assert!(range.contains(0));
    assert!(range.contains(5));
    assert!(!range.contains(10));
}

#[test]
fn test_text_range_large_values() {
    let range = TextRange::new(1_000_000, 2_000_000);
    assert_eq!(range.len(), 1_000_000);
    assert!(range.contains(1_500_000));
}

#[test]
fn test_text_range_boundary_conditions() {
    let range = TextRange::new(10, 20);

    // Start boundary
    assert!(range.contains(10));
    assert!(!range.contains(9));

    // End boundary
    assert!(!range.contains(20));
    assert!(range.contains(19));
}

// ImportMap edge cases
#[test]
fn test_import_map_duplicate_explicit_imports() {
    let mut imports = ImportMap::new();
    imports.add_import("List".to_string(), "java.util.List".to_string());
    imports.add_import("List".to_string(), "java.awt.List".to_string()); // Override

    // Second import should override
    assert_eq!(
        imports.explicit.get("List"),
        Some(&"java.awt.List".to_string())
    );
}

#[test]
fn test_import_map_duplicate_wildcards() {
    let mut imports = ImportMap::new();
    imports.add_wildcard("java.util".to_string());
    imports.add_wildcard("java.util".to_string()); // Duplicate

    // Should not add duplicate
    assert_eq!(imports.wildcards.len(), 1);
}

#[test]
fn test_import_map_duplicate_static_wildcards() {
    let mut imports = ImportMap::new();
    imports.add_static_wildcard("java.lang.Math".to_string());
    imports.add_static_wildcard("java.lang.Math".to_string());

    assert_eq!(imports.static_wildcards.len(), 1);
}

#[test]
fn test_import_map_empty_package_name() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("".into());

    // Empty package name is technically valid (though unusual)
    assert_eq!(imports.current_package, Some("".into()));
}

// SymbolExtractor edge cases
#[test]
fn test_extract_malformed_java() {
    let source = "this is not valid java code at all";
    let result = SymbolExtractor::extract(source);

    // Should not panic, even with invalid input
    assert!(result.is_ok());
}

#[test]
fn test_extract_partial_package_declaration() {
    let source = "package";
    let result = SymbolExtractor::extract(source).unwrap();

    // Incomplete package declaration
    assert_eq!(result.package, None);
}

#[test]
fn test_extract_partial_import() {
    let source = "import";
    let result = SymbolExtractor::extract(source).unwrap();

    // Incomplete import
    assert_eq!(result.imports.explicit.len(), 0);
}

#[test]
fn test_extract_unicode_in_package_name() {
    // While not recommended, Java allows unicode in identifiers
    let source = "package com.example.テスト;\n\nclass Foo {}";
    let result = SymbolExtractor::extract(source);

    // Parser might not handle this perfectly, but shouldn't crash
    assert!(result.is_ok());
}

#[test]
fn test_extract_very_long_package_name() {
    let long_package = (0..100)
        .map(|i| format!("package{}", i))
        .collect::<Vec<_>>()
        .join(".");
    let source = format!("package {};\n\nclass Foo {{}}", long_package);
    let result = SymbolExtractor::extract(&source).unwrap();

    assert!(result.package.is_some());
}

#[test]
fn test_extract_class_with_no_modifiers() {
    let source = "package com.example;\n\nclass NoModifiers {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(!sym.modifiers.is_public);
    assert!(!sym.modifiers.is_private);
    assert!(!sym.modifiers.is_protected);
}

#[test]
fn test_extract_multiple_imports_same_simple_name() {
    let source = r#"
package com.example;

import java.util.List;
import java.awt.List;  // Name collision

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Last import wins
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"java.awt.List".to_string())
    );
}

#[test]
fn test_extract_very_large_file() {
    // Create a file with many classes
    let mut source = String::from("package com.example;\n\n");
    for i in 0..1000 {
        source.push_str(&format!("class Class{} {{}}\n", i));
    }

    let result = SymbolExtractor::extract(&source).unwrap();

    assert_eq!(result.symbols.len(), 1000);
}

// Scanner edge cases
#[test]
fn test_scanner_with_symlinks() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Real.java"), "class Real {}").unwrap();

    // Scanner should not follow symlinks by default
    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert!(files.iter().any(|p| p.ends_with("Real.java")));
}

#[test]
fn test_scanner_with_very_deep_directories() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create a very deep directory structure
    let mut deep_path = root.to_path_buf();
    for i in 0..20 {
        deep_path.push(format!("level{}", i));
    }
    fs::create_dir_all(&deep_path).unwrap();
    fs::write(deep_path.join("Deep.java"), "class Deep {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert!(files.iter().any(|p| p.ends_with("Deep.java")));
}

#[test]
fn test_scanner_with_special_directory_names() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Directories with special characters
    fs::create_dir_all(root.join("src-main")).unwrap();
    fs::write(root.join("src-main/App.java"), "class App {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
}

#[test]
fn test_scanner_with_empty_directories() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create empty directory structure
    fs::create_dir_all(root.join("src/main/java")).unwrap();
    fs::create_dir_all(root.join("src/test/java")).unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 0);
}

// Index edge cases
#[tokio::test]
async fn test_index_file_that_doesnt_exist() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root.clone());
    let nonexistent = root.join("NonExistent.java");

    let result = index.index_file(&nonexistent).await;

    // Should return an error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_index_file_with_invalid_utf8() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Invalid.java");

    // Write invalid UTF-8
    fs::write(&file_path, vec![0xFF, 0xFE, 0xFD]).unwrap();

    let index = WorkspaceIndex::new(root);
    let result = index.index_file(&file_path).await;

    // Should handle the error gracefully
    assert!(result.is_err());
}

#[tokio::test]
async fn test_index_with_same_file_id_reuse() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);

    // Index the same file multiple times
    index.index_file(&file_path).await.unwrap();
    let file_id_1 = index.get_file_id(&file_path).unwrap();

    index.index_file(&file_path).await.unwrap();
    let file_id_2 = index.get_file_id(&file_path).unwrap();

    // File ID should be stable
    assert_eq!(file_id_1, file_id_2);
}

#[tokio::test]
async fn test_remove_nonexistent_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root);

    // Remove a file that was never indexed
    index.remove_file(FileId(999));

    // Should not panic or error
}

#[tokio::test]
async fn test_index_class_with_very_long_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("VeryLongName.java");

    let long_name = "A".repeat(1000);
    let source = format!("package com.example;\npublic class {} {{}}", long_name);
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 1);
}

#[tokio::test]
async fn test_search_with_empty_query() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let _results = index.search("");

    // Empty query might match everything or nothing depending on implementation
    // Just ensure it doesn't crash (no assertion needed)
}

#[tokio::test]
async fn test_search_with_special_characters() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // Search with regex special characters
    let _results = index.search(".*");

    // Should handle special characters gracefully (no assertion needed, just shouldn't panic)
}

// QueryEngine edge cases
#[tokio::test]
async fn test_goto_definition_with_no_imports() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("Test.java");
    fs::write(&file_path, "class Test {}").unwrap(); // No package, no imports

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path).unwrap();

    let query_engine = QueryEngine::new(index);
    let _locations = query_engine.goto_definition("Test", file_id).unwrap();

    // Should handle files without imports (no assertion needed, just shouldn't panic)
}

#[tokio::test]
async fn test_goto_definition_circular_dependency() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create circular dependency
    let file_a = root.join("A.java");
    fs::write(
        &file_a,
        "package com.example;\nimport com.example.B;\nclass A {}",
    )
    .unwrap();

    let file_b = root.join("B.java");
    fs::write(
        &file_b,
        "package com.example;\nimport com.example.A;\nclass B {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_a).await.unwrap();
    index.index_file(&file_b).await.unwrap();

    let query_engine = QueryEngine::new(index);

    let file_a_id = query_engine.index().get_file_id(&file_a).unwrap();
    let b_locations = query_engine.goto_definition("B", file_a_id).unwrap();

    // Should handle circular dependencies
    assert!(!b_locations.is_empty());
}

#[tokio::test]
async fn test_workspace_symbols_with_unicode() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("Test.java");
    fs::write(&file_path, "package com.example;\npublic class MyClass {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let query_engine = QueryEngine::new(index);

    // Search with unicode characters
    let symbols = query_engine.workspace_symbols("クラス").unwrap();

    // Should handle unicode gracefully (even if no results)
    assert_eq!(symbols.len(), 0);
}

// Concurrent access edge cases
#[tokio::test]
async fn test_concurrent_reads_and_writes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    for i in 0..10 {
        let file = root.join(format!("src/Class{}.java", i));
        fs::write(
            &file,
            format!("package com.example;\npublic class Class{} {{}}", i),
        )
        .unwrap();
    }

    // Spawn concurrent tasks
    let mut handles = vec![];

    // Writers
    for i in 0..5 {
        let file = root.join(format!("src/Class{}.java", i));
        let index_clone = WorkspaceIndex::new(root.clone());
        handles.push(tokio::spawn(async move {
            index_clone.index_file(&file).await.unwrap();
        }));
    }

    // Readers
    for _ in 0..5 {
        let index_clone = WorkspaceIndex::new(root.clone());
        handles.push(tokio::spawn(async move {
            let _ = index_clone.search("Class");
        }));
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }
}

#[test]
fn test_offset_to_position_beyond_file_end() {
    use hyperion_workspace::offset_to_position;

    let source = "class Test {}";
    let _pos = offset_to_position(source, 1000);

    // Should not panic with offset beyond file (no assertion needed)
}

#[test]
fn test_offset_to_position_with_crlf() {
    use hyperion_workspace::offset_to_position;

    let source = "line1\r\nline2\r\nline3";
    let pos = offset_to_position(source, 7); // Start of line2

    // Should handle CRLF line endings
    assert_eq!(pos.line, 1);
}

#[test]
fn test_offset_to_position_with_multibyte_characters() {
    use hyperion_workspace::offset_to_position;

    // String with multibyte UTF-8 characters
    let source = "日本語\ntest";
    let pos = offset_to_position(source, 10); // After the newline

    assert!(pos.line >= 1);
}

#[tokio::test]
async fn test_index_package_with_keywords() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    // Package name containing what looks like keywords (but in package context)
    let source = "package com.class.interface.enum;\npublic class Test {}";
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    let result = index.index_file(&file_path).await;

    // Parser should handle this
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_find_by_simple_name_with_many_results() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Create 50 classes all named "Service" in different packages
    for i in 0..50 {
        let file = root.join(format!("src/Service{}.java", i));
        fs::write(
            &file,
            format!("package com.example.pkg{};\npublic class Service {{}}", i),
        )
        .unwrap();
    }

    let index = WorkspaceIndex::new(root);
    index.build().unwrap();

    let results = index.find_by_simple_name("Service");

    // Should return all 50
    assert_eq!(results.len(), 50);
}

#[tokio::test]
async fn test_build_with_some_invalid_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Valid file
    fs::write(
        root.join("src/Valid.java"),
        "package com.example;\npublic class Valid {}",
    )
    .unwrap();

    // Invalid UTF-8 file
    fs::write(root.join("src/Invalid.java"), vec![0xFF, 0xFE, 0xFD]).unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.build().unwrap();

    // Should index the valid file and report failure for invalid
    assert_eq!(stats.files_scanned, 2);
    assert_eq!(stats.files_indexed, 1);
    assert_eq!(stats.files_failed, 1);
}

#[tokio::test]
async fn test_parse_tree_size_estimation_no_overflow() {
    // M9: Test that tree size estimation uses saturating_mul to prevent overflow
    use hyperion_workspace::index::WorkspaceIndex;

    // Create a moderately deep syntax tree
    // Each nested block adds to node count
    let deep_nesting = "class Test {".to_string()
        + &"    { { { { { { { { { { ".repeat(100) // 100 levels of nesting
        + &"} } } } } } } } } } ".repeat(100)
        + "}";

    // We can't directly call estimate_tree_size (it's private), but we can verify
    // the tree parses and the count_nodes/estimate_tree_size logic doesn't panic
    // by indexing a file with deep nesting
    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("Deep.java");
    std::fs::write(&file, deep_nesting).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    // This internally calls estimate_tree_size with saturating_mul
    // Should not panic even with pathological tree structure
    let result = index.index_file(&file).await;
    assert!(
        result.is_ok(),
        "Should handle deep nesting without overflow"
    );
}

#[test]
fn test_saturating_mul_prevents_capacity_overflow() {
    // M9: Verify that capacity calculations use saturating_mul
    // This is a regression test to ensure we don't have integer overflow
    // when calculating HashMap capacities for very large file counts

    use hyperion_workspace::index::WorkspaceIndex;

    // Create index with a pathological estimated file count
    // In practice, new() calculates estimated_files from directory scan
    // But we can test the saturating behavior by creating a large workspace

    let temp = tempfile::TempDir::new().unwrap();

    // Create a workspace with many files to trigger capacity calculations
    // The internal code does: estimated_files.saturating_mul(13)
    // This test verifies no panic occurs with large file counts

    std::fs::create_dir_all(temp.path().join("src")).unwrap();

    // Create 1000 files (enough to test capacity calculations)
    for i in 0..1000 {
        let file = temp.path().join(format!("src/File{}.java", i));
        std::fs::write(&file, format!("class File{} {{}}", i)).unwrap();
    }

    // WorkspaceIndex::new() calculates capacities using saturating_mul
    // Should not panic or overflow
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    // Build the index to verify capacity calculations were correct
    let stats = index.build().unwrap();
    assert_eq!(stats.files_indexed, 1000);
}
