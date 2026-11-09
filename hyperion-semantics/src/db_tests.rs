// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for the database module
//!
//! These tests cover the Salsa-based database operations, query caching,
//! incremental computation, and symbol extraction from parse trees.

#[cfg(test)]
mod db_comprehensive_tests {
    use crate::db::*;
    use std::sync::Arc;

    // ========== FileId Tests ==========

    #[test]
    fn test_file_id_creation() {
        let file1 = FileId::new(1);
        let file2 = FileId::new(2);
        let file3 = FileId::new(1);

        assert_eq!(file1.as_u32(), 1);
        assert_eq!(file2.as_u32(), 2);
        assert_eq!(file1, file3);
        assert_ne!(file1, file2);
    }

    #[test]
    fn test_file_id_uniqueness() {
        let mut file_ids = std::collections::HashSet::new();

        for i in 0..1000 {
            file_ids.insert(FileId::new(i));
        }

        assert_eq!(file_ids.len(), 1000);
    }

    // ========== Database Creation Tests ==========

    #[test]
    fn test_database_new() {
        let _db = Database::new();
        // Database should be created successfully (test passes if no panic)
    }

    #[test]
    fn test_database_default() {
        let _db = Database::default();
        // Default should work the same as new (test passes if no panic)
    }

    // ========== Source Database Tests ==========

    #[test]
    fn test_set_and_get_file_text() {
        let mut db = Database::new();
        let file = FileId::new(1);
        let content = "public class Test {}";

        db.set_file_text(file, Arc::new(content.to_string()));
        db.set_file_language(file, Language::Java);
        let retrieved = db.file_text(file);

        assert_eq!(retrieved.as_ref(), content);
    }

    #[test]
    fn test_multiple_files() {
        let mut db = Database::new();

        let files = vec![
            (FileId::new(1), "class A {}"),
            (FileId::new(2), "class B {}"),
            (FileId::new(3), "class C {}"),
            (FileId::new(4), "interface D {}"),
        ];

        for (file_id, content) in &files {
            db.set_file_text(*file_id, Arc::new(content.to_string()));
            db.set_file_language(*file_id, Language::Java);
        }

        for (file_id, content) in &files {
            let retrieved = db.file_text(*file_id);
            assert_eq!(retrieved.as_ref(), *content);
        }
    }

    #[test]
    fn test_file_text_update() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class A {}".to_string()));
        db.set_file_language(file, Language::Java);
        let text1 = db.file_text(file);
        assert_eq!(text1.as_ref(), "class A {}");

        db.set_file_text(file, Arc::new("class B {}".to_string()));
        db.set_file_language(file, Language::Java);
        let text2 = db.file_text(file);
        assert_eq!(text2.as_ref(), "class B {}");
    }

    #[test]
    fn test_large_file_content() {
        let mut db = Database::new();
        let file = FileId::new(1);

        // Create a large file content
        let mut large_content = String::new();
        for i in 0..1000 {
            large_content.push_str(&format!("class Class{} {{}}\n", i));
        }

        db.set_file_text(file, Arc::new(large_content.clone()));
        db.set_file_language(file, Language::Java);
        let retrieved = db.file_text(file);

        assert_eq!(retrieved.as_ref(), &large_content);
    }

    #[test]
    fn test_empty_file() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new(String::new()));
        db.set_file_language(file, Language::Java);
        let retrieved = db.file_text(file);

        assert_eq!(retrieved.as_ref(), "");
    }

    #[test]
    fn test_file_with_special_characters() {
        let mut db = Database::new();
        let file = FileId::new(1);

        let content =
            "class Test {\n    // Comment with émojis 🚀\n    String name = \"Hello\\nWorld\";\n}";
        db.set_file_text(file, Arc::new(content.to_string()));
        db.set_file_language(file, Language::Java);
        let retrieved = db.file_text(file);

        assert_eq!(retrieved.as_ref(), content);
    }

    // ========== Parse Database Tests ==========

    #[test]
    fn test_parse_file_basic() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Hello {}".to_string()));
        db.set_file_language(file, Language::Java);
        let parse_tree = db.parse_file(file);

        assert!(
            parse_tree.errors.is_empty(),
            "Simple class should parse without syntax errors: {:?}",
            parse_tree.errors
        );
    }

    #[test]
    fn test_parse_file_caching() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Foo {}".to_string()));
        db.set_file_language(file, Language::Java);

        let tree1 = db.parse_file(file);
        let tree2 = db.parse_file(file);

        // Should return the same cached instance
        assert!(Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_parse_file_invalidation_on_change() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Foo {}".to_string()));
        db.set_file_language(file, Language::Java);
        let tree1 = db.parse_file(file);

        db.set_file_text(file, Arc::new("class Bar {}".to_string()));
        db.set_file_language(file, Language::Java);
        let tree2 = db.parse_file(file);

        // Should not return the same instance after change
        assert!(!Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_parse_multiple_files_independence() {
        let mut db = Database::new();
        let file1 = FileId::new(1);
        let file2 = FileId::new(2);

        db.set_file_text(file1, Arc::new("class A {}".to_string()));
        db.set_file_language(file1, Language::Java);
        db.set_file_text(file2, Arc::new("class B {}".to_string()));
        db.set_file_language(file2, Language::Java);

        let tree1 = db.parse_file(file1);
        let tree2 = db.parse_file(file2);

        // Should be different trees
        assert!(!Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_parse_with_syntax_errors() {
        let mut db = Database::new();
        let file = FileId::new(1);

        // Invalid Java syntax
        db.set_file_text(file, Arc::new("class { missing name }".to_string()));
        db.set_file_language(file, Language::Java);
        let parse_tree = db.parse_file(file);

        assert!(
            !parse_tree.errors.is_empty(),
            "Malformed source should produce parse errors"
        );
    }

    #[test]
    fn test_parse_empty_file() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new(String::new()));
        db.set_file_language(file, Language::Java);
        let parse_tree = db.parse_file(file);

        assert!(
            parse_tree.errors.is_empty(),
            "Empty source should parse without syntax errors"
        );
        assert_eq!(parse_tree.syntax().text().to_string(), "");
    }

    #[test]
    fn test_parse_complex_java() {
        let mut db = Database::new();
        let file = FileId::new(1);

        let complex_java = r#"
package com.example;

import java.util.*;

public class ComplexClass<T extends Comparable<T>> {
    private List<T> items;

    public ComplexClass() {
        this.items = new ArrayList<>();
    }

    public void add(T item) {
        items.add(item);
    }

    public T get(int index) {
        return items.get(index);
    }
}
"#;

        db.set_file_text(file, Arc::new(complex_java.to_string()));
        db.set_file_language(file, Language::Java);
        let parse_tree = db.parse_file(file);

        assert!(
            parse_tree.errors.is_empty(),
            "Valid complex Java should parse without syntax errors: {:?}",
            parse_tree.errors
        );
    }

    // ========== Semantic Database Tests ==========

    #[test]
    fn test_file_symbols_basic() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class MyClass {}".to_string()));
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        assert!(!symbols.symbols.is_empty());
        assert_eq!(&*symbols.symbols[0].name, "MyClass");
        assert_eq!(symbols.symbols[0].kind, SymbolKind::Class);
    }

    #[test]
    fn test_file_symbols_multiple_classes() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class Class1 {}\nclass Class2 {}\nclass Class3 {}".to_string()),
        );
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        assert_eq!(symbols.symbols.len(), 3);
        assert_eq!(&*symbols.symbols[0].name, "Class1");
        assert_eq!(&*symbols.symbols[1].name, "Class2");
        assert_eq!(&*symbols.symbols[2].name, "Class3");
    }

    #[test]
    fn test_file_symbols_interface() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("interface MyInterface {}".to_string()));
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        assert!(!symbols.symbols.is_empty());
        assert_eq!(&*symbols.symbols[0].name, "MyInterface");
        assert_eq!(symbols.symbols[0].kind, SymbolKind::Interface);
    }

    #[test]
    fn test_file_symbols_mixed_types() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class MyClass {}\ninterface MyInterface {}\nclass AnotherClass {}".into()),
        );
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        assert_eq!(symbols.symbols.len(), 3);

        // Check kinds
        let class_count = symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .count();
        let interface_count = symbols
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Interface)
            .count();

        assert_eq!(class_count, 2);
        assert_eq!(interface_count, 1);
    }

    #[test]
    fn test_file_symbols_with_methods() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class MyClass { void myMethod() {} }".to_string()),
        );
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        // Should find at least the class, possibly the method too
        assert!(!symbols.symbols.is_empty());
    }

    #[test]
    fn test_file_symbols_empty_file() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new(String::new()));
        db.set_file_language(file, Language::Java);
        let symbols = db.file_symbols(file);

        assert_eq!(symbols.symbols.len(), 0);
    }

    #[test]
    fn test_file_symbols_caching() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Test {}".to_string()));
        db.set_file_language(file, Language::Java);

        let symbols1 = db.file_symbols(file);
        let symbols2 = db.file_symbols(file);

        // Should return cached result
        assert!(Arc::ptr_eq(&symbols1, &symbols2));
    }

    #[test]
    fn test_file_symbols_invalidation() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class A {}".to_string()));
        db.set_file_language(file, Language::Java);
        let symbols1 = db.file_symbols(file);

        db.set_file_text(file, Arc::new("class B {}".to_string()));
        db.set_file_language(file, Language::Java);
        let symbols2 = db.file_symbols(file);

        // Should not return cached result after change
        assert!(!Arc::ptr_eq(&symbols1, &symbols2));

        // New symbols should reflect the change
        assert_eq!(&*symbols2.symbols[0].name, "B");
    }

    // ========== Incremental Computation Tests ==========

    #[test]
    fn test_incremental_single_file_change() {
        let mut db = Database::new();
        let file1 = FileId::new(1);
        let file2 = FileId::new(2);

        db.set_file_text(file1, Arc::new("class A {}".to_string()));
        db.set_file_language(file1, Language::Java);
        db.set_file_text(file2, Arc::new("class B {}".to_string()));
        db.set_file_language(file2, Language::Java);

        let symbols1_v1 = db.file_symbols(file1);
        let symbols2_v1 = db.file_symbols(file2);

        // Change only file1
        db.set_file_text(file1, Arc::new("class A_Modified {}".to_string()));
        db.set_file_language(file1, Language::Java);

        let symbols1_v2 = db.file_symbols(file1);
        let symbols2_v2 = db.file_symbols(file2);

        // file1 should be recomputed
        assert!(!Arc::ptr_eq(&symbols1_v1, &symbols1_v2));

        // file2 should be cached
        assert!(Arc::ptr_eq(&symbols2_v1, &symbols2_v2));
    }

    #[test]
    fn test_incremental_multiple_queries() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Original {}".to_string()));
        db.set_file_language(file, Language::Java);

        // Query multiple times
        let _ = db.parse_file(file);
        let _ = db.file_symbols(file);

        let parse1 = db.parse_file(file);
        let symbols1 = db.file_symbols(file);

        // Both should be cached
        let parse2 = db.parse_file(file);
        let symbols2 = db.file_symbols(file);

        assert!(Arc::ptr_eq(&parse1, &parse2));
        assert!(Arc::ptr_eq(&symbols1, &symbols2));
    }

    #[test]
    fn test_incremental_chain_invalidation() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Test {}".to_string()));
        db.set_file_language(file, Language::Java);

        // Build dependency chain: file_text -> parse_file -> file_symbols
        let symbols1 = db.file_symbols(file);

        // Change input (file_text)
        db.set_file_text(file, Arc::new("class Test2 {}".to_string()));
        db.set_file_language(file, Language::Java);

        // All downstream queries should invalidate
        let symbols2 = db.file_symbols(file);

        assert!(!Arc::ptr_eq(&symbols1, &symbols2));
    }

    #[test]
    fn test_many_files_incremental() {
        let mut db = Database::new();
        let num_files = 100;

        // Create many files
        for i in 0..num_files {
            let file = FileId::new(i);
            db.set_file_text(file, Arc::new(format!("class Class{} {{}}", i)));
            db.set_file_language(file, Language::Java);
        }

        // Query all files
        let mut initial_symbols = Vec::new();
        for i in 0..num_files {
            let file = FileId::new(i);
            initial_symbols.push(db.file_symbols(file));
        }

        // Modify only one file
        let changed_file = FileId::new(50);
        db.set_file_text(changed_file, Arc::new("class Modified {}".to_string()));
        db.set_file_language(changed_file, Language::Java);

        // Re-query all files
        for i in 0..num_files {
            let file = FileId::new(i);
            let new_symbols = db.file_symbols(file);

            if i == 50 {
                // Changed file should be recomputed
                assert!(!Arc::ptr_eq(&initial_symbols[i as usize], &new_symbols));
            } else {
                // Other files should be cached
                assert!(Arc::ptr_eq(&initial_symbols[i as usize], &new_symbols));
            }
        }
    }

    // ========== Symbol and FileSymbols Tests ==========

    #[test]
    fn test_symbol_creation() {
        let symbol = Symbol {
            name: Arc::from("test"),
            kind: SymbolKind::Class,
            file: FileId::new(1),
            offset: 42,
            is_deprecated: false,
        };

        assert_eq!(&*symbol.name, "test");
        assert_eq!(symbol.kind, SymbolKind::Class);
        assert_eq!(symbol.file, FileId::new(1));
        assert_eq!(symbol.offset, 42);
    }

    #[test]
    fn test_file_symbols_empty() {
        let empty = FileSymbols::empty();
        assert_eq!(empty.symbols.len(), 0);
    }

    #[test]
    fn test_file_symbols_new() {
        let symbols = vec![
            Symbol {
                name: Arc::from("Class1"),
                kind: SymbolKind::Class,
                file: FileId::new(1),
                offset: 0,
                is_deprecated: false,
            },
            Symbol {
                name: Arc::from("Class2"),
                kind: SymbolKind::Class,
                file: FileId::new(1),
                offset: 100,
                is_deprecated: false,
            },
        ];

        let file_symbols = FileSymbols::new(symbols);
        assert_eq!(file_symbols.symbols.len(), 2);
    }

    #[test]
    fn test_symbol_kinds() {
        let kinds = [
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::Enum,
            SymbolKind::Method,
            SymbolKind::Field,
            SymbolKind::Variable,
            SymbolKind::Parameter,
            SymbolKind::Package,
        ];

        // All kinds should be distinct
        for (i, k1) in kinds.iter().enumerate() {
            for (j, k2) in kinds.iter().enumerate() {
                if i == j {
                    assert_eq!(k1, k2);
                } else {
                    assert_ne!(k1, k2);
                }
            }
        }
    }

    // ========== Integration Tests ==========

    #[test]
    fn test_full_workflow() {
        let mut db = Database::new();
        let file = FileId::new(1);

        // Step 1: Set file text
        db.set_file_text(file, Arc::new("class Workflow {}".to_string()));
        db.set_file_language(file, Language::Java);

        // Step 2: Parse
        let parse_tree = db.parse_file(file);
        assert!(
            parse_tree.errors.is_empty(),
            "Workflow source should parse without syntax errors"
        );

        // Step 3: Extract symbols
        let symbols = db.file_symbols(file);
        assert_eq!(symbols.symbols.len(), 1);
        assert_eq!(&*symbols.symbols[0].name, "Workflow");

        // Step 4: Modify and requery
        db.set_file_text(file, Arc::new("class Workflow2 {}".to_string()));
        db.set_file_language(file, Language::Java);
        let new_symbols = db.file_symbols(file);
        assert_eq!(&*new_symbols.symbols[0].name, "Workflow2");
    }

    #[test]
    fn test_concurrent_file_operations() {
        let mut db = Database::new();

        // Simulate concurrent file operations
        for i in 0..10 {
            let file = FileId::new(i);
            db.set_file_text(file, Arc::new(format!("class Class{} {{}}", i)));
            db.set_file_language(file, Language::Java);
        }

        // Query in different order
        for i in (0..10).rev() {
            let file = FileId::new(i);
            let symbols = db.file_symbols(file);
            assert_eq!(&*symbols.symbols[0].name, format!("Class{}", i));
        }
    }

    #[test]
    fn test_parse_tree_syntax_node() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class TreeTest {}".to_string()));
        db.set_file_language(file, Language::Java);
        let parse_tree = db.parse_file(file);

        // Should be able to get syntax node
        let syntax_node = parse_tree.syntax();
        assert_eq!(syntax_node.text().to_string(), "class TreeTest {}");
    }
}
