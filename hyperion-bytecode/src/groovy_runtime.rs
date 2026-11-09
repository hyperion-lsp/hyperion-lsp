// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Private compiler/interpreter protocol for Groovy runtime operations.

pub(crate) const NUMBERS_CLASS: &str = "hyperion/runtime/GroovyNumbers";
pub(crate) const BINARY_NUMBER_DESCRIPTOR: &str =
    "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
pub(crate) const COMPARE_NUMBER_DESCRIPTOR: &str = "(Ljava/lang/Object;Ljava/lang/Object;)I";
pub(crate) const UNARY_NUMBER_DESCRIPTOR: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";

pub(crate) const TRUTH_CLASS: &str = "hyperion/runtime/GroovyTruth";
pub(crate) const TRUTH_METHOD: &str = "isTruthy";
pub(crate) const TRUTH_DESCRIPTOR: &str = "(Ljava/lang/Object;)Z";
