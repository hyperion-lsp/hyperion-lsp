// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Wildcard Type Logic Asymmetry Tests
//!
//! Tests for asymmetric wildcard type relationships.
//! Wildcard assignability is not symmetric - just because A is assignable to B
//! does not mean B is assignable to A.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_extends_asymmetry() {
        // List<? extends Number> is assignable to List<? extends Object>
        // but NOT vice versa

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
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

        // Forward direction: should succeed
        assert!(
            list_extends_object.is_assignable_from(&list_extends_number),
            "List<? extends Number> should be assignable to List<? extends Object>"
        );

        // Reverse direction: should fail
        assert!(
            !list_extends_number.is_assignable_from(&list_extends_object),
            "List<? extends Object> should NOT be assignable to List<? extends Number>"
        );
    }

    #[test]
    fn test_super_asymmetry() {
        // A target List<? super Integer> accepts a source
        // List<? super Number>, but not vice versa.

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
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

        let list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        let list_super_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(number_type.clone())),
            }],
            ..Default::default()
        });

        // Forward direction: should succeed (contravariant)
        assert!(
            list_super_integer.is_assignable_from(&list_super_number),
            "target List<? super Integer> should accept source List<? super Number>"
        );

        // Reverse direction: should fail
        assert!(
            !list_super_number.is_assignable_from(&list_super_integer),
            "target List<? super Number> should NOT accept source List<? super Integer>"
        );
    }

    #[test]
    fn test_concrete_to_wildcard_asymmetry() {
        // List<String> is assignable to List<? extends String>
        // but List<? extends String> is NOT assignable to List<String>

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

        let list_extends_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(string_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Concrete to wildcard: should succeed
        assert!(
            list_extends_string.is_assignable_from(&list_string),
            "List<String> should be assignable to List<? extends String>"
        );

        // Wildcard to concrete: should fail
        assert!(
            !list_string.is_assignable_from(&list_extends_string),
            "List<? extends String> should NOT be assignable to List<String>"
        );
    }

    #[test]
    fn test_unbounded_wildcard_asymmetry() {
        // List<?> is assignable from List<String>
        // but List<String> is NOT assignable from List<?>

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
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

        let list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Concrete to unbounded: should succeed
        assert!(
            list_wildcard.is_assignable_from(&list_string),
            "List<String> should be assignable to List<?>"
        );

        // Unbounded to concrete: should fail
        assert!(
            !list_string.is_assignable_from(&list_wildcard),
            "List<?> should NOT be assignable to List<String>"
        );
    }

    #[test]
    fn test_mixed_variance_asymmetry() {
        // List<? extends Number> is NOT assignable to List<? super Integer>
        // (different variance directions)

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
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

        let list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        let list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // No relationship between extends and super wildcards
        assert!(
            !list_super_integer.is_assignable_from(&list_extends_number),
            "List<? extends Number> should NOT be assignable to List<? super Integer>"
        );
        assert!(
            !list_extends_number.is_assignable_from(&list_super_integer),
            "List<? super Integer> should NOT be assignable to List<? extends Number>"
        );
    }

    #[test]
    fn test_nested_asymmetry() {
        // Map<String, ? extends Number> vs Map<String, ? extends Object>

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

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let map_extends_number = Type::Class(ClassType {
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

        let map_extends_object = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![
                string_type.clone(),
                Type::Wildcard {
                    upper_bound: Some(Box::new(object_type.clone())),
                    lower_bound: None,
                },
            ],
            ..Default::default()
        });

        // Forward: should succeed
        assert!(
            map_extends_object.is_assignable_from(&map_extends_number),
            "Map<String, ? extends Number> should be assignable to Map<String, ? extends Object>"
        );

        // Reverse: should fail
        assert!(
            !map_extends_number.is_assignable_from(&map_extends_object),
            "Map<String, ? extends Object> should NOT be assignable to Map<String, ? extends Number>"
        );
    }

    #[test]
    fn test_array_covariance_vs_wildcard() {
        // Arrays are covariant: Number[] is assignable to Object[]
        // But List<Number> is NOT assignable to List<Object>
        // Only List<Number> is assignable to List<? extends Object>

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        let list_object = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![object_type.clone()],
            ..Default::default()
        });

        // Generics are invariant
        assert!(
            !list_object.is_assignable_from(&list_number),
            "List<Number> should NOT be assignable to List<Object> (invariant)"
        );

        // But with wildcards it works
        let list_extends_object = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(object_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        assert!(
            list_extends_object.is_assignable_from(&list_number),
            "List<Number> should be assignable to List<? extends Object>"
        );
    }

    #[test]
    fn test_wildcard_not_reflexive_with_different_bounds() {
        // List<? extends Number> is not the same as List<? extends Integer>
        // Neither is assignable to the other

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
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

        let list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        let list_extends_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(integer_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Integer extends Number, so this should work
        assert!(
            list_extends_number.is_assignable_from(&list_extends_integer),
            "List<? extends Integer> should be assignable to List<? extends Number>"
        );

        // But not the reverse
        assert!(
            !list_extends_integer.is_assignable_from(&list_extends_number),
            "List<? extends Number> should NOT be assignable to List<? extends Integer>"
        );
    }

    #[test]
    fn test_pecs_producer_asymmetry() {
        // Producer Extends: List<? extends T> for reading
        // You can read T (or its supertypes) but cannot write

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Can assign List<Integer> to this (producer)
        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![integer_type.clone()],
            ..Default::default()
        });

        assert!(
            list_extends_number.is_assignable_from(&list_integer),
            "List<Integer> should be assignable to List<? extends Number> (producer)"
        );
    }

    #[test]
    fn test_pecs_consumer_asymmetry() {
        // Consumer Super: List<? super T> for writing
        // You can write T (or its subtypes) but can only read Object

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // Can assign List<Number> to this (consumer)
        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        assert!(
            list_super_integer.is_assignable_from(&list_number),
            "List<Number> should be assignable to List<? super Integer> (consumer)"
        );
    }
}
