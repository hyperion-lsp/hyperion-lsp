// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Taint tracking for data flow analysis

pub mod state;
pub mod tracker;

pub use state::{FlowStep, TaintState, TrustLevel, VulnTypes};
pub use tracker::TaintTracker;
