// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Raw Type Handling Tests
//!
//! Tests for raw type semantics.
//! A raw type is a generic class used without type arguments (e.g., List instead of List<String>).
//! Raw types should be handled specially for backwards compatibility with pre-generics Java code.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_raw_type_creation() {
        // List (raw) vs List<String> (parameterized)

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![], // Empty = raw type
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Both are valid List types but with different type safety
        assert!(matches!(raw_list, Type::Class(_)));
        assert!(matches!(list_string, Type::Class(_)));
    }

    #[test]
    fn test_raw_type_to_parameterized() {
        // List (raw) should be assignable to List<String>
        // (unchecked conversion warning)

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Raw type can be assigned to any parameterization
        // (should generate unchecked warning in practice)
        assert!(
            list_string.is_assignable_from(&raw_list),
            "List (raw) should be assignable to List<String> with unchecked warning"
        );
    }

    #[test]
    fn test_parameterized_to_raw_type() {
        // List<String> should be assignable to List (raw)

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Parameterized type can be assigned to raw type (safe direction)
        assert!(
            raw_list.is_assignable_from(&list_string),
            "List<String> should be assignable to List (raw)"
        );
    }

    #[test]
    fn test_raw_type_vs_unbounded_wildcard() {
        // List (raw) is different from List<?>
        // Raw type: erased to Object internally
        // List<?> : can hold any type but is type-safe

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // These are different types with different semantics
        // Raw type allows any operation (unchecked)
        // List<?> only allows safe read operations
        assert_ne!(raw_list, list_wildcard);
    }

    #[test]
    fn test_raw_type_nested_generics() {
        // Map (raw) vs Map<String, Integer> vs Map<?, ?>

        let raw_map = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let map_string_integer = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone(), integer_type.clone()],
            ..Default::default()
        });

        // Raw map can be assigned to any parameterization
        assert!(
            map_string_integer.is_assignable_from(&raw_map),
            "Map (raw) should be assignable to Map<String, Integer>"
        );
    }

    #[test]
    fn test_raw_type_array() {
        // List[] (array of raw List) vs List<String>[] (array of parameterized List)

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let raw_list_array = Type::Array {
            element_type: Box::new(raw_list.clone()),
            dimensions: 1,
        };

        // Arrays of raw types are valid but lose even more type safety
        assert!(matches!(raw_list_array, Type::Array { .. }));
    }

    #[test]
    fn test_raw_type_instanceof() {
        // instanceof with raw types
        // "obj instanceof List" is valid (raw type)
        // "obj instanceof List<String>" is illegal (cannot test parameterized type at runtime)

        let _raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Raw types are the only valid form for instanceof checks
        // This documents the expected behavior
    }

    #[test]
    fn test_raw_type_constructor() {
        // new List() is different from new ArrayList<String>()
        // Constructor calls on raw types

        let _raw_arraylist = Type::Class(ClassType {
            name: "ArrayList".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Raw constructor creates a raw instance
        // All operations on it are unchecked
    }

    #[test]
    fn test_raw_type_inheritance() {
        // class MyList extends List { } // Raw superclass
        // This creates a raw type hierarchy

        let _raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _my_list = Type::Class(ClassType {
            name: "MyList".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        // If MyList extends raw List, it inherits raw type semantics
    }

    #[test]
    fn test_raw_type_field_access() {
        // Accessing fields on raw types

        let _raw_pair = Type::Class(ClassType {
            name: "Pair".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        // class Pair<T> { T first; T second; }
        // Pair p = new Pair();
        // Object f = p.first; // Returns Object (erasure)
    }

    #[test]
    fn test_multiple_raw_types_interaction() {
        // Map<List, Set> - using raw types as type arguments
        // This is valid but generates warnings

        let raw_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let raw_set = Type::Class(ClassType {
            name: "Set".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let map_raw_raw = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![raw_list.clone(), raw_set.clone()],
            ..Default::default()
        });

        // Using raw types as type arguments is valid but discouraged
        assert!(matches!(map_raw_raw, Type::Class(_)));
    }
}
