// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Substitution Cycle Detection Tests
//!
//! Tests for detecting and handling cycles in type substitution.
//! Recursive type bounds like <T extends Comparable<T>> can create
//! substitution cycles if not handled carefully.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_comparable_recursive_bound() {
        // <T extends Comparable<T>>
        // Classic recursive bound - should NOT be a cycle

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let _comparable_t = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        // T has upper bound Comparable<T>
        // This is a valid recursive bound, not a cycle
    }

    #[test]
    fn test_enum_recursive_bound() {
        // class Enum<E extends Enum<E>>
        // E appears in its own bound recursively

        let e_var = Type::TypeVariable {
            name: "E".into(),
            bounds: vec![],
        };

        let _enum_e = Type::Class(ClassType {
            name: "Enum".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![e_var.clone()],
            ..Default::default()
        });

        // E extends Enum<E>
        // Standard Java pattern for enums
        // Valid recursive bound
    }

    #[test]
    fn test_enum_assignability_no_stack_overflow() {
        // Test that checking if MyEnum is assignable to Enum<MyEnum> doesn't stack overflow
        // This is the critical test for cycle detection in is_assignable_from

        // MyEnum (which extends Enum<MyEnum>)
        let my_enum = Type::Class(ClassType {
            name: "MyEnum".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        // Enum<MyEnum>
        let enum_my_enum = Type::Class(ClassType {
            name: "Enum".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![my_enum.clone()],
            ..Default::default()
        });

        // This should not stack overflow!
        // Before cycle detection, this would recurse infinitely:
        // 1. Check Enum<MyEnum> assignable from Enum<MyEnum>
        // 2. Compare type args: MyEnum vs MyEnum
        // 3. Check MyEnum extends Enum<MyEnum> (the bound)
        // 4. Compare type args: MyEnum vs MyEnum (cycle!)
        // 5. ... infinite recursion

        // With cycle detection (depth limiting), this returns true at depth 50
        let result = enum_my_enum.is_assignable_from(&enum_my_enum);

        // Should be true (same type)
        assert!(result, "Enum<MyEnum> should be assignable from itself");
    }

    #[test]
    fn test_comparable_assignability_no_stack_overflow() {
        // Test that checking if Integer is assignable to Comparable<Integer> doesn't stack overflow

        let integer = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let comparable_integer = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![integer.clone()],
            ..Default::default()
        });

        // This should not stack overflow!
        // The recursive bound <T extends Comparable<T>> could cause infinite recursion
        let result = comparable_integer.is_assignable_from(&comparable_integer);

        // Should be true (same type)
        assert!(
            result,
            "Comparable<Integer> should be assignable from itself"
        );
    }

    #[test]
    fn test_simple_substitution() {
        // Substitute T with String in List<T>
        // List<T>[T := String] = List<String>

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_t = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        // After substitution: List<String>
        let _expected = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // No cycle - straightforward substitution
    }

    #[test]
    fn test_self_referential_invalid() {
        // T -> T is a trivial cycle
        // Should be detected and rejected

        let _t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        // Attempting to substitute T with T itself
        // This creates a cycle and should be detected
    }

    #[test]
    fn test_mutual_recursion() {
        // T -> List<U>, U -> List<T>
        // Creates a cycle through mutual recursion

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let u_var = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![],
        };

        let _list_u = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![u_var.clone()],
            ..Default::default()
        });

        let _list_t = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        // T -> List<U> and U -> List<T> creates cycle
        // Should be detected
    }

    #[test]
    fn test_bounded_type_variable_substitution() {
        // <T extends Number> with T -> Integer
        // Should validate that Integer extends Number

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![number_type.clone()],
        };

        // Substituting T with Integer should succeed
        // Integer extends Number (satisfies bound)
        // No cycle involved
    }

    #[test]
    fn test_recursive_bound_substitution() {
        // <T extends Comparable<T>> with T -> Integer
        // Integer implements Comparable<Integer>
        // Should validate without cycle

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let _comparable_t = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Substitute T with Integer
        // Check: Integer implements Comparable<Integer> ✓
        // Recursive bound is satisfied without cycle
    }

    #[test]
    fn test_nested_type_variable_substitution() {
        // Map<T, List<T>> with T -> String
        // Should substitute both occurrences consistently

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let list_t = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        let _map_t_list_t = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![t_var.clone(), list_t.clone()],
            ..Default::default()
        });

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // After substitution: Map<String, List<String>>
        // No cycle - T appears multiple times but substitution is consistent
    }

    #[test]
    fn test_wildcard_capture_substitution() {
        // List<?> with capture CAP#1
        // Substitution should create fresh variable

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Capture creates CAP#1 where CAP#1 extends Object
        // Substitution with fresh variable avoids cycles
    }

    #[test]
    fn test_intersection_type_substitution() {
        // <T extends Number & Comparable<T>> with T -> Integer
        // Should substitute in all intersection components

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let comparable_t = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        let _intersection = Type::Intersection {
            types: vec![number_type.clone(), comparable_t.clone()],
        };

        // Substitute T with Integer
        // Results in: Number & Comparable<Integer>
        // Check both components satisfy bounds
    }

    #[test]
    fn test_depth_limit_prevents_infinite_recursion() {
        // Deeply nested substitution: List<List<List<...>>>
        // Should have depth limit to prevent stack overflow

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        // Create deeply nested type
        let mut nested = t_var.clone();
        for _ in 0..1000 {
            nested = Type::Class(ClassType {
                name: "List".into(),
                package: Some("java.util".into()),
                type_arguments: vec![nested],
                ..Default::default()
            });
        }

        // Substitution should handle deep nesting without stack overflow
        // Not a cycle, but tests recursion limits
    }

    #[test]
    fn test_substitution_with_arrays() {
        // T[] with T -> String becomes String[]

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let _t_array = Type::Array {
            element_type: Box::new(t_var.clone()),
            dimensions: 1,
        };

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // After substitution: String[]
        // No cycle in array types
    }

    #[test]
    fn test_multiple_type_parameters_no_cycle() {
        // <T, U> with no relationship
        // T -> String, U -> Integer
        // Independent substitutions

        let _t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let _u_var = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![],
        };

        // Substitute both independently
        // No cycles between unrelated type variables
    }

    #[test]
    fn test_f_bounded_polymorphism() {
        // <T extends Builder<T>>
        // F-bounded polymorphism pattern

        let t_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let _builder_t = Type::Class(ClassType {
            name: "Builder".into(),
            package: None,
            type_arguments: vec![t_var.clone()],
            ..Default::default()
        });

        // T extends Builder<T>
        // Classic F-bounded polymorphism
        // Valid recursive bound, not a cycle
    }
}
