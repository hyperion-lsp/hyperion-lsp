// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method dispatch and resolution

/// Method dispatch implementation
pub struct MethodDispatcher;

impl MethodDispatcher {
    /// Create a new method dispatcher
    pub fn new() -> Self {
        MethodDispatcher
    }
}

impl Default for MethodDispatcher {
    fn default() -> Self {
        Self::new()
    }
}
