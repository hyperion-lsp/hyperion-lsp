// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Debug information generation for compiled methods
//!
//! This module generates LineNumberTable and LocalVariableTable attributes
//! which are essential for debugging and providing meaningful stack traces.

use crate::producer::BytecodeResult;
use std::collections::HashMap;

/// Entry in the LineNumberTable attribute
#[derive(Debug, Clone, Copy)]
pub struct LineNumberEntry {
    /// Start of bytecode range (offset in Code attribute)
    pub start_pc: u16,
    /// Line number in source file
    pub line_number: u16,
}

/// Entry in the LocalVariableTable attribute
#[derive(Debug, Clone)]
pub struct LocalVariableEntry {
    /// Start of bytecode range where variable is valid
    pub start_pc: u16,
    /// Length of bytecode range
    pub length: u16,
    /// Index into constant pool (CONSTANT_Utf8)
    pub name_index: u16,
    /// Index into constant pool (CONSTANT_Utf8 descriptor)
    pub descriptor_index: u16,
    /// Local variable slot number
    pub index: u16,
}

/// Builder for debug information attributes
#[derive(Debug, Default)]
pub struct DebugInfoBuilder {
    /// Line number table entries
    line_numbers: Vec<LineNumberEntry>,

    /// Local variable table entries
    local_variables: Vec<LocalVariableEntry>,

    /// Current line number being processed
    current_line: u16,

    /// Track PC to line number changes
    last_pc_line: HashMap<u16, u16>,
}

impl DebugInfoBuilder {
    /// Create a new debug info builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a line number change at bytecode offset
    /// Only emits an entry if the line number is different from the last emitted line
    pub fn set_line(&mut self, pc: u16, line: u16) {
        // Only emit if line number has changed from the last emitted line
        if self.current_line != line {
            self.current_line = line;
            self.line_numbers.push(LineNumberEntry {
                start_pc: pc,
                line_number: line,
            });
        }
    }

    /// Record a local variable with its scope
    pub fn add_local_variable(
        &mut self,
        start_pc: u16,
        length: u16,
        name_index: u16,
        descriptor_index: u16,
        slot: u16,
    ) {
        self.local_variables.push(LocalVariableEntry {
            start_pc,
            length,
            name_index,
            descriptor_index,
            index: slot,
        });
    }

    /// Build LineNumberTable attribute data
    pub fn build_line_number_table(&self) -> BytecodeResult<Vec<u8>> {
        let mut output = Vec::new();

        // Number of entries
        output.extend_from_slice(
            &crate::groovy_producer::checked_u16_count(
                self.line_numbers.len(),
                "line number table",
            )?
            .to_be_bytes(),
        );

        // Each entry: start_pc (u16) + line_number (u16)
        for entry in &self.line_numbers {
            output.extend_from_slice(&entry.start_pc.to_be_bytes());
            output.extend_from_slice(&entry.line_number.to_be_bytes());
        }

        Ok(output)
    }

    /// Build LocalVariableTable attribute data
    pub fn build_local_variable_table(&self) -> BytecodeResult<Vec<u8>> {
        let mut output = Vec::new();

        // Number of entries
        output.extend_from_slice(
            &crate::groovy_producer::checked_u16_count(
                self.local_variables.len(),
                "local variable table",
            )?
            .to_be_bytes(),
        );

        // Each entry
        for entry in &self.local_variables {
            output.extend_from_slice(&entry.start_pc.to_be_bytes());
            output.extend_from_slice(&entry.length.to_be_bytes());
            output.extend_from_slice(&entry.name_index.to_be_bytes());
            output.extend_from_slice(&entry.descriptor_index.to_be_bytes());
            output.extend_from_slice(&entry.index.to_be_bytes());
        }

        Ok(output)
    }

    /// Get line number entries (for testing)
    #[cfg(test)]
    pub fn line_numbers(&self) -> &[LineNumberEntry] {
        &self.line_numbers
    }

    /// Get local variable entries (for testing)
    #[cfg(test)]
    pub fn local_variables(&self) -> &[LocalVariableEntry] {
        &self.local_variables
    }

    /// Clear all debug info
    pub fn clear(&mut self) {
        self.line_numbers.clear();
        self.local_variables.clear();
        self.current_line = 0;
        self.last_pc_line.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_number_table_basic() {
        let mut builder = DebugInfoBuilder::new();
        builder.set_line(0, 10);
        builder.set_line(5, 11);
        builder.set_line(10, 12);

        let table = builder.build_line_number_table().unwrap();

        // Should have 3 entries: count (2 bytes) + 3 * (start_pc + line) = 2 + 12 = 14 bytes
        assert_eq!(table.len(), 14);

        // Check entry count
        assert_eq!(u16::from_be_bytes([table[0], table[1]]), 3);

        // Check entries
        assert_eq!(builder.line_numbers().len(), 3);
        assert_eq!(builder.line_numbers()[0].start_pc, 0);
        assert_eq!(builder.line_numbers()[0].line_number, 10);
        assert_eq!(builder.line_numbers()[1].start_pc, 5);
        assert_eq!(builder.line_numbers()[1].line_number, 11);
    }

    #[test]
    fn test_line_number_table_no_duplicates() {
        let mut builder = DebugInfoBuilder::new();
        builder.set_line(0, 10);
        builder.set_line(5, 10); // Same line
        builder.set_line(10, 11); // Different line

        // Should have 2 entries, not 3
        assert_eq!(builder.line_numbers().len(), 2);
    }

    #[test]
    fn test_local_variable_table() {
        let mut builder = DebugInfoBuilder::new();
        builder.add_local_variable(0, 10, 5, 10, 0); // Variable at slot 0
        builder.add_local_variable(0, 10, 6, 11, 1); // Variable at slot 1

        let table = builder.build_local_variable_table().unwrap();

        // 2 bytes (count) + 2 entries * 10 bytes = 22 bytes
        assert_eq!(table.len(), 22);

        // Check count
        assert_eq!(u16::from_be_bytes([table[0], table[1]]), 2);

        // Check first entry
        let locals = builder.local_variables();
        assert_eq!(locals[0].start_pc, 0);
        assert_eq!(locals[0].length, 10);
        assert_eq!(locals[0].name_index, 5);
        assert_eq!(locals[0].index, 0);
    }

    #[test]
    fn test_debug_info_clear() {
        let mut builder = DebugInfoBuilder::new();
        builder.set_line(0, 10);
        builder.add_local_variable(0, 10, 5, 10, 0);

        assert_eq!(builder.line_numbers().len(), 1);
        assert_eq!(builder.local_variables().len(), 1);

        builder.clear();

        assert_eq!(builder.line_numbers().len(), 0);
        assert_eq!(builder.local_variables().len(), 0);
    }

    #[test]
    fn test_line_number_same_pc_different_line() {
        let mut builder = DebugInfoBuilder::new();
        builder.set_line(5, 10);
        builder.set_line(5, 11); // Same PC but different line number

        // Should have 2 entries since line changed
        assert_eq!(builder.line_numbers().len(), 2);
        assert_eq!(builder.line_numbers()[0].line_number, 10);
        assert_eq!(builder.line_numbers()[1].line_number, 11);
    }

    #[test]
    fn test_line_number_encoding() {
        let mut builder = DebugInfoBuilder::new();
        builder.set_line(100, 42);

        let table = builder.build_line_number_table().unwrap();

        // Verify binary encoding
        assert_eq!(table.len(), 6); // 2 (count) + 4 (one entry)
        assert_eq!(u16::from_be_bytes([table[2], table[3]]), 100); // start_pc
        assert_eq!(u16::from_be_bytes([table[4], table[5]]), 42); // line_number
    }
}
