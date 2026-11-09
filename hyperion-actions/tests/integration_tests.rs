// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for hyperion-actions
//!
//! These tests verify the functionality of code actions, quick fixes,
//! and code generation features.

use hyperion_actions::{CodeGenerator, ImportOrganizer};

#[test]
fn test_code_generator_getter() {
    let getter = CodeGenerator::generate_getter("name", "String");
    assert!(getter.contains("public String getName()"));
    assert!(getter.contains("return this.name;"));
}

#[test]
fn test_code_generator_setter() {
    let setter = CodeGenerator::generate_setter("age", "int");
    assert!(setter.contains("public void setAge(int age)"));
    assert!(setter.contains("this.age = age;"));
}

#[test]
fn test_code_generator_to_string_empty() {
    let to_string = CodeGenerator::generate_to_string("Person", &[]);
    assert!(to_string.contains("@Override"));
    assert!(to_string.contains("Person{}"));
}

#[test]
fn test_code_generator_to_string_with_fields() {
    let fields = vec![("name", "String"), ("age", "int")];
    let to_string = CodeGenerator::generate_to_string("Person", &fields);

    assert!(to_string.contains("@Override"));
    assert!(to_string.contains("public String toString()"));
    assert!(to_string.contains("Person"));
    assert!(to_string.contains("name"));
    assert!(to_string.contains("age"));
}

#[test]
fn test_code_generator_equals_method() {
    let fields = vec![("id", "int"), ("name", "String")];
    let equals = CodeGenerator::generate_equals("Person", &fields);

    assert!(equals.contains("@Override"));
    assert!(equals.contains("public boolean equals(Object obj)"));
    assert!(equals.contains("if (this == obj) return true;"));
    assert!(equals.contains("Person other"));
}

#[test]
fn test_code_generator_hashcode_method() {
    let fields = vec![("id", "int"), ("name", "String")];
    let hash_code = CodeGenerator::generate_hash_code(&fields);

    assert!(hash_code.contains("@Override"));
    assert!(hash_code.contains("public int hashCode()"));
    assert!(hash_code.contains("java.util.Objects.hash"));
}

#[test]
fn test_code_generator_constructor() {
    let fields = vec![("name", "String"), ("age", "int")];
    let constructor = CodeGenerator::generate_constructor("Person", &fields);

    assert!(constructor.contains("public Person(String name, int age)"));
    assert!(constructor.contains("this.name = name;"));
    assert!(constructor.contains("this.age = age;"));
}

#[test]
fn test_code_generator_default_constructor() {
    let constructor = CodeGenerator::generate_default_constructor("Person");
    assert!(constructor.contains("public Person()"));
}

#[test]
fn test_code_generator_builder() {
    let fields = vec![("name", "String"), ("age", "int")];
    let builder = CodeGenerator::generate_builder("Person", &fields);

    assert!(builder.contains("public static class Builder"));
    assert!(builder.contains("private String name;"));
    assert!(builder.contains("private int age;"));
    assert!(builder.contains("public Builder name(String name)"));
    assert!(builder.contains("public Builder age(int age)"));
    assert!(builder.contains("public Person build()"));
}

#[test]
fn test_import_organizer_remove_duplicates() {
    let imports = vec![
        "import java.util.List;".into(),
        "import java.util.Map;".into(),
        "import java.util.List;".into(),
    ];

    let result = ImportOrganizer::remove_duplicates(imports);
    assert_eq!(result.len(), 2);
}

#[test]
fn test_import_organizer_sort() {
    let imports = vec![
        "import java.util.Map;".into(),
        "import java.util.List;".into(),
        "import java.io.File;".into(),
    ];

    let result = ImportOrganizer::sort_imports(imports);
    assert_eq!(result[0], "import java.io.File;");
    assert_eq!(result[1], "import java.util.List;");
    assert_eq!(result[2], "import java.util.Map;");
}

#[test]
fn test_accessor_methods_generation() {
    let accessors = CodeGenerator::generate_accessor_methods("value", "double");
    assert!(accessors.contains("getValue"));
    assert!(accessors.contains("setValue"));
    assert!(accessors.contains("public double getValue()"));
    assert!(accessors.contains("public void setValue(double value)"));
}
