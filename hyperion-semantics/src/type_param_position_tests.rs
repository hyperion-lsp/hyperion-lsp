// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Parameter Position Tests
//!
//! Tests for handling type parameters that appear in different positions
//! (covariant, contravariant, invariant). The position affects variance
//! and assignability rules.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_covariant_position_return_type() {
        // Type parameter in return type is covariant
        // interface Producer<T> { T get(); }
        // If Integer <: Number, then Producer<Integer> <: Producer<? extends Number>

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

        let producer_integer = Type::Class(ClassType {
            name: "Producer".into(),
            package: None,
            type_arguments: vec![integer_type.clone()],
            ..Default::default()
        });

        let producer_extends_number = Type::Class(ClassType {
            name: "Producer".into(),
            package: None,
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Covariant: can assign to wildcard with extends
        assert!(
            producer_extends_number.is_assignable_from(&producer_integer),
            "Producer<Integer> should be assignable to Producer<? extends Number>"
        );
    }

    #[test]
    fn test_contravariant_position_parameter_type() {
        // Type parameter in method parameter is contravariant
        // interface Consumer<T> { void accept(T value); }
        // If Integer <: Number, then Consumer<Number> <: Consumer<? super Integer>

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

        let consumer_number = Type::Class(ClassType {
            name: "Consumer".into(),
            package: None,
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        let consumer_super_integer = Type::Class(ClassType {
            name: "Consumer".into(),
            package: None,
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // Contravariant: can assign to wildcard with super
        assert!(
            consumer_super_integer.is_assignable_from(&consumer_number),
            "Consumer<Number> should be assignable to Consumer<? super Integer>"
        );
    }

    #[test]
    fn test_invariant_position_both_in_out() {
        // Type parameter used in both positions is invariant
        // interface Function<T> { T apply(T value); }
        // Function<Integer> is NOT assignable to Function<Number>

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

        let function_integer = Type::Class(ClassType {
            name: "Function".into(),
            package: None,
            type_arguments: vec![integer_type.clone()],
            ..Default::default()
        });

        let function_number = Type::Class(ClassType {
            name: "Function".into(),
            package: None,
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        // Invariant: no subtype relationship
        assert!(
            !function_number.is_assignable_from(&function_integer),
            "Function<Integer> should NOT be assignable to Function<Number> (invariant)"
        );
        assert!(
            !function_integer.is_assignable_from(&function_number),
            "Function<Number> should NOT be assignable to Function<Integer> (invariant)"
        );
    }

    #[test]
    fn test_multiple_type_parameters_different_positions() {
        // interface BiFunction<T, U, R> { R apply(T t, U u); }
        // T and U are contravariant (parameters)
        // R is covariant (return type)

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

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // BiFunction<Number, Number, Integer>
        let _bifunction = Type::Class(ClassType {
            name: "BiFunction".into(),
            package: None,
            type_arguments: vec![
                number_type.clone(),
                number_type.clone(),
                integer_type.clone(),
            ],
            ..Default::default()
        });

        // This could be assigned to BiFunction<? super Integer, ? super Integer, ? extends Number>
    }

    #[test]
    fn test_nested_type_parameter_position() {
        // List<Producer<T>> - T is in covariant position
        // List<Consumer<T>> - T is in contravariant position

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let producer_integer = Type::Class(ClassType {
            name: "Producer".into(),
            package: None,
            type_arguments: vec![integer_type.clone()],
            ..Default::default()
        });

        let _list_producer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![producer_integer.clone()],
            ..Default::default()
        });

        // The variance of nested positions multiplies:
        // List is invariant, Producer is covariant over T
        // Result: invariant
    }

    #[test]
    fn test_array_type_parameter_position() {
        // Arrays are covariant but mutable, causing type unsafety
        // T[] in parameter position should be invariant

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

        let integer_array = Type::Array {
            element_type: Box::new(integer_type.clone()),
            dimensions: 1,
        };

        let number_array = Type::Array {
            element_type: Box::new(number_type.clone()),
            dimensions: 1,
        };

        // Arrays are covariant (but unsafely so)
        assert!(
            number_array.is_assignable_from(&integer_array),
            "Integer[] should be assignable to Number[] (array covariance)"
        );
    }

    #[test]
    fn test_field_type_parameter_position() {
        // class Box<T> { T value; }
        // Field is both readable (covariant) and writable (contravariant)
        // Therefore Box<T> is invariant

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

        let box_integer = Type::Class(ClassType {
            name: "Box".into(),
            package: None,
            type_arguments: vec![integer_type.clone()],
            ..Default::default()
        });

        let box_number = Type::Class(ClassType {
            name: "Box".into(),
            package: None,
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        // Invariant due to mutable field
        assert!(
            !box_number.is_assignable_from(&box_integer),
            "Box<Integer> should NOT be assignable to Box<Number> (invariant)"
        );
    }

    #[test]
    fn test_lambda_parameter_position() {
        // Lambda infers types based on target type and position
        // Consumer<Number> c = (Number n) -> { }
        // Parameter position makes Number contravariant

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _consumer_number = Type::Class(ClassType {
            name: "Consumer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![number_type.clone()],
            ..Default::default()
        });

        // Lambda parameter gets type from Consumer<Number>
    }

    #[test]
    fn test_recursive_type_parameter_position() {
        // interface Comparable<T> { int compareTo(T other); }
        // T appears in parameter position (contravariant)
        // But T is bound to self-type, creating recursion

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Comparable<Integer> is invariant despite T in contravariant position
        // because of the recursive bound
    }

    #[test]
    fn test_builder_pattern_position() {
        // interface Builder<T> {
        //     Builder<T> add(T element);
        //     T build();
        // }
        // T in parameter (contravariant) and return (covariant)
        // Result: invariant

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _builder_string = Type::Class(ClassType {
            name: "Builder".into(),
            package: None,
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Invariant due to both input and output positions
    }

    #[test]
    fn test_wildcard_position_in_bounds() {
        // <T extends Comparable<? super T>>
        // T is in upper bound, ? super T creates contravariant position

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _comparable_super_integer = Type::Class(ClassType {
            name: "Comparable".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // This pattern allows comparing with supertypes
        // Common in Collections.sort
    }
}
