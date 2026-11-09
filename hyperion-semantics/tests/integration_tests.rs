// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for hyperion-semantics
//!
//! These tests verify that all components work together correctly:
//! - Database + Parser + Symbol extraction
//! - Resolver + Symbol table + Scope management
//! - End-to-end semantic analysis workflows

use hyperion_semantics::db::{Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_semantics::resolver::{ResolverBuilder, SymbolResolver};
use hyperion_semantics::symbol::*;
use hyperion_workspace::Language;
use std::sync::Arc;

// ========== End-to-End Workflow Tests ==========

#[test]
fn test_complete_semantic_analysis_workflow() {
    let mut db = Database::new();
    let file = FileId::new(1);

    // Step 1: Set source code
    let source = r"
public class Calculator {
    private int result;

    public Calculator() {
        this.result = 0;
    }

    public int add(int a, int b) {
        return a + b;
    }

    public int getResult() {
        return result;
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    // Step 2: Parse the file
    let parse_tree = db.parse_file(file);
    assert!(
        parse_tree.errors.is_empty(),
        "Valid class should parse without syntax errors: {:?}",
        parse_tree.errors
    );

    // Step 3: Extract symbols
    let symbols = db.file_symbols(file);
    assert!(!symbols.symbols.is_empty()); // At least the Calculator class

    // Step 4: Verify symbol extraction
    let class_symbol = symbols
        .symbols
        .iter()
        .find(|s| s.name.as_ref() == "Calculator");
    assert!(class_symbol.is_some());
}

#[test]
fn test_multiple_files_semantic_analysis() {
    let mut db = Database::new();

    let files = vec![
        (
            FileId::new(1),
            "public class Person { private String name; }",
        ),
        (FileId::new(2), "public interface Drawable { void draw(); }"),
        (FileId::new(3), "public class Circle implements Drawable {}"),
    ];

    // Add all files
    for (file_id, source) in &files {
        db.set_file_text(*file_id, Arc::new((*source).to_string()));
        db.set_file_language(*file_id, Language::Java);
    }

    // Verify each file is parsed and has symbols
    for (file_id, _) in &files {
        let symbols = db.file_symbols(*file_id);
        assert!(!symbols.symbols.is_empty());
    }

    // Verify incremental updates work across files
    db.set_file_text(files[0].0, Arc::new("public class Person { }".to_string()));
    db.set_file_language(files[0].0, Language::Java);
    let updated_symbols = db.file_symbols(files[0].0);
    assert!(!updated_symbols.symbols.is_empty());

    // Other files should still be cached
    let file2_symbols = db.file_symbols(files[1].0);
    assert!(!file2_symbols.symbols.is_empty());
}

#[test]
fn test_symbol_resolution_with_database() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Example {
    int field;

    void method(int param) {
        int local = field + param;
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    // Parse and extract symbols
    let parse_tree = db.parse_file(file);
    let syntax_root = parse_tree.syntax();

    // Build resolver from syntax
    let builder = ResolverBuilder::new(file);
    let resolver = builder.build_from_syntax(&syntax_root);

    let all_symbols = resolver.symbol_table().all_symbols();
    assert!(
        !all_symbols.is_empty(),
        "Resolver should register symbols from syntax tree"
    );
    assert!(
        all_symbols.iter().any(|s| s.name.as_str() == "Example"),
        "Expected class symbol to be registered"
    );
    assert!(
        all_symbols.iter().any(|s| s.name.as_str() == "field"),
        "Expected field symbol to be registered"
    );
    assert!(
        all_symbols.iter().any(|s| s.name.as_str() == "method"),
        "Expected method symbol to be registered"
    );
}

#[test]
fn test_parse_and_symbols_integration() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Math {
    int add(int a, int b) {
        return a + b;
    }

    double multiply(double x, double y) {
        return x * y;
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    let _parse_tree = db.parse_file(file);
    let symbols = db.file_symbols(file);

    // Verify symbols were extracted
    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "Math"));
    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "add"));
    assert!(symbols
        .symbols
        .iter()
        .any(|s| s.name.as_ref() == "multiply"));
}

// ========== Complex Java Feature Tests ==========

#[test]
fn test_nested_classes() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
public class Outer {
    class Inner {
        class DeepInner {
        }
    }

    static class StaticNested {
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    // Should find at least the outer class
    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "Outer"));
}

#[test]
fn test_generic_classes() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
public class Box<T> {
    private T value;

    public T getValue() {
        return value;
    }

    public void setValue(T value) {
        this.value = value;
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "Box"));
}

#[test]
fn test_interfaces_and_implementations() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
interface Comparable<T> {
    int compareTo(T other);
}

class Person implements Comparable<Person> {
    public int compareTo(Person other) {
        return 0;
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    let comparable_found = symbols
        .symbols
        .iter()
        .any(|s| s.name.as_ref() == "Comparable");
    let person_found = symbols.symbols.iter().any(|s| s.name.as_ref() == "Person");

    assert!(comparable_found, "Expected interface symbol Comparable");
    assert!(person_found, "Expected implementing class symbol Person");
}

#[test]
fn test_enum_types() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
public enum Color {
    RED, GREEN, BLUE;

    public String toString() {
        return name();
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "Color"));
}

#[test]
fn test_annotations() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r#"
@interface MyAnnotation {
    String value();
    int count() default 1;
}

@MyAnnotation(value = "test", count = 5)
class AnnotatedClass {
}
"#;

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    let annotation = symbols
        .symbols
        .iter()
        .find(|s| s.name.as_ref() == "MyAnnotation")
        .expect("annotation declaration should be indexed");
    assert_eq!(
        annotation.kind,
        hyperion_semantics::db::SymbolKind::Interface
    );
    assert!(symbols
        .symbols
        .iter()
        .any(|s| s.name.as_ref() == "AnnotatedClass"));
}

#[test]
fn test_record_symbol() {
    let mut db = Database::new();
    let file = FileId::new(1);

    db.set_file_text(file, Arc::new("record Point(int x, int y) {}".to_string()));
    db.set_file_language(file, Language::Java);

    let symbols = db.file_symbols(file);
    let point = symbols
        .symbols
        .iter()
        .find(|symbol| symbol.name.as_ref() == "Point")
        .expect("record declaration should be indexed");
    assert_eq!(point.kind, hyperion_semantics::db::SymbolKind::Class);
}

// ========== Error Recovery Tests ==========

#[test]
fn test_syntax_error_recovery() {
    let mut db = Database::new();
    let file = FileId::new(1);

    // Intentionally malformed Java
    let source = "class { missing name }";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    let parse_tree = db.parse_file(file);
    assert!(
        !parse_tree.errors.is_empty(),
        "Malformed source should produce parse errors"
    );
    let _symbols = db.file_symbols(file);
}

#[test]
fn test_incomplete_code() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
class Incomplete {
    public void method(
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    let parse_tree = db.parse_file(file);
    assert!(
        !parse_tree.errors.is_empty(),
        "Incomplete source should produce parse errors"
    );
    let _symbols = db.file_symbols(file);
}

#[test]
fn test_empty_and_whitespace() {
    let mut db = Database::new();

    let test_cases = [
        ("", 0),
        ("   ", 0),
        ("\n\n\n", 0),
        ("\t\t\t", 0),
        ("// just a comment", 0),
        ("/* block comment */", 0),
    ];

    for (i, (source, expected_symbols)) in test_cases.iter().enumerate() {
        let file = FileId::new(i as u32);
        db.set_file_text(file, Arc::new((*source).to_string()));
        db.set_file_language(file, Language::Java);

        let symbols = db.file_symbols(file);
        assert_eq!(symbols.symbols.len(), *expected_symbols);
    }
}

// ========== Performance and Scalability Tests ==========

#[test]
fn test_many_files_performance() {
    let mut db = Database::new();
    let num_files = 100;

    // Create many files
    for i in 0..num_files {
        let file = FileId::new(i);
        let source = format!("public class Class{i} {{ private int field{i}; }}");
        db.set_file_text(file, Arc::new(source));
        db.set_file_language(file, Language::Java);
    }

    // Query all files
    for i in 0..num_files {
        let file = FileId::new(i);
        let symbols = db.file_symbols(file);
        assert!(!symbols.symbols.is_empty());
    }

    // Modify one file and verify others are cached
    let changed_file = FileId::new(50);
    let original_symbols = db.file_symbols(changed_file);

    db.set_file_text(changed_file, Arc::new("class Modified {}".to_string()));
    db.set_file_language(changed_file, Language::Java);
    let new_symbols = db.file_symbols(changed_file);

    // Should be different
    assert!(!Arc::ptr_eq(&original_symbols, &new_symbols));

    // Others should be cached
    let other_file = FileId::new(49);
    let other_symbols_1 = db.file_symbols(other_file);
    let other_symbols_2 = db.file_symbols(other_file);
    assert!(Arc::ptr_eq(&other_symbols_1, &other_symbols_2));
}

#[test]
fn test_large_file() {
    let mut db = Database::new();
    let file = FileId::new(1);

    // Generate a large Java file
    let mut source = String::from("public class LargeClass {\n");

    for i in 0..1000 {
        source.push_str(&format!("    private int field{i};\n"));
    }

    for i in 0..1000 {
        source.push_str(&format!(
            "    public int getField{i}() {{ return field{i}; }}\n"
        ));
    }

    source.push_str("}\n");

    db.set_file_text(file, Arc::new(source));
    db.set_file_language(file, Language::Java);

    // Should handle large file
    let _parse_tree = db.parse_file(file);
    let symbols = db.file_symbols(file);

    assert!(!symbols.symbols.is_empty());
}

// ========== Larger Scenarios ==========

#[test]
fn test_java_collections_style_code() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
import java.util.*;

public class MyList<E> implements List<E> {
    private Object[] elements;
    private int size;

    public MyList() {
        elements = new Object[10];
        size = 0;
    }

    public boolean add(E element) {
        elements[size++] = element;
        return true;
    }

    public E get(int index) {
        return (E) elements[index];
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "MyList"));
}

#[test]
fn test_builder_pattern() {
    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
public class Person {
    private String name;
    private int age;

    private Person(Builder builder) {
        this.name = builder.name;
        this.age = builder.age;
    }

    public static class Builder {
        private String name;
        private int age;

        public Builder name(String name) {
            this.name = name;
            return this;
        }

        public Builder age(int age) {
            this.age = age;
            return this;
        }

        public Person build() {
            return new Person(this);
        }
    }
}
";

    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);
    let symbols = db.file_symbols(file);

    assert!(symbols.symbols.iter().any(|s| s.name.as_ref() == "Person"));
}

// ========== Type System Integration Tests ==========

#[test]
fn test_type_hierarchy() {
    let table = SymbolTable::new();
    let _root = table.root_scope();

    // Create type hierarchy: Object -> Number -> Integer
    let object_type = Type::Class(ClassType::object());
    let number_type = Type::Class(ClassType::new(
        "Number".into(),
        Some("java.lang".to_string()),
    ));
    let integer_type = Type::Class(ClassType::new(
        "Integer".into(),
        Some("java.lang".to_string()),
    ));

    // Object is assignable from anything
    assert!(object_type.is_assignable_from(&number_type));
    assert!(object_type.is_assignable_from(&integer_type));

    // Same types are assignable
    assert!(integer_type.is_assignable_from(&integer_type));
}

#[test]
fn test_generic_type_erasure() {
    // List<String> erases to List
    let list_of_string = Type::Class(ClassType::with_type_args(
        "List".into(),
        Some("java.util".to_string()),
        vec![Type::Class(ClassType::string())],
    ));

    let erased = list_of_string.erasure();

    match erased {
        Type::Class(c) => {
            assert_eq!(&c.name, "List");
            assert_eq!(c.package.as_deref(), Some("java.util"));
            assert!(c.type_arguments.is_empty());
        }
        _ => panic!("Expected class type"),
    }
}

#[test]
fn test_array_covariance() {
    let object = Type::Class(ClassType::object());
    let string_array = Type::Array {
        element_type: Box::new(Type::Class(ClassType::string())),
        dimensions: 1,
    };

    // Object[] is assignable from String[] (covariance)
    // Actually, Object (not Object[]) is assignable from any array
    assert!(object.is_assignable_from(&string_array));
}

// ========== Resolver Integration Tests ==========

#[test]
fn test_resolver_with_complex_scoping() {
    let mut resolver = SymbolResolver::new();

    // Build complex scope hierarchy
    resolver.enter_scope(ScopeKind::Package);
    let _class1_scope = resolver.enter_scope(ScopeKind::Class);

    // Add field to class1
    let field = Symbol::new(
        "sharedName".into(),
        SymbolKind::Field {
            field_type: Type::Primitive(PrimitiveType::Int),
        },
        Type::Primitive(PrimitiveType::Int),
        Visibility::Private,
        SourceLocation::new(FileId(0), 0, 10),
    );
    resolver.define_symbol(field);

    resolver.exit_scope();
    resolver.exit_scope();

    // Second class (sibling to first)
    resolver.enter_scope(ScopeKind::Package);
    resolver.enter_scope(ScopeKind::Class);

    // Add field with same name to class2
    let field2 = Symbol::new(
        "sharedName".into(),
        SymbolKind::Field {
            field_type: Type::Class(ClassType::string()),
        },
        Type::Class(ClassType::string()),
        Visibility::Private,
        SourceLocation::new(FileId(0), 20, 30),
    );
    resolver.define_symbol(field2);

    // Should resolve to class2's field (different type)
    let resolved = resolver.resolve("sharedName");
    assert!(resolved.is_some());
    assert!(matches!(resolved.unwrap().ty, Type::Class(_)));
}

#[test]
fn test_full_semantic_pipeline() {
    // This test exercises the complete pipeline:
    // Source -> Parse -> Symbols -> Resolver

    let mut db = Database::new();
    let file = FileId::new(1);

    let source = r"
public class Calculator {
    public int calculate(int x, int y) {
        int result = x + y;
        return result;
    }
}
";

    // Step 1: Set source
    db.set_file_text(file, Arc::new(source.to_string()));
    db.set_file_language(file, Language::Java);

    // Step 2: Parse
    let parse_tree = db.parse_file(file);
    let syntax_root = parse_tree.syntax();

    // Step 3: Extract symbols
    let symbols = db.file_symbols(file);
    assert!(!symbols.symbols.is_empty());

    // Step 4: Build resolver
    let builder = ResolverBuilder::new(file);
    let _resolver = builder.build_from_syntax(&syntax_root);

    // Full pipeline executed successfully
}
