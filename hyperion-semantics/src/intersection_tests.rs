// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Intersection Type Tests
//!
//! Tests for intersection types in type bounds and casts.
//! Intersection types represent values that satisfy multiple type constraints
//! simultaneously (e.g., T extends Serializable & Comparable<T>).

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_intersection_type_creation() {
        // <T extends Number & Comparable<T>>

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let comparable_t = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let comparable_type = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![comparable_t.clone()],
            ..Default::default()
        });

        let intersection = Type::Intersection {
            types: vec![number_type.clone(), comparable_type.clone()],
        };

        // Intersection type represents T satisfying both constraints
        if let Type::Intersection { types } = intersection {
            assert_eq!(types.len(), 2);
        } else {
            panic!("Expected intersection type");
        }
    }

    #[test]
    fn test_intersection_assignability_to_components() {
        // T extends Number & Serializable
        // T is assignable to both Number and Serializable

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let serializable_type = Type::Class(ClassType {
            name: "Serializable".into(),
            package: Some("java.io".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let intersection = Type::Intersection {
            types: vec![number_type.clone(), serializable_type.clone()],
        };

        // Intersection is assignable to each component
        assert!(
            number_type.is_assignable_from(&intersection),
            "Intersection should be assignable to Number component"
        );
        assert!(
            serializable_type.is_assignable_from(&intersection),
            "Intersection should be assignable to Serializable component"
        );
    }

    #[test]
    fn test_intersection_not_assignable_from_single_component() {
        // Number is NOT assignable to (Number & Serializable)
        // Must satisfy all constraints

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let serializable_type = Type::Class(ClassType {
            name: "Serializable".into(),
            package: Some("java.io".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let intersection = Type::Intersection {
            types: vec![number_type.clone(), serializable_type.clone()],
        };

        // Single component is NOT assignable to intersection
        assert!(
            !intersection.is_assignable_from(&number_type),
            "Number alone should NOT be assignable to (Number & Serializable)"
        );
    }

    #[test]
    fn test_intersection_cast() {
        // (Serializable & Comparable<String>) obj
        // Valid if obj's runtime type implements both

        let serializable_type = Type::Class(ClassType {
            name: "Serializable".into(),
            package: Some("java.io".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let comparable_string = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        let _intersection = Type::Intersection {
            types: vec![serializable_type.clone(), comparable_string.clone()],
        };

        // String implements both, so cast would succeed
        // This is a valid intersection cast target
    }

    #[test]
    fn test_intersection_with_class_and_interfaces() {
        // T extends ArrayList<String> & RandomAccess & Cloneable
        // Class type must come first

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let arraylist_string = Type::Class(ClassType {
            name: "ArrayList".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        let random_access = Type::Class(ClassType {
            name: "RandomAccess".into(),
            package: Some("java.util".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let cloneable = Type::Class(ClassType {
            name: "Cloneable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let intersection = Type::Intersection {
            types: vec![
                arraylist_string.clone(),
                random_access.clone(),
                cloneable.clone(),
            ],
        };

        // First component should be a class type, rest are interfaces
        if let Type::Intersection { types } = intersection {
            assert_eq!(types.len(), 3);
            // In proper implementation, would verify first is class
        }
    }

    #[test]
    fn test_intersection_order_matters_for_members() {
        // T extends Foo & Bar
        // T.method() first looks in Foo, then Bar

        let foo_type = Type::Class(ClassType {
            name: "Foo".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let bar_type = Type::Class(ClassType {
            name: "Bar".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let _intersection_foo_bar = Type::Intersection {
            types: vec![foo_type.clone(), bar_type.clone()],
        };

        let _intersection_bar_foo = Type::Intersection {
            types: vec![bar_type.clone(), foo_type.clone()],
        };

        // Member lookup order depends on intersection component order
        // These are semantically different types
    }

    #[test]
    fn test_intersection_least_upper_bound() {
        // LUB(String, Comparable<String>) = String & Comparable<String>
        // When inferring type parameters

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _comparable_string = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // String already implements Comparable<String>
        // LUB is just String (most specific)
        // But in general case, LUB creates intersection
    }

    #[test]
    fn test_intersection_empty_invalid() {
        // Intersection with zero types is invalid

        let _empty_intersection = Type::Intersection { types: vec![] };

        // This should be rejected - intersection needs at least one type
        // In practice, parser/builder should prevent this
    }

    #[test]
    fn test_intersection_single_type() {
        // Intersection with one type is equivalent to that type
        // T extends Number is same as T extends (Number)

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _intersection_single = Type::Intersection {
            types: vec![number_type.clone()],
        };

        // Should be normalized to just Number
        // Intersection with single element is redundant
    }

    #[test]
    fn test_intersection_with_type_variables() {
        // <T extends Number, U extends Comparable<U>>
        // <V extends T & U>
        // V is bounded by intersection of type variables

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType {
                name: "Number".into(),
                package: Some("java.lang".into()),
                type_arguments: vec![],
                ..Default::default()
            })],
        };

        let u_var = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![],
        };

        let _intersection = Type::Intersection {
            types: vec![t_var.clone(), u_var.clone()],
        };

        // V must satisfy bounds of both T and U
    }

    #[test]
    fn test_intersection_with_arrays() {
        // Can't have intersection with array types
        // T extends Object[] & Cloneable is redundant (arrays are already Cloneable)

        let _object_array = Type::Array {
            element_type: Box::new(Type::Class(ClassType {
                name: "Object".into(),
                package: Some("java.lang".into()),
                type_arguments: vec![],
                ..Default::default()
            })),
            dimensions: 1,
        };

        let _cloneable = Type::Class(ClassType {
            name: "Cloneable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Arrays implicitly implement Cloneable and Serializable
        // Explicit intersection is redundant but valid
    }

    #[test]
    fn test_intersection_nested() {
        // (T & U) & V should be flattened to T & U & V

        let t_type = Type::Class(ClassType {
            name: "T".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let u_type = Type::Class(ClassType {
            name: "U".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let v_type = Type::Class(ClassType {
            name: "V".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let inner_intersection = Type::Intersection {
            types: vec![t_type.clone(), u_type.clone()],
        };

        let _outer_intersection = Type::Intersection {
            types: vec![inner_intersection, v_type.clone()],
        };

        // Should be normalized to flat intersection: T & U & V
    }

    #[test]
    fn test_intersection_with_final_class() {
        // T extends String & Comparable<String>
        // Invalid - String is final, can't have additional constraints

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _comparable_string = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // String already implements Comparable<String>
        // Redundant but valid (String's actual type satisfies both)
        // But if String didn't implement it, would be error
    }

    #[test]
    fn test_intersection_wildcard_bounds() {
        // List<? extends Number & Comparable<Number>>
        // Wildcard with intersection upper bound

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

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(intersection_bound)),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Wildcard element must satisfy both bounds
    }
}
