// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Oppen-style pretty printer
//!
//! This module implements the printing algorithm that converts a Document IR
//! into formatted text output, making decisions about where to break lines.

mod print;
mod scan;

pub use print::Printer;
