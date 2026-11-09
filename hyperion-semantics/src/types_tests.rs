// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for the types module
//!
//! These tests cover type extension functionality, boxing/unboxing, numeric promotion,
//! and expression type inference utilities.

#[cfg(test)]
mod types_comprehensive_tests {
    use crate::symbol::{ClassType, PrimitiveType, Type};
    use crate::types::{expr_types, TypeExt};

    // ========== Type Extension Tests ==========

    #[test]
    fn test_assignability_with_unknown() {
        let unknown = Type::Unknown;
        let int_type = Type::Primitive(PrimitiveType::Int);
        let string_type = Type::Class(ClassType::string());

        // Unknown is compatible with everything (for error recovery)
        assert!(unknown.is_assignable_from(&int_type));
        assert!(unknown.is_assignable_from(&string_type));
        assert!(int_type.is_assignable_from(&unknown));
        assert!(string_type.is_assignable_from(&unknown));
    }

    #[test]
    fn test_primitive_widening_conversions() {
        use PrimitiveType::*;

        // byte -> short, int, long, float, double
        let byte_type = Type::Primitive(Byte);
        assert!(Type::Primitive(Short).is_assignable_from(&byte_type));
        assert!(Type::Primitive(Int).is_assignable_from(&byte_type));
        assert!(Type::Primitive(Long).is_assignable_from(&byte_type));
        assert!(Type::Primitive(Float).is_assignable_from(&byte_type));
        assert!(Type::Primitive(Double).is_assignable_from(&byte_type));

        // short -> int, long, float, double
        let short_type = Type::Primitive(Short);
        assert!(Type::Primitive(Int).is_assignable_from(&short_type));
        assert!(Type::Primitive(Long).is_assignable_from(&short_type));
        assert!(Type::Primitive(Float).is_assignable_from(&short_type));
        assert!(Type::Primitive(Double).is_assignable_from(&short_type));

        // char -> int, long, float, double
        let char_type = Type::Primitive(Char);
        assert!(Type::Primitive(Int).is_assignable_from(&char_type));
        assert!(Type::Primitive(Long).is_assignable_from(&char_type));
        assert!(Type::Primitive(Float).is_assignable_from(&char_type));
        assert!(Type::Primitive(Double).is_assignable_from(&char_type));

        // int -> long, float, double
        let int_type = Type::Primitive(Int);
        assert!(Type::Primitive(Long).is_assignable_from(&int_type));
        assert!(Type::Primitive(Float).is_assignable_from(&int_type));
        assert!(Type::Primitive(Double).is_assignable_from(&int_type));

        // long -> float, double
        let long_type = Type::Primitive(Long);
        assert!(Type::Primitive(Float).is_assignable_from(&long_type));
        assert!(Type::Primitive(Double).is_assignable_from(&long_type));

        // float -> double
        let float_type = Type::Primitive(Float);
        assert!(Type::Primitive(Double).is_assignable_from(&float_type));

        // No narrowing conversions
        assert!(!Type::Primitive(Byte).is_assignable_from(&short_type));
        assert!(!Type::Primitive(Int).is_assignable_from(&long_type));
        assert!(!Type::Primitive(Float).is_assignable_from(&Type::Primitive(Double)));
    }

    #[test]
    fn test_boxing_all_primitives() {
        use PrimitiveType::*;

        let test_cases = vec![
            (Boolean, "Boolean"),
            (Byte, "Byte"),
            (Short, "Short"),
            (Int, "Integer"),
            (Long, "Long"),
            (Char, "Character"),
            (Float, "Float"),
            (Double, "Double"),
        ];

        for (prim, wrapper_name) in test_cases {
            let prim_type = Type::Primitive(prim);
            let boxed = prim_type.box_type();

            match boxed {
                Type::Class(c) => {
                    assert_eq!(&*c.name, wrapper_name);
                    assert_eq!(c.package.as_deref(), Some("java.lang".into()));
                }
                _ => panic!("Expected class type for boxed {}", wrapper_name),
            }
        }
    }

    #[test]
    fn test_boxing_non_primitives() {
        // Boxing non-primitive types returns the same type
        let string_type = Type::Class(ClassType::string());
        let boxed = string_type.box_type();
        assert_eq!(boxed, string_type);

        let array_type = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        let boxed_array = array_type.box_type();
        assert_eq!(boxed_array, array_type);
    }

    #[test]
    fn test_unboxing_all_wrappers() {
        use PrimitiveType::*;

        let test_cases = vec![
            ("Boolean", Boolean),
            ("Byte", Byte),
            ("Short", Short),
            ("Integer", Int),
            ("Long", Long),
            ("Character", Char),
            ("Float", Float),
            ("Double", Double),
        ];

        for (wrapper_name, prim) in test_cases {
            let wrapper_type = Type::Class(ClassType::new(
                wrapper_name.into(),
                Some("java.lang".into()),
            ));

            let unboxed = wrapper_type.unbox_type();
            assert_eq!(unboxed, Some(Type::Primitive(prim)));
        }
    }

    #[test]
    fn test_unboxing_non_wrappers() {
        // Non-wrapper classes return None
        let string_type = Type::Class(ClassType::string());
        assert_eq!(string_type.unbox_type(), None);

        let custom_class =
            Type::Class(ClassType::new("MyClass".into(), Some("com.example".into())));
        assert_eq!(custom_class.unbox_type(), None);

        // Primitives return None (can't unbox primitives)
        let int_type = Type::Primitive(PrimitiveType::Int);
        assert_eq!(int_type.unbox_type(), None);
    }

    #[test]
    fn test_boxing_unboxing_round_trip() {
        use PrimitiveType::*;

        for prim in [Boolean, Byte, Short, Int, Long, Char, Float, Double] {
            let prim_type = Type::Primitive(prim);
            let boxed = prim_type.box_type();
            let unboxed = boxed.unbox_type();

            assert_eq!(unboxed, Some(prim_type.clone()));
        }
    }

    #[test]
    fn test_numeric_promotion_rules() {
        use PrimitiveType::*;

        // Double beats everything
        assert_eq!(
            Type::Primitive(Double).promote_with(&Type::Primitive(Int)),
            Type::Primitive(Double)
        );
        assert_eq!(
            Type::Primitive(Int).promote_with(&Type::Primitive(Double)),
            Type::Primitive(Double)
        );
        assert_eq!(
            Type::Primitive(Float).promote_with(&Type::Primitive(Double)),
            Type::Primitive(Double)
        );

        // Float beats everything except double
        assert_eq!(
            Type::Primitive(Float).promote_with(&Type::Primitive(Long)),
            Type::Primitive(Float)
        );
        assert_eq!(
            Type::Primitive(Int).promote_with(&Type::Primitive(Float)),
            Type::Primitive(Float)
        );

        // Long beats int and smaller
        assert_eq!(
            Type::Primitive(Long).promote_with(&Type::Primitive(Int)),
            Type::Primitive(Long)
        );
        assert_eq!(
            Type::Primitive(Byte).promote_with(&Type::Primitive(Long)),
            Type::Primitive(Long)
        );

        // Everything else promotes to int
        assert_eq!(
            Type::Primitive(Byte).promote_with(&Type::Primitive(Short)),
            Type::Primitive(Int)
        );
        assert_eq!(
            Type::Primitive(Byte).promote_with(&Type::Primitive(Byte)),
            Type::Primitive(Int)
        );
        assert_eq!(
            Type::Primitive(Short).promote_with(&Type::Primitive(Char)),
            Type::Primitive(Int)
        );
    }

    #[test]
    fn test_promotion_with_non_primitives() {
        let string_type = Type::Class(ClassType::string());
        let int_type = Type::Primitive(PrimitiveType::Int);

        // Promoting non-primitives returns Unknown
        assert_eq!(string_type.promote_with(&int_type), Type::Unknown);
        assert_eq!(int_type.promote_with(&string_type), Type::Unknown);

        let array_type = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        assert_eq!(array_type.promote_with(&int_type), Type::Unknown);
    }

    #[test]
    fn test_reference_type_predicate() {
        // Reference types
        assert!(Type::Class(ClassType::object()).is_reference_type());
        assert!(Type::Class(ClassType::string()).is_reference_type());
        assert!(Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        }
        .is_reference_type());
        assert!(Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        }
        .is_reference_type());
        assert!(Type::Null.is_reference_type());

        // Non-reference types
        assert!(!Type::Primitive(PrimitiveType::Int).is_reference_type());
        assert!(!Type::Primitive(PrimitiveType::Boolean).is_reference_type());
        assert!(!Type::Void.is_reference_type());
        assert!(!Type::Unknown.is_reference_type());
    }

    #[test]
    fn test_primitive_type_predicate() {
        // Primitive types
        assert!(Type::Primitive(PrimitiveType::Int).is_primitive_type());
        assert!(Type::Primitive(PrimitiveType::Boolean).is_primitive_type());
        assert!(Type::Primitive(PrimitiveType::Double).is_primitive_type());

        // Non-primitive types
        assert!(!Type::Class(ClassType::object()).is_primitive_type());
        assert!(!Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        }
        .is_primitive_type());
        assert!(!Type::Void.is_primitive_type());
        assert!(!Type::Null.is_primitive_type());
    }

    #[test]
    fn test_class_assignability() {
        let object_type = Type::Class(ClassType::object());
        let string_type = Type::Class(ClassType::string());

        // Object is assignable from any class type
        assert!(object_type.is_assignable_from(&string_type));

        // Same types are assignable
        assert!(string_type.is_assignable_from(&string_type));

        // Different class types (without hierarchy info) are not assignable
        let custom_type = Type::Class(ClassType::new("MyClass".into(), Some("com.example".into())));
        assert!(!string_type.is_assignable_from(&custom_type));
    }

    #[test]
    fn test_array_assignability() {
        let object_type = Type::Class(ClassType::object());

        let int_array = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };

        let string_array = Type::Array {
            element_type: Box::new(Type::Class(ClassType::string())),
            dimensions: 1,
        };

        // Object is assignable from any array
        assert!(object_type.is_assignable_from(&int_array));
        assert!(object_type.is_assignable_from(&string_array));

        // Same array types are assignable
        assert!(int_array.is_assignable_from(&int_array));

        // Arrays with same dimensions and compatible element types
        let int_array2 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        assert!(int_array.is_assignable_from(&int_array2));

        // Different dimensions are not assignable
        let int_array_2d = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 2,
        };
        assert!(!int_array.is_assignable_from(&int_array_2d));

        // Different element types are not assignable
        assert!(!int_array.is_assignable_from(&string_array));
    }

    #[test]
    fn test_null_assignability() {
        let null_type = Type::Null;

        // Null is assignable to all reference types
        assert!(Type::Class(ClassType::string()).is_assignable_from(&null_type));
        assert!(Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        }
        .is_assignable_from(&null_type));
        assert!(Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        }
        .is_assignable_from(&null_type));

        // Null is NOT assignable to primitive types
        assert!(!Type::Primitive(PrimitiveType::Int).is_assignable_from(&null_type));
        assert!(!Type::Primitive(PrimitiveType::Boolean).is_assignable_from(&null_type));
        assert!(!Type::Void.is_assignable_from(&null_type));
    }

    #[test]
    fn test_type_variable_assignability() {
        let type_var1 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType::object())],
        };

        let type_var2 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType::object())],
        };

        let type_var3 = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![Type::Class(ClassType::object())],
        };

        // Same name type variables are assignable
        assert!(type_var1.is_assignable_from(&type_var2));

        // Different name type variables are not assignable
        assert!(!type_var1.is_assignable_from(&type_var3));
    }

    #[test]
    fn test_wildcard_assignability() {
        let wildcard_extends_number = Type::Wildcard {
            upper_bound: Some(Box::new(Type::Class(ClassType::new(
                "Number".into(),
                Some("java.lang".into()),
            )))),
            lower_bound: None,
        };

        let object_type = Type::Class(ClassType::object());
        let number_type = Type::Class(ClassType::new("Number".into(), Some("java.lang".into())));

        // Wildcard with upper bound is assignable from types that are subtypes of the bound
        assert!(wildcard_extends_number.is_assignable_from(&number_type));

        // Object is NOT a subtype of Number, so wildcard should NOT accept it
        assert!(!wildcard_extends_number.is_assignable_from(&object_type));
    }

    // ========== Expression Type Tests ==========

    #[test]
    fn test_binary_arithmetic_numeric() {
        use PrimitiveType::*;

        let int_type = Type::Primitive(Int);
        let double_type = Type::Primitive(Double);
        let byte_type = Type::Primitive(Byte);

        // int + int = int
        let result = expr_types::binary_arithmetic_type(&int_type, &int_type);
        assert_eq!(result, Type::Primitive(Int));

        // int + double = double
        let result = expr_types::binary_arithmetic_type(&int_type, &double_type);
        assert_eq!(result, Type::Primitive(Double));

        // byte + byte = int (promotion)
        let result = expr_types::binary_arithmetic_type(&byte_type, &byte_type);
        assert_eq!(result, Type::Primitive(Int));
    }

    #[test]
    fn test_binary_arithmetic_string_concatenation() {
        let string_type = Type::Class(ClassType::string());
        let int_type = Type::Primitive(PrimitiveType::Int);
        let double_type = Type::Primitive(PrimitiveType::Double);

        // String + anything = String
        let result = expr_types::binary_arithmetic_type(&string_type, &int_type);
        assert!(matches!(result, Type::Class(c) if c.qualified_name() == "java.lang.String"));

        let result = expr_types::binary_arithmetic_type(&int_type, &string_type);
        assert!(matches!(result, Type::Class(c) if c.qualified_name() == "java.lang.String"));

        let result = expr_types::binary_arithmetic_type(&string_type, &double_type);
        assert!(matches!(result, Type::Class(c) if c.qualified_name() == "java.lang.String"));

        let result = expr_types::binary_arithmetic_type(&string_type, &string_type);
        assert!(matches!(result, Type::Class(c) if c.qualified_name() == "java.lang.String"));
    }

    #[test]
    fn test_comparison_type() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        let double_type = Type::Primitive(PrimitiveType::Double);
        let string_type = Type::Class(ClassType::string());

        // All comparisons return boolean
        assert_eq!(
            expr_types::comparison_type(&int_type, &int_type),
            Type::Primitive(PrimitiveType::Boolean)
        );
        assert_eq!(
            expr_types::comparison_type(&int_type, &double_type),
            Type::Primitive(PrimitiveType::Boolean)
        );
        assert_eq!(
            expr_types::comparison_type(&string_type, &string_type),
            Type::Primitive(PrimitiveType::Boolean)
        );
    }

    #[test]
    fn test_logical_type() {
        let bool_type = Type::Primitive(PrimitiveType::Boolean);
        let int_type = Type::Primitive(PrimitiveType::Int);

        // boolean && boolean = boolean
        assert_eq!(
            expr_types::logical_type(&bool_type, &bool_type),
            Type::Primitive(PrimitiveType::Boolean)
        );

        // boolean && int = Unknown (invalid)
        assert_eq!(
            expr_types::logical_type(&bool_type, &int_type),
            Type::Unknown
        );

        // int && int = Unknown (invalid)
        assert_eq!(
            expr_types::logical_type(&int_type, &int_type),
            Type::Unknown
        );
    }

    #[test]
    fn test_bitwise_type() {
        use PrimitiveType::*;

        let int_type = Type::Primitive(Int);
        let long_type = Type::Primitive(Long);
        let byte_type = Type::Primitive(Byte);
        let bool_type = Type::Primitive(Boolean);

        // int & int = int
        assert_eq!(
            expr_types::bitwise_type(&int_type, &int_type),
            Type::Primitive(Int)
        );

        // int & long = long
        assert_eq!(
            expr_types::bitwise_type(&int_type, &long_type),
            Type::Primitive(Long)
        );

        // byte & byte = int (promotion)
        assert_eq!(
            expr_types::bitwise_type(&byte_type, &byte_type),
            Type::Primitive(Int)
        );

        // boolean & anything = Unknown (bitwise not valid for boolean)
        assert_eq!(
            expr_types::bitwise_type(&bool_type, &int_type),
            Type::Unknown
        );

        // float & int = Unknown (bitwise not valid for floats)
        let float_type = Type::Primitive(Float);
        assert_eq!(
            expr_types::bitwise_type(&float_type, &int_type),
            Type::Unknown
        );
    }

    #[test]
    fn test_complex_expression_types() {
        // (int + int) -> int
        let int_type = Type::Primitive(PrimitiveType::Int);
        let result1 = expr_types::binary_arithmetic_type(&int_type, &int_type);
        assert_eq!(result1, Type::Primitive(PrimitiveType::Int));

        // (int + double) -> double
        let double_type = Type::Primitive(PrimitiveType::Double);
        let result2 = expr_types::binary_arithmetic_type(&int_type, &double_type);
        assert_eq!(result2, Type::Primitive(PrimitiveType::Double));

        // (String + int) -> String
        let string_type = Type::Class(ClassType::string());
        let result3 = expr_types::binary_arithmetic_type(&string_type, &int_type);
        assert!(matches!(result3, Type::Class(c) if c.qualified_name() == "java.lang.String"));
    }

    #[test]
    fn test_edge_case_unknown_types() {
        let unknown = Type::Unknown;
        let int_type = Type::Primitive(PrimitiveType::Int);

        // Operations with Unknown should handle gracefully
        let _result = expr_types::binary_arithmetic_type(&unknown, &int_type);
        // Should return some result (implementation-specific)

        let result = expr_types::comparison_type(&unknown, &int_type);
        assert_eq!(result, Type::Primitive(PrimitiveType::Boolean));

        let _result = expr_types::logical_type(&unknown, &unknown);
        // Should handle Unknown gracefully
    }

    #[test]
    fn test_void_type_operations() {
        let void_type = Type::Void;
        let int_type = Type::Primitive(PrimitiveType::Int);

        // Void should not be assignable or have operations
        assert!(!void_type.is_reference_type());
        assert!(!void_type.is_primitive_type());
        assert!(!void_type.is_assignable_from(&int_type));
        assert!(!int_type.is_assignable_from(&void_type));
    }

    #[test]
    fn test_all_primitive_type_display() {
        use PrimitiveType::*;

        assert_eq!(format!("{}", Boolean), "boolean");
        assert_eq!(format!("{}", Byte), "byte");
        assert_eq!(format!("{}", Short), "short");
        assert_eq!(format!("{}", Int), "int");
        assert_eq!(format!("{}", Long), "long");
        assert_eq!(format!("{}", Char), "char");
        assert_eq!(format!("{}", Float), "float");
        assert_eq!(format!("{}", Double), "double");
    }
}
