// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Intermediate Representation for the formatter
//!
//! This module implements an Oppen-style document IR that represents
//! the structure of formatted output before actual line-breaking decisions
//! are made.

mod builder;
mod document;

pub use builder::DocumentBuilder;
pub use document::{BeginToken, BreakToken, BreakType, ConditionalToken, DocToken, Document};
