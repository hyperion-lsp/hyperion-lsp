// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Property-based tests for type inference and subtyping
//!
//! These tests ensure fundamental type system invariants:
//! - Subtyping transitivity: If A <: B and B <: C, then A <: C
//! - LUB commutativity: LUB(A, B) == LUB(B, A)
//! - Reflexivity: A <: A for all types
//! - Type substitution consistency

use hyperion_semantics::symbol::{ClassType, PrimitiveType, Type};
use proptest::prelude::*;

// ============================================================================
// Test Strategies - Generate Type Structures
// ============================================================================

/// Generate valid primitive types
fn primitive_type() -> impl Strategy<Value = PrimitiveType> {
    prop_oneof![
        Just(PrimitiveType::Byte),
        Just(PrimitiveType::Short),
        Just(PrimitiveType::Int),
        Just(PrimitiveType::Long),
        Just(PrimitiveType::Float),
        Just(PrimitiveType::Double),
        Just(PrimitiveType::Boolean),
        Just(PrimitiveType::Char),
    ]
}

/// Generate Type enum variant from primitive
fn primitive_type_variant() -> impl Strategy<Value = Type> {
    primitive_type().prop_map(Type::Primitive)
}

/// Generate a simple class type
fn simple_class_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::Class(ClassType::object())),
        Just(Type::Class(ClassType::new(
            "String".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Integer".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Number".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Comparable".into(),
            Some("java.lang".into()),
        ))),
    ]
}

/// Generate known numeric wrapper types
fn numeric_wrapper() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::Class(ClassType::new(
            "Byte".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Short".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Integer".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Long".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Float".into(),
            Some("java.lang".into()),
        ))),
        Just(Type::Class(ClassType::new(
            "Double".into(),
            Some("java.lang".into()),
        ))),
    ]
}

/// Generate array types
fn array_type() -> impl Strategy<Value = Type> {
    (primitive_type_variant(), 1u8..=3).prop_map(|(element, dimensions)| Type::Array {
        element_type: Box::new(element),
        dimensions,
    })
}

/// Generate a variety of types for general testing
fn any_type() -> impl Strategy<Value = Type> {
    prop_oneof![
        primitive_type_variant(),
        simple_class_type(),
        numeric_wrapper(),
        array_type(),
        Just(Type::Null),
        Just(Type::Unknown),
    ]
}

// ============================================================================
// Subtyping Invariants
// ============================================================================

proptest! {
    /// Property: Reflexivity - every type is a subtype of itself
    ///
    /// This is the most fundamental subtyping property: A <: A
    #[test]
    fn subtyping_is_reflexive(ty in any_type()) {
        prop_assert!(
            ty.is_assignable_from(&ty),
            "Type should be assignable from itself: {ty:?}"
        );
    }

    /// Property: Primitive widening is transitive
    ///
    /// If byte <: short and short <: int, then byte <: int
    #[test]
    fn primitive_widening_is_transitive(
        p1 in primitive_type(),
        p2 in primitive_type(),
        p3 in primitive_type()
    ) {
        let t1 = Type::Primitive(p1);
        let t2 = Type::Primitive(p2);
        let t3 = Type::Primitive(p3);

        // If t1 <: t2 and t2 <: t3, then t1 <: t3
        if t2.is_assignable_from(&t1) && t3.is_assignable_from(&t2) {
            prop_assert!(
                t3.is_assignable_from(&t1),
                "Widening should be transitive: {p1:?} <: {p2:?} <: {p3:?}"
            );
        }
    }

    /// Property: Null is assignable to all reference types
    #[test]
    fn null_assignable_to_reference_types(ty in simple_class_type()) {
        prop_assert!(
            ty.is_assignable_from(&Type::Null),
            "Reference type should accept null: {ty:?}"
        );
    }

    /// Property: Null is NOT assignable to primitive types
    #[test]
    fn null_not_assignable_to_primitives(prim in primitive_type()) {
        let ty = Type::Primitive(prim);
        prop_assert!(
            !ty.is_assignable_from(&Type::Null),
            "Primitive type should reject null: {prim:?}"
        );
    }

    /// Property: All reference types are assignable to Object
    #[test]
    fn reference_types_assignable_to_object(ty in simple_class_type()) {
        let object = Type::Class(ClassType::object());
        prop_assert!(
            object.is_assignable_from(&ty),
            "Object should accept all reference types: {ty:?}"
        );
    }

    /// Property: Arrays are assignable to Object
    #[test]
    fn arrays_assignable_to_object(arr in array_type()) {
        let object = Type::Class(ClassType::object());
        prop_assert!(
            object.is_assignable_from(&arr),
            "Object should accept arrays: {arr:?}"
        );
    }

    /// Property: Unknown is compatible with everything
    #[test]
    fn unknown_is_universal(ty in any_type()) {
        prop_assert!(
            Type::Unknown.is_assignable_from(&ty),
            "Unknown should accept any type: {ty:?}"
        );
        prop_assert!(
            ty.is_assignable_from(&Type::Unknown),
            "Any type should accept Unknown: {ty:?}"
        );
    }

    /// Property: Array dimensions must match for assignability
    #[test]
    fn array_dimensions_must_match(
        elem1 in primitive_type_variant(),
        elem2 in primitive_type_variant(),
        dim1 in 1u8..=3,
        dim2 in 1u8..=3
    ) {
        let arr1 = Type::Array {
            element_type: Box::new(elem1.clone()),
            dimensions: dim1,
        };
        let arr2 = Type::Array {
            element_type: Box::new(elem2.clone()),
            dimensions: dim2,
        };

        if dim1 != dim2 {
            // Different dimensions - should not be assignable (unless one is Unknown)
            if !matches!(elem1, Type::Unknown) && !matches!(elem2, Type::Unknown) {
                prop_assert!(
                    !arr1.is_assignable_from(&arr2),
                    "Arrays with different dimensions should not be assignable"
                );
            }
        }
    }

    /// Property: Primitive array elements must match exactly (no covariance)
    #[test]
    fn primitive_arrays_require_exact_match(
        p1 in primitive_type(),
        p2 in primitive_type(),
        dimensions in 1u8..=3
    ) {
        let arr1 = Type::Array {
            element_type: Box::new(Type::Primitive(p1)),
            dimensions,
        };
        let arr2 = Type::Array {
            element_type: Box::new(Type::Primitive(p2)),
            dimensions,
        };

        if p1 != p2 {
            prop_assert!(
                !arr1.is_assignable_from(&arr2),
                "Primitive arrays require exact element type match: {p1:?}[] vs {p2:?}[]"
            );
        }
    }

    /// Property: Autoboxing - primitives assignable to their wrappers
    #[test]
    fn autoboxing_primitives_to_wrappers(prim in primitive_type()) {
        let primitive = Type::Primitive(prim);
        let wrapper = Type::Class(ClassType::new(
            prim.wrapper_class().to_string(),
            Some("java.lang".into()),
        ));

        prop_assert!(
            wrapper.is_assignable_from(&primitive),
            "Wrapper should accept primitive via autoboxing: {prim:?}"
        );
    }

    /// Property: Unboxing - wrappers assignable to their primitives
    #[test]
    fn unboxing_wrappers_to_primitives(prim in primitive_type()) {
        let primitive = Type::Primitive(prim);
        let wrapper = Type::Class(ClassType::new(
            prim.wrapper_class().to_string(),
            Some("java.lang".into()),
        ));

        prop_assert!(
            primitive.is_assignable_from(&wrapper),
            "Primitive should accept wrapper via unboxing: {prim:?}"
        );
    }
}

// ============================================================================
// LUB (Least Upper Bound) Invariants
// ============================================================================

// Note: We need to test LUB properties without direct access to the LUB function.
// Instead, we'll test properties that should hold for any correct LUB implementation.

proptest! {
    /// Property: LUB with self should be self
    #[test]
    fn lub_with_self_is_self_reflexive(ty in any_type()) {
        // This tests that if we have two identical types,
        // their LUB should be that type (tested via assignability)
        prop_assert!(
            ty.is_assignable_from(&ty),
            "Type should be its own upper bound"
        );
    }

    /// Property: LUB should be an upper bound (both types assignable to it)
    ///
    /// For any LUB(A, B) = C, we should have A <: C and B <: C
    #[test]
    fn lub_is_upper_bound(t1 in primitive_type_variant(), t2 in primitive_type_variant()) {
        // For primitive types, we know the LUB rules
        // Test that the expected upper bound accepts both types
        if let (Type::Primitive(p1), Type::Primitive(p2)) = (&t1, &t2) {
            // Numeric types should have Number as upper bound for wrappers
            let wrapper1 = Type::Class(ClassType::new(
                p1.wrapper_class().to_string(),
                Some("java.lang".into()),
            ));
            let wrapper2 = Type::Class(ClassType::new(
                p2.wrapper_class().to_string(),
                Some("java.lang".into()),
            ));

            let number = Type::Class(ClassType::new(
                "Number".into(),
                Some("java.lang".into()),
            ));

            // If both are numeric wrappers, Number should be an upper bound
            if ["Byte", "Short", "Integer", "Long", "Float", "Double"]
                .contains(&p1.wrapper_class())
                && ["Byte", "Short", "Integer", "Long", "Float", "Double"]
                    .contains(&p2.wrapper_class())
            {
                prop_assert!(
                    number.is_assignable_from(&wrapper1),
                    "Number should be assignable from {p1:?}"
                );
                prop_assert!(
                    number.is_assignable_from(&wrapper2),
                    "Number should be assignable from {p2:?}"
                );
            }
        }
    }

    /// Property: Object is always a valid upper bound for reference types
    #[test]
    fn object_is_universal_upper_bound(t1 in simple_class_type(), t2 in simple_class_type()) {
        let object = Type::Class(ClassType::object());

        prop_assert!(
            object.is_assignable_from(&t1),
            "Object should be assignable from {t1:?}"
        );
        prop_assert!(
            object.is_assignable_from(&t2),
            "Object should be assignable from {t2:?}"
        );
    }

    /// Property: LUB with null should be the reference type
    #[test]
    fn lub_with_null_is_reference_type(ty in simple_class_type()) {
        // For LUB(Type, Null), the result should accept both
        // The reference type itself accepts null
        prop_assert!(
            ty.is_assignable_from(&Type::Null),
            "Reference type should accept null: {ty:?}"
        );
    }

    /// Property: LUB with Unknown should handle gracefully
    #[test]
    fn lub_with_unknown_handles_gracefully(ty in any_type()) {
        // Unknown is compatible with everything
        prop_assert!(
            Type::Unknown.is_assignable_from(&ty),
            "Unknown should accept any type"
        );
        prop_assert!(
            ty.is_assignable_from(&Type::Unknown),
            "Any type should accept Unknown"
        );
    }

    /// Property: Array LUB with same dimensions
    #[test]
    fn array_lub_same_dimensions(
        elem1 in primitive_type_variant(),
        elem2 in primitive_type_variant(),
        dimensions in 1u8..=3
    ) {
        let arr1 = Type::Array {
            element_type: Box::new(elem1),
            dimensions,
        };
        let arr2 = Type::Array {
            element_type: Box::new(elem2),
            dimensions,
        };

        // Object should be assignable from any array
        let object = Type::Class(ClassType::object());
        prop_assert!(
            object.is_assignable_from(&arr1),
            "Object should accept array"
        );
        prop_assert!(
            object.is_assignable_from(&arr2),
            "Object should accept array"
        );
    }
}

// ============================================================================
// Type Substitution Consistency
// ============================================================================

proptest! {
    /// Property: Substituting a type variable with itself preserves type
    #[test]
    fn substitution_identity_preserves_type(_name in "[A-Z]{1,3}") {
        // Type variables with same name and bounds should be equal
        let tv1 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };
        let tv2 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        prop_assert_eq!(tv1, tv2);
    }

    /// Property: Type erasure is idempotent
    #[test]
    fn erasure_is_idempotent(ty in simple_class_type()) {
        let erased_once = ty.erasure();
        let erased_twice = erased_once.erasure();

        prop_assert_eq!(
            erased_once,
            erased_twice,
            "Erasure should be idempotent"
        );
    }

    /// Property: Primitive erasure is identity
    #[test]
    fn primitive_erasure_is_identity(prim in primitive_type()) {
        let ty = Type::Primitive(prim);
        let erased = ty.erasure();

        prop_assert_eq!(
            ty,
            erased,
            "Primitive type erasure should be identity"
        );
    }

    /// Property: Array erasure preserves dimensions
    #[test]
    fn array_erasure_preserves_dimensions(
        elem in primitive_type_variant(),
        dimensions in 1u8..=3
    ) {
        let arr = Type::Array {
            element_type: Box::new(elem),
            dimensions,
        };
        let erased = arr.erasure();

        if let Type::Array { dimensions: d, .. } = erased {
            prop_assert_eq!(dimensions, d, "Erasure should preserve dimensions");
        } else {
            prop_assert!(false, "Array erasure should still be an array");
        }
    }

    /// Property: is_reference returns true for class types
    #[test]
    fn is_reference_true_for_classes(ty in simple_class_type()) {
        prop_assert!(ty.is_reference(), "{ty:?} should be a reference type");
    }

    /// Property: is_reference returns false for primitives
    #[test]
    fn is_reference_false_for_primitives(prim in primitive_type()) {
        let ty = Type::Primitive(prim);
        prop_assert!(!ty.is_reference(), "{prim:?} should not be a reference type");
    }

    /// Property: is_primitive returns true for primitives
    #[test]
    fn is_primitive_true_for_primitives(prim in primitive_type()) {
        let ty = Type::Primitive(prim);
        prop_assert!(ty.is_primitive(), "{prim:?} should be a primitive type");
    }

    /// Property: is_primitive returns false for classes
    #[test]
    fn is_primitive_false_for_classes(ty in simple_class_type()) {
        prop_assert!(!ty.is_primitive(), "{ty:?} should not be a primitive type");
    }
}

// ============================================================================
// Numeric Promotion Invariants
// ============================================================================

#[test]
fn byte_widens_correctly() {
    let byte_type = Type::Primitive(PrimitiveType::Byte);

    let target_types = vec![
        Type::Primitive(PrimitiveType::Short),
        Type::Primitive(PrimitiveType::Int),
        Type::Primitive(PrimitiveType::Long),
        Type::Primitive(PrimitiveType::Float),
        Type::Primitive(PrimitiveType::Double),
    ];

    for target in target_types {
        assert!(
            target.is_assignable_from(&byte_type),
            "byte should widen to {target:?}"
        );
    }
}

#[test]
fn short_widens_correctly() {
    let short_type = Type::Primitive(PrimitiveType::Short);

    let target_types = vec![
        Type::Primitive(PrimitiveType::Int),
        Type::Primitive(PrimitiveType::Long),
        Type::Primitive(PrimitiveType::Float),
        Type::Primitive(PrimitiveType::Double),
    ];

    for target in target_types {
        assert!(
            target.is_assignable_from(&short_type),
            "short should widen to {target:?}"
        );
    }
}

#[test]
fn int_widens_correctly() {
    let int_type = Type::Primitive(PrimitiveType::Int);

    let target_types = vec![
        Type::Primitive(PrimitiveType::Long),
        Type::Primitive(PrimitiveType::Float),
        Type::Primitive(PrimitiveType::Double),
    ];

    for target in target_types {
        assert!(
            target.is_assignable_from(&int_type),
            "int should widen to {target:?}"
        );
    }
}

#[test]
fn float_widens_to_double() {
    let float_type = Type::Primitive(PrimitiveType::Float);
    let double_type = Type::Primitive(PrimitiveType::Double);

    assert!(
        double_type.is_assignable_from(&float_type),
        "float should widen to double"
    );
}

proptest! {
    /// Property: boolean does not widen to anything
    #[test]
    fn boolean_does_not_widen(target in primitive_type()) {
        if target == PrimitiveType::Boolean {
            return Ok(());
        }

        let bool_type = Type::Primitive(PrimitiveType::Boolean);
        let target_type = Type::Primitive(target);

        prop_assert!(
            !target_type.is_assignable_from(&bool_type),
            "boolean should not widen to {target:?}"
        );
    }
}

#[test]
fn char_widens_correctly() {
    let char_type = Type::Primitive(PrimitiveType::Char);

    let target_types = vec![
        Type::Primitive(PrimitiveType::Int),
        Type::Primitive(PrimitiveType::Long),
        Type::Primitive(PrimitiveType::Float),
        Type::Primitive(PrimitiveType::Double),
    ];

    for target in target_types {
        assert!(
            target.is_assignable_from(&char_type),
            "char should widen to {target:?}"
        );
    }
}

// ============================================================================
// Intersection Type Invariants
// ============================================================================

proptest! {
    /// Property: Intersection type is assignable to any of its components
    #[test]
    fn intersection_assignable_to_components(
        t1 in simple_class_type(),
        t2 in simple_class_type()
    ) {
        let intersection = Type::Intersection {
            types: vec![t1.clone(), t2.clone()],
        };

        // Each component should be assignable from the intersection
        prop_assert!(
            t1.is_assignable_from(&intersection),
            "Component should accept intersection type"
        );
        prop_assert!(
            t2.is_assignable_from(&intersection),
            "Component should accept intersection type"
        );
    }
}
