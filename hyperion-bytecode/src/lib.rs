// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test fixtures use familiar decimal sentinels such as 3.14 to verify exact
// byte encoding; they are not intended as approximations of mathematical PI.
#![cfg_attr(test, allow(clippy::approx_constant))]

//! Hyperion Bytecode - Bytecode analysis and interpretation for JVM class files
//!
//! This crate provides:
//! - Analysis of compiled .class files for JDK indexing and dependency analysis
//! - Full JVM bytecode interpreter for executing .class files
//!
//! # Features
//!
//! - **Class File Parsing**: Parse Java .class files using cafebabe
//! - **Descriptor Parsing**: Convert bytecode descriptors to type representations
//! - **Symbol Extraction**: Extract methods, fields, and class metadata
//! - **JDK Indexing**: Index JDK classes for method resolution
//! - **Bytecode Interpreter**: Execute JVM bytecode with spec-compliant semantics
//!
//! # Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`parser`]: Low-level class file parsing and metadata extraction
//! - [`classpath`]: Bounded class loading from directories and JARs
//! - [`descriptor`]: Parse method and field descriptors into structured types
//! - [`extractor`]: Extract symbol information from class files
//! - [`interpreter`]: JVM bytecode interpreter with complete instruction support
//!
//! # Example
//!
//! ```rust,ignore
//! use hyperion_bytecode::parser::parse_class_file;
//! use hyperion_bytecode::descriptor::MethodDescriptor;
//! use hyperion_bytecode::interpreter::{VirtualMachine, MemoryConfig};
//!
//! // Parse a class file
//! let bytes = std::fs::read("String.class")?;
//! let class = parse_class_file(&bytes)?;
//!
//! // Parse a method descriptor
//! let desc = MethodDescriptor::parse("(I)Ljava/lang/String;")?;
//!
//! // Create and execute on the interpreter
//! let vm = VirtualMachine::new(MemoryConfig::default());
//! ```

/// Class file parsing and metadata extraction
pub mod parser;

/// Bounded class loading from directories and JARs
pub mod classpath;

/// Method and field descriptor parsing
pub mod descriptor;

/// Symbol extraction from parsed class files
pub mod extractor;

/// JDK class discovery and extraction
pub mod jdk_indexer;

pub mod constant_pool;
pub mod debug_info;
pub mod gradle;
pub mod gradle_executor;
pub mod gradle_plugin_executor;
pub mod gradle_stubs;
mod groovy_ast;
pub mod groovy_ast_compiler;
pub mod groovy_compiler;
pub mod groovy_producer;
mod groovy_runtime;
pub mod java_backend;
pub mod java_compiler;
pub mod java_converter;
pub mod java_producer;
pub mod kotlin_converter;
pub mod kotlin_metadata;
pub mod kotlin_producer;
mod modified_utf8;

/// Readable Java stubs rendered from class files
pub mod stub;

/// Bytecode production (compilation from source)
///
/// This module provides infrastructure for compiling Java, Kotlin, and Groovy
/// source code to JVM bytecode:
///
/// - [`producer`]: Core trait and types for bytecode producers
/// - [`constant_pool`]: Constant pool construction and deduplication
/// - [`java_compiler`]: Java expression/statement compilation
/// - [`debug_info`]: Debug information (LineNumberTable, LocalVariableTable)
pub mod producer;

/// JVM bytecode interpreter with complete instruction support
/// and class loading/linking infrastructure
pub mod interpreter;
