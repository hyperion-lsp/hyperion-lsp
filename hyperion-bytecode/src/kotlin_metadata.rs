// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin metadata preservation for JVM bytecode
//!
//! The Kotlin compiler stores metadata in @kotlin.Metadata annotations to preserve
//! type information that Java bytecode cannot express, such as:
//! - Nullable vs non-nullable types
//! - Type parameter variance
//! - Data class information
//! - Lambda information
//!
//! This module contains both a reader for compiler-produced `@Metadata` payloads
//! and a small writer for Hyperion-produced class/function metadata. The writer
//! preserves only the model represented by the types below; it is not a full
//! Kotlin compiler metadata encoder.

use crate::producer::BytecodeResult;

mod reader;

pub use reader::{
    KotlinClassInfo, KotlinClassMetadata, KotlinClassifier, KotlinConstructorInfo,
    KotlinFunctionInfo, KotlinJvmFieldSignature, KotlinJvmMethodSignature,
    KotlinJvmPropertySignature, KotlinMetadataAnnotation, KotlinMetadataError, KotlinMetadataKind,
    KotlinParameterInfo, KotlinPropertyInfo, KotlinTypeArgument, KotlinTypeInfo,
    KotlinTypeParameterInfo, KotlinVariance,
};

/// Represents Kotlin metadata for a class or function
#[derive(Debug, Clone)]
pub struct KotlinMetadata {
    /// Kotlin compiler version used
    pub compiler_version: String,
    /// Metadata version (proto format version)
    pub metadata_version: u32,
    /// Class kind (class=1, interface=2, object=3, companion=4)
    pub kind: u8,
    /// Message data (serialized protobuf)
    pub message_data: Vec<u8>,
    /// Extra metadata (serialized protobuf)
    pub extra_data: Option<Vec<u8>>,
}

impl Default for KotlinMetadata {
    fn default() -> Self {
        Self {
            compiler_version: "1.9.0".to_string(),
            metadata_version: 1,
            kind: 1, // class
            message_data: Vec::new(),
            extra_data: None,
        }
    }
}

/// Kotlin parameter metadata for nullability tracking
#[derive(Debug, Clone)]
pub struct KotlinParameter {
    pub name: String,
    pub nullable: bool,
    pub type_name: String,
}

/// Kotlin function metadata
#[derive(Debug, Clone)]
pub struct KotlinFunction {
    pub name: String,
    pub parameters: Vec<KotlinParameter>,
    pub return_type: String,
    pub return_nullable: bool,
}

/// Kotlin class metadata
#[derive(Debug, Clone)]
pub struct KotlinClass {
    pub name: String,
    pub functions: Vec<KotlinFunction>,
    pub properties: Vec<(String, String, bool)>, // name, type, nullable
    pub is_data_class: bool,
    pub is_sealed: bool,
    pub is_object: bool,
}

impl KotlinClass {
    /// Create a new Kotlin class metadata
    pub fn new(name: String) -> Self {
        Self {
            name,
            functions: Vec::new(),
            properties: Vec::new(),
            is_data_class: false,
            is_sealed: false,
            is_object: false,
        }
    }

    /// Add a function to this class
    pub fn add_function(&mut self, func: KotlinFunction) {
        self.functions.push(func);
    }

    /// Add a property to this class
    pub fn add_property(&mut self, name: String, type_name: String, nullable: bool) {
        self.properties.push((name, type_name, nullable));
    }

    /// Generate Kotlin metadata annotation data
    pub fn generate_metadata(&self) -> BytecodeResult<Vec<u8>> {
        // For MVP: Generate basic metadata structure
        // Full implementation would serialize to protobuf format
        let mut data = Vec::new();

        // Add class kind (1 = class, 3 = object)
        let kind = if self.is_object {
            3 // object
        } else {
            1 // class (data or regular)
        };
        data.push(kind);

        // Add metadata version
        data.extend_from_slice(&1_u32.to_le_bytes());

        // Add function count
        data.push(self.functions.len() as u8);

        // Add property count
        data.push(self.properties.len() as u8);

        Ok(data)
    }
}

/// Generate Kotlin @Metadata annotation for a class
pub fn generate_class_metadata(
    _class_name: &str,
    is_data_class: bool,
    is_sealed: bool,
    is_object: bool,
) -> BytecodeResult<KotlinMetadata> {
    let mut metadata = KotlinMetadata {
        kind: if is_object {
            3 // object
        } else {
            1 // class (sealed or regular)
        },
        ..KotlinMetadata::default()
    };

    // Generate minimal protobuf-like message data
    let mut message = Vec::new();
    message.push(metadata.kind);

    // Add flags
    let mut flags = 0u32;
    if is_data_class {
        flags |= 1 << 0; // DATA flag
    }
    if is_sealed {
        flags |= 1 << 1; // SEALED flag
    }
    if is_object {
        flags |= 1 << 2; // OBJECT flag
    }
    message.extend_from_slice(&flags.to_le_bytes());

    metadata.message_data = message;
    Ok(metadata)
}

/// Generate Kotlin metadata for a function
pub fn generate_function_metadata(
    func_name: &str,
    param_types: &[(&str, bool)], // (type_name, nullable)
    return_type: &str,
    return_nullable: bool,
) -> KotlinFunction {
    let parameters = param_types
        .iter()
        .enumerate()
        .map(|(i, (tname, nullable))| KotlinParameter {
            name: format!("param{}", i),
            nullable: *nullable,
            type_name: tname.to_string(),
        })
        .collect();

    KotlinFunction {
        name: func_name.to_string(),
        parameters,
        return_type: return_type.to_string(),
        return_nullable,
    }
}
