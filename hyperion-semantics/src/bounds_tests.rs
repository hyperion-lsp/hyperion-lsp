// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Parameter Bound Subsumption Tests
//!
//! Tests for handling type parameter bounds in assignability checks.
//! When checking if List<? extends Number> is assignable to List<? extends Object>,
//! the type system must recognize that the extends clause creates a subsumption
//! relationship, not exact type equality.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_extends_subsumption_basic() {
        // List<? extends Number> should be assignable to List<? extends Object>
        // because Number <: Object

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

        // This should succeed due to bound subsumption
        assert!(
            list_extends_object.is_assignable_from(&list_extends_number),
            "List<? extends Number> should be assignable to List<? extends Object>"
        );
    }

    #[test]
    fn test_extends_subsumption_reversed() {
        // List<? extends Object> is NOT assignable to List<? extends Number>
        // (reverse direction should fail)

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

        // This should fail - can't narrow the bound
        assert!(
            !list_extends_number.is_assignable_from(&list_extends_object),
            "List<? extends Object> should NOT be assignable to List<? extends Number>"
        );
    }

    #[test]
    fn test_super_subsumption_basic() {
        // A source List<? super Number> can be assigned to a target
        // List<? super Integer>: every possible instantiation of the source is
        // also admitted by the target because Integer <: Number.

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

        // This should succeed (contravariant lower bound)
        assert!(
            list_super_integer.is_assignable_from(&list_super_number),
            "target List<? super Integer> should accept source List<? super Number>"
        );
    }

    #[test]
    fn test_unbounded_wildcard_subsumption() {
        // List<?> should be assignable from List<? extends Number>
        // (unbounded wildcard is the most general)

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let list_unbounded = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
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

        // List<?> is equivalent to List<? extends Object>
        assert!(
            list_unbounded.is_assignable_from(&list_extends_number),
            "List<? extends Number> should be assignable to List<?>"
        );
    }

    #[test]
    fn test_concrete_type_to_bounded_wildcard() {
        // List<String> should be assignable to List<? extends String>

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

        assert!(
            list_extends_string.is_assignable_from(&list_string),
            "List<String> should be assignable to List<? extends String>"
        );
    }

    #[test]
    fn test_concrete_type_to_super_wildcard() {
        // List<Number> should be assignable to List<? super Integer>
        // if Number is a supertype of Integer

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

        let list_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![number_type.clone()],
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

        // Number is a supertype of Integer
        assert!(
            list_super_integer.is_assignable_from(&list_number),
            "List<Number> should be assignable to List<? super Integer>"
        );
    }

    #[test]
    fn test_nested_bound_subsumption() {
        // Map<String, ? extends Number> -> Map<String, ? extends Object>

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

        let map_string_extends_number = Type::Class(ClassType {
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

        let map_string_extends_object = Type::Class(ClassType {
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

        assert!(
            map_string_extends_object.is_assignable_from(&map_string_extends_number),
            "Map<String, ? extends Number> should be assignable to Map<String, ? extends Object>"
        );
    }

    #[test]
    fn test_multiple_bounds_subsumption() {
        // List<? extends Number & Comparable<Number>>
        // should be assignable to List<? extends Number>

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let comparable_number = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        let intersection_bound = Type::Intersection {
            types: vec![number_type.clone(), comparable_number.clone()],
        };

        let list_intersection = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(intersection_bound)),
                lower_bound: None,
            }],
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

        // Intersection is more specific than single bound
        assert!(
            list_extends_number.is_assignable_from(&list_intersection),
            "List<? extends Number & Comparable<Number>> should be assignable to List<? extends Number>"
        );
    }

    #[test]
    fn test_bound_not_exact_match() {
        // List<? extends Integer> is NOT assignable to List<Integer>
        // (wildcard vs concrete type)

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

        let list_extends_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(integer_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Wildcards don't convert back to concrete types
        assert!(
            !list_integer.is_assignable_from(&list_extends_integer),
            "List<? extends Integer> should NOT be assignable to List<Integer>"
        );
    }
}
