// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code generation utilities
//!
//! This module provides code generation for common Java patterns:
//! - Getter and setter methods
//! - toString() method
//! - equals() and hashCode() methods
//! - Constructors
//! - Builder patterns

/// Code generator for common Java patterns
pub struct CodeGenerator;

impl CodeGenerator {
    /// Generate a getter method for a field
    ///
    /// # Arguments
    /// * `field_name` - The name of the field
    /// * `field_type` - The type of the field
    ///
    /// # Returns
    /// A string containing the getter method code
    ///
    /// # Examples
    /// ```
    /// use hyperion_actions::CodeGenerator;
    ///
    /// let getter = CodeGenerator::generate_getter("name", "String");
    /// assert!(getter.contains("public String getName()"));
    /// ```
    pub fn generate_getter(field_name: &str, field_type: &str) -> String {
        let method_name = format!("get{}", Self::capitalize(field_name));
        format!(
            "public {} {}() {{\n    return this.{};\n}}",
            field_type, method_name, field_name
        )
    }

    /// Generate a setter method for a field
    ///
    /// # Arguments
    /// * `field_name` - The name of the field
    /// * `field_type` - The type of the field
    ///
    /// # Returns
    /// A string containing the setter method code
    pub fn generate_setter(field_name: &str, field_type: &str) -> String {
        let method_name = format!("set{}", Self::capitalize(field_name));
        format!(
            "public void {}({} {}) {{\n    this.{} = {};\n}}",
            method_name, field_type, field_name, field_name, field_name
        )
    }

    /// Generate both getter and setter for a field
    pub fn generate_accessor_methods(field_name: &str, field_type: &str) -> String {
        format!(
            "{}\n\n{}",
            Self::generate_getter(field_name, field_type),
            Self::generate_setter(field_name, field_type)
        )
    }

    /// Generate a toString method
    ///
    /// # Arguments
    /// * `class_name` - The name of the class
    /// * `fields` - A list of (field_name, field_type) tuples
    ///
    /// # Returns
    /// A string containing the toString method code
    pub fn generate_to_string(class_name: &str, fields: &[(&str, &str)]) -> String {
        if fields.is_empty() {
            return format!(
                "@Override\npublic String toString() {{\n    return \"{}{{}}\";\n}}",
                class_name
            );
        }

        let field_str = fields
            .iter()
            .map(|(name, _)| format!("{}='\" + {} + \"'", name, name))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "@Override\npublic String toString() {{\n    return \"{}{{{}}}\";\n}}",
            class_name, field_str
        )
    }

    /// Generate equals method
    pub fn generate_equals(class_name: &str, fields: &[(&str, &str)]) -> String {
        let mut result = String::from("@Override\npublic boolean equals(Object obj) {\n");
        result.push_str("    if (this == obj) return true;\n");
        result.push_str("    if (obj == null || getClass() != obj.getClass()) return false;\n");

        if !fields.is_empty() {
            result.push_str(&format!(
                "    {} other = ({}) obj;\n",
                class_name, class_name
            ));

            let comparisons = fields
                .iter()
                .map(|(name, ty)| {
                    if Self::is_primitive(ty) {
                        format!("{} == other.{}", name, name)
                    } else {
                        format!("java.util.Objects.equals({}, other.{})", name, name)
                    }
                })
                .collect::<Vec<_>>()
                .join(" &&\n           ");

            result.push_str(&format!("    return {};\n", comparisons));
        } else {
            result.push_str("    return true;\n");
        }

        result.push('}');
        result
    }

    /// Generate hashCode method
    pub fn generate_hash_code(fields: &[(&str, &str)]) -> String {
        let mut result = String::from("@Override\npublic int hashCode() {\n");

        if fields.is_empty() {
            result.push_str("    return 0;\n");
        } else {
            let field_names = fields
                .iter()
                .map(|(name, _)| name.to_string())
                .collect::<Vec<_>>()
                .join(", ");

            result.push_str(&format!(
                "    return java.util.Objects.hash({});\n",
                field_names
            ));
        }

        result.push('}');
        result
    }

    /// Generate a constructor
    pub fn generate_constructor(class_name: &str, fields: &[(&str, &str)]) -> String {
        let mut result = format!("public {}(", class_name);

        let params = fields
            .iter()
            .map(|(name, ty)| format!("{} {}", ty, name))
            .collect::<Vec<_>>()
            .join(", ");

        result.push_str(&params);
        result.push_str(") {\n");

        for (name, _) in fields {
            result.push_str(&format!("    this.{} = {};\n", name, name));
        }

        result.push('}');
        result
    }

    /// Generate a default (no-arg) constructor
    pub fn generate_default_constructor(class_name: &str) -> String {
        format!("public {}() {{\n}}", class_name)
    }

    /// Generate a builder class
    pub fn generate_builder(class_name: &str, fields: &[(&str, &str)]) -> String {
        let mut result = "public static class Builder {\n".to_string();

        // Fields
        for (name, ty) in fields {
            result.push_str(&format!("    private {} {};\n", ty, name));
        }

        result.push('\n');

        // Setter methods
        for (name, ty) in fields {
            result.push_str(&format!(
                "    public Builder {}({} {}) {{\n        this.{} = {};\n        return this;\n    }}\n\n",
                name, ty, name, name, name
            ));
        }

        // Build method
        result.push_str(&format!("    public {} build() {{\n", class_name));
        result.push_str(&format!("        return new {}(", class_name));

        let args = fields
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        result.push_str(&args);
        result.push_str(");\n    }\n");
        result.push('}');

        result
    }

    /// Capitalize the first letter of a string
    fn capitalize(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    }

    /// Check if a type is a primitive
    fn is_primitive(type_name: &str) -> bool {
        matches!(
            type_name,
            "int" | "long" | "short" | "byte" | "float" | "double" | "boolean" | "char"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize() {
        assert_eq!(CodeGenerator::capitalize("name"), "Name");
        assert_eq!(CodeGenerator::capitalize("firstName"), "FirstName");
        assert_eq!(CodeGenerator::capitalize("a"), "A");
        assert_eq!(CodeGenerator::capitalize(""), "");
    }

    #[test]
    fn test_is_primitive() {
        assert!(CodeGenerator::is_primitive("int"));
        assert!(CodeGenerator::is_primitive("boolean"));
        assert!(CodeGenerator::is_primitive("double"));
        assert!(!CodeGenerator::is_primitive("String"));
        assert!(!CodeGenerator::is_primitive("Integer"));
    }

    #[test]
    fn test_generate_getter() {
        let getter = CodeGenerator::generate_getter("name", "String");
        assert!(getter.contains("public String getName()"));
        assert!(getter.contains("return this.name;"));
    }

    #[test]
    fn test_generate_setter() {
        let setter = CodeGenerator::generate_setter("age", "int");
        assert!(setter.contains("public void setAge(int age)"));
        assert!(setter.contains("this.age = age;"));
    }

    #[test]
    fn test_generate_accessor_methods() {
        let accessors = CodeGenerator::generate_accessor_methods("value", "double");
        assert!(accessors.contains("getValue"));
        assert!(accessors.contains("setValue"));
    }

    #[test]
    fn test_generate_to_string_empty() {
        let to_string = CodeGenerator::generate_to_string("Empty", &[]);
        assert!(to_string.contains("@Override"));
        assert!(to_string.contains("Empty{}"));
    }

    #[test]
    fn test_generate_to_string_with_fields() {
        let fields = vec![("name", "String"), ("age", "int")];
        let to_string = CodeGenerator::generate_to_string("Person", &fields);
        assert!(to_string.contains("@Override"));
        assert!(to_string.contains("Person{"));
        assert!(to_string.contains("name"));
        assert!(to_string.contains("age"));
        // The return statement must be terminated with a semicolon, otherwise the
        // generated method does not compile.
        assert!(
            to_string.contains("\";\n}"),
            "return statement must end with a semicolon: {to_string}"
        );
    }

    #[test]
    fn test_generate_equals_empty() {
        let equals = CodeGenerator::generate_equals("Empty", &[]);
        assert!(equals.contains("@Override"));
        assert!(equals.contains("if (this == obj) return true;"));
        assert!(equals.contains("return true;"));
    }

    #[test]
    fn test_generate_equals_with_fields() {
        let fields = vec![("id", "int"), ("name", "String")];
        let equals = CodeGenerator::generate_equals("Person", &fields);
        assert!(equals.contains("@Override"));
        assert!(equals.contains("Person other"));
        assert!(equals.contains("id == other.id"));
        assert!(equals.contains("Objects.equals(name, other.name)"));
    }

    #[test]
    fn test_generate_hash_code_empty() {
        let hash_code = CodeGenerator::generate_hash_code(&[]);
        assert!(hash_code.contains("@Override"));
        assert!(hash_code.contains("return 0;"));
    }

    #[test]
    fn test_generate_hash_code_with_fields() {
        let fields = vec![("id", "int"), ("name", "String")];
        let hash_code = CodeGenerator::generate_hash_code(&fields);
        assert!(hash_code.contains("@Override"));
        assert!(hash_code.contains("Objects.hash"));
        assert!(hash_code.contains("id, name"));
    }

    #[test]
    fn test_generate_constructor() {
        let fields = vec![("name", "String"), ("age", "int")];
        let constructor = CodeGenerator::generate_constructor("Person", &fields);
        assert!(constructor.contains("public Person(String name, int age)"));
        assert!(constructor.contains("this.name = name;"));
        assert!(constructor.contains("this.age = age;"));
    }

    #[test]
    fn test_generate_default_constructor() {
        let constructor = CodeGenerator::generate_default_constructor("Person");
        assert!(constructor.contains("public Person()"));
    }

    #[test]
    fn test_generate_builder() {
        let fields = vec![("name", "String"), ("age", "int")];
        let builder = CodeGenerator::generate_builder("Person", &fields);
        assert!(builder.contains("public static class Builder"));
        assert!(builder.contains("private String name;"));
        assert!(builder.contains("private int age;"));
        assert!(builder.contains("public Builder name(String name)"));
        assert!(builder.contains("public Builder age(int age)"));
        assert!(builder.contains("public Person build()"));
    }
}
