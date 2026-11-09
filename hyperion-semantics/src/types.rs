// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type System Extensions and Utilities
//!
//! This module extends the type system defined in `symbol.rs` with additional
//! functionality needed for type inference and semantic analysis.
//!
//! The core type definitions are in `symbol.rs`, but this module provides:
//! - Type compatibility checking
//! - Numeric type promotion
//! - Boxing/unboxing conversions
//! - Subtyping relationships

use crate::symbol::{ClassType, PrimitiveType, Type};

/// Check if subtype is assignable to supertype using workspace index for type hierarchy.
///
/// This is the **primary** subtype check function. It queries the workspace index
/// which contains inheritance information from:
/// - Source files (parsed extends/implements clauses)
/// - JDK bytecode (parsed from jimage/jmod files)
/// - External JARs (parsed from class files)
///
/// Returns `true` if:
/// - The types are exactly equal
/// - supertype is java.lang.Object (all reference types extend Object)
/// - subtype extends supertype (directly or transitively)
/// - subtype implements supertype interface (directly or transitively)
///
/// Falls back to false if the workspace doesn't contain the types (shouldn't happen
/// with JDK indexed).
pub fn is_subtype_in_workspace(
    subtype: &str,
    supertype: &str,
    workspace: &hyperion_workspace::WorkspaceIndex,
) -> bool {
    // All reference types extend Object
    if supertype == "java.lang.Object" {
        return true;
    }

    // Nominal identity is exact. In particular, `a.User` and `b.User` are
    // unrelated even though their simple names happen to match.
    if subtype == supertype {
        return true;
    }

    // The cached chain is resolved from one coherent workspace read: source-
    // level contextual spellings have already become exact edges, while
    // ambiguous or missing edges are absent. Skip the declaration itself so
    // identity remains the explicit exact check above.
    workspace
        .get_inheritance_chain(subtype)
        .into_iter()
        .skip(1)
        .any(|ancestor| ancestor == supertype)
}

/// JLS-constant subtype relationships for common JDK types.
///
/// These are compile-time constants from the Java Language Specification.
/// Used as fallback when workspace lookup fails (unit tests, startup races).
/// In production with JDK indexed, workspace lookup is authoritative.
pub fn is_known_jdk_subtype(subtype: &str, supertype: &str) -> bool {
    // Unqualified names are retained for parser-only/unit-test callers. Once a
    // package is present it must name the actual JDK declaration; otherwise a
    // user type such as `example.Integer` would inherit Number accidentally.
    if !is_known_jdk_type_spelling(subtype) || !is_known_jdk_type_spelling(supertype) {
        return false;
    }

    let simple_sub = subtype.rsplit('.').next().unwrap_or(subtype);
    let simple_super = supertype.rsplit('.').next().unwrap_or(supertype);

    matches!(
        (simple_sub, simple_super),
        // Collection hierarchy (since Java 1.2)
        ("ArrayList" | "LinkedList" | "Vector" | "Stack", "List" | "Collection" | "Iterable")
            | ("List" | "Set", "Collection" | "Iterable")
            | ("HashSet" | "TreeSet" | "LinkedHashSet", "Set" | "Collection" | "Iterable")
            | ("Collection", "Iterable")
            // Map hierarchy
            | ("HashMap" | "TreeMap" | "LinkedHashMap" | "ConcurrentHashMap" | "Hashtable", "Map")
            // Number hierarchy (since Java 1.0)
            | ("Integer" | "Long" | "Short" | "Byte" | "Float" | "Double" | "BigInteger" | "BigDecimal", "Number")
            // CharSequence hierarchy (since Java 1.4)
            | ("String" | "StringBuilder" | "StringBuffer", "CharSequence")
            // Throwable hierarchy (since Java 1.0)
            | ("RuntimeException" | "IOException" | "FileNotFoundException" | "SocketException",
                "Exception" | "Throwable")
            | ("Exception" | "Error", "Throwable")
            | ("IllegalArgumentException" | "NullPointerException" | "IllegalStateException"
                | "IndexOutOfBoundsException" | "UnsupportedOperationException",
                "RuntimeException" | "Exception" | "Throwable")
            // IO hierarchy
            | ("FileInputStream" | "ByteArrayInputStream" | "BufferedInputStream", "InputStream")
            | ("FileOutputStream" | "ByteArrayOutputStream" | "BufferedOutputStream", "OutputStream")
            | ("FileReader" | "BufferedReader" | "InputStreamReader" | "StringReader", "Reader")
            | ("FileWriter" | "BufferedWriter" | "OutputStreamWriter" | "StringWriter", "Writer")
            // URL hierarchy
            | ("HttpURLConnection" | "HttpsURLConnection", "URLConnection")
            | ("HttpsURLConnection", "HttpURLConnection")
    )
}

fn is_known_jdk_type_spelling(name: &str) -> bool {
    if !name.contains('.') {
        return true;
    }

    matches!(
        name,
        "java.lang.Object"
            | "java.lang.Iterable"
            | "java.lang.Number"
            | "java.lang.String"
            | "java.lang.StringBuilder"
            | "java.lang.StringBuffer"
            | "java.lang.CharSequence"
            | "java.lang.Integer"
            | "java.lang.Long"
            | "java.lang.Short"
            | "java.lang.Byte"
            | "java.lang.Float"
            | "java.lang.Double"
            | "java.lang.Throwable"
            | "java.lang.Exception"
            | "java.lang.RuntimeException"
            | "java.lang.Error"
            | "java.lang.IllegalArgumentException"
            | "java.lang.NullPointerException"
            | "java.lang.IllegalStateException"
            | "java.lang.IndexOutOfBoundsException"
            | "java.lang.UnsupportedOperationException"
            | "java.util.Collection"
            | "java.util.List"
            | "java.util.Set"
            | "java.util.ArrayList"
            | "java.util.LinkedList"
            | "java.util.Vector"
            | "java.util.Stack"
            | "java.util.HashSet"
            | "java.util.TreeSet"
            | "java.util.LinkedHashSet"
            | "java.util.Map"
            | "java.util.HashMap"
            | "java.util.TreeMap"
            | "java.util.LinkedHashMap"
            | "java.util.Hashtable"
            | "java.util.concurrent.ConcurrentHashMap"
            | "java.math.BigInteger"
            | "java.math.BigDecimal"
            | "java.io.IOException"
            | "java.io.FileNotFoundException"
            | "java.io.InputStream"
            | "java.io.FileInputStream"
            | "java.io.ByteArrayInputStream"
            | "java.io.BufferedInputStream"
            | "java.io.OutputStream"
            | "java.io.FileOutputStream"
            | "java.io.ByteArrayOutputStream"
            | "java.io.BufferedOutputStream"
            | "java.io.Reader"
            | "java.io.FileReader"
            | "java.io.BufferedReader"
            | "java.io.InputStreamReader"
            | "java.io.StringReader"
            | "java.io.Writer"
            | "java.io.FileWriter"
            | "java.io.BufferedWriter"
            | "java.io.OutputStreamWriter"
            | "java.io.StringWriter"
            | "java.net.SocketException"
            | "java.net.URLConnection"
            | "java.net.HttpURLConnection"
            | "javax.net.ssl.HttpsURLConnection"
    )
}

/// Extension trait for Type providing inference-related operations
///
/// Note: is_assignable_from() is an inherent method on Type (see symbol.rs),
/// not part of this trait. This avoids duplication.
pub trait TypeExt {
    /// Check if this is a reference type
    fn is_reference_type(&self) -> bool;

    /// Check if this is a primitive type
    fn is_primitive_type(&self) -> bool;

    /// Get the boxed version of a primitive type
    fn box_type(&self) -> Type;

    /// Get the unboxed version of a wrapper type
    fn unbox_type(&self) -> Option<Type>;

    /// Perform numeric promotion on two types
    fn promote_with(&self, other: &Type) -> Type;
}

impl TypeExt for Type {
    fn is_reference_type(&self) -> bool {
        matches!(
            self,
            Self::Class(_) | Self::Array { .. } | Self::TypeVariable { .. } | Self::Null
        )
    }

    fn is_primitive_type(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    fn box_type(&self) -> Type {
        match self {
            Self::Primitive(prim) => {
                let class_name = prim.wrapper_class();
                Self::Class(ClassType::new(
                    class_name.to_string(),
                    Some("java.lang".into()),
                ))
            }
            _ => self.clone(),
        }
    }

    fn unbox_type(&self) -> Option<Type> {
        match self {
            Self::Class(c) if c.package == Some("java.lang".into()) => {
                let prim = match &*c.name {
                    "Boolean" => Some(PrimitiveType::Boolean),
                    "Byte" => Some(PrimitiveType::Byte),
                    "Short" => Some(PrimitiveType::Short),
                    "Integer" => Some(PrimitiveType::Int),
                    "Long" => Some(PrimitiveType::Long),
                    "Character" => Some(PrimitiveType::Char),
                    "Float" => Some(PrimitiveType::Float),
                    "Double" => Some(PrimitiveType::Double),
                    _ => None,
                };
                prim.map(Type::Primitive)
            }
            _ => None,
        }
    }

    fn promote_with(&self, other: &Type) -> Type {
        use PrimitiveType::{Double, Float, Int, Long};

        match (self, other) {
            // If either is not primitive, no promotion
            (Self::Primitive(p1), Self::Primitive(p2)) => {
                // Numeric promotion rules from JLS
                Self::Primitive(match (p1, p2) {
                    // Double beats everything
                    (Double, _) | (_, Double) => Double,
                    // Float beats everything except double
                    (Float, _) | (_, Float) => Float,
                    // Long beats everything except float/double
                    (Long, _) | (_, Long) => Long,
                    // Otherwise, promote to int
                    _ => Int,
                })
            }
            _ => Self::Unknown,
        }
    }
}

/// Helper functions for working with types in the context of expressions
pub mod expr_types {
    use super::{ClassType, PrimitiveType, Type, TypeExt};

    /// Determine the result type of a binary arithmetic operation
    pub fn binary_arithmetic_type(left: &Type, right: &Type) -> Type {
        // Handle string concatenation
        if is_string(left) || is_string(right) {
            return Type::Class(ClassType::string());
        }

        // Numeric promotion
        left.promote_with(right)
    }

    /// Determine the result type of a comparison operation
    pub const fn comparison_type(_left: &Type, _right: &Type) -> Type {
        Type::Primitive(PrimitiveType::Boolean)
    }

    /// Determine the result type of a logical operation
    pub const fn logical_type(left: &Type, right: &Type) -> Type {
        match (left, right) {
            (Type::Primitive(PrimitiveType::Boolean), Type::Primitive(PrimitiveType::Boolean)) => {
                Type::Primitive(PrimitiveType::Boolean)
            }
            _ => Type::Unknown,
        }
    }

    /// Determine the result type of a bitwise operation
    pub fn bitwise_type(left: &Type, right: &Type) -> Type {
        match (left, right) {
            (Type::Primitive(p1), Type::Primitive(p2)) if is_integral(*p1) && is_integral(*p2) => {
                left.promote_with(right)
            }
            // `boolean & boolean`, `|`, `^` are valid Java: logical operators
            // without short-circuiting (JLS §15.22.2).
            (Type::Primitive(PrimitiveType::Boolean), Type::Primitive(PrimitiveType::Boolean)) => {
                Type::Primitive(PrimitiveType::Boolean)
            }
            _ => Type::Unknown,
        }
    }

    /// Check if a type is String
    fn is_string(ty: &Type) -> bool {
        matches!(ty, Type::Class(c) if c.qualified_name() == "java.lang.String")
    }

    /// Check if a primitive type is integral
    const fn is_integral(prim: PrimitiveType) -> bool {
        matches!(
            prim,
            PrimitiveType::Byte
                | PrimitiveType::Short
                | PrimitiveType::Int
                | PrimitiveType::Long
                | PrimitiveType::Char
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{ClassType, PrimitiveType, Type};
    use hyperion_workspace::WorkspaceIndex;
    use tempfile::TempDir;

    #[test]
    fn test_primitive_assignability() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        let byte_type = Type::Primitive(PrimitiveType::Byte);
        let long_type = Type::Primitive(PrimitiveType::Long);

        // int can be assigned from byte (widening)
        assert!(int_type.is_assignable_from(&byte_type));

        // byte cannot be assigned from int (narrowing)
        assert!(!byte_type.is_assignable_from(&int_type));

        // long can be assigned from int
        assert!(long_type.is_assignable_from(&int_type));
    }

    #[test]
    fn test_reference_assignability() {
        let object_type = Type::Class(ClassType::object());
        let string_type = Type::Class(ClassType::string());
        let null_type = Type::Null;

        // Object is assignable from any reference type (simplified)
        assert!(object_type.is_assignable_from(&string_type));

        // String can be assigned null
        assert!(string_type.is_assignable_from(&null_type));

        // Primitives cannot be assigned null
        let int_type = Type::Primitive(PrimitiveType::Int);
        assert!(!int_type.is_assignable_from(&null_type));
    }

    #[test]
    fn test_boxing() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        let boxed = int_type.box_type();

        match boxed {
            Type::Class(c) => {
                assert_eq!(&*c.name, "Integer");
                assert_eq!(c.package.as_deref(), Some("java.lang".into()));
            }
            _ => panic!("Expected class type"),
        }
    }

    #[test]
    fn test_unboxing() {
        let integer_type = Type::Class(ClassType::new("Integer".into(), Some("java.lang".into())));

        let unboxed = integer_type.unbox_type();
        assert_eq!(unboxed, Some(Type::Primitive(PrimitiveType::Int)));
    }

    #[test]
    fn test_numeric_promotion() {
        let byte_type = Type::Primitive(PrimitiveType::Byte);
        let int_type = Type::Primitive(PrimitiveType::Int);
        let long_type = Type::Primitive(PrimitiveType::Long);
        let double_type = Type::Primitive(PrimitiveType::Double);

        // byte + int = int
        assert_eq!(byte_type.promote_with(&int_type), int_type);

        // int + long = long
        assert_eq!(int_type.promote_with(&long_type), long_type);

        // long + double = double
        assert_eq!(long_type.promote_with(&double_type), double_type);
    }

    #[test]
    fn test_is_reference_type() {
        assert!(Type::Class(ClassType::object()).is_reference_type());
        assert!(Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        }
        .is_reference_type());
        assert!(Type::Null.is_reference_type());
        assert!(!Type::Primitive(PrimitiveType::Int).is_reference_type());
        assert!(!Type::Void.is_reference_type());
    }

    #[test]
    fn test_array_assignability() {
        let int_array = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        let object_type = Type::Class(ClassType::object());

        // Object is assignable from array
        assert!(object_type.is_assignable_from(&int_array));
    }

    #[test]
    fn test_string_concatenation() {
        let string_type = Type::Class(ClassType::string());
        let int_type = Type::Primitive(PrimitiveType::Int);

        let result = expr_types::binary_arithmetic_type(&string_type, &int_type);
        assert!(matches!(result, Type::Class(c) if c.qualified_name() == "java.lang.String"));
    }

    #[test]
    fn test_comparison_result() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        let result = expr_types::comparison_type(&int_type, &int_type);
        assert_eq!(result, Type::Primitive(PrimitiveType::Boolean));
    }

    #[test]
    fn test_known_subtype_stdlib() {
        // Test that Object is assignable from anything
        let object = Type::Class(ClassType::new("Object".into(), Some("java.lang".into())));
        let string_type = Type::Class(ClassType::string());

        assert!(object.is_assignable_from(&string_type));
    }

    #[test]
    fn test_collection_hierarchy() {
        let collection = Type::Class(ClassType::new(
            "Collection".into(),
            Some("java.util".into()),
        ));
        let list = Type::Class(ClassType::new("List".into(), Some("java.util".into())));
        let arraylist = Type::Class(ClassType::new("ArrayList".into(), Some("java.util".into())));

        // Collection <- List <- ArrayList (JLS constants)
        assert!(collection.is_assignable_from(&list));
        assert!(collection.is_assignable_from(&arraylist));
        assert!(list.is_assignable_from(&arraylist));

        // Reverse should not work
        assert!(!arraylist.is_assignable_from(&collection));
        assert!(!list.is_assignable_from(&collection));
    }

    #[test]
    fn qualified_user_type_does_not_inherit_jdk_simple_name_relationships() {
        assert!(!is_known_jdk_subtype("example.Integer", "java.lang.Number"));
        assert!(!is_known_jdk_subtype("example.ArrayList", "java.util.List"));
    }

    #[test]
    fn workspace_subtyping_uses_exact_canonical_edges() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        for (relative, source) in [
            ("q/Base.java", "package q; public class Base {}"),
            ("other/Base.java", "package other; public class Base {}"),
            (
                "p/Child.java",
                "package p; import q.Base; public class Child extends Base {}",
            ),
        ] {
            let path = temp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, source).unwrap();
            index.index_file_with_source(&path, source).unwrap();
        }

        assert!(is_subtype_in_workspace("p.Child", "q.Base", &index));
        assert!(!is_subtype_in_workspace("p.Child", "other.Base", &index));
        assert!(!is_subtype_in_workspace("q.Base", "other.Base", &index));
        assert!(!is_subtype_in_workspace("p.Child", "Object", &index));
    }

    #[test]
    fn test_exception_hierarchy() {
        let throwable = Type::Class(ClassType::new("Throwable".into(), Some("java.lang".into())));
        let exception = Type::Class(ClassType::new("Exception".into(), Some("java.lang".into())));
        let runtime_exception = Type::Class(ClassType::new(
            "RuntimeException".into(),
            Some("java.lang".into()),
        ));

        // Throwable <- Exception <- RuntimeException (JLS constants)
        assert!(throwable.is_assignable_from(&exception));
        assert!(throwable.is_assignable_from(&runtime_exception));
        assert!(exception.is_assignable_from(&runtime_exception));

        // Reverse should not work
        assert!(!exception.is_assignable_from(&throwable));
        assert!(!runtime_exception.is_assignable_from(&exception));
    }

    #[test]
    fn test_type_variable_bounds_checking() {
        let type_var_no_bounds = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let type_var_object_bound = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType::object())],
        };

        let type_var_number_bound = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![Type::Class(ClassType::new(
                "Number".into(),
                Some("java.lang".into()),
            ))],
        };

        let type_var_different_name = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![],
        };

        // Same name, same bounds (empty) - should be assignable
        assert!(type_var_no_bounds.is_assignable_from(&type_var_no_bounds));

        // Same name, different bounds - should NOT be assignable
        assert!(!type_var_no_bounds.is_assignable_from(&type_var_object_bound));
        assert!(!type_var_object_bound.is_assignable_from(&type_var_number_bound));

        // Different names - should NOT be assignable
        assert!(!type_var_no_bounds.is_assignable_from(&type_var_different_name));
        assert!(!type_var_different_name.is_assignable_from(&type_var_no_bounds));
    }

    #[test]
    fn test_type_variable_exact_bounds_match() {
        let type_var1 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![
                Type::Class(ClassType::new(
                    "Comparable".into(),
                    Some("java.lang".into()),
                )),
                Type::Class(ClassType::new(
                    "Serializable".into(),
                    Some("java.io".into()),
                )),
            ],
        };

        let type_var2 = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![
                Type::Class(ClassType::new(
                    "Comparable".into(),
                    Some("java.lang".into()),
                )),
                Type::Class(ClassType::new(
                    "Serializable".into(),
                    Some("java.io".into()),
                )),
            ],
        };

        // Same name and exact same bounds - should be assignable
        assert!(type_var1.is_assignable_from(&type_var2));
        assert!(type_var2.is_assignable_from(&type_var1));
    }
}
