// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Quarkus Framework security patterns
//!
//! Patterns for Quarkus, Red Hat's Kubernetes-native Java framework.
//! Quarkus uses JAX-RS under the hood, so most patterns are inherited from JAX-RS.

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::ContextValidator;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use std::collections::HashSet;

/// Quarkus Framework
///
/// Quarkus is a Kubernetes-native Java framework with fast startup and low memory footprint.
/// Uses JAX-RS for REST endpoints, so inherits JAX-RS security patterns.
pub struct QuarkusFramework;

impl Framework for QuarkusFramework {
    fn id(&self) -> &'static str {
        "quarkus"
    }

    fn name(&self) -> &'static str {
        "Quarkus"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "io.quarkus:quarkus-core",
            "io.quarkus:quarkus-resteasy",
            "io.quarkus:quarkus-resteasy-reactive",
            "io.quarkus:quarkus-rest-client",
            "io.quarkus:quarkus-arc",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec!["io.quarkus", "jakarta.ws.rs", "javax.ws.rs"]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        vec![
            "Path",
            "GET",
            "POST",
            "PUT",
            "DELETE",
            "PATCH",
            "HEAD",
            "OPTIONS",
            "QuarkusTest",
        ]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // Quarkus uses JAX-RS UriInfo (same as JAX-RS)
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

        // Quarkus Reactive Routes - RoutingContext
        let routing_methods: HashSet<String> =
            ["request", "queryParam", "pathParam", "getBodyAsString"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        let routing_receivers: HashSet<String> = ["RoutingContext", "routingContext", "context"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(routing_receivers, routing_methods, SourceKind::HttpRequest);

        // Quarkus REST Client - external API sources
        let client_methods: HashSet<String> = ["get", "post", "put", "delete"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let client_receivers: HashSet<String> = ["RestClient", "restClient"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(client_receivers, client_methods, SourceKind::ExternalApi);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // JAX-RS Response.entity() - same as JAX-RS
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

        // Quarkus Reactive Routes - RoutingContext.end()
        let routing_methods: HashSet<String> =
            ["end", "json"].iter().map(|s| (*s).to_string()).collect();

        let routing_receivers: HashSet<String> =
            ["RoutingContext", "routingContext", "HttpServerResponse"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern_with_context(
            routing_receivers,
            routing_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // Quarkus Panache - SQL injection (if using raw queries)
        let panache_methods: HashSet<String> = ["find", "update", "delete"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let panache_receivers: HashSet<String> = ["PanacheEntity", "PanacheRepository"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            panache_receivers,
            panache_methods,
            VulnType::SqlInjection,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // No Quarkus-specific sanitizers (uses JAX-RS and standard Java sanitization)
    }

    fn is_source_annotation(&self, annotation_name: &str) -> bool {
        // Quarkus uses JAX-RS annotations for REST endpoints
        matches!(
            annotation_name,
            "QueryParam"
                | "HeaderParam"
                | "CookieParam"
                | "PathParam"
                | "FormParam"
                | "BeanParam"
                // Quarkus-specific config injection (also untrusted in some contexts)
                | "ConfigProperty"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceKind;

    #[test]
    fn test_quarkus_framework_metadata() {
        let framework = QuarkusFramework;
        assert_eq!(framework.id(), "quarkus");
        assert_eq!(framework.name(), "Quarkus");
    }

    #[test]
    fn test_quarkus_dependency_patterns() {
        let framework = QuarkusFramework;
        let deps = framework.dependency_patterns();
        assert!(deps.contains(&"io.quarkus:quarkus-core"));
        assert!(deps.contains(&"io.quarkus:quarkus-resteasy"));
    }

    #[test]
    fn test_quarkus_import_patterns() {
        let framework = QuarkusFramework;
        let imports = framework.import_patterns();
        assert!(imports.contains(&"io.quarkus"));
        assert!(imports.contains(&"jakarta.ws.rs"));
        assert!(imports.contains(&"javax.ws.rs"));
    }

    #[test]
    fn test_quarkus_signature_annotations() {
        let framework = QuarkusFramework;
        let annotations = framework.signature_annotations();
        assert!(annotations.contains(&"Path"));
        assert!(annotations.contains(&"GET"));
        assert!(annotations.contains(&"POST"));
        assert!(annotations.contains(&"QuarkusTest"));
    }

    #[test]
    fn test_quarkus_sources() {
        let framework = QuarkusFramework;
        let mut registry = SourceRegistry::empty();
        framework.register_sources(&mut registry);

        // JAX-RS UriInfo methods should be registered
        assert_eq!(
            registry.is_source(Some("UriInfo"), "getQueryParameters"),
            Some(SourceKind::HttpRequest)
        );

        // Reactive RoutingContext methods
        assert_eq!(
            registry.is_source(Some("RoutingContext"), "queryParam"),
            Some(SourceKind::HttpRequest)
        );

        // REST Client methods
        assert_eq!(
            registry.is_source(Some("RestClient"), "get"),
            Some(SourceKind::ExternalApi)
        );
    }

    #[test]
    fn test_quarkus_sinks() {
        let framework = QuarkusFramework;
        let mut registry = SinkRegistry::empty();
        framework.register_sinks(&mut registry);

        // JAX-RS Response methods should be registered
        assert!(registry.is_sink(Some("Response"), "entity").is_some());
        assert!(registry.is_sink(Some("Response"), "seeOther").is_some());

        // Reactive routes
        assert!(registry.is_sink(Some("RoutingContext"), "end").is_some());

        // Panache methods
        assert!(registry
            .is_sink(Some("PanacheRepository"), "find")
            .is_some());
    }

    #[test]
    fn test_quarkus_source_annotations() {
        let framework = QuarkusFramework;

        // JAX-RS annotations
        assert!(framework.is_source_annotation("QueryParam"));
        assert!(framework.is_source_annotation("PathParam"));
        assert!(framework.is_source_annotation("HeaderParam"));

        // Quarkus-specific
        assert!(framework.is_source_annotation("ConfigProperty"));

        assert!(!framework.is_source_annotation("Path"));
        assert!(!framework.is_source_annotation("GET"));
    }
}
