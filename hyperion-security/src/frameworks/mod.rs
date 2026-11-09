// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Framework abstraction layer for pluggable security patterns
//!
//! This module provides the core abstractions for registering security patterns
//! from different web frameworks (Spring, JAX-RS, Dropwizard, Micronaut, etc.).
//!
//! # Architecture
//!
//! - `Framework` trait: Defines how frameworks register their security patterns
//! - `FrameworkRegistry`: Manages enabled frameworks and builds unified registries
//!
//! # Usage
//!
//! ```ignore
//! use hyperion_security::frameworks::{FrameworkRegistry, Framework};
//!
//! // Create registry with default frameworks
//! let registry = FrameworkRegistry::with_builtin();
//!
//! // Or start empty and add custom frameworks
//! let mut registry = FrameworkRegistry::new();
//! registry.register(Box::new(MyCustomFramework));
//! registry.enable_frameworks(&["my-custom"]);
//!
//! // Build unified source/sink/sanitizer registries
//! let (sources, sinks, sanitizers) = registry.rebuild_registries();
//! ```

pub mod detection;
pub mod dropwizard;
pub mod jaxrs;
pub mod micronaut;
pub mod quarkus;
pub mod registry;
pub mod servlet;
pub mod spring;
pub mod vertx;

use crate::sanitizers::SanitizerRegistry;
use crate::sinks::SinkRegistry;
use crate::sources::SourceRegistry;

/// A framework that can register security patterns
///
/// Frameworks implement this trait to provide their source/sink/sanitizer patterns.
/// This allows the scanner to be extended with new frameworks without modifying core code.
pub trait Framework: Send + Sync {
    /// Unique identifier for this framework (e.g., "spring", "jaxrs", "dropwizard")
    fn id(&self) -> &'static str;

    /// Human-readable name (e.g., "Spring Framework", "JAX-RS")
    fn name(&self) -> &'static str;

    /// Maven/Gradle dependency patterns that indicate this framework is in use
    ///
    /// Examples:
    /// - `["org.springframework:spring-web", "org.springframework.boot:spring-boot-starter-web"]`
    /// - `["javax.ws.rs:javax.ws.rs-api", "jakarta.ws.rs:jakarta.ws.rs-api"]`
    ///
    /// Used by [`FrameworkDetector`] during automatic framework detection.
    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Java import prefixes that indicate this framework is in use
    ///
    /// Examples:
    /// - `["org.springframework.web", "org.springframework.stereotype"]`
    /// - `["javax.ws.rs", "jakarta.ws.rs"]`
    ///
    /// Used by [`FrameworkDetector`] during automatic framework detection.
    fn import_patterns(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Signature annotations that indicate this framework is in use
    ///
    /// Examples:
    /// - `["RestController", "RequestMapping", "GetMapping"]` for Spring
    /// - `["Path", "GET", "POST"]` for JAX-RS
    ///
    /// Used by [`FrameworkDetector`] during automatic framework detection.
    fn signature_annotations(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Register taint sources into the provided registry
    ///
    /// This is called during registry building to populate source patterns.
    fn register_sources(&self, registry: &mut SourceRegistry);

    /// Register taint sinks into the provided registry
    ///
    /// This is called during registry building to populate sink patterns.
    fn register_sinks(&self, registry: &mut SinkRegistry);

    /// Register sanitizers into the provided registry
    ///
    /// This is called during registry building to populate sanitizer patterns.
    fn register_sanitizers(&self, registry: &mut SanitizerRegistry);

    /// Check if an annotation indicates a taint source for this framework
    ///
    /// Examples:
    /// - Spring: `@RequestParam`, `@PathVariable`, `@RequestHeader`
    /// - JAX-RS: `@QueryParam`, `@PathParam`, `@HeaderParam`
    ///
    /// # Arguments
    /// * `annotation_name` - The simple name of the annotation (without package)
    ///
    /// # Returns
    /// `true` if this annotation marks a taint source in this framework
    fn is_source_annotation(&self, annotation_name: &str) -> bool;
}

pub use detection::{Confidence, DetectionEvidence, DetectionResult, FrameworkDetector};
pub use registry::FrameworkRegistry;
