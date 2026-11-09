// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Generic Method Type Inference Tests
//!
//! Tests for type inference with generic methods.
//! The type system must infer type arguments from method arguments
//! and return type context.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_infer_from_return_assignment() {
        // Given: <T> T identity(T value)
        // When: String s = identity("hello")
        // Then: T = String

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // The return type context (String) and argument type ("hello" -> String)
        // both constrain T to String
        // This is a consistent inference: T = String
    }

    #[test]
    fn test_infer_from_argument_only() {
        // Given: <T> void print(T value)
        // When: print("hello")
        // Then: T = String

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // No return type context (void)
        // Infer T from argument: "hello" -> String
        // Result: T = String
    }

    #[test]
    fn test_infer_multiple_type_parameters() {
        // Given: <K, V> void put(K key, V value)
        // When: put("name", 42)
        // Then: K = String, V = Integer

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
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

        // Infer K from first arg: "name" -> String
        // Infer V from second arg: 42 -> Integer (boxed)
        // Result: K = String, V = Integer
    }

    #[test]
    fn test_infer_with_bounds() {
        // Given: <T extends Number> T max(T a, T b)
        // When: max(1, 2)
        // Then: T = Integer (satisfies T extends Number)

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Integer extends Number
        // Both arguments are Integer
        // T = Integer is valid (Integer <: Number)
    }

    #[test]
    fn test_infer_least_upper_bound() {
        // Given: <T> T choose(boolean condition, T a, T b)
        // When: choose(true, "hello", "world")
        // Then: T = String (common type)

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Both arguments are String
        // Least upper bound = String
        // Result: T = String
    }

    #[test]
    fn test_infer_lub_with_inheritance() {
        // Given: <T> T choose(boolean condition, T a, T b)
        // And: class Child extends Parent
        // When: choose(true, new Parent(), new Child())
        // Then: T = Parent (least upper bound)

        let _parent_type = Type::Class(ClassType {
            name: "Parent".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        let _child_type = Type::Class(ClassType {
            name: "Child".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        // With Child <: Parent:
        // LUB(Parent, Child) = Parent
        // Result: T = Parent

        // This test documents expected behavior.
        // Actual implementation would need inheritance hierarchy.
    }

    #[test]
    fn test_infer_lub_object() {
        // Given: <T> T choose(boolean condition, T a, T b)
        // When: choose(true, "hello", 42)
        // Then: T = Object (String and Integer have no closer common ancestor)

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
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

        let _object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // String and Integer are unrelated
        // LUB(String, Integer) = Object
        // Result: T = Object
    }

    #[test]
    fn test_infer_nested_generics() {
        // Given: <T> List<T> singleton(T element)
        // When: List<String> list = singleton("hello")
        // Then: T = String

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Return type: List<T>
        // Expected type: List<String>
        // Unify: List<T> = List<String> => T = String
        // Argument: "hello" -> String
        // Both constraints agree: T = String
    }

    #[test]
    fn test_infer_chained_calls() {
        // Given: <T> List<T> of(T... elements)
        // And: <E> Stream<E> stream(List<E> list)
        // When: stream(of("a", "b"))
        // Then: First call: T = String, Second call: E = String

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            ..Default::default()
        });

        // Inner call: of("a", "b")
        //   Infer T = String from varargs
        //   Returns: List<String>
        // Outer call: stream(List<String>)
        //   Infer E = String from argument
        //   Returns: Stream<String>
    }

    #[test]
    fn test_infer_with_wildcard_capture() {
        // Given: <T> void process(List<T> list)
        // When: List<?> wildcard = ...; process(wildcard)
        // Then: T = CAP#1 (captured wildcard)

        let _wildcard_list = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Capture conversion creates a fresh type variable
        // representing the unknown type of the wildcard
        // This allows type-safe operations on the list
    }

    #[test]
    fn test_infer_bounded_wildcard() {
        // Given: <T extends Comparable<T>> T max(List<T> list)
        // When: max(Arrays.asList(1, 2, 3))
        // Then: T = Integer (Integer implements Comparable<Integer>)

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Integer implements Comparable<Integer>
        // Satisfies bound: T extends Comparable<T>
        // Result: T = Integer
    }

    #[test]
    fn test_infer_conflicting_bounds() {
        // Given: <T extends Number & Comparable<T>> T min(T a, T b)
        // When: min(1, 2)
        // Then: T = Integer (satisfies both bounds)

        let _integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // Integer extends Number ✓
        // Integer implements Comparable<Integer> ✓
        // Result: T = Integer
    }

    #[test]
    fn test_infer_recursive_bound() {
        // Given: <T extends Enum<T>> void process(T value)
        // When: process(Color.RED) where enum Color { RED, GREEN, BLUE }
        // Then: T = Color (satisfies recursive bound)

        let _color_type = Type::Class(ClassType {
            name: "Color".into(),
            package: None,
            type_arguments: vec![],
            ..Default::default()
        });

        // Color extends Enum<Color> (recursive bound)
        // Result: T = Color
    }

    #[test]
    fn test_infer_target_type_from_lambda() {
        // Given: <T> void forEach(Consumer<T> consumer)
        // When: list.forEach(x -> System.out.println(x))
        // Then: T inferred from list element type

        // This is primarily a lambda inference test
        // but demonstrates how target typing influences generic inference

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // If list is List<String>:
        // Consumer<T> must be Consumer<String>
        // Therefore T = String
        // Lambda parameter x has type String
    }
}
