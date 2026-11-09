// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle build system integration for Hyperion.
//!
//! This module provides comprehensive Gradle support including:
//! - Parsing build.gradle and build.gradle.kts files
//! - Resolving dependencies from Maven repositories
//! - Building classpaths for Java projects
//!
//! # Architecture
//!
//! The Gradle integration is structured into several submodules:
//!
//! - `model`: Data structures representing Gradle projects
//! - `parser`: Parsing Gradle build files (Groovy and Kotlin DSL)
//! - `resolver`: Resolving dependencies to JAR files
//! - `maven`: Maven repository client and POM parsing
//! - `classpath`: Building complete classpaths
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_build::gradle::{GradleClasspathBuilder, GradleParser};
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Parse a Gradle project
//! let project = GradleParser::parse_project(Path::new("/path/to/project")).await?;
//!
//! // Resolve dependencies
//! let resolver = GradleResolver::new();
//! let classpath = GradleClasspathBuilder::new().build(&project).await?;
//!
//! println!("Found {} classpath entries", classpath.len());
//! # Ok(())
//! # }
//! ```

pub mod classpath;
pub(crate) mod composite_plugin;
pub mod model;
pub mod parser;
pub(crate) mod plugin_bytecode;
pub mod resolver;
pub mod sniff_cache;
pub mod sniff_report;
pub(crate) mod syntax;

// Re-export main types
pub use classpath::{GradleClasspath, GradleClasspathBuilder};
pub use model::{
    DependencyConfiguration, GradleDependency, GradlePlugin, GradleProject, GradleRepository,
    SourceSet,
};
pub use parser::{GradleParser, GradleParserError};
pub use resolver::{
    DependencyEdge, DependencyEdgeKind, GradleIncompleteReason, GradleResolutionReport,
    GradleResolver, GradleResolverError,
};
pub use sniff_report::GradleSniffReport;
