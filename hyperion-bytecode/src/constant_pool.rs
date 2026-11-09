// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constant pool builder for JVM class files
//!
//! The constant pool is a table of structures containing all symbolic references used by the
//! class and interfaces. This module provides efficient building of constant pools with
//! automatic deduplication.
//!
//! # Design
//!
//! - Automatic deduplication of constants (same UTF8 string returns same index)
//! - Proper handling of wide entries (Long, Double take 2 slots)
//! - Support for all JVM spec constant types
//! - Builder pattern for ergonomic construction

use crate::producer::{BytecodeError, BytecodeResult, ConstantPoolItem};
use std::collections::HashMap;

/// Bootstrap method metadata for class attributes
#[derive(Debug, Clone)]
pub struct BootstrapMethodEntry {
    /// Index to MethodHandle constant pool entry (bootstrap method)
    pub bootstrap_method_ref: u16,
    /// Indices to additional CP entries passed to bootstrap method
    pub bootstrap_arguments: Vec<u16>,
}

/// Builder for JVM constant pool with deduplication
#[derive(Debug)]
pub struct ConstantPoolBuilder {
    /// Constant entries (index 0 is unused per JVM spec)
    entries: Vec<ConstantPoolItem>,

    /// UTF8 string cache: string -> index
    utf8_cache: HashMap<String, u16>,

    /// Class cache: internal_name -> index
    class_cache: HashMap<String, u16>,

    /// NameAndType cache: (name_idx, desc_idx) -> index
    nat_cache: HashMap<(u16, u16), u16>,

    /// Method reference cache: (class_idx, nat_idx) -> index
    method_ref_cache: HashMap<(u16, u16), u16>,

    /// Field reference cache: (class_idx, nat_idx) -> index
    field_ref_cache: HashMap<(u16, u16), u16>,

    /// Bootstrap methods for invokedynamic (tracked for BootstrapMethods attribute)
    bootstrap_methods: Vec<BootstrapMethodEntry>,
}

impl ConstantPoolBuilder {
    /// Create a new constant pool builder
    ///
    /// The constant pool always starts with an empty entry at index 0 (per JVM spec).
    pub fn new() -> Self {
        Self {
            // Index 0 is unused per JVM spec - add placeholder
            entries: vec![ConstantPoolItem::Utf8(String::new())],
            utf8_cache: HashMap::new(),
            class_cache: HashMap::new(),
            nat_cache: HashMap::new(),
            method_ref_cache: HashMap::new(),
            field_ref_cache: HashMap::new(),
            bootstrap_methods: Vec::new(),
        }
    }

    /// Get all constant pool entries (excluding the placeholder at index 0)
    ///
    /// Returns entries starting from index 1. To access an entry by its CP index,
    /// use `entries[index - 1]` since arrays are 0-based but CP indices are 1-based.
    pub fn get_entries(&self) -> &[ConstantPoolItem] {
        // Skip the placeholder at index 0
        &self.entries[1..]
    }

    /// Add a UTF8 constant, returning its index
    ///
    /// Automatically deduplicates identical strings.
    pub fn add_utf8(&mut self, s: &str) -> u16 {
        if let Some(&idx) = self.utf8_cache.get(s) {
            return idx;
        }

        // JVM constant pool indices are 1-based (index 0 is unused)
        // Since we start with a placeholder at index 0, entries.len() gives us the next 1-based index
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Utf8(s.to_string()));
        self.utf8_cache.insert(s.to_string(), idx);

        idx
    }

    /// Add an integer constant
    pub fn add_integer(&mut self, value: i32) -> u16 {
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Integer(value));
        idx
    }

    /// Add a float constant
    pub fn add_float(&mut self, value: f32) -> u16 {
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Float(value));
        idx
    }

    /// Add a long constant (takes 2 constant pool slots)
    pub fn add_long(&mut self, value: i64) -> u16 {
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Long(value));
        // Long entries take 2 slots - add a placeholder
        self.entries.push(ConstantPoolItem::Utf8(String::new())); // Placeholder
        idx
    }

    /// Add a double constant (takes 2 constant pool slots)
    pub fn add_double(&mut self, value: f64) -> u16 {
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Double(value));
        // Double entries take 2 slots - add a placeholder
        self.entries.push(ConstantPoolItem::Utf8(String::new())); // Placeholder
        idx
    }

    /// Add a class reference
    ///
    /// # Arguments
    ///
    /// * `internal_name` - Internal form of class name (e.g., "java/lang/String")
    pub fn add_class(&mut self, internal_name: &str) -> u16 {
        if let Some(&idx) = self.class_cache.get(internal_name) {
            return idx;
        }

        let name_idx = self.add_utf8(internal_name);
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::Class {
            name_index: name_idx,
        });
        self.class_cache.insert(internal_name.to_string(), idx);

        idx
    }

    /// Add a string constant
    pub fn add_string(&mut self, value: &str) -> u16 {
        let string_idx = self.add_utf8(value);
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::String {
            string_index: string_idx,
        });
        idx
    }

    /// Add a method reference
    pub fn add_method_ref(&mut self, class: &str, name: &str, descriptor: &str) -> u16 {
        let class_idx = self.add_class(class);
        let nat_idx = self.add_name_and_type(name, descriptor);
        let key = (class_idx, nat_idx);

        if let Some(&idx) = self.method_ref_cache.get(&key) {
            return idx;
        }

        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::MethodRef {
            class_index: class_idx,
            name_and_type_index: nat_idx,
        });
        self.method_ref_cache.insert(key, idx);

        idx
    }

    /// Add a method reference by pre-computed indices
    ///
    /// Used when class_index and name_and_type_index are already computed
    pub fn add_method_ref_by_indices(&mut self, class_index: u16, name_and_type_index: u16) -> u16 {
        let key = (class_index, name_and_type_index);

        if let Some(&idx) = self.method_ref_cache.get(&key) {
            return idx;
        }

        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::MethodRef {
            class_index,
            name_and_type_index,
        });
        self.method_ref_cache.insert(key, idx);

        idx
    }

    /// Add a field reference
    pub fn add_field_ref(&mut self, class: &str, name: &str, descriptor: &str) -> u16 {
        let class_idx = self.add_class(class);
        let nat_idx = self.add_name_and_type(name, descriptor);
        let key = (class_idx, nat_idx);

        if let Some(&idx) = self.field_ref_cache.get(&key) {
            return idx;
        }

        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::FieldRef {
            class_index: class_idx,
            name_and_type_index: nat_idx,
        });
        self.field_ref_cache.insert(key, idx);

        idx
    }

    /// Add an interface method reference
    pub fn add_interface_method_ref(
        &mut self,
        interface: &str,
        name: &str,
        descriptor: &str,
    ) -> u16 {
        let class_idx = self.add_class(interface);
        let nat_idx = self.add_name_and_type(name, descriptor);

        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::InterfaceMethodRef {
            class_index: class_idx,
            name_and_type_index: nat_idx,
        });

        idx
    }

    /// Add a name and type entry
    pub fn add_name_and_type(&mut self, name: &str, descriptor: &str) -> u16 {
        let name_idx = self.add_utf8(name);
        let desc_idx = self.add_utf8(descriptor);
        let key = (name_idx, desc_idx);

        if let Some(&idx) = self.nat_cache.get(&key) {
            return idx;
        }

        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::NameAndType {
            name_index: name_idx,
            descriptor_index: desc_idx,
        });
        self.nat_cache.insert(key, idx);

        idx
    }

    /// Add a method handle
    pub fn add_method_handle(&mut self, reference_kind: u8, reference_index: u16) -> u16 {
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::MethodHandle {
            reference_kind,
            reference_index,
        });
        idx
    }

    /// Add a method type
    pub fn add_method_type(&mut self, descriptor: &str) -> u16 {
        let descriptor_idx = self.add_utf8(descriptor);
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::MethodType {
            descriptor_index: descriptor_idx,
        });
        idx
    }

    /// Add an invoke dynamic entry
    pub fn add_invoke_dynamic(
        &mut self,
        bootstrap_index: u16,
        name: &str,
        descriptor: &str,
    ) -> u16 {
        let nat_idx = self.add_name_and_type(name, descriptor);
        let idx = self.entries.len() as u16;
        self.entries.push(ConstantPoolItem::InvokeDynamic {
            bootstrap_method_attr_index: bootstrap_index,
            name_and_type_index: nat_idx,
        });
        idx
    }

    /// Get current constant pool size (number of entries including index 0)
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if constant pool is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Build the final constant pool
    pub fn build(self) -> Vec<ConstantPoolItem> {
        self.entries
    }

    /// Build the constant pool and validate
    pub fn build_validated(self) -> BytecodeResult<Vec<ConstantPoolItem>> {
        self.validate()?;
        Ok(self.entries)
    }

    /// Validate limits before a producer serializes this pool.
    pub fn validate(&self) -> BytecodeResult<()> {
        if self.entries.len() > u16::MAX as usize {
            return Err(BytecodeError::ConstantPoolOverflow {
                message: format!(
                    "Constant pool has {} entries, max is 65535",
                    self.entries.len()
                ),
            });
        }
        Ok(())
    }

    /// Register a bootstrap method and return its index in the BootstrapMethods attribute
    pub fn register_bootstrap_method(&mut self, bootstrap_method_ref: u16) -> u16 {
        let idx = self.bootstrap_methods.len() as u16;
        self.bootstrap_methods.push(BootstrapMethodEntry {
            bootstrap_method_ref,
            bootstrap_arguments: Vec::new(),
        });
        idx
    }

    /// Get the registered bootstrap methods
    pub fn bootstrap_methods(&self) -> &[BootstrapMethodEntry] {
        &self.bootstrap_methods
    }

    /// Check if any bootstrap methods have been registered
    pub fn has_bootstrap_methods(&self) -> bool {
        !self.bootstrap_methods.is_empty()
    }
}

impl Default for ConstantPoolBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_pool_builder_creation() {
        let builder = ConstantPoolBuilder::new();
        // With placeholder at index 0, len() is 1
        assert_eq!(builder.len(), 1);
        assert!(!builder.is_empty());
    }

    #[test]
    fn test_add_utf8_with_deduplication() {
        let mut builder = ConstantPoolBuilder::new();

        let idx1 = builder.add_utf8("hello");
        let idx2 = builder.add_utf8("world");
        let idx3 = builder.add_utf8("hello"); // Should return same as idx1

        assert_eq!(idx1, idx3);
        assert_ne!(idx1, idx2);
        // 1 placeholder + 2 added entries = 3
        assert_eq!(builder.len(), 3);
    }

    #[test]
    fn test_add_integers() {
        let mut builder = ConstantPoolBuilder::new();

        let idx1 = builder.add_integer(42);
        let idx2 = builder.add_integer(42);
        let idx3 = builder.add_integer(100);

        // Integers are not deduplicated - each call creates a new entry
        assert_ne!(idx1, idx2, "Same value should still get separate entries");
        assert_ne!(idx2, idx3);
        // 1 placeholder + 3 integers = 4
        assert_eq!(builder.len(), 4);
    }

    #[test]
    fn test_add_class_with_deduplication() {
        let mut builder = ConstantPoolBuilder::new();

        let idx1 = builder.add_class("java/lang/String");
        let idx2 = builder.add_class("java/lang/Object");
        let idx3 = builder.add_class("java/lang/String"); // Should return same as idx1

        assert_eq!(idx1, idx3);
        assert_ne!(idx1, idx2);
    }

    #[test]
    fn test_add_method_ref_with_caching() {
        let mut builder = ConstantPoolBuilder::new();

        let idx1 = builder.add_method_ref("java/lang/String", "substring", "(I)Ljava/lang/String;");
        let idx2 = builder.add_method_ref("java/lang/String", "substring", "(I)Ljava/lang/String;");
        let idx3 = builder.add_method_ref("java/lang/String", "length", "()I");

        assert_eq!(idx1, idx2);
        assert_ne!(idx1, idx3);
    }

    #[test]
    fn test_add_field_ref() {
        let mut builder = ConstantPoolBuilder::new();

        let idx1 = builder.add_field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");
        let idx2 = builder.add_field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");

        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_add_string() {
        let mut builder = ConstantPoolBuilder::new();

        let idx = builder.add_string("Hello, World!");
        assert!(idx > 0);
    }

    #[test]
    fn test_add_long_takes_two_slots() {
        let mut builder = ConstantPoolBuilder::new();

        let idx = builder.add_long(123456789i64);
        // Long takes 2 slots, so next entry should be at idx + 2
        let next_idx = builder.add_integer(42);
        assert_eq!(next_idx, idx + 2);
    }

    #[test]
    fn test_add_double_takes_two_slots() {
        let mut builder = ConstantPoolBuilder::new();

        let idx = builder.add_double(3.14);
        // Double takes 2 slots, so next entry should be at idx + 2
        let next_idx = builder.add_float(2.71);
        assert_eq!(next_idx, idx + 2);
    }

    #[test]
    fn test_build_and_validate() {
        let mut builder = ConstantPoolBuilder::new();
        builder.add_utf8("test");
        builder.add_class("Test"); // Adds both UTF8("Test") and Class entry

        let cp = builder.build_validated().unwrap();
        // Should be: Utf8("") placeholder, UTF8("test"), UTF8("Test"), Class
        assert_eq!(cp.len(), 4);
    }

    #[test]
    fn test_add_invoke_dynamic() {
        let mut builder = ConstantPoolBuilder::new();

        let idx = builder.add_invoke_dynamic(0, "apply", "(Ljava/lang/Object;)Ljava/lang/Object;");
        assert!(idx > 0);
    }
}
