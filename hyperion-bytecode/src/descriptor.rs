// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java method and field descriptor parsing
//!
//! This module parses Java type descriptors from bytecode into structured types.
//!
//! # Descriptor Format
//!
//! ## Field Descriptors
//! - `B` - byte
//! - `C` - char
//! - `D` - double
//! - `F` - float
//! - `I` - int
//! - `J` - long
//! - `S` - short
//! - `Z` - boolean
//! - `Ljava/lang/String;` - reference type (class/interface)
//! - `[I` - array type
//!
//! ## Method Descriptors
//! Format: `(parameter-descriptors)return-descriptor`
//!
//! Examples:
//! - `()V` - void method with no parameters
//! - `(I)I` - int method(int)
//! - `(Ljava/lang/String;)Z` - boolean method(String)
//! - `([ILjava/lang/Object;)V` - void method(int[], Object)
//!
//! ## Generic Type Erasure
//!
//! **Important**: Method descriptors in bytecode use erased types, not generic types.
//! This is correct behavior per JVM specification (JVMS §4.3.4).
//!
//! Generic type information is stored separately in the `Signature` attribute (JVMS §4.7.9).
//! The descriptor uses the raw erased types for runtime execution.
//!
//! Examples:
//! - `List<String>` → `Ljava/util/List;` (descriptor) + `Ljava/util/List<Ljava/lang/String;>;` (signature)
//! - `T extends Number` → `Ljava/lang/Number;` (descriptor) + `TT;` (signature)
//! - `void foo(List<String> list)` → `(Ljava/util/List;)V` (descriptor)
//!
//! This erasure is intentional and required for:
//! - Runtime type checking and method dispatch
//! - Backward compatibility with pre-generics JVM bytecode
//! - Efficient runtime execution without generic type overhead

use std::iter::Peekable;
use std::str::Chars;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DescriptorError {
    #[error("Invalid descriptor format: expected '(' at start")]
    InvalidFormat,

    #[error("Unexpected end of descriptor")]
    UnexpectedEnd,

    #[error("Unknown type character: {0}")]
    UnknownType(char),

    #[error("Unterminated class reference (missing ';')")]
    UnterminatedClass,

    #[error("Field descriptor has trailing characters")]
    TrailingCharacters,

    #[error("Class reference has an invalid internal name")]
    InvalidClassName,

    #[error("Array descriptor exceeds 255 dimensions")]
    TooManyArrayDimensions,
}

/// Represents a parsed type from a descriptor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptorType {
    Byte,
    Char,
    Double,
    Float,
    Int,
    Long,
    Short,
    Boolean,
    Void,
    Class {
        /// Fully qualified name (e.g., "java.lang.String")
        name: String,
    },
    Array {
        element_type: Box<DescriptorType>,
        dimensions: usize,
    },
}

impl std::fmt::Display for DescriptorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Byte => f.write_str("byte"),
            Self::Char => f.write_str("char"),
            Self::Double => f.write_str("double"),
            Self::Float => f.write_str("float"),
            Self::Int => f.write_str("int"),
            Self::Long => f.write_str("long"),
            Self::Short => f.write_str("short"),
            Self::Boolean => f.write_str("boolean"),
            Self::Void => f.write_str("void"),
            Self::Class { name } => f.write_str(name),
            Self::Array {
                element_type,
                dimensions,
            } => {
                write!(f, "{}{}", element_type, "[]".repeat(*dimensions))
            }
        }
    }
}

/// Parsed method descriptor
#[derive(Debug, Clone)]
pub struct MethodDescriptor {
    pub parameters: Vec<DescriptorType>,
    pub return_type: DescriptorType,
}

impl MethodDescriptor {
    /// Parse a method descriptor string
    ///
    /// # Examples
    /// ```
    /// use hyperion_bytecode::descriptor::MethodDescriptor;
    ///
    /// let desc = MethodDescriptor::parse("()V").unwrap();
    /// assert_eq!(desc.parameters.len(), 0);
    ///
    /// let desc = MethodDescriptor::parse("(I)I").unwrap();
    /// assert_eq!(desc.parameters.len(), 1);
    /// ```
    pub fn parse(descriptor: &str) -> Result<Self, DescriptorError> {
        let mut chars = descriptor.chars().peekable();

        // Expect opening '('
        if chars.next() != Some('(') {
            return Err(DescriptorError::InvalidFormat);
        }

        // Parse parameters until ')'
        let mut parameters = Vec::new();
        let mut closed_parameters = false;
        while let Some(&ch) = chars.peek() {
            if ch == ')' {
                chars.next();
                closed_parameters = true;
                break;
            }
            parameters.push(parse_field_type(&mut chars)?);
        }
        if !closed_parameters {
            return Err(DescriptorError::UnexpectedEnd);
        }

        // Parse return type
        let return_type = if chars.peek() == Some(&'V') {
            chars.next();
            DescriptorType::Void
        } else {
            parse_field_type(&mut chars)?
        };
        if chars.next().is_some() {
            return Err(DescriptorError::TrailingCharacters);
        }

        Ok(MethodDescriptor {
            parameters,
            return_type,
        })
    }

    /// Count the number of argument slots needed for this method's parameters
    /// (accounting for long/double taking 2 slots each)
    pub fn argument_count(&self) -> usize {
        self.parameters
            .iter()
            .map(|param| match param {
                DescriptorType::Long | DescriptorType::Double => 2,
                _ => 1,
            })
            .sum()
    }
}

/// Parse a field descriptor string
///
/// # Examples
/// ```
/// use hyperion_bytecode::descriptor::{parse_field_descriptor, DescriptorType};
///
/// let ty = parse_field_descriptor("I").unwrap();
/// assert!(matches!(ty, DescriptorType::Int));
///
/// let ty = parse_field_descriptor("Ljava/lang/String;").unwrap();
/// if let DescriptorType::Class { name } = ty {
///     assert_eq!(name, "java.lang.String");
/// } else {
///     panic!("Expected class type");
/// }
/// ```
pub fn parse_field_descriptor(descriptor: &str) -> Result<DescriptorType, DescriptorError> {
    let mut chars = descriptor.chars().peekable();
    let field_type = parse_field_type(&mut chars)?;
    if chars.next().is_some() {
        return Err(DescriptorError::TrailingCharacters);
    }
    Ok(field_type)
}

/// Parse a single field type from a character iterator
fn parse_field_type(chars: &mut Peekable<Chars>) -> Result<DescriptorType, DescriptorError> {
    let ch = chars.next().ok_or(DescriptorError::UnexpectedEnd)?;

    match ch {
        'B' => Ok(DescriptorType::Byte),
        'C' => Ok(DescriptorType::Char),
        'D' => Ok(DescriptorType::Double),
        'F' => Ok(DescriptorType::Float),
        'I' => Ok(DescriptorType::Int),
        'J' => Ok(DescriptorType::Long),
        'S' => Ok(DescriptorType::Short),
        'Z' => Ok(DescriptorType::Boolean),
        'L' => {
            // Class type: Ljava/lang/String;
            let mut internal_name = String::new();
            loop {
                let ch = chars.next().ok_or(DescriptorError::UnterminatedClass)?;
                if ch == ';' {
                    break;
                }
                internal_name.push(ch);
            }
            if internal_name.is_empty()
                || internal_name.starts_with('/')
                || internal_name.ends_with('/')
                || internal_name.contains("//")
                || internal_name
                    .chars()
                    .any(|ch| matches!(ch, '.' | '[' | ';'))
            {
                return Err(DescriptorError::InvalidClassName);
            }
            Ok(DescriptorType::Class {
                name: internal_name.replace('/', "."),
            })
        }
        '[' => {
            // Array type - count dimensions
            let mut dimensions = 1;
            while chars.peek() == Some(&'[') {
                chars.next();
                dimensions += 1;
                if dimensions > 255 {
                    return Err(DescriptorError::TooManyArrayDimensions);
                }
            }

            // Parse element type
            let element_type = Box::new(parse_field_type(chars)?);

            Ok(DescriptorType::Array {
                element_type,
                dimensions,
            })
        }
        _ => Err(DescriptorError::UnknownType(ch)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primitives() {
        assert!(matches!(
            parse_field_descriptor("I").unwrap(),
            DescriptorType::Int
        ));
        assert!(matches!(
            parse_field_descriptor("Z").unwrap(),
            DescriptorType::Boolean
        ));
        assert!(matches!(
            parse_field_descriptor("D").unwrap(),
            DescriptorType::Double
        ));
    }

    #[test]
    fn test_parse_class() {
        let ty = parse_field_descriptor("Ljava/lang/String;").unwrap();
        if let DescriptorType::Class { name } = ty {
            assert_eq!(name, "java.lang.String");
        } else {
            panic!("Expected class type");
        }
    }

    #[test]
    fn test_parse_array() {
        let ty = parse_field_descriptor("[I").unwrap();
        match ty {
            DescriptorType::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                assert!(matches!(*element_type, DescriptorType::Int));
            }
            _ => panic!("Expected array type"),
        }

        let ty = parse_field_descriptor("[[Ljava/lang/String;").unwrap();
        match ty {
            DescriptorType::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 2);
                if let DescriptorType::Class { name } = *element_type {
                    assert_eq!(name, "java.lang.String");
                } else {
                    panic!("Expected class element type");
                }
            }
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn field_descriptor_rejects_trailing_and_malformed_class_names() {
        assert!(matches!(
            parse_field_descriptor("IJ"),
            Err(DescriptorError::TrailingCharacters)
        ));
        assert!(matches!(
            parse_field_descriptor("L;"),
            Err(DescriptorError::InvalidClassName)
        ));
        assert!(matches!(
            parse_field_descriptor("Ljava.lang.String;"),
            Err(DescriptorError::InvalidClassName)
        ));
        assert!(matches!(
            parse_field_descriptor("Ljava//lang/String;"),
            Err(DescriptorError::InvalidClassName)
        ));
        assert!(matches!(
            parse_field_descriptor(&format!("{}I", "[".repeat(256))),
            Err(DescriptorError::TooManyArrayDimensions)
        ));
    }

    #[test]
    fn test_parse_method_void_no_params() {
        let desc = MethodDescriptor::parse("()V").unwrap();
        assert_eq!(desc.parameters.len(), 0);
        assert!(matches!(desc.return_type, DescriptorType::Void));
    }

    #[test]
    fn test_parse_method_int_param() {
        let desc = MethodDescriptor::parse("(I)I").unwrap();
        assert_eq!(desc.parameters.len(), 1);
        assert!(matches!(desc.parameters[0], DescriptorType::Int));
        assert!(matches!(desc.return_type, DescriptorType::Int));
    }

    #[test]
    fn test_parse_method_complex() {
        let desc = MethodDescriptor::parse("(Ljava/lang/String;)Z").unwrap();
        assert_eq!(desc.parameters.len(), 1);
        if let DescriptorType::Class { name } = &desc.parameters[0] {
            assert_eq!(name, "java.lang.String");
        } else {
            panic!("Expected class parameter");
        }
        assert!(matches!(desc.return_type, DescriptorType::Boolean));
    }

    #[test]
    fn method_descriptor_rejects_trailing_input_and_missing_parameter_close() {
        assert!(matches!(
            MethodDescriptor::parse("()VI"),
            Err(DescriptorError::TrailingCharacters)
        ));
        assert!(matches!(
            MethodDescriptor::parse("()Ljava/lang/String;garbage"),
            Err(DescriptorError::TrailingCharacters)
        ));
        assert!(matches!(
            MethodDescriptor::parse("(I"),
            Err(DescriptorError::UnexpectedEnd)
        ));
    }

    #[test]
    fn test_parse_method_array_and_object() {
        let desc = MethodDescriptor::parse("([ILjava/lang/Object;)V").unwrap();
        assert_eq!(desc.parameters.len(), 2);

        match &desc.parameters[0] {
            DescriptorType::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(*dimensions, 1);
                assert!(matches!(**element_type, DescriptorType::Int));
            }
            _ => panic!("Expected array parameter"),
        }

        if let DescriptorType::Class { name } = &desc.parameters[1] {
            assert_eq!(name, "java.lang.Object");
        } else {
            panic!("Expected class parameter");
        }

        assert!(matches!(desc.return_type, DescriptorType::Void));
    }
}
