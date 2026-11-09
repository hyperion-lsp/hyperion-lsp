// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Legacy exception-table compatibility types.
//!
//! Interpreter execution uses [`super::frame::ExceptionTableEntry`] and
//! [`super::exception_handler::handle_exception`] for handler matching, frame
//! unwinding, stack traces and monitor cleanup. The types in this module predate
//! that implementation and remain public only for source compatibility; new
//! interpreter code must use the frame and exception-handler modules.

/// Exception table entry from class file
#[derive(Debug, Clone)]
pub struct ExceptionTableEntry {
    /// Start of protected region (inclusive)
    pub start_pc: u16,
    /// End of protected region (exclusive)
    pub end_pc: u16,
    /// Jump target for exception handler
    pub handler_pc: u16,
    /// Exception class constant pool index (0 = catch all)
    pub catch_type: u16,
}

impl ExceptionTableEntry {
    /// Check if this handler catches exceptions at the given PC
    pub fn covers_pc(&self, pc: u16) -> bool {
        pc >= self.start_pc && pc < self.end_pc
    }

    /// Check if this handler catches all exceptions
    pub fn catches_all(&self) -> bool {
        self.catch_type == 0
    }
}

/// Legacy zero-sized marker; it does not perform exception dispatch.
pub struct ExceptionHandler;

impl ExceptionHandler {
    /// Create a new exception handler
    pub fn new() -> Self {
        ExceptionHandler
    }
}

impl Default for ExceptionHandler {
    fn default() -> Self {
        Self::new()
    }
}
