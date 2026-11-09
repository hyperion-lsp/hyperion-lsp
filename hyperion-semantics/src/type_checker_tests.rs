// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use crate::db::FileId;
use crate::symbol::{ClassType, PrimitiveType, SourceLocation, SymbolKind, Visibility};
use hyperion_parser::{lexer::Lexer, parser::Parser};

fn parse(source: &str) -> SyntaxNode {
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

/// Attach the file's name-resolution facts the way the diagnostics pipeline
/// does; calls are only judged through receivers whose declared type the
/// resolver can point at.
fn with_file_semantics(
    checker: &mut TypeChecker,
    root: &SyntaxNode,
    file_id: hyperion_workspace::FileId,
    index: &hyperion_workspace::WorkspaceIndex,
) {
    let semantics = crate::semantic_walker::compute_file_semantics(
        root,
        file_id,
        &crate::symbol::SymbolTable::new(),
        index,
    );
    checker.set_file_semantics(Arc::new(semantics));
}

/// Index a minimal, hierarchy-complete `java.lang` so provability-based checks
/// (which stay silent about types the index does not fully know) can decide
/// calls that involve `String` and boxed primitives.
fn index_java_lang_stubs(index: &hyperion_workspace::WorkspaceIndex, root: &std::path::Path) {
    let stubs: &[(&str, &str)] = &[
        ("java/lang/Object.java", "package java.lang; public class Object {}"),
        ("java/lang/Comparable.java", "package java.lang; public interface Comparable<T> {}"),
        ("java/io/Serializable.java", "package java.io; public interface Serializable {}"),
        ("java/lang/CharSequence.java", "package java.lang; public interface CharSequence {}"),
        (
            "java/lang/String.java",
            "package java.lang; public final class String implements java.io.Serializable, Comparable<String>, CharSequence {}",
        ),
        (
            "java/lang/Number.java",
            "package java.lang; public abstract class Number implements java.io.Serializable {}",
        ),
        (
            "java/lang/Integer.java",
            "package java.lang; public final class Integer extends Number implements Comparable<Integer> {}",
        ),
        (
            "java/lang/Long.java",
            "package java.lang; public final class Long extends Number implements Comparable<Long> {}",
        ),
        (
            "java/lang/Double.java",
            "package java.lang; public final class Double extends Number implements Comparable<Double> {}",
        ),
        (
            "java/lang/Boolean.java",
            "package java.lang; public final class Boolean implements java.io.Serializable, Comparable<Boolean> {}",
        ),
    ];
    for (path, source) in stubs {
        index
            .index_file_with_source(&root.join(path), source)
            .unwrap_or_else(|error| panic!("failed to index stub {path}: {error}"));
    }
}

fn make_symbol(name: &str, typ: Type) -> Symbol {
    Symbol::new(
        name.to_string(),
        SymbolKind::LocalVariable {
            var_type: typ.clone(),
            is_final: false,
        },
        typ,
        Visibility::Private,
        SourceLocation::new(FileId(0), 0, 10),
    )
}

#[test]
fn test_type_checker_creation() {
    let checker = TypeChecker::new();
    assert_eq!(checker.errors.len(), 0);
    assert_eq!(checker.variable_types_stack.len(), 1);
    assert_eq!(checker.variable_types_stack[0].len(), 0);
}

#[test]
fn test_declaration_type_mismatch_string_to_int() {
    // String foo = 1;
    let source = r#"
        class Test {
            void test() {
                String foo = 1;
            }
        }
    "#;
    let root = parse(source);

    // Debug: find the LOCAL_VAR_DECL and print its structure
    for node in root.descendants() {
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            println!("Found LOCAL_VAR_DECL: {:?}", node.text());
            for child in node.children() {
                println!("  Child: {:?} = {:?}", child.kind(), child.text());
            }
            // Test extraction methods
            let checker = TypeChecker::new();
            let declared = checker.extract_declared_type(&node);
            let initializer = checker.extract_initializer_type(&node);
            println!("  Declared type: {:?}", declared);
            println!("  Initializer type: {:?}", initializer);
        }
    }

    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "Expected 1 error for String = int");
    assert!(
        errors[0].message().contains("incompatible types"),
        "Error message should mention incompatible types"
    );
}

/// Parameter names were read with `descendants_with_tokens`, which enters the
/// type node first: `String value` extracted the name "String" and
/// `java.util.List<String> items` extracted "java". The real parameter was
/// never registered, so nothing that used it was type-checked, and the type's
/// own name was registered as a variable instead.
#[test]
fn parameter_names_come_from_the_declaration_not_its_type() {
    for (source, expected) in [
        ("class Test { void run(String value) {} }", "value"),
        (
            "class Test { void run(java.util.List<String> items) {} }",
            "items",
        ),
        ("class Test { void run(final int count) {} }", "count"),
        ("class Test { void run(String... parts) {} }", "parts"),
        ("class Test { void run(String[] names) {} }", "names"),
    ] {
        let root = parse(source);
        let param = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAM)
            .expect("PARAM node");
        let checker = TypeChecker::new();

        let (name, _) = checker
            .extract_param_name_and_type(&param)
            .unwrap_or_else(|| panic!("no parameter extracted from {source}"));

        assert_eq!(name, expected, "wrong parameter name for {source}");
    }
}

#[test]
fn test_declaration_type_mismatch_int_to_string() {
    // int foo = "hello";
    let source = r#"
        class Test {
            void test() {
                int foo = "hello";
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "Expected 1 error for int = String");
}

#[test]
fn test_null_to_primitive_int() {
    // int x = null;
    let source = r#"
        class Test {
            void test() {
                int x = null;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "Expected 1 error for int = null");
    assert!(
        errors[0].message().contains("null") && errors[0].message().contains("primitive"),
        "Error message should mention null and primitive"
    );
}

#[test]
fn test_null_to_primitive_boolean() {
    // boolean b = null;
    let source = r#"
        class Test {
            void test() {
                boolean b = null;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "Expected 1 error for boolean = null");
}

#[test]
fn test_numeric_promotion_with_long_literal() {
    let source = r#"
        class Test {
            void test() {
                long value = 1 + 2L;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert!(
        errors.is_empty(),
        "Expected no errors for numeric promotion to long, got: {:?}",
        errors
    );
}

#[test]
fn test_string_concat_with_number() {
    let source = r#"
        class Test {
            void test() {
                String value = "id=" + 42;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert!(
        errors.is_empty(),
        "Expected no errors for string concatenation, got: {:?}",
        errors
    );
}

#[test]
fn test_boolean_bitwise_and() {
    let source = r#"
        class Test {
            void test() {
                boolean value = true & false;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert!(
        errors.is_empty(),
        "Expected no errors for boolean bitwise and, got: {:?}",
        errors
    );
}

#[test]
fn test_valid_string_assignment() {
    // String s = "hello";
    let source = r#"
        class Test {
            void test() {
                String s = "hello";
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 0, "String = String literal should be valid");
}

#[test]
fn test_valid_int_assignment() {
    // int x = 42;
    let source = r#"
        class Test {
            void test() {
                int x = 42;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 0, "int = int literal should be valid");
}

#[test]
fn test_valid_null_to_reference_type() {
    // String s = null;
    let source = r#"
        class Test {
            void test() {
                String s = null;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 0, "String = null should be valid");
}

#[test]
fn test_var_declaration_no_type_check() {
    // var declarations are inferred, not checked
    let source = r#"
        class Test {
            void test() {
                var x = 42;
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(
        errors.len(),
        0,
        "var declarations should not produce type errors"
    );
}

#[test]
fn test_assignment_type_mismatch_with_symbol_table() {
    // var foo = 1; foo = "Hey";
    let source = r#"
        class Test {
            void test() {
                var foo = 1;
                foo = "Hey";
            }
        }
    "#;
    let root = parse(source);

    // Provide symbol table with foo as int
    let symbols = vec![make_symbol("foo", Type::Primitive(PrimitiveType::Int))];

    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &symbols);

    assert_eq!(
        errors.len(),
        1,
        "Expected 1 error for int = String assignment"
    );
    assert!(
        errors[0].message().contains("incompatible types"),
        "Error should mention incompatible types"
    );
}

#[test]
fn test_valid_constructor_call() {
    // User user = new User();
    let source = r#"
        class Test {
            void test() {
                User user = new User();
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 0, "User = new User() should be valid");
}

#[test]
fn test_type_mismatch_constructor_call() {
    // User user = new String();
    let source = r#"
        class Test {
            void test() {
                User user = new String();
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "User = new String() should be an error");
}

#[test]
fn test_cast_expression_infers_target_type() {
    // HttpURLConnection conn = (HttpURLConnection) expr;
    // The cast should be valid - we assign the cast target type to the variable
    let source = r#"
        class Test {
            void test() {
                HttpURLConnection conn = (HttpURLConnection) getConnection();
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    // Should be 0 errors - the cast target type (HttpURLConnection) matches the declared type
    assert_eq!(
        errors.len(),
        0,
        "Cast expression should use target type, not source type"
    );
}

#[test]
fn test_cast_to_primitive_type() {
    // int x = (int) someDouble;
    let source = r#"
        class Test {
            void test() {
                int x = (int) getValue();
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    // Should be 0 errors - the cast target type (int) matches the declared type
    assert_eq!(errors.len(), 0, "Primitive cast should use target type");
}

#[test]
fn test_diamond_operator_with_list() {
    // List<String> recipients = new ArrayList<>();
    // The diamond operator should be compatible with the target type
    let source = r#"
        import java.util.List;
        import java.util.ArrayList;

        class Test {
            void test() {
                List<String> recipients = new ArrayList<>();
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    // Should be 0 errors - ArrayList<> with diamond should be compatible with List<String>
    // Uses is_known_jdk_subtype() fallback for JLS-constant relationships.
    assert_eq!(
        errors.len(),
        0,
        "Diamond operator should be compatible with target type: {:?}",
        errors
    );
}

// =========================================================================
// Method Signature Validation Tests
// =========================================================================

#[test]
fn test_method_call_missing_required_argument() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index User class with setName(String) method
    let user_path = workspace_root.join("User.java");
    let user_source = r#"
        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class with method call missing argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                User user = new User();
                user.setName();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    // Type check with workspace context
    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have WrongArgumentCount error
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. })),
        "Expected WrongArgumentCount error for missing argument, got: {:?}",
        errors
    );

    let error = errors
        .iter()
        .find(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. }))
        .unwrap();

    if let SemanticErrorKind::WrongArgumentCount {
        method_name,
        expected,
        actual,
    } = &error.kind
    {
        assert_eq!(method_name, "setName");
        assert_eq!(*expected, 1);
        assert_eq!(*actual, 0);
    }
}

#[test]
fn test_method_call_too_many_arguments() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index User class
    let user_path = workspace_root.join("User.java");
    let user_source = r#"
        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class with too many arguments
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                User user = new User();
                user.setName("John", "Doe");
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have WrongArgumentCount error
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. })),
        "Expected WrongArgumentCount error for too many arguments, got: {:?}",
        errors
    );

    let error = errors
        .iter()
        .find(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. }))
        .unwrap();

    if let SemanticErrorKind::WrongArgumentCount {
        method_name,
        expected,
        actual,
    } = &error.kind
    {
        assert_eq!(method_name, "setName");
        assert_eq!(*expected, 1);
        assert_eq!(*actual, 2);
    }
}

#[test]
fn test_method_call_wrong_argument_type() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());
    index_java_lang_stubs(&index, &workspace_root);

    // Index User class
    let user_path = workspace_root.join("User.java");
    let user_source = r#"
        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class with wrong type
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                User user = new User();
                user.setName(123);
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have MethodArgumentTypeMismatch error
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::MethodArgumentTypeMismatch { .. })),
        "Expected MethodArgumentTypeMismatch error for wrong type, got: {:?}",
        errors
    );
}

#[test]
fn test_method_call_correct_signature() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index User class
    let user_path = workspace_root.join("User.java");
    let user_source = r#"
        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class with correct call
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                User user = new User();
                user.setName("John");
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO signature errors for correct call
    let signature_errors: Vec<_> = errors
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                SemanticErrorKind::WrongArgumentCount { .. }
                    | SemanticErrorKind::MethodArgumentTypeMismatch { .. }
            )
        })
        .collect();

    assert_eq!(
        signature_errors.len(),
        0,
        "Should have no signature errors for correct method call, got: {:?}",
        signature_errors
    );
}

#[test]
fn test_method_call_with_generic_params() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index Service class with process method taking Map<String, Integer>
    let service_path = workspace_root.join("Service.java");
    let service_source = r#"
        public class Service {
            public void process(java.util.Map<java.lang.String, java.lang.Integer> data) {
            }
        }
    "#;
    std::fs::write(&service_path, service_source).unwrap();
    index
        .index_file_with_source(&service_path, service_source)
        .unwrap();

    // Index Main class calling process with Map argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                Service service = new Service();
                java.util.Map<java.lang.String, java.lang.Integer> map = null;
                service.process(map);
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    // Type check - should pass without errors
    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO signature errors for correct call with generic params
    let signature_errors: Vec<_> = errors
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                SemanticErrorKind::WrongArgumentCount { .. }
                    | SemanticErrorKind::MethodArgumentTypeMismatch { .. }
            )
        })
        .collect();

    assert_eq!(
        signature_errors.len(),
        0,
        "Should have no signature errors for method with generic params, got: {:?}",
        signature_errors
    );
}

#[test]
fn test_method_call_with_packaged_classes() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Create package directory structure
    let model_dir = workspace_root.join("com").join("example").join("model");
    std::fs::create_dir_all(&model_dir).unwrap();

    // Index User class in com.example.model package
    let user_path = model_dir.join("User.java");
    let user_source = r#"
        package com.example.model;

        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class in com.example package that imports User
    let example_dir = workspace_root.join("com").join("example");
    let main_path = example_dir.join("Main.java");
    let main_source = r#"
        package com.example;
        import com.example.model.User;

        public class Main {
            public void test() {
                User user = new User();
                user.setName();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    // Type check - should detect wrong argument count
    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have WrongArgumentCount error for packaged class method
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. })),
        "Expected WrongArgumentCount error for packaged class method, got: {:?}",
        errors
    );

    let error = errors
        .iter()
        .find(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. }))
        .unwrap();

    if let SemanticErrorKind::WrongArgumentCount {
        method_name,
        expected,
        actual,
    } = &error.kind
    {
        assert_eq!(method_name, "setName");
        assert_eq!(*expected, 1);
        assert_eq!(*actual, 0);
    }
}

#[test]
fn test_var_variable_type_tracking() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index User class
    let user_path = workspace_root.join("User.java");
    let user_source = r#"
        public class User {
            public void setName(String name) {
            }
        }
    "#;
    std::fs::write(&user_path, user_source).unwrap();
    index
        .index_file_with_source(&user_path, user_source)
        .unwrap();

    // Index Main class using var declaration
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                var user = new User();
                user.setName();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    // Type check - should detect wrong argument count on var-declared variable
    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have WrongArgumentCount error even for var variable
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::WrongArgumentCount { .. })),
        "Expected WrongArgumentCount error for var variable method call, got: {:?}",
        errors
    );
}

#[test]
fn test_varargs_parameter_handling() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());
    index_java_lang_stubs(&index, &workspace_root);

    // Index Logger class with varargs method
    let logger_path = workspace_root.join("Logger.java");
    let logger_source = r#"
        public class Logger {
            public void log(String... messages) {
            }
        }
    "#;
    std::fs::write(&logger_path, logger_source).unwrap();
    index
        .index_file_with_source(&logger_path, logger_source)
        .unwrap();

    // Index Main class using varargs method
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                Logger logger = new Logger();
                logger.log("message1", "message2", "message3");
                logger.log(42);
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    // Parse and type check Main
    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let mismatches: Vec<_> = errors
        .iter()
        .filter(|error| {
            matches!(
                error.kind,
                SemanticErrorKind::MethodArgumentTypeMismatch { .. }
            )
        })
        .collect();
    assert_eq!(
        mismatches.len(),
        1,
        "expanded String varargs should accept strings and reject only the int: {errors:?}"
    );
    let SemanticErrorKind::MethodArgumentTypeMismatch { expected_type, .. } = &mismatches[0].kind
    else {
        unreachable!("filtered to argument mismatches")
    };
    assert!(
        matches!(expected_type, Type::Class(class_type) if class_type.name == "String"),
        "expanded varargs diagnostics should name the element type, got {expected_type:?}"
    );
}

#[test]
fn test_long_literal_type_inference() {
    // Test that long literals are correctly inferred as long, not int
    let source = r#"
        class Test {
            void test() {
                int x = 9999999999999L;  // Should error - long can't fit in int
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(errors.len(), 1, "Expected 1 error for int = long literal");
    assert!(
        errors[0].message().contains("incompatible types"),
        "Error message should mention incompatible types, got: {}",
        errors[0].message()
    );
}

#[test]
fn test_long_literal_to_long_variable() {
    // Test that long literals can be assigned to long variables
    let source = r#"
        class Test {
            void test() {
                long x = 9999999999999L;  // Should be OK
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(
        errors.len(),
        0,
        "Expected no errors for long = long literal, got: {:?}",
        errors
    );
}

#[test]
fn test_int_literal_to_int_variable() {
    // Test that int literals still work correctly
    let source = r#"
        class Test {
            void test() {
                int x = 42;  // Should be OK
            }
        }
    "#;
    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    assert_eq!(
        errors.len(),
        0,
        "Expected no errors for int = int literal, got: {:?}",
        errors
    );
}

#[test]
fn test_void_type_parsing() {
    // Test that void is correctly parsed as Type::Void, not Type::Class
    let checker = TypeChecker::new();

    let void_type = checker.parse_type_from_string("void");
    assert!(
        matches!(void_type, Some(Type::Void)),
        "Expected Type::Void, got: {:?}",
        void_type
    );

    // Also test that it's not parsed as a class type
    if let Some(Type::Class(_)) = void_type {
        panic!("void should not be parsed as a Class type");
    }
}

#[test]
fn test_shadowed_variable_scope_tracking() {
    // Test the fix for Variable scope tracking
    // This reproduces the bug from BUGS.md where shadowed variables
    // overwrite each other in a flat HashMap
    let source = r#"
        class Test {
            void test() {
                int x = 1;
                {
                    String x = "hello";
                }
                x = "oops";
            }
        }
    "#;

    let root = parse(source);
    let mut checker = TypeChecker::new();

    // No symbols needed for this test - just checking assignments
    let errors = checker.check(&root, &[]);

    // Should have 1 error: trying to assign String "oops" to int x
    assert_eq!(
        errors.len(),
        1,
        "Expected 1 error for assignment type mismatch, got {}: {:?}",
        errors.len(),
        errors
    );

    // Verify it's an assignment type mismatch
    assert!(
        matches!(
            &errors[0].kind,
            SemanticErrorKind::AssignmentTypeMismatch { .. }
        ),
        "Expected AssignmentTypeMismatch, got: {:?}",
        errors[0].kind
    );
}

#[test]
fn test_shadowed_variable_outer_scope_valid() {
    // Verify that after a shadowing block exits, the outer variable is still valid
    let source = r#"
        class Test {
            void test() {
                int x = 1;
                {
                    String x = "hello";
                }
                x = 2;
            }
        }
    "#;

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    // Should have 0 errors: assigning int 2 to int x is valid
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors, got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_multiple_nested_scopes() {
    // Test multiple levels of nested blocks with shadowing
    let source = r#"
        class Test {
            void test() {
                int x = 1;
                {
                    String x = "a";
                    {
                        boolean x = true;
                        x = false;
                    }
                    x = "b";
                }
                x = 2;
            }
        }
    "#;

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);

    // All assignments should be valid in their respective scopes
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors, got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_method_overload_resolution_int_vs_string() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index ResultSet-like class with overloaded getString methods
    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public String getString(int columnIndex) {
                return "";
            }
            public String getString(String columnLabel) {
                return "";
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    // Index Main class calling getString with int argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                ResultSet rs = new ResultSet();
                rs.getString(1);
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO errors - overload resolution should pick getString(int)
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors (correct overload selected), got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_method_overload_resolution_string_arg() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index ResultSet-like class with overloaded getString methods
    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public String getString(int columnIndex) {
                return "";
            }
            public String getString(String columnLabel) {
                return "";
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    // Index Main class calling getString with String argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                ResultSet rs = new ResultSet();
                rs.getString("column_name");
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO errors - overload resolution should pick getString(String)
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors (correct overload selected), got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_assignment_to_field_type_resolves_through_receiver() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    let entry_path = workspace_root.join("EntryType.java");
    let entry_source = r#"
        public class EntryType {
            public long id;
        }
    "#;
    std::fs::write(&entry_path, entry_source).unwrap();
    index
        .index_file_with_source(&entry_path, entry_source)
        .unwrap();

    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public long getLong(String columnLabel) {
                return 0L;
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test(ResultSet rs, EntryType entry) {
                entry.id = rs.getLong("id");
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors for field assignment, got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_new_expr_resolves_to_current_package_type() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    let service_path = workspace_root.join("org/example/library/CatalogService.java");
    std::fs::create_dir_all(service_path.parent().unwrap()).unwrap();
    let service_source = r#"
        package org.example.library;

        public class CatalogService {}
    "#;
    std::fs::write(&service_path, service_source).unwrap();
    index
        .index_file_with_source(&service_path, service_source)
        .unwrap();

    let controller_path = workspace_root.join("org/example/library/Controller.java");
    let controller_source = r#"
        package org.example.library;

        public class Controller {
            private org.example.library.CatalogService catalogService;

            public void build() {
                this.catalogService = new CatalogService();
            }
        }
    "#;
    std::fs::write(&controller_path, controller_source).unwrap();
    index
        .index_file_with_source(&controller_path, controller_source)
        .unwrap();
    let file_id = index.get_file_id(&controller_path).unwrap();

    let root = parse(controller_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors for same-package new expression, got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_method_overload_resolution_wrong_type() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());
    index_java_lang_stubs(&index, &workspace_root);

    // Index ResultSet-like class with overloaded getString methods
    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public String getString(int columnIndex) {
                return "";
            }
            public String getString(String columnLabel) {
                return "";
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    // Index Main class calling getString with boolean argument (no matching overload)
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                ResultSet rs = new ResultSet();
                rs.getString(true);
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    with_file_semantics(&mut checker, &root, file_id, &index);
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have an error - no overload accepts boolean
    assert!(
        !errors.is_empty(),
        "Expected type mismatch error when no overload matches"
    );
    assert!(
        errors
            .iter()
            .any(|e| matches!(e.kind, SemanticErrorKind::MethodArgumentTypeMismatch { .. })),
        "Expected MethodArgumentTypeMismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_var_inference_with_overloaded_method_int_arg() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index ResultSet-like class with overloaded getString methods
    // getString(int) returns String, getString(String) also returns String
    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public String getString(int columnIndex) {
                return "";
            }
            public String getString(String columnLabel) {
                return "";
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    // Index Main class using var with overloaded method call
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                ResultSet rs = new ResultSet();
                var result = rs.getString(1);
                // result should be String, so this should be valid
                String s = result;
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO errors - var should infer String from getString(int)
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors (var correctly inferred as String), got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_var_inference_with_overloaded_method_string_arg() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index ResultSet-like class with overloaded getString methods
    let resultset_path = workspace_root.join("ResultSet.java");
    let resultset_source = r#"
        public class ResultSet {
            public String getString(int columnIndex) {
                return "";
            }
            public String getString(String columnLabel) {
                return "";
            }
        }
    "#;
    std::fs::write(&resultset_path, resultset_source).unwrap();
    index
        .index_file_with_source(&resultset_path, resultset_source)
        .unwrap();

    // Index Main class using var with String argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                ResultSet rs = new ResultSet();
                var result = rs.getString("column_name");
                // result should be String
                String s = result;
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO errors - var should infer String from getString(String)
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors (var correctly inferred as String), got {}: {:?}",
        errors.len(),
        errors
    );
}

#[test]
fn test_var_inference_with_different_return_types() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    // Index class with overloaded get methods returning different types
    let data_path = workspace_root.join("DataSource.java");
    let data_source = r#"
        public class DataSource {
            public int get(int index) {
                return 0;
            }
            public String get(String key) {
                return "";
            }
        }
    "#;
    std::fs::write(&data_path, data_source).unwrap();
    index
        .index_file_with_source(&data_path, data_source)
        .unwrap();

    // Test that var infers correct type based on argument
    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                DataSource ds = new DataSource();
                var intResult = ds.get(1);
                var stringResult = ds.get("key");

                // These should be valid
                int i = intResult;
                String s = stringResult;
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO errors - var should infer correct types
    assert_eq!(
        errors.len(),
        0,
        "Expected 0 errors (var correctly inferred different types), got {}: {:?}",
        errors.len(),
        errors
    );
}

// =============================================================================
// P0 FIX: Diagnostic and Navigation Resolution Unification Tests
// =============================================================================
// These tests verify that the TypeChecker's diagnostic resolution matches
// the navigation (goto-definition) resolution. Before the fix, diagnostics
// would report "cannot find symbol" for inner classes and same-file classes
// that goto-definition could resolve correctly.

#[test]
fn test_inner_class_no_false_positive() {
    // Inner class references should NOT produce "cannot find symbol" errors
    let source = r#"
        package com.example;

        public class Outer {
            public void test() {
                Inner inner = new Inner();
                inner.doStuff();
            }

            public static class Inner {
                public void doStuff() {}
            }
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let file_path = workspace_root.path().join("Outer.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO "cannot find symbol" errors for Inner class
    let inner_class_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("Inner"))
        .collect();

    assert!(
        inner_class_errors.is_empty(),
        "Inner class reference should not produce 'cannot find symbol' error. Got: {:?}",
        inner_class_errors
    );
}

#[test]
fn test_same_file_class_reference_no_false_positive() {
    // Reference to another class in the same file should resolve correctly
    let source = r#"
        package com.example;

        public class Container {
            public void test() {
                Helper helper = new Helper();
                helper.help();
            }
        }

        class Helper {
            public void help() {}
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let file_path = workspace_root.path().join("Container.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO "cannot find symbol" errors for Helper class
    let helper_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("Helper"))
        .collect();

    assert!(
        helper_errors.is_empty(),
        "Same-file class reference should not produce 'cannot find symbol' error. Got: {:?}",
        helper_errors
    );
}

#[test]
fn test_same_package_type_resolution_uses_syntax_imports_when_workspace_imports_stale() {
    // This reproduces an LSP-edge case:
    // - The workspace index may have a stale ImportMap (e.g., old package declaration),
    //   while the in-memory buffer (syntax) has the correct package.
    // - Type checking must use the syntax-derived package/imports to avoid false
    //   "cannot find symbol" diagnostics for same-package types.

    let enum_source = r#"
        package com.example;

        public enum MyEnum {
            SOME_ENUM_VALUE
        }
    "#;

    // Stale on-disk (indexed) version: no package declaration.
    // This makes workspace-index imports.current_package = None for Use.java.
    let use_source_indexed = r#"
        public class Use {
            public MyEnum foo() {
                return MyEnum.SOME_ENUM_VALUE;
            }
        }
    "#;

    // Current in-memory version: correct package declaration (same package as MyEnum).
    let use_source_current = r#"
        package com.example;

        public class Use {
            public MyEnum foo() {
                return MyEnum.SOME_ENUM_VALUE;
            }
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let enum_path = workspace_root.path().join("MyEnum.java");
    std::fs::write(&enum_path, enum_source).unwrap();
    index
        .index_file_with_source(&enum_path, enum_source)
        .unwrap();

    // Index the stale version of Use.java under a stable FileId.
    let use_path = workspace_root.path().join("Use.java");
    std::fs::write(&use_path, use_source_indexed).unwrap();
    let use_file_id = FileId::new(4242);
    index
        .index_file_with_source_and_id(&use_path, use_source_indexed, use_file_id)
        .unwrap();

    // Run type checker against the CURRENT buffer version (with correct package decl).
    let root = parse(use_source_current);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], use_file_id, Arc::new(index.clone()));

    let my_enum_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("MyEnum"))
        .collect();

    assert!(
        my_enum_errors.is_empty(),
        "Same-package enum reference should not produce 'cannot find symbol' even if workspace imports are stale. Got: {:?}",
        my_enum_errors
    );
}

#[test]
fn test_nested_inner_class_reference() {
    // Nested inner class references should also resolve
    let source = r#"
        package com.example;

        public class Outer {
            public void test() {
                Inner.Nested nested = new Inner.Nested();
            }

            public static class Inner {
                public static class Nested {
                    public void doStuff() {}
                }
            }
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let file_path = workspace_root.path().join("Outer.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO "cannot find symbol" errors for Inner or Nested classes
    let inner_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("Inner") || e.message().contains("Nested"))
        .collect();

    assert!(
        inner_errors.is_empty(),
        "Nested inner class reference should not produce 'cannot find symbol' error. Got: {:?}",
        inner_errors
    );
}

#[test]
fn test_local_interface_reference() {
    // Local interface references should resolve
    let source = r#"
        package com.example;

        public class Service {
            public void test() {
                Callback callback = () -> {};
            }

            public interface Callback {
                void onComplete();
            }
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let file_path = workspace_root.path().join("Service.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO "cannot find symbol" errors for Callback interface
    let callback_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("Callback"))
        .collect();

    assert!(
        callback_errors.is_empty(),
        "Local interface reference should not produce 'cannot find symbol' error. Got: {:?}",
        callback_errors
    );
}

#[test]
fn test_enum_in_same_file_reference() {
    // Enum defined in same file should resolve
    let source = r#"
        package com.example;

        public class Config {
            public void test() {
                Status status = Status.ACTIVE;
            }

            public enum Status {
                ACTIVE,
                INACTIVE
            }
        }
    "#;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let file_path = workspace_root.path().join("Config.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Should have NO "cannot find symbol" errors for Status enum
    let status_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.message().contains("Status"))
        .collect();

    assert!(
        status_errors.is_empty(),
        "Same-file enum reference should not produce 'cannot find symbol' error. Got: {:?}",
        status_errors
    );
}

#[test]
fn test_enum_constants_in_switch_case_no_false_positive() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    let source = r#"
        package com.example;

        public class TrafficLight {
            enum Color { RED, GREEN, BLUE }

            public void describe(Color c) {
                switch (c) {
                    case RED:
                        break;
                    case GREEN:
                        break;
                    case BLUE:
                        break;
                }
            }
        }
    "#;

    let file_path = workspace_root.path().join("TrafficLight.java");
    std::fs::write(&file_path, source).unwrap();
    index.index_file_with_source(&file_path, source).unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();

    let root = parse(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    // Enum constants RED, GREEN, BLUE in case labels should NOT be flagged
    let undefined_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::UndefinedSymbol { .. }))
        .collect();

    assert!(
        undefined_errors.is_empty(),
        "Enum constants in switch case labels should not produce 'cannot find symbol' errors. Got: {:?}",
        undefined_errors
    );
}

#[test]
fn test_static_import_explicit_no_false_positive() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    // Index Constants class
    let constants_path = workspace_root.path().join("com/example/Constants.java");
    std::fs::create_dir_all(constants_path.parent().unwrap()).unwrap();
    let constants_source = r#"
        package com.example;

        public class Constants {
            public static final String FOO = "foo";
            public static final int BAR = 42;
        }
    "#;
    std::fs::write(&constants_path, constants_source).unwrap();
    index
        .index_file_with_source(&constants_path, constants_source)
        .unwrap();

    // Index Main class that uses explicit static import
    let main_path = workspace_root.path().join("com/example/Main.java");
    let main_source = r#"
        package com.example;

        import static com.example.Constants.FOO;

        public class Main {
            public void test() {
                String s = FOO;
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let undefined_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::UndefinedSymbol { .. }))
        .collect();

    assert!(
        undefined_errors.is_empty(),
        "Statically imported symbol FOO should not produce 'cannot find symbol' error. Got: {:?}",
        undefined_errors
    );
}

#[test]
fn test_static_wildcard_import_no_false_positive() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    let workspace_root = tempfile::tempdir().unwrap();
    let index = WorkspaceIndex::new(workspace_root.path().to_path_buf());

    // Index Constants class
    let constants_path = workspace_root.path().join("com/example/Constants.java");
    std::fs::create_dir_all(constants_path.parent().unwrap()).unwrap();
    let constants_source = r#"
        package com.example;

        public class Constants {
            public static final String FOO = "foo";
            public static final int BAR = 42;
        }
    "#;
    std::fs::write(&constants_path, constants_source).unwrap();
    index
        .index_file_with_source(&constants_path, constants_source)
        .unwrap();

    // Index Main class that uses wildcard static import
    let main_path = workspace_root.path().join("com/example/Main.java");
    let main_source = r#"
        package com.example;

        import static com.example.Constants.*;

        public class Main {
            public void test() {
                String s = FOO;
                int i = BAR;
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let undefined_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::UndefinedSymbol { .. }))
        .collect();

    assert!(
        undefined_errors.is_empty(),
        "Wildcard statically imported symbols should not produce 'cannot find symbol' errors. Got: {:?}",
        undefined_errors
    );
}

// =============================================================================
// parse_class_type generic handling tests
// =============================================================================

#[test]
fn test_parse_class_type_simple() {
    let ct = TypeChecker::parse_class_type("String");
    assert_eq!(ct.name, "String");
    assert_eq!(ct.package.as_deref(), Some("java.lang"));
    assert!(ct.type_arguments.is_empty());
}

#[test]
fn test_parse_class_type_fqn() {
    let ct = TypeChecker::parse_class_type("java.util.List");
    assert_eq!(ct.name, "List");
    assert_eq!(ct.package.as_deref(), Some("java.util"));
    assert!(ct.type_arguments.is_empty());
}

#[test]
fn test_parse_class_type_with_generics() {
    let ct = TypeChecker::parse_class_type("Map<String, Integer>");
    assert_eq!(ct.name, "Map");
    assert!(ct.package.is_none());
    assert_eq!(ct.type_arguments.len(), 2);

    // First arg: String → java.lang.String
    match &ct.type_arguments[0] {
        Type::Class(inner) => {
            assert_eq!(inner.name, "String");
            assert_eq!(inner.package.as_deref(), Some("java.lang"));
        }
        other => panic!("Expected Class type for first arg, got: {:?}", other),
    }

    // Second arg: Integer → java.lang.Integer
    match &ct.type_arguments[1] {
        Type::Class(inner) => {
            assert_eq!(inner.name, "Integer");
            assert_eq!(inner.package.as_deref(), Some("java.lang"));
        }
        other => panic!("Expected Class type for second arg, got: {:?}", other),
    }
}

#[test]
fn test_parse_class_type_fqn_with_generics() {
    let ct = TypeChecker::parse_class_type("java.util.Map<java.lang.String, com.foo.Foo>");
    assert_eq!(ct.name, "Map");
    assert_eq!(ct.package.as_deref(), Some("java.util"));
    assert_eq!(ct.type_arguments.len(), 2);

    match &ct.type_arguments[0] {
        Type::Class(inner) => {
            assert_eq!(inner.name, "String");
            assert_eq!(inner.package.as_deref(), Some("java.lang"));
        }
        other => panic!("Expected Class for first arg, got: {:?}", other),
    }

    match &ct.type_arguments[1] {
        Type::Class(inner) => {
            assert_eq!(inner.name, "Foo");
            assert_eq!(inner.package.as_deref(), Some("com.foo"));
        }
        other => panic!("Expected Class for second arg, got: {:?}", other),
    }
}

#[test]
fn test_parse_class_type_nested_generics() {
    let ct = TypeChecker::parse_class_type("Map<String, List<Integer>>");
    assert_eq!(ct.name, "Map");
    assert_eq!(ct.type_arguments.len(), 2);

    // Second arg should be List<Integer>
    match &ct.type_arguments[1] {
        Type::Class(inner) => {
            assert_eq!(inner.name, "List");
            assert_eq!(inner.type_arguments.len(), 1);
            match &inner.type_arguments[0] {
                Type::Class(innermost) => {
                    assert_eq!(innermost.name, "Integer");
                    assert_eq!(innermost.package.as_deref(), Some("java.lang"));
                }
                other => panic!("Expected Class for nested arg, got: {:?}", other),
            }
        }
        other => panic!("Expected Class for second arg, got: {:?}", other),
    }
}

#[test]
fn test_parse_class_type_diamond() {
    let ct = TypeChecker::parse_class_type("ArrayList<>");
    assert_eq!(ct.name, "ArrayList");
    assert!(ct.has_diamond());
    assert!(ct.type_arguments.is_empty());
}

#[test]
fn test_parse_class_type_wildcard() {
    let ct = TypeChecker::parse_class_type("List<?>");
    assert_eq!(ct.name, "List");
    assert_eq!(ct.type_arguments.len(), 1);
    assert!(matches!(
        ct.type_arguments[0],
        Type::Wildcard {
            upper_bound: None,
            lower_bound: None
        }
    ));
}

#[test]
fn test_parse_class_type_wildcard_extends() {
    let ct = TypeChecker::parse_class_type("List<? extends Number>");
    assert_eq!(ct.name, "List");
    assert_eq!(ct.type_arguments.len(), 1);
    match &ct.type_arguments[0] {
        Type::Wildcard {
            upper_bound: Some(bound),
            lower_bound: None,
        } => match bound.as_ref() {
            Type::Class(inner) => {
                assert_eq!(inner.name, "Number");
                assert_eq!(inner.package.as_deref(), Some("java.lang"));
            }
            other => panic!("Expected Class bound, got: {:?}", other),
        },
        other => panic!("Expected Wildcard with upper bound, got: {:?}", other),
    }
}

#[test]
fn workspace_fallback_does_not_merge_qualified_simple_names() {
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_java_lang_stubs(&index, temp.path());
    index
        .index_file_with_source(&temp.path().join("a/User.java"), "package a; class User {}")
        .unwrap();
    index
        .index_file_with_source(&temp.path().join("b/User.java"), "package b; class User {}")
        .unwrap();
    let mut checker = TypeChecker::new();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: FileId(0),
        imports: ImportMap::default(),
        workspace_index: Arc::new(index),
    });
    let declared = Type::Class(ClassType::new("User".into(), Some("a".into())));
    let actual = Type::Class(ClassType::new("User".into(), Some("b".into())));

    assert!(checker.is_definitely_not_assignable(&declared, &actual, None));
}

#[test]
fn assignability_uses_current_imports_not_stale_file_imports() {
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_java_lang_stubs(&index, temp.path());
    index
        .index_file_with_source(&temp.path().join("a/User.java"), "package a; class User {}")
        .unwrap();
    index
        .index_file_with_source(&temp.path().join("b/User.java"), "package b; class User {}")
        .unwrap();
    let use_path = temp.path().join("app/Use.java");
    index
        .index_file_with_source(
            &use_path,
            "package app; import a.User; class Use { User value; }",
        )
        .unwrap();

    let mut current_imports = ImportMap::new();
    current_imports.current_package = Some("app".into());
    current_imports.add_import("User".into(), "b.User".into());
    let mut checker = TypeChecker::new();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: index.get_file_id(&use_path).unwrap(),
        imports: current_imports,
        workspace_index: Arc::new(index),
    });

    let declared = Type::Class(ClassType::new("User".into(), None));
    let current = Type::Class(ClassType::new("User".into(), Some("b".into())));
    let stale = Type::Class(ClassType::new("User".into(), Some("a".into())));
    assert!(!checker.is_definitely_not_assignable(&declared, &current, None));
    assert!(checker.is_definitely_not_assignable(&declared, &stale, None));
}

#[test]
fn imported_simple_receiver_projects_inherited_field_type() {
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("api/Types.java"),
            "package api; class Base<T> { T value; } public class Holder<T> extends Base<T> {}",
        )
        .unwrap();
    let use_path = temp.path().join("app/Use.java");
    index
        .index_file_with_source(
            &use_path,
            "package app; import api.Holder; class Use { Holder<String> holder; }",
        )
        .unwrap();
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());
    imports.add_import("Holder".into(), "api.Holder".into());
    let mut checker = TypeChecker::new();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: index.get_file_id(&use_path).unwrap(),
        imports,
        workspace_index: Arc::new(index),
    });
    let receiver = Type::Class(ClassType::with_type_args(
        "Holder".into(),
        None,
        vec![Type::Class(ClassType::string())],
    ));
    let ctx = checker.workspace_context.as_ref().unwrap();

    assert_eq!(
        checker.resolve_field_type_for_receiver(&receiver, "value", ctx),
        Some(Type::Class(ClassType::string()))
    );
}

#[test]
fn only_contextually_resolved_java_lang_object_is_universal() {
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_java_lang_stubs(&index, temp.path());
    index
        .index_file_with_source(
            &temp.path().join("shadow/Object.java"),
            "package shadow; class Object {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("shadow/Value.java"),
            "package shadow; class Value {}",
        )
        .unwrap();

    let mut shadow_imports = ImportMap::new();
    shadow_imports.current_package = Some("shadow".into());
    let mut checker = TypeChecker::new();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: FileId(0),
        imports: shadow_imports,
        workspace_index: Arc::new(index),
    });

    let simple_object = Type::Class(ClassType::new("Object".into(), None));
    let java_object = Type::Class(ClassType::object());
    let value = Type::Class(ClassType::new("Value".into(), Some("shadow".into())));
    // `Object` here is `shadow.Object`, which `Value` does not extend; only
    // the contextually resolved `java.lang.Object` accepts everything.
    assert!(checker.is_definitely_not_assignable(&simple_object, &value, None));
    assert!(!checker.is_definitely_not_assignable(&java_object, &value, None));

    let empty = TempDir::new().unwrap();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: FileId(0),
        imports: ImportMap::new(),
        workspace_index: Arc::new(WorkspaceIndex::new(empty.path().to_path_buf())),
    });
    assert!(
        !checker.is_definitely_not_assignable(&simple_object, &value, None),
        "an unresolved simple Object is never grounds for an error"
    );
}

#[test]
fn ambiguous_file_local_inner_types_are_not_merged_by_simple_name() {
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_java_lang_stubs(&index, temp.path());
    let owners_path = temp.path().join("p/Owners.java");
    index
        .index_file_with_source(
            &owners_path,
            concat!(
                "package p; ",
                "class Left { class Inner {} } ",
                "class Right { class Inner {} }"
            ),
        )
        .unwrap();

    let mut imports = ImportMap::new();
    imports.current_package = Some("p".into());
    let mut checker = TypeChecker::new();
    checker.workspace_context = Some(WorkspaceContext {
        file_id: index.get_file_id(&owners_path).unwrap(),
        imports,
        workspace_index: Arc::new(index),
    });

    let unresolved_inner = Type::Class(ClassType::new("Inner".into(), None));
    let left_inner = Type::Class(ClassType::new("Inner".into(), Some("p.Left".into())));
    // The ambiguous spelling `Inner` must not be silently merged with Left.Inner,
    // and never be grounds for an error either.
    assert!(!checker.is_definitely_not_assignable(&unresolved_inner, &left_inner, None));
}

// Interfaces implemented by superclasses must be visible to the
// subtype check. `class Base implements Greeter {} class Derived extends
// Base {}` — `Greeter g = new Derived();` is valid Java.
#[test]
fn test_interface_of_superclass_accepted_in_declaration() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    for (file, source) in [
        ("Greeter.java", "public interface Greeter { void greet(); }"),
        (
            "Base.java",
            "public class Base implements Greeter { public void greet() {} }",
        ),
        ("Derived.java", "public class Derived extends Base {}"),
    ] {
        let path = workspace_root.join(file);
        std::fs::write(&path, source).unwrap();
        index.index_file_with_source(&path, source).unwrap();
    }

    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                Greeter g = new Derived();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let declaration_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::DeclarationTypeMismatch { .. }))
        .collect();

    assert_eq!(
        declaration_errors.len(),
        0,
        "Greeter g = new Derived() is valid via Base's implements clause, got: {:?}",
        declaration_errors
    );
}

// Negative control: an unrelated class must still be rejected, so
// the recursive hierarchy walk is not silencing real mismatches.
#[test]
fn test_unrelated_class_still_rejected_in_declaration() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    for (file, source) in [
        ("Greeter.java", "public interface Greeter { void greet(); }"),
        ("Unrelated.java", "public class Unrelated {}"),
    ] {
        let path = workspace_root.join(file);
        std::fs::write(&path, source).unwrap();
        index.index_file_with_source(&path, source).unwrap();
    }

    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                Greeter g = new Unrelated();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let declaration_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::DeclarationTypeMismatch { .. }))
        .collect();

    assert_eq!(
        declaration_errors.len(),
        1,
        "Greeter g = new Unrelated() must still be a DeclarationTypeMismatch, got: {:?}",
        declaration_errors
    );
}

// Interface inherited through a two-level superclass chain.
#[test]
fn test_interface_of_grandparent_class_accepted() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();
    let index = WorkspaceIndex::new(workspace_root.clone());

    for (file, source) in [
        ("Greeter.java", "public interface Greeter { void greet(); }"),
        (
            "Base.java",
            "public class Base implements Greeter { public void greet() {} }",
        ),
        ("Middle.java", "public class Middle extends Base {}"),
        ("Derived.java", "public class Derived extends Middle {}"),
    ] {
        let path = workspace_root.join(file);
        std::fs::write(&path, source).unwrap();
        index.index_file_with_source(&path, source).unwrap();
    }

    let main_path = workspace_root.join("Main.java");
    let main_source = r#"
        public class Main {
            public void test() {
                Greeter g = new Derived();
            }
        }
    "#;
    std::fs::write(&main_path, main_source).unwrap();
    index
        .index_file_with_source(&main_path, main_source)
        .unwrap();
    let file_id = index.get_file_id(&main_path).unwrap();

    let root = parse(main_source);
    let mut checker = TypeChecker::new();
    let errors = checker.check_with_context(&root, &[], file_id, Arc::new(index.clone()));

    let declaration_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e.kind, SemanticErrorKind::DeclarationTypeMismatch { .. }))
        .collect();

    assert_eq!(
        declaration_errors.len(),
        0,
        "Greeter is inherited through Derived -> Middle -> Base, got: {:?}",
        declaration_errors
    );
}

#[test]
fn logger_overloads_do_not_invent_marker_mismatch_for_caught_exception() {
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    let directory = tempfile::tempdir().unwrap();
    let index = Arc::new(WorkspaceIndex::new(directory.path().to_path_buf()));
    for (name, source) in [
        ("Logger.java", "package logging; public interface Logger { void error(Marker marker, String message); void error(String message, Object value); void error(String message, Throwable failure); void error(String message, Object... values); }"),
        ("Failure.java", "package external; public class Failure extends Exception {}"),
    ] {
        index.index_file_with_source(&directory.path().join(name), source).unwrap();
    }
    let source = r#"package app; import logging.Logger; import external.Failure;
        class Use { void fail() throws Failure {} void test(Logger logger) { try { fail(); } catch (Failure e) { logger.error("Example operation failed", e); } } }"#;
    let path = directory.path().join("Use.java");
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let root = parse(source);
    let semantics = crate::semantic_walker::compute_file_semantics(
        &root,
        file_id,
        &crate::symbol::SymbolTable::new(),
        &index,
    );
    let mut checker = TypeChecker::new();
    checker.set_file_semantics(Arc::new(semantics));
    let errors = checker.check_with_context(&root, &[], file_id, index);
    assert!(errors.is_empty(), "{errors:?}");
}
