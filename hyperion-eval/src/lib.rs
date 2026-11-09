// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion Eval - Execute JVM code instantly without the JVM
//!
//! `hyperion-eval '1+1'` → `2`
//!
//! This crate provides instant evaluation of Java, Kotlin, and Groovy expressions
//! and statements without JVM startup overhead.

pub mod class_runner;
pub mod executor;
pub mod snippet;

pub use class_runner::run_class;
pub use executor::{eval, java_class_bytes, EvalError, EvalOptions, Language};
pub use snippet::SnippetType;
