// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! hyperion-build: Gradle and Maven project models and dependency resolution
//! for Hyperion. Nothing here talks to a build server; the opt-in that runs
//! the real build tool lives in `hyperion-lsp` (`hyperion.buildToolClasspath`).

pub mod gradle;
mod io;
pub mod maven;
pub(crate) mod pom;
