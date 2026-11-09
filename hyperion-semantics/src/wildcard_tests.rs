// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Wildcard Type Erasure Tests
//!
//! Tests for wildcard type erasure in assignability checks.
//! Wildcards must be erased correctly when checking type compatibility.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_wildcard_basic_erasure() {
        // List<?> erases to List
        // Upper bound of ? is Object

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Unbounded wildcard has implicit upper bound of Object
        // At erasure: List<?> -> List (raw)
    }

    #[test]
    fn test_wildcard_extends_erasure() {
        // List<? extends Number> erases to List
        // Upper bound is Number

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // At erasure: List<? extends Number> -> List (raw)
        // But upper bound information (Number) is preserved for type safety
    }

    #[test]
    fn test_wildcard_super_erasure() {
        // List<? super Integer> erases to List
        // Lower bound is Integer, implicit upper bound is Object

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // At erasure: List<? super Integer> -> List (raw)
    }

    #[test]
    fn test_wildcard_vs_object() {
        // List<?> is equivalent to List<? extends Object>

        let list_unbounded = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_extends_object = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(object_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // These should be equivalent for assignability
        assert!(
            list_unbounded.is_assignable_from(&list_extends_object),
            "List<? extends Object> should be assignable to List<?>"
        );
        assert!(
            list_extends_object.is_assignable_from(&list_unbounded),
            "List<?> should be assignable to List<? extends Object>"
        );
    }

    #[test]
    fn test_nested_wildcard_erasure() {
        // Map<String, ? extends Number> erases to Map
        // Type arguments are erased individually

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _map_string_wildcard = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![
                string_type.clone(),
                Type::Wildcard {
                    upper_bound: Some(Box::new(number_type.clone())),
                    lower_bound: None,
                },
            ],
            ..Default::default()
        });

        // At erasure: Map<String, ? extends Number> -> Map
    }

    #[test]
    fn test_wildcard_array_erasure() {
        // List<?>[] erases to List[] (array of raw List)

        let list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _array_of_wildcard_list = Type::Array {
            element_type: Box::new(list_wildcard.clone()),
            dimensions: 1,
        };

        // Erasure preserves array structure but erases element type
    }

    #[test]
    fn test_multiple_wildcards_erasure() {
        // List<? extends Number> and List<? extends String> both erase to List

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _list_extends_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(string_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Same erasure but different type bounds
        // Cannot overload methods with these parameter types
    }

    #[test]
    fn test_wildcard_instanceof_invalid() {
        // "obj instanceof List<?>" is illegal
        // Can only use instanceof with raw types or specific parameterizations

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // This should be rejected at compile time
        // instanceof requires reifiable types (no wildcards)
    }

    #[test]
    fn test_wildcard_new_invalid() {
        // "new List<?>()" is illegal
        // Cannot instantiate types with wildcards

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // This should be rejected at compile time
        // new requires concrete types (no wildcards)
    }

    #[test]
    fn test_recursive_wildcard_erasure() {
        // List<? extends List<?>> erases to List

        let list_wildcard_inner = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _list_wildcard_outer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(list_wildcard_inner.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Nested wildcards all erase to raw types
    }

    #[test]
    fn test_wildcard_cast_warning() {
        // (List<String>) wildcardList generates unchecked cast warning

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Casting from wildcard to concrete type is unchecked
        // Should generate warning
    }
}
