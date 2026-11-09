// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Opcode handler modules
//!
//! This module contains handler functions for each category of JVM bytecode opcodes.
//! Each submodule handles a logical grouping of related opcodes.

pub mod arithmetic;
pub mod arrays;
pub mod bitwise;
pub mod class_init;
pub mod comparisons;
pub mod constants;
pub mod control_flow;
pub mod conversions;
pub mod cp_resolution;
pub mod dgm;
pub mod fields;
pub mod groovy_numbers;
pub mod groovy_truth;
pub mod invocation;
pub mod invokedynamic;
pub mod ldc;
pub mod loads_stores;
pub mod monitors;
pub mod native_virtual;
pub mod objects;
pub mod returns;
pub mod stack;
pub mod streams;
pub mod wide;

pub use arithmetic::*;
pub use arrays::*;
pub use bitwise::*;
pub use comparisons::*;
pub use constants::*;
pub use control_flow::*;
pub use conversions::*;
pub use loads_stores::*;
pub use objects::*;
pub use stack::*;
