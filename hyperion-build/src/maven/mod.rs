// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven build system integration for Hyperion.
//!
//! This module provides Maven support including:
//! - Parsing pom.xml files
//! - Resolving dependencies from local repository
//! - Building classpaths for Java projects
//!
//! # Architecture
//!
//! The Maven integration is structured into several submodules:
//!
//! - `model`: Data structures representing Maven projects
//! - `parser`: Parsing pom.xml files
//! - `resolver`: Resolving dependencies to JAR files
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_build::maven::{MavenParser, MavenResolver};
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Parse a Maven project
//! let project = MavenParser::parse_project(Path::new("/path/to/project")).await?;
//!
//! // Resolve dependencies
//! let resolver = MavenResolver::new();
//! let jars = resolver.resolve_dependencies(&project).await?;
//!
//! println!("Found {} JARs", jars.len());
//! # Ok(())
//! # }
//! ```

pub mod activation;
pub mod model;
pub mod parser;
pub mod resolver;

// Re-export main types
pub use activation::{ProfileActivationContext, ProfileEvaluator};
pub use model::{
    DependencyScope, MavenDependency, MavenProfile, MavenProject, MavenRepository, ParentPom,
    ProfileActivation,
};
pub use parser::{MavenParser, MavenParserError};
pub use resolver::{
    EffectiveModelRepair, MavenResolutionBudget, MavenResolver, MavenResolverError,
    MissingDependency, ResolutionReport,
};
