// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! hyperion-fmt: Java formatter implementing palantir-java-format style
//!
//! This crate provides a native Rust implementation of a Java code formatter
//! that follows the palantir-java-format conventions:
//! - 120 character line width
//! - 4 space indentation
//! - 80 character method chain dot limit
//! - Lambda-friendly formatting
//! - Builder pattern optimization
//!
//! # Third-party attribution
//!
//! The layout solver in [`doc`] is a Rust port of parts of
//! [palantir-java-format](https://github.com/palantir/palantir-java-format)
//! (Copyright Palantir Technologies Inc.), which is itself derived from
//! [google-java-format](https://github.com/google/google-java-format)
//! (Copyright Google LLC). Both are licensed under the Apache License,
//! Version 2.0. See the `NOTICE` and `LICENSE-APACHE-2.0` files at the root of
//! this repository, and the per-file headers in `src/doc/`, for the required
//! notices.
//!
//! The rest of this crate is original work; it emulates palantir-java-format's
//! *output style* without being derived from its source.

pub mod config;
pub mod doc;
pub mod ir;
pub mod printer;
pub mod visitors;

mod api;
mod error;

pub use api::{format_range, format_source, format_to_edits};
pub use config::FormatConfig;
pub use error::FormatError;
