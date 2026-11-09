// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bytecode producer trait for compiling source code to JVM bytecode
//!
//! This module defines the `BytecodeProducer` trait, which provides a unified interface
//! for compiling Java, Kotlin, and Groovy source code to JVM bytecode (.class files).
//!
//! # Design
//!
//! The trait supports:
//! - Language abstraction: Single trait implements Java, Kotlin, Groovy
//! - Error recovery: Comprehensive error types with source locations
//!
//! # Example
//!
//! ```rust,ignore
//! use hyperion_bytecode::producer::{BytecodeProducer, JvmLanguage, CompilationContext};
//!
//! let java_producer = JavaBytecodeProducer::new();
//! let context = CompilationContext::default();
//! let bytecode = java_producer.compile("class Foo {}", "Foo.java", &context)?;
//! ```

use std::collections::HashMap;
use thiserror::Error;

/// JVM language being compiled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JvmLanguage {
    Java,
    Kotlin,
    Groovy,
}

impl JvmLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            JvmLanguage::Java => "Java",
            JvmLanguage::Kotlin => "Kotlin",
            JvmLanguage::Groovy => "Groovy",
        }
    }
}

/// Errors that can occur during bytecode production
#[derive(Debug, Error)]
pub enum BytecodeError {
    #[error("Parse error at {file}:{line}:{column}: {message}")]
    ParseError {
        file: String,
        line: u32,
        column: u32,
        message: String,
    },

    #[error("Type error: {message}")]
    TypeError { message: String },

    #[error("Unresolved symbol: {name}")]
    UnresolvedSymbol { name: String },

    #[error("Unsupported feature: {feature}")]
    UnsupportedFeature { feature: String },

    #[error("Internal compiler error: {message}")]
    InternalError { message: String },

    #[error("Constant pool overflow: {message}")]
    ConstantPoolOverflow { message: String },

    #[error("Code size limit exceeded for method {method}")]
    CodeSizeExceeded { method: String },

    #[error("Duplicate class name: {name}")]
    DuplicateClassName { name: String },

    #[error("Invalid class name: {name}")]
    InvalidClassName { name: String },
}

/// Result type for bytecode production
pub type BytecodeResult<T> = Result<T, BytecodeError>;

/// Constant pool item types (matching JVM spec)
#[derive(Debug, Clone)]
pub enum ConstantPoolItem {
    Utf8(String),
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class {
        name_index: u16,
    },
    String {
        string_index: u16,
    },
    FieldRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    MethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    InterfaceMethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    NameAndType {
        name_index: u16,
        descriptor_index: u16,
    },
    MethodHandle {
        reference_kind: u8,
        reference_index: u16,
    },
    MethodType {
        descriptor_index: u16,
    },
    Dynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    InvokeDynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    Module {
        name_index: u16,
    },
    Package {
        name_index: u16,
    },
}

/// Context passed to bytecode production
pub struct CompilationContext {
    /// Source code to compile
    pub source: Option<String>,

    /// Target JVM class file version (e.g., 65 for Java 21)
    pub target_version: u16,

    /// Whether to include debug info (LineNumberTable, LocalVariableTable)
    pub debug_info: bool,

    /// Whether to include source file attribute
    pub include_source_file: bool,

    /// Source file path (for SourceFile attribute)
    pub source_path: Option<String>,

    /// Additional metadata (could be extended)
    pub metadata: HashMap<String, String>,
}

impl Default for CompilationContext {
    fn default() -> Self {
        CompilationContext {
            source: None,
            target_version: 65, // Java 21
            debug_info: true,
            include_source_file: true,
            source_path: None,
            metadata: HashMap::new(),
        }
    }
}

impl CompilationContext {
    /// Create a context with source code
    pub fn with_source(source: &str) -> Self {
        CompilationContext {
            source: Some(source.to_string()),
            ..Default::default()
        }
    }
}

/// Language features that may vary by implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageFeature {
    // Java features
    Records,           // Java 14+
    SealedClasses,     // Java 17+
    PatternMatching,   // Java 21+
    TextBlocks,        // Java 15+
    SwitchExpressions, // Java 14+

    // Kotlin features
    Coroutines,
    DataClasses,
    ExtensionFunctions,
    NullSafety,

    // Groovy features
    Closures,
    MetaObjectProtocol,
    GStrings,
    NamedParameters,
}

/// The main trait for bytecode production
///
/// Implementations should compile source code of a specific language to JVM bytecode.
/// The trait is designed to be language-agnostic while supporting language-specific features.
pub trait BytecodeProducer: Send + Sync {
    /// Parse source code into an AST (language-specific type)
    ///
    /// Returns parse errors with source location if parsing fails.
    fn parse(&self, source: &str, file_path: &str) -> BytecodeResult<()>;

    /// Compile the parsed source to bytecode.
    ///
    /// This is the primary compilation method.
    fn produce_bytes(&self, context: &CompilationContext) -> BytecodeResult<Vec<u8>>;

    /// Compile source directly to bytecode (convenience method)
    fn compile(
        &self,
        source: &str,
        file_path: &str,
        context: &CompilationContext,
    ) -> BytecodeResult<Vec<u8>> {
        self.parse(source, file_path)?;
        // Create context with source for produce_bytes
        let ctx_with_source = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some(file_path.to_string()),
            target_version: context.target_version,
            debug_info: context.debug_info,
            include_source_file: context.include_source_file,
            metadata: context.metadata.clone(),
        };
        self.produce_bytes(&ctx_with_source)
    }

    /// Get the language this producer handles
    fn language(&self) -> JvmLanguage;

    /// Get supported language features
    fn supported_features(&self) -> Vec<LanguageFeature>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jvm_language_as_str() {
        assert_eq!(JvmLanguage::Java.as_str(), "Java");
        assert_eq!(JvmLanguage::Kotlin.as_str(), "Kotlin");
        assert_eq!(JvmLanguage::Groovy.as_str(), "Groovy");
    }

    #[test]
    fn test_compilation_context_default() {
        let ctx = CompilationContext::default();
        assert_eq!(ctx.target_version, 65);
        assert!(ctx.debug_info);
        assert!(ctx.include_source_file);
        assert!(ctx.source_path.is_none());
    }

    #[test]
    fn test_bytecode_error_parse_error() {
        let err = BytecodeError::ParseError {
            file: "Foo.java".to_string(),
            line: 42,
            column: 10,
            message: "expected identifier".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("Foo.java"));
        assert!(msg.contains("42"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_constant_pool_items() {
        let items = [
            ConstantPoolItem::Utf8("hello".to_string()),
            ConstantPoolItem::Integer(42),
            ConstantPoolItem::Float(3.14),
            ConstantPoolItem::Class { name_index: 1 },
            ConstantPoolItem::String { string_index: 2 },
        ];
        assert_eq!(items.len(), 5);
    }
}
