// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hover Information Provider Module
//!
//! This module provides hover functionality for the Hyperion LSP, showing
//! information about symbols when the user hovers over them in the editor.

pub(crate) mod documentation;
pub(crate) mod extractors;
pub(crate) mod gradle;
pub(crate) mod groovy;
pub(crate) mod jpa;
pub(crate) mod kotlin;
pub(crate) mod lombok;
pub(crate) mod resolution;
pub(crate) mod scala;
pub(crate) mod utils;

// Re-export the main HoverProvider from the provider module
mod provider;
pub use provider::HoverProvider;
