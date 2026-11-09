// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Dropwizard framework security patterns
//!
//! Dropwizard is built on top of JAX-RS, so it inherits most JAX-RS patterns.
//! This module adds Dropwizard-specific patterns and dependencies.

use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::SinkRegistry;
use crate::sources::SourceRegistry;

/// Dropwizard framework
///
/// Dropwizard is a high-level framework built on JAX-RS (Jersey), Jetty, and Jackson.
/// Most security patterns come from the underlying JAX-RS implementation.
pub struct DropwizardFramework;

impl Framework for DropwizardFramework {
    fn id(&self) -> &'static str {
        "dropwizard"
    }

    fn name(&self) -> &'static str {
        "Dropwizard"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "io.dropwizard:dropwizard-core",
            "io.dropwizard:dropwizard-jersey",
            "io.dropwizard:dropwizard-jdbi",
            "io.dropwizard:dropwizard-jdbi3",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec!["io.dropwizard"]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        // Dropwizard uses JAX-RS annotations
        vec![]
    }

    fn register_sources(&self, _registry: &mut SourceRegistry) {
        // Dropwizard uses JAX-RS sources - no additional patterns needed
    }

    fn register_sinks(&self, _registry: &mut SinkRegistry) {
        // Dropwizard uses JAX-RS sinks - no additional patterns needed
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // Dropwizard uses standard sanitizers - no additional patterns needed
    }

    fn is_source_annotation(&self, _annotation_name: &str) -> bool {
        // Dropwizard uses JAX-RS annotations - delegate to JAX-RS framework
        false
    }
}
