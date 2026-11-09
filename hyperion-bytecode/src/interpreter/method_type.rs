// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Runtime representation of java.lang.invoke.MethodType
//!
//! MethodType is an immutable, threadable factory for method type descriptors.
//! It's used by invokedynamic and method handles to describe method signatures.

use std::fmt;

/// A method type descriptor representing parameter and return types
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MethodTypeRef {
    pub return_type: TypeDescriptor,
    pub parameter_types: Vec<TypeDescriptor>,
}

/// Type descriptor from method descriptors in JVM method signatures
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeDescriptor {
    // Primitive types
    Void,
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    // Reference types
    Reference(String), // Class name (e.g., "java/lang/String")
    Array(Box<TypeDescriptor>),
}

impl TypeDescriptor {
    /// Check if this is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            TypeDescriptor::Void
                | TypeDescriptor::Boolean
                | TypeDescriptor::Byte
                | TypeDescriptor::Char
                | TypeDescriptor::Short
                | TypeDescriptor::Int
                | TypeDescriptor::Long
                | TypeDescriptor::Float
                | TypeDescriptor::Double
        )
    }

    /// Check if this is a reference type
    pub fn is_reference(&self) -> bool {
        matches!(
            self,
            TypeDescriptor::Reference(_) | TypeDescriptor::Array(_)
        )
    }

    /// Get the slot count (doubles and longs use 2 slots)
    pub fn slot_count(&self) -> usize {
        match self {
            TypeDescriptor::Long | TypeDescriptor::Double => 2,
            _ => 1,
        }
    }

    /// Get the single-character descriptor for this type (for method descriptors)
    pub fn descriptor_char(&self) -> char {
        match self {
            TypeDescriptor::Void => 'V',
            TypeDescriptor::Boolean => 'Z',
            TypeDescriptor::Byte => 'B',
            TypeDescriptor::Char => 'C',
            TypeDescriptor::Short => 'S',
            TypeDescriptor::Int => 'I',
            TypeDescriptor::Long => 'J',
            TypeDescriptor::Float => 'F',
            TypeDescriptor::Double => 'D',
            TypeDescriptor::Reference(_) => 'L',
            TypeDescriptor::Array(_) => '[',
        }
    }
}

impl fmt::Display for TypeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeDescriptor::Void => write!(f, "V"),
            TypeDescriptor::Boolean => write!(f, "Z"),
            TypeDescriptor::Byte => write!(f, "B"),
            TypeDescriptor::Char => write!(f, "C"),
            TypeDescriptor::Short => write!(f, "S"),
            TypeDescriptor::Int => write!(f, "I"),
            TypeDescriptor::Long => write!(f, "J"),
            TypeDescriptor::Float => write!(f, "F"),
            TypeDescriptor::Double => write!(f, "D"),
            TypeDescriptor::Reference(name) => write!(f, "L{};", name),
            TypeDescriptor::Array(elem) => write!(f, "[{}", elem),
        }
    }
}

impl MethodTypeRef {
    /// Create a new MethodType with given return and parameter types
    pub fn new(return_type: TypeDescriptor, parameter_types: Vec<TypeDescriptor>) -> Self {
        MethodTypeRef {
            return_type,
            parameter_types,
        }
    }

    /// Parse a method descriptor string like "(ILjava/lang/String;)V"
    ///
    /// Format: (param_types)return_type
    /// - '(' marks start of parameters
    /// - ')' marks end of parameters
    /// - Followed by return type
    /// - Types: V=void, Z=boolean, B=byte, C=char, S=short, I=int, J=long, F=float, D=double
    /// - L...;= class reference
    /// - [= array
    pub fn from_descriptor(descriptor: &str) -> Result<Self, String> {
        if !descriptor.starts_with('(') {
            return Err(format!(
                "Method descriptor must start with '(': {}",
                descriptor
            ));
        }

        let close_paren = descriptor
            .find(')')
            .ok_or_else(|| format!("Method descriptor must contain ')': {}", descriptor))?;

        // Parse parameters
        let mut params = Vec::new();
        let param_str = &descriptor[1..close_paren];
        let mut chars = param_str.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                'V' => {
                    return Err("void is not allowed in parameter list".to_string());
                }
                'Z' => params.push(TypeDescriptor::Boolean),
                'B' => params.push(TypeDescriptor::Byte),
                'C' => params.push(TypeDescriptor::Char),
                'S' => params.push(TypeDescriptor::Short),
                'I' => params.push(TypeDescriptor::Int),
                'J' => params.push(TypeDescriptor::Long),
                'F' => params.push(TypeDescriptor::Float),
                'D' => params.push(TypeDescriptor::Double),
                'L' => {
                    // Parse class name until ';'
                    let mut class_name = String::new();
                    for next_ch in chars.by_ref() {
                        if next_ch == ';' {
                            break;
                        }
                        class_name.push(next_ch);
                    }
                    params.push(TypeDescriptor::Reference(class_name));
                }
                '[' => {
                    // Parse array
                    let elem = Self::parse_type(&mut chars)?;
                    params.push(TypeDescriptor::Array(Box::new(elem)));
                }
                ' ' | '\t' | '\n' | '\r' => {
                    // Skip whitespace
                }
                _ => {
                    return Err(format!("Invalid type descriptor character: {}", ch));
                }
            }
        }

        // Parse return type
        let return_str = &descriptor[close_paren + 1..];
        let mut return_chars = return_str.chars().peekable();
        let return_type = Self::parse_type(&mut return_chars)?;

        Ok(MethodTypeRef {
            return_type,
            parameter_types: params,
        })
    }

    /// Parse a single type from characters
    fn parse_type(
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<TypeDescriptor, String> {
        match chars.next() {
            Some('V') => Ok(TypeDescriptor::Void),
            Some('Z') => Ok(TypeDescriptor::Boolean),
            Some('B') => Ok(TypeDescriptor::Byte),
            Some('C') => Ok(TypeDescriptor::Char),
            Some('S') => Ok(TypeDescriptor::Short),
            Some('I') => Ok(TypeDescriptor::Int),
            Some('J') => Ok(TypeDescriptor::Long),
            Some('F') => Ok(TypeDescriptor::Float),
            Some('D') => Ok(TypeDescriptor::Double),
            Some('L') => {
                let mut class_name = String::new();
                for ch in chars.by_ref() {
                    if ch == ';' {
                        break;
                    }
                    class_name.push(ch);
                }
                Ok(TypeDescriptor::Reference(class_name))
            }
            Some('[') => {
                let elem = Self::parse_type(chars)?;
                Ok(TypeDescriptor::Array(Box::new(elem)))
            }
            Some(ch) => Err(format!("Invalid type descriptor character: {}", ch)),
            None => Err("Unexpected end of method descriptor".to_string()),
        }
    }

    /// Get the method descriptor string representation
    pub fn to_descriptor(&self) -> String {
        let mut result = String::from("(");
        for param in &self.parameter_types {
            result.push_str(&param.to_string());
        }
        result.push(')');
        result.push_str(&self.return_type.to_string());
        result
    }

    /// Get parameter count
    pub fn parameter_count(&self) -> usize {
        self.parameter_types.len()
    }

    /// Get total slot count for parameters (doubles/longs take 2 slots)
    pub fn parameter_slot_count(&self) -> usize {
        self.parameter_types.iter().map(|t| t.slot_count()).sum()
    }

    /// Get return type slot count
    pub fn return_slot_count(&self) -> usize {
        self.return_type.slot_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_descriptor() {
        let mt = MethodTypeRef::from_descriptor("()V").unwrap();
        assert_eq!(mt.parameter_count(), 0);
        assert_eq!(mt.return_type, TypeDescriptor::Void);
    }

    #[test]
    fn test_parse_int_to_int() {
        let mt = MethodTypeRef::from_descriptor("(I)I").unwrap();
        assert_eq!(mt.parameter_count(), 1);
        assert_eq!(mt.parameter_types[0], TypeDescriptor::Int);
        assert_eq!(mt.return_type, TypeDescriptor::Int);
    }

    #[test]
    fn test_parse_object_descriptor() {
        let mt = MethodTypeRef::from_descriptor("(Ljava/lang/String;)V").unwrap();
        assert_eq!(mt.parameter_count(), 1);
        assert_eq!(
            mt.parameter_types[0],
            TypeDescriptor::Reference("java/lang/String".to_string())
        );
    }

    #[test]
    fn test_parse_complex_descriptor() {
        let mt = MethodTypeRef::from_descriptor("(IJLjava/lang/String;D)Z").unwrap();
        assert_eq!(mt.parameter_count(), 4);
        assert_eq!(mt.parameter_types[0], TypeDescriptor::Int);
        assert_eq!(mt.parameter_types[1], TypeDescriptor::Long);
        assert_eq!(
            mt.parameter_types[2],
            TypeDescriptor::Reference("java/lang/String".to_string())
        );
        assert_eq!(mt.parameter_types[3], TypeDescriptor::Double);
        assert_eq!(mt.return_type, TypeDescriptor::Boolean);
    }

    #[test]
    fn test_slot_count() {
        let mt = MethodTypeRef::from_descriptor("(IJD)V").unwrap();
        // I=1, J=2, D=2 => total 5
        assert_eq!(mt.parameter_slot_count(), 5);
    }

    #[test]
    fn test_roundtrip_descriptor() {
        let desc = "(IJLjava/lang/String;)Z";
        let mt = MethodTypeRef::from_descriptor(desc).unwrap();
        assert_eq!(mt.to_descriptor(), desc);
    }
}
