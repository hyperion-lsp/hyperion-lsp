// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Overload Resolution Tests
//!
//! Tests for method overload resolution with type inference.
//! The type system must select the most specific applicable method
//! when multiple overloads are available.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, PrimitiveType, Type};
    use crate::types::TypeExt;

    #[test]
    fn test_overload_string_vs_object() {
        // Given: void foo(String s) and void foo(Object o)
        // When: foo("hello")
        // Then: Should resolve to foo(String) as more specific

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        // String is assignable to Object (subtype)
        assert!(object_type.is_assignable_from(&string_type));

        // Object is NOT assignable to String (not a supertype)
        assert!(!string_type.is_assignable_from(&object_type));

        // For overload resolution:
        // - Both methods are applicable (String is assignable to Object)
        // - foo(String) is more specific (String <: Object)
        // - Should select foo(String)

        // This test documents the expected behavior.
        // Actual overload resolution would need method signature comparison.
    }

    #[test]
    fn test_overload_int_vs_long() {
        // Given: void foo(int x) and void foo(long x)
        // When: foo(42)
        // Then: Should resolve to foo(int) as exact match

        let int_type = Type::Primitive(PrimitiveType::Int);
        let long_type = Type::Primitive(PrimitiveType::Long);

        // int is widening-convertible to long
        assert!(long_type.is_assignable_from(&int_type));

        // long is NOT assignable to int (narrowing conversion needed)
        assert!(!int_type.is_assignable_from(&long_type));

        // For overload resolution with literal 42:
        // - Both methods are applicable (int widens to long)
        // - foo(int) is more specific (exact match, no conversion)
        // - Should select foo(int)
    }

    #[test]
    fn test_overload_varargs_vs_array() {
        // Given: void foo(String... args) and void foo(String[] args)
        // When: foo("a", "b")
        // Then: Should resolve to foo(String...) for variable args

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let _string_array = Type::Array {
            element_type: Box::new(string_type.clone()),
            dimensions: 1,
        };

        // Varargs and array are structurally equivalent at runtime
        // but varargs is preferred for variable argument calls

        // This test documents the expected behavior.
        // Actual implementation would need varargs tracking in method signatures.
    }

    #[test]
    fn test_overload_generic_type_arguments() {
        // Given: void foo(List<String> list) and void foo(List<Object> list)
        // When: foo(new ArrayList<String>())
        // Then: Should resolve to foo(List<String>) by exact type match

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            is_diamond: false,
        });

        let list_object = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![object_type.clone()],
            is_diamond: false,
        });

        // Important: List<String> is NOT assignable to List<Object>
        // (generics are invariant)
        assert!(!list_object.is_assignable_from(&list_string));
        assert!(!list_string.is_assignable_from(&list_object));

        // For overload resolution:
        // - Only foo(List<String>) is applicable
        // - foo(List<Object>) is not applicable (generics are invariant)
        // - Should select foo(List<String>)
    }

    #[test]
    fn test_overload_boxing_conversion() {
        // Given: void foo(int x) and void foo(Integer x)
        // When: foo(42)
        // Then: Should prefer foo(int) over boxing conversion

        let int_type = Type::Primitive(PrimitiveType::Int);
        let boxed_int = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        // Both are applicable (int can be boxed to Integer)
        // but unboxed version is preferred

        // Boxing conversion check
        assert_eq!(int_type.box_type(), boxed_int);

        // For overload resolution:
        // - Both methods are applicable
        // - foo(int) is more specific (no conversion needed)
        // - Should select foo(int)
    }

    #[test]
    fn test_overload_null_literal() {
        // Given: void foo(String s) and void foo(Integer i)
        // When: foo(null)
        // Then: Ambiguous - should fail (both applicable, neither more specific)

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let null_type = Type::Null;

        // null is assignable to any reference type
        assert!(string_type.is_assignable_from(&null_type));
        assert!(integer_type.is_assignable_from(&null_type));

        // Neither String nor Integer is more specific than the other
        assert!(!string_type.is_assignable_from(&integer_type));
        assert!(!integer_type.is_assignable_from(&string_type));

        // For overload resolution:
        // - Both methods are applicable
        // - Neither is more specific
        // - Should fail with ambiguous method call error
    }

    #[test]
    fn test_overload_primitive_widening() {
        // Given: void foo(long x) and void foo(double x)
        // When: foo(42)
        // Then: Should resolve to foo(long) (first applicable)

        let int_literal = Type::Primitive(PrimitiveType::Int);
        let long_type = Type::Primitive(PrimitiveType::Long);
        let double_type = Type::Primitive(PrimitiveType::Double);

        // int can widen to both long and double
        assert!(long_type.is_assignable_from(&int_literal));
        assert!(double_type.is_assignable_from(&int_literal));

        // For overload resolution:
        // - Both methods are applicable
        // - Neither is more specific (unrelated types)
        // - Java chooses by widening preference: int -> long -> float -> double
        // - Should select foo(long)
    }

    #[test]
    fn test_overload_with_inheritance() {
        // Given: class Child extends Parent
        // And: void foo(Parent p) and void foo(Child c)
        // When: foo(new Child())
        // Then: Should resolve to foo(Child) as more specific

        let _parent_type = Type::Class(ClassType {
            name: "Parent".into(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        });

        let _child_type = Type::Class(ClassType {
            name: "Child".into(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        });

        // This would require actual inheritance tracking
        // For now, document the expected behavior:
        // - Both methods should be applicable if Child <: Parent
        // - foo(Child) should be more specific
        // - Should select foo(Child)
    }

    #[test]
    fn test_overload_wildcard_capture() {
        // Given: void foo(List<?> list) and void foo(List<String> list)
        // When: foo(new ArrayList<String>())
        // Then: Should resolve to foo(List<String>) as more specific

        let string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            is_diamond: false,
        });

        let _list_string = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![string_type.clone()],
            is_diamond: false,
        });

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            is_diamond: false,
        });

        // List<String> is assignable to List<?> (wildcard subsumption)
        // but not vice versa

        // For overload resolution:
        // - Both methods are applicable
        // - foo(List<String>) is more specific
        // - Should select foo(List<String>)
    }
}
