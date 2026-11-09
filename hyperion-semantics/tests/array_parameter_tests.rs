// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Array Parameter Type Preservation Tests
//!
//! Regression: `User[] users` must resolve to Array(User), not User.
//!
//! This suite tests the resolver's ability to parse array types from parameter
//! declarations and preserve the array wrapper with correct dimensions.

use hyperion_parser::{Lexer, Parser, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::{PrimitiveType, SymbolKind, Type};

/// Helper to parse Java source, build resolver, and return symbol table
fn parse_and_resolve(source: &str) -> hyperion_semantics::symbol::SymbolTable {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    let builder = ResolverBuilder::new(FileId::new(1));
    let resolver = builder.build_from_syntax(&root);

    resolver.symbol_table().clone()
}

/// Test that single-dimensional class array parameters create Type::Array
#[test]
fn test_single_dimension_class_array_parameter() {
    let source = r#"
class Service {
    void process(User[] users) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'users' parameter
    let mut found_users = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("users") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_users = true;

                    // Verify it's a parameter
                    assert!(
                        matches!(symbol.kind, SymbolKind::Parameter { .. }),
                        "users should be a Parameter, got {:?}",
                        symbol.kind
                    );

                    // CRITICAL: Verify type_info is Array, not just User
                    match &symbol.type_info {
                        Type::Array {
                            element_type,
                            dimensions,
                        } => {
                            assert_eq!(
                                *dimensions, 1u8,
                                "Array should be single-dimensional (User[])"
                            );

                            match element_type.as_ref() {
                                Type::Class(ct) => {
                                    assert_eq!(
                                        ct.name, "User",
                                        "Element type should be User class"
                                    );
                                }
                                other => panic!(
                                    "Expected element type to be Class(User), got {:?}",
                                    other
                                ),
                            }
                        }
                        other => panic!(
                            "Parameter 'users' type_info should be Array{{User, 1}}, got {:?}",
                            other
                        ),
                    }

                    break;
                }
            }
        }
    }

    assert!(found_users, "Should find 'users' parameter in symbol table");
}

/// Test that primitive array parameters create Type::Array with primitive element
#[test]
fn test_primitive_array_parameter() {
    let source = r#"
class Calculator {
    int sum(int[] numbers) {
        return 0;
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'numbers' parameter
    let mut found_numbers = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("numbers") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_numbers = true;

                    match &symbol.type_info {
                        Type::Array {
                            element_type,
                            dimensions,
                        } => {
                            assert_eq!(*dimensions, 1u8);
                            match element_type.as_ref() {
                                Type::Primitive(PrimitiveType::Int) => {
                                    // Correct!
                                }
                                other => panic!("Expected int element type, got {:?}", other),
                            }
                        }
                        other => panic!("Expected Array{{int, 1}}, got {:?}", other),
                    }

                    break;
                }
            }
        }
    }

    assert!(
        found_numbers,
        "Should find 'numbers' parameter in symbol table"
    );
}

/// Test that multi-dimensional arrays preserve correct dimension count
#[test]
fn test_multidimensional_array_parameter() {
    let source = r#"
class Matrix {
    void process(int[][] matrix) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'matrix' parameter
    let mut found_matrix = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("matrix") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_matrix = true;

                    match &symbol.type_info {
                        Type::Array {
                            element_type,
                            dimensions,
                        } => {
                            assert_eq!(
                                *dimensions, 2u8,
                                "matrix should be 2-dimensional (int[][])"
                            );
                            match element_type.as_ref() {
                                Type::Primitive(PrimitiveType::Int) => {} // Correct
                                other => panic!("Expected int element type, got {:?}", other),
                            }
                        }
                        other => panic!("Expected Array{{int, 2}}, got {:?}", other),
                    }

                    break;
                }
            }
        }
    }

    assert!(
        found_matrix,
        "Should find 'matrix' parameter in symbol table"
    );
}

/// Test varargs parameters (which are array types)
#[test]
fn test_varargs_parameter_as_array() {
    let source = r#"
class Logger {
    void log(String format, Object... args) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'args' parameter
    let mut found_args = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("args") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_args = true;

                    // Verify it's marked as varargs
                    if let SymbolKind::Parameter {
                        is_varargs,
                        param_type,
                    } = &symbol.kind
                    {
                        assert!(is_varargs, "args should be marked as varargs");

                        // Varargs should still be array type
                        match param_type {
                            Type::Array {
                                element_type,
                                dimensions,
                            } => {
                                assert_eq!(*dimensions, 1u8, "Varargs should be 1D array");
                                match element_type.as_ref() {
                                    Type::Class(ct) => {
                                        assert_eq!(&ct.name, "Object");
                                    }
                                    other => panic!("Expected Object element, got {:?}", other),
                                }
                            }
                            other => {
                                panic!("Varargs should be Array{{Object, 1}}, got {:?}", other)
                            }
                        }
                    } else {
                        panic!("args should be a Parameter");
                    }

                    break;
                }
            }
        }
    }

    assert!(found_args, "Should find 'args' parameter in symbol table");
}

/// Test that 3D arrays work correctly
#[test]
fn test_three_dimensional_array() {
    let source = r#"
class Cube {
    void process(String[][][] cube) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'cube' parameter
    let mut found_cube = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("cube") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_cube = true;

                    match &symbol.type_info {
                        Type::Array {
                            element_type,
                            dimensions,
                        } => {
                            assert_eq!(
                                *dimensions, 3u8,
                                "cube should be 3-dimensional (String[][][])"
                            );
                            match element_type.as_ref() {
                                Type::Class(ct) => {
                                    assert_eq!(&ct.name, "String");
                                }
                                other => panic!("Expected String element, got {:?}", other),
                            }
                        }
                        other => panic!("Expected Array{{String, 3}}, got {:?}", other),
                    }

                    break;
                }
            }
        }
    }

    assert!(found_cube, "Should find 'cube' parameter in symbol table");
}

/// Test primitive varargs (int... numbers)
#[test]
fn test_primitive_varargs() {
    let source = r#"
class Math {
    int sum(int... numbers) {
        return 0;
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'numbers' parameter
    let mut found_numbers = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("numbers") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_numbers = true;

                    if let SymbolKind::Parameter {
                        is_varargs,
                        param_type,
                    } = &symbol.kind
                    {
                        assert!(is_varargs, "numbers should be marked as varargs");

                        // Varargs should be array type
                        match param_type {
                            Type::Array {
                                element_type,
                                dimensions,
                            } => {
                                assert_eq!(*dimensions, 1u8, "Varargs should be 1D array");
                                assert!(
                                    matches!(
                                        element_type.as_ref(),
                                        Type::Primitive(PrimitiveType::Int)
                                    ),
                                    "Expected int element type, got {:?}",
                                    element_type
                                );
                            }
                            other => {
                                panic!("Varargs should be Array{{int, 1}}, got {:?}", other)
                            }
                        }
                    } else {
                        panic!("numbers should be a Parameter");
                    }

                    break;
                }
            }
        }
    }

    assert!(
        found_numbers,
        "Should find 'numbers' parameter in symbol table"
    );
}

/// Test generic varargs (List<String>... lists)
#[test]
fn test_generic_varargs() {
    let source = r#"
import java.util.List;

class Container {
    void merge(List<String>... lists) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find the 'lists' parameter
    let mut found_lists = false;
    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            if let Some(symbol_id) = scope.lookup_local("lists") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_lists = true;

                    if let SymbolKind::Parameter {
                        is_varargs,
                        param_type,
                    } = &symbol.kind
                    {
                        assert!(is_varargs, "lists should be marked as varargs");

                        // Varargs should be array type with List<String> element type
                        match param_type {
                            Type::Array {
                                element_type,
                                dimensions,
                            } => {
                                assert_eq!(*dimensions, 1u8, "Varargs should be 1D array");
                                match element_type.as_ref() {
                                    Type::Class(ct) => {
                                        assert_eq!(&ct.name, "List");
                                        assert_eq!(ct.type_arguments.len(), 1);
                                    }
                                    other => {
                                        panic!("Expected List<String> element, got {:?}", other)
                                    }
                                }
                            }
                            other => {
                                panic!(
                                    "Varargs should be Array{{List<String>, 1}}, got {:?}",
                                    other
                                )
                            }
                        }
                    } else {
                        panic!("lists should be a Parameter");
                    }

                    break;
                }
            }
        }
    }

    assert!(found_lists, "Should find 'lists' parameter in symbol table");
}

/// Test multiple parameters where only the last is varargs
#[test]
fn test_mixed_parameters_with_varargs() {
    let source = r#"
class Logger {
    void log(String prefix, int level, Object... args) {
    }
}
"#;

    let table = parse_and_resolve(source);

    // Find parameters and verify only 'args' is varargs
    let mut found_prefix = false;
    let mut found_level = false;
    let mut found_args = false;

    for scope_id in 0..10 {
        if let Some(scope) = table.get_scope(hyperion_semantics::symbol::ScopeId(scope_id)) {
            // Check 'prefix' - should NOT be varargs
            if let Some(symbol_id) = scope.lookup_local("prefix") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_prefix = true;
                    if let SymbolKind::Parameter { is_varargs, .. } = &symbol.kind {
                        assert!(!is_varargs, "prefix should NOT be varargs");
                    }
                }
            }

            // Check 'level' - should NOT be varargs
            if let Some(symbol_id) = scope.lookup_local("level") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_level = true;
                    if let SymbolKind::Parameter { is_varargs, .. } = &symbol.kind {
                        assert!(!is_varargs, "level should NOT be varargs");
                    }
                }
            }

            // Check 'args' - should BE varargs
            if let Some(symbol_id) = scope.lookup_local("args") {
                if let Some(symbol) = table.get_symbol(symbol_id) {
                    found_args = true;
                    if let SymbolKind::Parameter {
                        is_varargs,
                        param_type,
                    } = &symbol.kind
                    {
                        assert!(is_varargs, "args should be varargs");
                        assert!(
                            matches!(param_type, Type::Array { .. }),
                            "args should be array type"
                        );
                    }
                }
            }
        }
    }

    assert!(found_prefix, "Should find 'prefix' parameter");
    assert!(found_level, "Should find 'level' parameter");
    assert!(found_args, "Should find 'args' parameter");
}
