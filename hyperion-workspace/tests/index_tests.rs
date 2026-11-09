// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive tests for WorkspaceIndex

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::FileId;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_index_single_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\n\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 1);

    // Check symbol was indexed
    let sym = index.find_symbol("com.example.Test");
    assert!(sym.is_some());
    let sym = sym.unwrap();
    assert_eq!(&*sym.name, "Test");
}

#[tokio::test]
async fn test_index_multiple_symbols_in_one_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Multi.java");

    let source = r#"
package com.example;

public class Foo {}
class Bar {}
interface Baz {}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 3);

    assert!(index.find_symbol("com.example.Foo").is_some());
    assert!(index.find_symbol("com.example.Bar").is_some());
    assert!(index.find_symbol("com.example.Baz").is_some());
}

#[tokio::test]
async fn test_find_by_simple_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("Foo.java");
    fs::write(&file1, "package com.a;\npublic class Foo {}").unwrap();

    let file2 = root.join("Foo2.java");
    fs::write(&file2, "package com.b;\npublic class Foo {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();

    let results = index.find_by_simple_name("Foo");
    assert_eq!(results.len(), 2);

    // Check that both FQNs are represented
    let fqns: Vec<String> = results
        .iter()
        .map(|loc| {
            if loc.name.as_ref() == "Foo" {
                // We need to determine FQN from the file_id
                let file_path = index.get_file_path(loc.file_id).unwrap();
                if file_path == file1 {
                    "com.a.Foo".to_string()
                } else {
                    "com.b.Foo".to_string()
                }
            } else {
                String::new()
            }
        })
        .collect();

    assert!(fqns.iter().any(|fqn| fqn == "com.a.Foo"));
    assert!(fqns.iter().any(|fqn| fqn == "com.b.Foo"));
}

#[tokio::test]
async fn test_find_by_simple_name_no_results() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("Test.java");
    fs::write(&file, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    let results = index.find_by_simple_name("NonExistent");
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_get_imports() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    let source = r#"
package com.example;

import java.util.List;
import java.util.Map;

public class Test {}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path).unwrap();
    let imports = index.get_imports(file_id).unwrap();

    assert_eq!(imports.current_package, Some("com.example".into()));
    assert_eq!(
        imports.explicit.get("List"),
        Some(&"java.util.List".to_string())
    );
    assert_eq!(
        imports.explicit.get("Map"),
        Some(&"java.util.Map".to_string())
    );
}

#[tokio::test]
async fn test_get_file_id_and_path() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path);
    assert!(file_id.is_some());

    let retrieved_path = index.get_file_path(file_id.unwrap());
    assert_eq!(retrieved_path, Some(file_path.clone()));
}

#[tokio::test]
async fn test_remove_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path).unwrap();

    // Verify symbol exists
    assert!(index.find_symbol("com.example.Test").is_some());

    // Remove file
    index.remove_file(file_id);

    // Verify symbol is gone
    assert!(index.find_symbol("com.example.Test").is_none());
    assert!(index.get_file_path(file_id).is_none());
}

#[tokio::test]
async fn test_remove_file_cleans_simple_name_index() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("Foo1.java");
    fs::write(&file1, "package com.a;\npublic class Foo {}").unwrap();

    let file2 = root.join("Foo2.java");
    fs::write(&file2, "package com.b;\npublic class Foo {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();

    // Both Foos should be findable
    assert_eq!(index.find_by_simple_name("Foo").len(), 2);

    // Remove first file
    let file_id1 = index.get_file_id(&file1).unwrap();
    index.remove_file(file_id1);

    // Should only have one Foo now
    assert_eq!(index.find_by_simple_name("Foo").len(), 1);
}

#[tokio::test]
async fn test_search_fuzzy() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("MyService.java");
    fs::write(&file, "package com.example;\npublic class MyService {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    // Search with lowercase (should be case-insensitive)
    let results = index.search("service");
    assert_eq!(results.len(), 1);
    assert_eq!(&*results[0].name, "MyService");
}

#[tokio::test]
async fn test_search_partial_match() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("UserService.java");
    fs::write(&file, "package com.example;\npublic class UserService {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    // Search with partial name
    let results = index.search("User");
    assert_eq!(results.len(), 1);
    assert_eq!(&*results[0].name, "UserService");
}

#[tokio::test]
async fn test_search_by_fqn_part() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("Service.java");
    fs::write(&file, "package com.acme.app;\npublic class Service {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    // Search by package part
    let results = index.search("acme");
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_search_limits_results() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create many files (more than the limit of 100)
    for i in 0..150 {
        let file = root.join(format!("Class{}.java", i));
        fs::write(
            &file,
            format!("package com.example;\npublic class Class{} {{}}", i),
        )
        .unwrap();
    }

    let index = WorkspaceIndex::new(root.clone());

    // Index all files
    for i in 0..150 {
        let file = root.join(format!("Class{}.java", i));
        index.index_file(&file).await.unwrap();
    }

    // Search should limit to 100 results
    let results = index.search("class");
    assert!(results.len() <= 100);
}

#[tokio::test]
async fn test_stats() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("Test1.java");
    fs::write(&file1, "package com.example;\npublic class Test1 {}").unwrap();

    let file2 = root.join("Test2.java");
    fs::write(&file2, "package com.example;\npublic class Test2 {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();

    let stats = index.stats();
    assert_eq!(stats.files_indexed, 2);
    assert_eq!(stats.symbols_indexed, 2);
}

#[tokio::test]
async fn test_index_file_with_source() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    let source = "package com.example;\npublic class Test {}";

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file_with_source(&file_path, source).unwrap();

    assert_eq!(stats.symbols_added, 1);
    assert!(index.find_symbol("com.example.Test").is_some());
}

#[tokio::test]
async fn test_reindex_file_updates_symbols() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    // Initial content
    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    assert!(index.find_symbol("com.example.Test").is_some());
    assert!(index.find_symbol("com.example.Updated").is_none());

    // Update content
    fs::write(&file_path, "package com.example;\npublic class Updated {}").unwrap();
    index.index_file(&file_path).await.unwrap();

    // After fix: Old symbol should be REMOVED, new symbol should be added
    assert!(
        index.find_symbol("com.example.Test").is_none(),
        "Old symbol should be removed"
    );
    assert!(
        index.find_symbol("com.example.Updated").is_some(),
        "New symbol should be added"
    );
}

#[tokio::test]
async fn test_index_handles_empty_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Empty.java");

    fs::write(&file_path, "").unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 0);
}

#[tokio::test]
async fn test_index_handles_comments_only_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Comments.java");

    let source = r#"
// This is a comment file
/*
 * With multiple comments
 */
// But no actual code
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 0);
}

#[tokio::test]
async fn test_index_multiple_files_same_simple_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("Test1.java");
    fs::write(&file1, "package com.a;\npublic class Service {}").unwrap();

    let file2 = root.join("Test2.java");
    fs::write(&file2, "package com.b;\npublic class Service {}").unwrap();

    let file3 = root.join("Test3.java");
    fs::write(&file3, "package com.c;\npublic class Service {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();
    index.index_file(&file3).await.unwrap();

    let results = index.find_by_simple_name("Service");
    assert_eq!(results.len(), 3);

    // All three should be findable by FQN
    assert!(index.find_symbol("com.a.Service").is_some());
    assert!(index.find_symbol("com.b.Service").is_some());
    assert!(index.find_symbol("com.c.Service").is_some());
}

#[tokio::test]
async fn test_file_id_stability() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let file_id1 = index.get_file_id(&file_path).unwrap();

    // Re-index the same file
    index.index_file(&file_path).await.unwrap();

    let file_id2 = index.get_file_id(&file_path).unwrap();

    // File ID should be stable
    assert_eq!(file_id1, file_id2);
}

#[tokio::test]
async fn test_index_with_different_symbol_kinds() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    let source = r#"
package com.example;

public class MyClass {}
interface MyInterface {}
enum MyEnum { A, B }
@interface MyAnnotation {}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    // Should extract at least class, interface, enum, annotation (and possibly enum members)
    assert!(
        stats.symbols_added >= 4,
        "Expected at least 4 symbols, got {}",
        stats.symbols_added
    );

    assert!(index.find_symbol("com.example.MyClass").is_some());
    assert!(index.find_symbol("com.example.MyInterface").is_some());
    assert!(index.find_symbol("com.example.MyEnum").is_some());
    assert!(index.find_symbol("com.example.MyAnnotation").is_some());
}

#[tokio::test]
async fn test_build_indexes_workspace() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a small workspace
    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
    fs::write(
        root.join("src/main/java/com/example/App.java"),
        "package com.example;\npublic class App {}",
    )
    .unwrap();
    fs::write(
        root.join("src/main/java/com/example/Service.java"),
        "package com.example;\npublic class Service {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.build().unwrap();

    assert_eq!(stats.files_scanned, 2);
    assert_eq!(stats.files_indexed, 2);
    assert_eq!(stats.symbols_indexed, 2);
    assert_eq!(stats.files_failed, 0);
}

#[tokio::test]
async fn test_concurrent_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create multiple files
    for i in 0..10 {
        let file = root.join(format!("Class{}.java", i));
        fs::write(
            &file,
            format!("package com.example;\npublic class Class{} {{}}", i),
        )
        .unwrap();
    }

    // Index files concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let file = root.join(format!("Class{}.java", i));
        let index_clone = WorkspaceIndex::new(root.clone());
        handles.push(tokio::spawn(
            async move { index_clone.index_file(&file).await },
        ));
    }

    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // All symbols should be indexed (using the original index won't see them though)
    // So we create a new index and build it
    let final_index = WorkspaceIndex::new(root);
    let stats = final_index.build().unwrap();
    assert_eq!(stats.symbols_indexed, 10);
}

#[tokio::test]
async fn test_remove_file_with_multiple_symbols() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Multi.java");

    let source = r#"
package com.example;

public class Foo {}
class Bar {}
interface Baz {}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // All symbols should be indexed
    assert!(index.find_symbol("com.example.Foo").is_some());
    assert!(index.find_symbol("com.example.Bar").is_some());
    assert!(index.find_symbol("com.example.Baz").is_some());

    // Remove the file
    let file_id = index.get_file_id(&file_path).unwrap();
    index.remove_file(file_id);

    // All symbols should be removed
    assert!(index.find_symbol("com.example.Foo").is_none());
    assert!(index.find_symbol("com.example.Bar").is_none());
    assert!(index.find_symbol("com.example.Baz").is_none());
}

#[tokio::test]
async fn test_index_preserves_symbol_metadata() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let sym = index.find_symbol("com.example.Test").unwrap();

    // Check metadata
    assert_eq!(&*sym.name, "Test");
    assert!(sym.range.start < sym.range.end);
    assert!(!sym.range.is_empty());
}

#[tokio::test]
async fn test_get_imports_nonexistent_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root);

    let imports = index.get_imports(FileId(999));
    assert!(imports.is_none());
}

#[tokio::test]
async fn test_find_symbol_nonexistent() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root);

    let sym = index.find_symbol("com.nonexistent.Class");
    assert!(sym.is_none());
}

#[tokio::test]
async fn test_search_returns_empty_for_no_matches() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("Test.java");
    fs::write(&file, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    let results = index.search("NonExistentPattern");
    assert_eq!(results.len(), 0);
}

// =============================================================================
// Phase 2: Multi-overload lookup API tests
// =============================================================================

#[tokio::test]
async fn test_find_methods_by_name_returns_all_overloads() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void process(String s, int n) {}
    public void process(String s, int n, boolean flag) {}
    public void other() {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let methods = index.find_methods_by_name("com.example.Service", "process");
    assert_eq!(methods.len(), 3, "Should find all 3 process overloads");

    // Verify parameter counts
    let param_counts: Vec<usize> = methods.iter().map(|m| m.parameters.len()).collect();
    assert!(param_counts.contains(&1), "Should have 1-param version");
    assert!(param_counts.contains(&2), "Should have 2-param version");
    assert!(param_counts.contains(&3), "Should have 3-param version");
}

#[tokio::test]
async fn test_find_methods_by_name_excludes_other_methods() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void processAll(String s) {}
    public void other() {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let methods = index.find_methods_by_name("com.example.Service", "process");
    assert_eq!(
        methods.len(),
        1,
        "Should only find 'process', not 'processAll'"
    );
    assert_eq!(methods[0].simple_name.as_ref(), "process");
}

#[tokio::test]
async fn test_find_methods_by_name_empty_for_nonexistent() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let methods = index.find_methods_by_name("com.example.Service", "nonexistent");
    assert_eq!(methods.len(), 0);
}

#[tokio::test]
async fn test_find_method_by_args_exact_match() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void process(String s, int n) {}
    public void process(String s, int n, boolean flag) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // Find 1-param version
    let method = index.find_method_by_args("com.example.Service", "process", 1);
    assert!(method.is_some());
    assert_eq!(method.unwrap().parameters.len(), 1);

    // Find 2-param version
    let method = index.find_method_by_args("com.example.Service", "process", 2);
    assert!(method.is_some());
    assert_eq!(method.unwrap().parameters.len(), 2);

    // Find 3-param version
    let method = index.find_method_by_args("com.example.Service", "process", 3);
    assert!(method.is_some());
    assert_eq!(method.unwrap().parameters.len(), 3);
}

#[tokio::test]
async fn test_find_method_by_args_varargs_fallback() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Logger.java");

    let source = r#"
package com.example;

public class Logger {
    public void log(String msg) {}
    public void log(String msg, Object... args) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // 1 arg should match the exact String version
    let method = index.find_method_by_args("com.example.Logger", "log", 1);
    assert!(method.is_some());
    assert_eq!(method.as_ref().unwrap().parameters.len(), 1);
    assert!(!method.unwrap().parameters[0].contains("..."));

    // 3 args should match varargs version (String + Object...)
    let method = index.find_method_by_args("com.example.Logger", "log", 3);
    assert!(method.is_some());
    let method = method.unwrap();
    assert_eq!(method.parameters.len(), 2);
    assert!(
        method.parameters[1].contains("..."),
        "Should be varargs: {:?}",
        method.parameters
    );
}

#[tokio::test]
async fn array_parameter_does_not_expand_like_declared_varargs() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Formatter.java");

    let source = r#"
package com.example;

public class Formatter {
    public void format(String pattern, Object[] args) {}
    public void printf(String pattern, Object... args) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // An ordinary array parameter consumes exactly one argument, so
    // `format(String, Object[])` is not applicable to a 5-argument call.
    // Only a declared `...` parameter expands (JLS 15.12.2.4).
    assert!(
        index
            .find_method_by_args("com.example.Formatter", "format", 5)
            .is_none(),
        "an array parameter must not match an arity it cannot accept"
    );
    assert!(
        index
            .find_method_by_args("com.example.Formatter", "format", 2)
            .is_some(),
        "the array overload must still match its own arity"
    );
    assert!(
        index
            .find_method_by_args("com.example.Formatter", "printf", 5)
            .is_some(),
        "a declared varargs parameter must expand"
    );
}

#[tokio::test]
async fn test_find_method_by_args_none_for_wrong_count() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void process(String s, int n) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // No 5-param version exists and no varargs
    let method = index.find_method_by_args("com.example.Service", "process", 5);
    assert!(method.is_none());
}

#[tokio::test]
async fn test_find_method_location_by_args() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void process(String s, int n) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let loc = index.find_method_location_by_args("com.example.Service", "process", 2);
    assert!(loc.is_some());
    let loc = loc.unwrap();
    assert_eq!(&*loc.name, "process");
}

#[tokio::test]
async fn test_find_method_locations_by_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.java");

    let source = r#"
package com.example;

public class Service {
    public void process(String s) {}
    public void process(String s, int n) {}
}
"#;
    fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let locs = index.find_method_locations_by_name("com.example.Service", "process");
    assert_eq!(locs.len(), 2);
}
