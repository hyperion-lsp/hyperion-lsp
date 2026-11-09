// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JAX-RS framework security patterns
//!
//! Patterns for JAX-RS (Jakarta RESTful Web Services and older javax.ws.rs).
//! Covers Jersey, RESTEasy, and other JAX-RS implementations.

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::ContextValidator;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use std::collections::HashSet;

/// JAX-RS framework (Jakarta/javax.ws.rs)
///
/// Supports both Jakarta EE (jakarta.ws.rs) and older Java EE (javax.ws.rs) packages.
pub struct JaxRsFramework;

impl Framework for JaxRsFramework {
    fn id(&self) -> &'static str {
        "jaxrs"
    }

    fn name(&self) -> &'static str {
        "JAX-RS"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "jakarta.ws.rs:jakarta.ws.rs-api",
            "javax.ws.rs:javax.ws.rs-api",
            "org.glassfish.jersey.core:jersey-server",
            "org.glassfish.jersey.core:jersey-client",
            "org.jboss.resteasy:resteasy-core",
            "org.apache.cxf:cxf-rt-frontend-jaxrs",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec!["jakarta.ws.rs", "javax.ws.rs"]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        vec![
            "Path", "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS",
        ]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // JAX-RS UriInfo
        let jaxrs_methods: HashSet<String> =
            ["getQueryParameters", "getPathParameters", "getRequestUri"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        let jaxrs_receivers: HashSet<String> = ["UriInfo", "uriInfo"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(jaxrs_receivers, jaxrs_methods, SourceKind::HttpRequest);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // JAX-RS Response.entity() - renders response body
        // Needs context validation for JSON responses
        let jaxrs_entity_methods: HashSet<String> = std::iter::once("entity".to_string()).collect();

        let jaxrs_entity_receivers: HashSet<String> =
            ["Response", "ResponseBuilder", "response", "responseBuilder"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        // Register sink pattern with context validation
        registry.register_pattern_with_context(
            jaxrs_entity_receivers.clone(),
            jaxrs_entity_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // JAX-RS redirect methods - open redirect
        let jaxrs_redirect_methods: HashSet<String> = ["seeOther", "temporaryRedirect", "created"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            jaxrs_entity_receivers,
            jaxrs_redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // No JAX-RS-specific sanitizers
    }

    fn is_source_annotation(&self, annotation_name: &str) -> bool {
        matches!(
            annotation_name,
            "QueryParam" | "HeaderParam" | "CookieParam" | "PathParam" | "FormParam" | "BeanParam"
        )
    }
}
