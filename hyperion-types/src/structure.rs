// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type structure representation (JLS §4)

/// Represents a Java type's structure
///
/// This is an intermediate representation between the parser's AST and
/// the final output format (FQN strings for workspace, Type enums for semantics).
///
/// Aligned with JLS §4:
/// - Type → PrimitiveType | ReferenceType
/// - ReferenceType → ClassOrInterfaceType | TypeVariable | ArrayType
/// - ArrayType → (PrimitiveType | ClassOrInterfaceType | TypeVariable) Dims
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeStructure {
    /// Base type name (e.g., "List", "User", "int", "java.util.Map")
    pub base: String,

    /// Generic type arguments (empty if none)
    ///
    /// Example: `List<User>` has one type argument
    pub type_args: Vec<TypeStructure>,

    /// Array dimensions (0 if not an array)
    ///
    /// Example: `User[][]` has dimensions = 2
    pub dimensions: usize,

    /// Whether this is a primitive type (int, boolean, etc.)
    pub is_primitive: bool,

    /// Whether this type uses the diamond operator (`new ArrayList<>()`)
    ///
    /// True when `TYPE_ARGS` node exists but contains no type children.
    pub is_diamond: bool,
}

impl TypeStructure {
    fn wildcard_bound(&self) -> Option<(&str, &Self)> {
        matches!(self.base.as_str(), "? extends" | "? super")
            .then(|| (self.base.as_str(), self.type_args.as_slice()))
            .and_then(|(variance, arguments)| match arguments {
                [bound] => Some((variance, bound)),
                _ => None,
            })
    }

    /// Create a new type structure
    pub const fn new(base: String, is_primitive: bool) -> Self {
        Self {
            base,
            type_args: Vec::new(),
            dimensions: 0,
            is_primitive,
            is_diamond: false,
        }
    }

    /// Create a primitive type
    pub fn primitive(name: &str) -> Self {
        Self::new(name.to_string(), true)
    }

    /// Create a class type
    pub const fn class(name: String) -> Self {
        Self::new(name, false)
    }

    /// Add a type argument (for generics)
    #[must_use]
    pub fn with_type_arg(mut self, arg: TypeStructure) -> Self {
        self.type_args.push(arg);
        self
    }

    /// Set type arguments
    #[must_use]
    pub fn with_type_args(mut self, args: Vec<TypeStructure>) -> Self {
        self.type_args = args;
        self
    }

    /// Add array dimensions
    #[must_use]
    pub const fn with_dimensions(mut self, dimensions: usize) -> Self {
        self.dimensions = dimensions;
        self
    }

    /// Format as FQN string with generics and array brackets
    ///
    /// Examples:
    /// - `java.util.List<com.example.User>[]`
    /// - `int[][]`
    /// - `com.example.User.Builder`
    pub fn to_fqn_string(&self) -> String {
        if let Some((variance, bound)) = self.wildcard_bound() {
            return format!("{variance} {}", bound.to_fqn_string());
        }
        let mut result = self.base.clone();

        // Add type arguments if present
        if !self.type_args.is_empty() {
            let args: Vec<String> = self
                .type_args
                .iter()
                .map(TypeStructure::to_fqn_string)
                .collect();
            result.push('<');
            result.push_str(&args.join(", "));
            result.push('>');
        }

        // Add array brackets
        if self.dimensions > 0 {
            result.push_str(&"[]".repeat(self.dimensions));
        }

        result
    }

    /// Format as simple string (without package qualifiers)
    ///
    /// Examples:
    /// - `List<User>[]`
    /// - `int[][]`
    /// - `Builder`
    pub fn to_simple_string(&self) -> String {
        if let Some((variance, bound)) = self.wildcard_bound() {
            return format!("{variance} {}", bound.to_simple_string());
        }
        let simple_base = self
            .base
            .split('.')
            .next_back()
            .unwrap_or(&self.base)
            .to_string();

        let mut result = simple_base;

        if !self.type_args.is_empty() {
            let args: Vec<String> = self
                .type_args
                .iter()
                .map(TypeStructure::to_simple_string)
                .collect();
            result.push('<');
            result.push_str(&args.join(", "));
            result.push('>');
        }

        if self.dimensions > 0 {
            result.push_str(&"[]".repeat(self.dimensions));
        }

        result
    }

    /// Get the base type name (without type arguments or array brackets)
    #[allow(clippy::missing_const_for_fn)]
    pub fn base_name(&self) -> &str {
        &self.base
    }

    /// Check if this is an array type
    pub const fn is_array(&self) -> bool {
        self.dimensions > 0
    }

    /// Check if this is a generic type
    pub const fn is_generic(&self) -> bool {
        !self.type_args.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_type() {
        let ty = TypeStructure::primitive("int");
        assert_eq!(ty.to_fqn_string(), "int");
        assert_eq!(ty.to_simple_string(), "int");
        assert!(ty.is_primitive);
    }

    #[test]
    fn test_primitive_array() {
        let ty = TypeStructure::primitive("int").with_dimensions(2);
        assert_eq!(ty.to_fqn_string(), "int[][]");
        assert!(ty.is_array());
    }

    #[test]
    fn test_class_type() {
        let ty = TypeStructure::class("com.example.User".to_string());
        assert_eq!(ty.to_fqn_string(), "com.example.User");
        assert_eq!(ty.to_simple_string(), "User");
    }

    #[test]
    fn test_generic_type() {
        let user = TypeStructure::class("com.example.User".to_string());
        let list = TypeStructure::class("java.util.List".to_string()).with_type_arg(user);

        assert_eq!(list.to_fqn_string(), "java.util.List<com.example.User>");
        assert_eq!(list.to_simple_string(), "List<User>");
        assert!(list.is_generic());
    }

    #[test]
    fn test_nested_generics() {
        let user = TypeStructure::class("com.example.User".to_string());
        let list = TypeStructure::class("java.util.List".to_string()).with_type_arg(user);
        let string = TypeStructure::class("java.lang.String".to_string());
        let map =
            TypeStructure::class("java.util.Map".to_string()).with_type_args(vec![string, list]);

        assert_eq!(
            map.to_fqn_string(),
            "java.util.Map<java.lang.String, java.util.List<com.example.User>>"
        );
        assert_eq!(map.to_simple_string(), "Map<String, List<User>>");
    }

    #[test]
    fn test_generic_array() {
        let user = TypeStructure::class("com.example.User".to_string());
        let list = TypeStructure::class("java.util.List".to_string())
            .with_type_arg(user)
            .with_dimensions(1);

        assert_eq!(list.to_fqn_string(), "java.util.List<com.example.User>[]");
        assert!(list.is_array());
        assert!(list.is_generic());
    }

    #[test]
    fn wildcard_marker_reuses_existing_fields_without_losing_format() {
        let bound = TypeStructure::class("java.lang.Number".to_string());
        let wildcard = TypeStructure::class("? extends".to_string()).with_type_arg(bound);
        let list = TypeStructure::class("java.util.List".to_string()).with_type_arg(wildcard);

        assert_eq!(
            list.to_fqn_string(),
            "java.util.List<? extends java.lang.Number>"
        );
        assert_eq!(list.to_simple_string(), "List<? extends Number>");
    }
}
