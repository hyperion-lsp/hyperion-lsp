// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test utilities and factory methods for creating Type variants
//!
//! Provides convenient factory functions for creating common type instances
//! in tests, reducing boilerplate and making tests more readable.

use crate::symbol::{ClassType, PrimitiveType, Type, TypeParameter};

/// Factory methods for creating ClassType instances
pub mod class_types {
    use super::*;

    /// Create java.lang.Object type
    pub fn object() -> ClassType {
        ClassType::object()
    }

    /// Create java.lang.String type
    pub fn string() -> ClassType {
        ClassType::string()
    }

    /// Create a simple class type with just a name (no package)
    pub fn simple(name: impl Into<String>) -> ClassType {
        ClassType::new(name.into(), None)
    }

    /// Create a qualified class type with package
    pub fn qualified(name: impl Into<String>, package: impl Into<String>) -> ClassType {
        ClassType::new(name.into(), Some(package.into()))
    }

    /// Create java.util.List type (non-generic)
    pub fn list() -> ClassType {
        ClassType::new("List".into(), Some("java.util".into()))
    }

    /// Create java.util.ArrayList type (non-generic)
    pub fn array_list() -> ClassType {
        ClassType::new("ArrayList".into(), Some("java.util".into()))
    }

    /// Create java.util.Map type (non-generic)
    pub fn map() -> ClassType {
        ClassType::new("Map".into(), Some("java.util".into()))
    }

    /// Create java.util.HashMap type (non-generic)
    pub fn hash_map() -> ClassType {
        ClassType::new("HashMap".into(), Some("java.util".into()))
    }

    /// Create java.util.Set type (non-generic)
    pub fn set() -> ClassType {
        ClassType::new("Set".into(), Some("java.util".into()))
    }

    /// Create java.lang.Number type
    pub fn number() -> ClassType {
        ClassType::new("Number".into(), Some("java.lang".into()))
    }

    /// Create java.lang.Integer type (wrapper)
    pub fn integer() -> ClassType {
        ClassType::new("Integer".into(), Some("java.lang".into()))
    }

    /// Create java.lang.Comparable type
    pub fn comparable() -> ClassType {
        ClassType::new("Comparable".into(), Some("java.lang".into()))
    }

    /// Create java.io.Serializable type
    pub fn serializable() -> ClassType {
        ClassType::new("Serializable".into(), Some("java.io".into()))
    }
}

/// Factory methods for creating primitive Type instances
pub mod primitive_types {
    use super::*;

    pub fn int() -> Type {
        Type::Primitive(PrimitiveType::Int)
    }

    pub fn long() -> Type {
        Type::Primitive(PrimitiveType::Long)
    }

    pub fn short() -> Type {
        Type::Primitive(PrimitiveType::Short)
    }

    pub fn byte() -> Type {
        Type::Primitive(PrimitiveType::Byte)
    }

    pub fn char() -> Type {
        Type::Primitive(PrimitiveType::Char)
    }

    pub fn float() -> Type {
        Type::Primitive(PrimitiveType::Float)
    }

    pub fn double() -> Type {
        Type::Primitive(PrimitiveType::Double)
    }

    pub fn boolean() -> Type {
        Type::Primitive(PrimitiveType::Boolean)
    }
}

/// Factory methods for creating reference Type instances
pub mod reference_types {
    use super::*;

    /// Create Object type
    pub fn object() -> Type {
        Type::Class(class_types::object())
    }

    /// Create String type
    pub fn string() -> Type {
        Type::Class(class_types::string())
    }

    /// Create a simple class type
    pub fn class(name: impl Into<String>) -> Type {
        Type::Class(class_types::simple(name))
    }

    /// Create a qualified class type
    pub fn qualified_class(name: impl Into<String>, package: impl Into<String>) -> Type {
        Type::Class(class_types::qualified(name, package))
    }

    /// Create void type
    pub fn void() -> Type {
        Type::Void
    }

    /// Create null type
    pub fn null() -> Type {
        Type::Null
    }

    /// Create unknown type
    pub fn unknown() -> Type {
        Type::Unknown
    }
}

/// Factory methods for creating generic types
pub mod generic_types {
    use super::*;

    /// Create List<T> where T is the element type
    pub fn list(element_type: Type) -> Type {
        let mut class = class_types::list();
        class.type_arguments = vec![element_type];
        Type::Class(class)
    }

    /// Create List<String>
    pub fn list_of_string() -> Type {
        list(reference_types::string())
    }

    /// Create List<Integer>
    pub fn list_of_integer() -> Type {
        list(Type::Class(class_types::integer()))
    }

    /// Create Map<K, V> where K is key type and V is value type
    pub fn map(key_type: Type, value_type: Type) -> Type {
        let mut class = class_types::map();
        class.type_arguments = vec![key_type, value_type];
        Type::Class(class)
    }

    /// Create Map<String, Object>
    pub fn map_string_to_object() -> Type {
        map(reference_types::string(), reference_types::object())
    }

    /// Create Set<T> where T is the element type
    pub fn set(element_type: Type) -> Type {
        let mut class = class_types::set();
        class.type_arguments = vec![element_type];
        Type::Class(class)
    }

    /// Create generic type with custom base ClassType and args
    pub fn custom(base: ClassType, args: Vec<Type>) -> Type {
        let mut class = base;
        class.type_arguments = args;
        Type::Class(class)
    }
}

/// Factory methods for creating array types
pub mod array_types {
    use super::*;

    /// Create int[] (1-dimensional int array)
    pub fn int_array() -> Type {
        Type::Array {
            element_type: Box::new(primitive_types::int()),
            dimensions: 1,
        }
    }

    /// Create String[] (1-dimensional String array)
    pub fn string_array() -> Type {
        Type::Array {
            element_type: Box::new(reference_types::string()),
            dimensions: 1,
        }
    }

    /// Create Object[] (1-dimensional Object array)
    pub fn object_array() -> Type {
        Type::Array {
            element_type: Box::new(reference_types::object()),
            dimensions: 1,
        }
    }

    /// Create array of given element type and dimensions
    pub fn of(element_type: Type, dimensions: u8) -> Type {
        Type::Array {
            element_type: Box::new(element_type),
            dimensions,
        }
    }

    /// Create 2-dimensional array (e.g., int[][])
    pub fn two_dimensional(element_type: Type) -> Type {
        of(element_type, 2)
    }

    /// Create int[][]
    pub fn int_2d() -> Type {
        two_dimensional(primitive_types::int())
    }

    /// Create String[][]
    pub fn string_2d() -> Type {
        two_dimensional(reference_types::string())
    }
}

/// Factory methods for creating wildcard types
pub mod wildcard_types {
    use super::*;

    /// Create unbounded wildcard (?)
    pub fn unbounded() -> Type {
        Type::Wildcard {
            upper_bound: None,
            lower_bound: None,
        }
    }

    /// Create upper-bounded wildcard (? extends T)
    pub fn extends(bound: Type) -> Type {
        Type::Wildcard {
            upper_bound: Some(Box::new(bound)),
            lower_bound: None,
        }
    }

    /// Create lower-bounded wildcard (? super T)
    pub fn super_bound(bound: Type) -> Type {
        Type::Wildcard {
            upper_bound: None,
            lower_bound: Some(Box::new(bound)),
        }
    }

    /// Create ? extends Object
    pub fn extends_object() -> Type {
        extends(reference_types::object())
    }

    /// Create ? extends Number
    pub fn extends_number() -> Type {
        extends(Type::Class(class_types::number()))
    }

    /// Create ? super String
    pub fn super_string() -> Type {
        super_bound(reference_types::string())
    }
}

/// Factory methods for creating type variables
pub mod type_variables {
    use super::*;

    /// Create unbounded type variable (e.g., T)
    pub fn unbounded(name: impl Into<String>) -> Type {
        Type::TypeVariable {
            name: name.into().into(),
            bounds: vec![],
        }
    }

    /// Create type variable with single bound (e.g., T extends Number)
    pub fn with_bound(name: impl Into<String>, bound: Type) -> Type {
        Type::TypeVariable {
            name: name.into().into(),
            bounds: vec![bound],
        }
    }

    /// Create type variable with multiple bounds (e.g., T extends Number & Comparable)
    pub fn with_bounds(name: impl Into<String>, bounds: Vec<Type>) -> Type {
        Type::TypeVariable {
            name: name.into().into(),
            bounds,
        }
    }

    /// Create T
    pub fn t() -> Type {
        unbounded("T")
    }

    /// Create E (commonly used for element types)
    pub fn e() -> Type {
        unbounded("E")
    }

    /// Create K (commonly used for key types)
    pub fn k() -> Type {
        unbounded("K")
    }

    /// Create V (commonly used for value types)
    pub fn v() -> Type {
        unbounded("V")
    }

    /// Create T extends Number
    pub fn t_extends_number() -> Type {
        with_bound("T", Type::Class(class_types::number()))
    }

    /// Create T extends Comparable<T>
    pub fn t_extends_comparable() -> Type {
        with_bound(
            "T",
            generic_types::custom(class_types::comparable(), vec![unbounded("T")]),
        )
    }
}

/// Factory methods for creating TypeParameter instances
pub mod type_parameters {
    use super::*;

    /// Create unbounded type parameter
    pub fn unbounded(name: impl Into<String>) -> TypeParameter {
        TypeParameter {
            name: name.into().into(),
            bounds: vec![],
        }
    }

    /// Create type parameter with single bound
    pub fn with_bound(name: impl Into<String>, bound: Type) -> TypeParameter {
        TypeParameter {
            name: name.into().into(),
            bounds: vec![bound],
        }
    }

    /// Create type parameter with multiple bounds
    pub fn with_bounds(name: impl Into<String>, bounds: Vec<Type>) -> TypeParameter {
        TypeParameter {
            name: name.into().into(),
            bounds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_factories() {
        assert_eq!(primitive_types::int(), Type::Primitive(PrimitiveType::Int));
        assert_eq!(
            primitive_types::boolean(),
            Type::Primitive(PrimitiveType::Boolean)
        );
    }

    #[test]
    fn test_reference_factories() {
        let obj = reference_types::object();
        assert!(matches!(obj, Type::Class(_)));

        let str_type = reference_types::string();
        assert!(matches!(str_type, Type::Class(_)));

        assert_eq!(reference_types::void(), Type::Void);
        assert_eq!(reference_types::null(), Type::Null);
    }

    #[test]
    fn test_generic_factories() {
        let list_str = generic_types::list_of_string();
        match list_str {
            Type::Class(class_type) => {
                assert_eq!(&class_type.name, "List");
                assert_eq!(class_type.type_arguments.len(), 1);
            }
            _ => panic!("Expected class type with type arguments"),
        }

        let map = generic_types::map_string_to_object();
        match map {
            Type::Class(class_type) => {
                assert_eq!(class_type.type_arguments.len(), 2);
            }
            _ => panic!("Expected class type with type arguments"),
        }
    }

    #[test]
    fn test_array_factories() {
        let int_arr = array_types::int_array();
        match int_arr {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                assert!(matches!(*element_type, Type::Primitive(PrimitiveType::Int)));
            }
            _ => panic!("Expected array type"),
        }

        let int_2d = array_types::int_2d();
        match int_2d {
            Type::Array { dimensions, .. } => {
                assert_eq!(dimensions, 2);
            }
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn test_wildcard_factories() {
        let unbounded = wildcard_types::unbounded();
        match unbounded {
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => {
                assert!(upper_bound.is_none());
                assert!(lower_bound.is_none());
            }
            _ => panic!("Expected wildcard type"),
        }

        let extends_obj = wildcard_types::extends_object();
        match extends_obj {
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => {
                assert!(upper_bound.is_some());
                assert!(lower_bound.is_none());
            }
            _ => panic!("Expected wildcard type"),
        }

        let super_str = wildcard_types::super_string();
        match super_str {
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => {
                assert!(upper_bound.is_none());
                assert!(lower_bound.is_some());
            }
            _ => panic!("Expected wildcard type"),
        }
    }

    #[test]
    fn test_type_variable_factories() {
        let t = type_variables::t();
        match t {
            Type::TypeVariable { name, bounds } => {
                assert_eq!(name, "T");
                assert!(bounds.is_empty());
            }
            _ => panic!("Expected type variable"),
        }

        let t_num = type_variables::t_extends_number();
        match t_num {
            Type::TypeVariable { name, bounds } => {
                assert_eq!(name, "T");
                assert_eq!(bounds.len(), 1);
            }
            _ => panic!("Expected type variable"),
        }
    }

    #[test]
    fn test_class_type_factories() {
        let obj = class_types::object();
        assert_eq!(&*obj.name, "Object");
        assert_eq!(obj.package.as_deref(), Some("java.lang".into()));

        let simple = class_types::simple("MyClass");
        assert_eq!(&*simple.name, "MyClass");
        assert_eq!(simple.package, None);

        let qualified = class_types::qualified("User", "com.example");
        assert_eq!(&*qualified.name, "User");
        assert_eq!(qualified.package.as_deref(), Some("com.example".into()));
    }

    #[test]
    fn test_complex_generic_types() {
        let list_of_maps = generic_types::list(generic_types::map_string_to_object());

        match list_of_maps {
            Type::Class(class_type) => {
                assert_eq!(&class_type.name, "List");
                assert_eq!(class_type.type_arguments.len(), 1);
                // The inner type should also be a Class with type arguments
                match &class_type.type_arguments[0] {
                    Type::Class(inner) => {
                        assert_eq!(&inner.name, "Map");
                        assert_eq!(inner.type_arguments.len(), 2);
                    }
                    _ => panic!("Expected inner Map type"),
                }
            }
            _ => panic!("Expected nested generic type"),
        }
    }
}
