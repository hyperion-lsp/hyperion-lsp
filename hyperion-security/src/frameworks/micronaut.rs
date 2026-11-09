// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Micronaut Framework security patterns
//!
//! Patterns for Micronaut, a modern JVM framework with compile-time dependency injection.

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::ContextValidator;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use std::collections::HashSet;

/// Micronaut Framework
///
/// Micronaut is a modern, JVM-based framework with compile-time DI and AOP.
/// Similar to Spring but with less runtime overhead.
pub struct MicronautFramework;

impl Framework for MicronautFramework {
    fn id(&self) -> &'static str {
        "micronaut"
    }

    fn name(&self) -> &'static str {
        "Micronaut"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "io.micronaut:micronaut-http-server",
            "io.micronaut:micronaut-http-client",
            "io.micronaut:micronaut-runtime",
            "io.micronaut:micronaut-inject",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec![
            "io.micronaut.http",
            "io.micronaut.security",
            "io.micronaut.runtime",
        ]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        vec![
            "Controller",
            "Get",
            "Post",
            "Put",
            "Delete",
            "Patch",
            "Head",
            "Options",
        ]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // Micronaut HttpRequest
        let request_methods: HashSet<String> = [
            "getParameters",
            "getHeaders",
            "getBody",
            "getCookies",
            "getPath",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let request_receivers: HashSet<String> = ["HttpRequest", "httpRequest", "request"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(request_receivers, request_methods, SourceKind::HttpRequest);

        // Micronaut HttpParameters - query/form parameters
        let params_methods: HashSet<String> = ["get", "getAll", "values"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let params_receivers: HashSet<String> = ["HttpParameters", "parameters"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(params_receivers, params_methods, SourceKind::HttpRequest);

        // Micronaut HttpClient - external API sources
        let client_methods: HashSet<String> = ["retrieve", "toBlocking", "blockingFirst", "body"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let client_receivers: HashSet<String> = ["HttpClient", "httpClient", "BlockingHttpClient"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(client_receivers, client_methods, SourceKind::ExternalApi);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // Micronaut HttpResponse.ok() and body() - similar to Spring ResponseEntity
        // Need context validation - JSON responses aren't XSS-vulnerable
        let response_methods: HashSet<String> = ["ok", "body", "created", "accepted"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let response_receivers: HashSet<String> =
            ["HttpResponse", "httpResponse", "MutableHttpResponse"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        // Register sink pattern with context validation
        registry.register_pattern_with_context(
            response_receivers,
            response_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // Micronaut redirect - open redirect
        let redirect_methods: HashSet<String> = [
            "redirect",
            "permanentRedirect",
            "temporaryRedirect",
            "seeOther",
            "uri",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let redirect_receivers: HashSet<String> =
            ["HttpResponse", "httpResponse", "MutableHttpResponse"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern(
            redirect_receivers,
            redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );

        // Micronaut Data JDBC - SQL injection
        // Similar to Spring JdbcTemplate
        let jdbc_methods: HashSet<String> = ["execute", "query", "update", "findOne", "findAll"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let jdbc_receivers: HashSet<String> = ["JdbcOperations", "jdbcOperations", "PreparedQuery"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            jdbc_receivers,
            jdbc_methods,
            VulnType::SqlInjection,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // No Micronaut-specific sanitizers (uses standard Java sanitization)
    }

    fn is_source_annotation(&self, annotation_name: &str) -> bool {
        matches!(
            annotation_name,
            "QueryValue" | "PathVariable" | "Header" | "CookieValue" | "Body" | "Part"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceKind;

    #[test]
    fn test_micronaut_framework_metadata() {
        let framework = MicronautFramework;
        assert_eq!(framework.id(), "micronaut");
        assert_eq!(framework.name(), "Micronaut");
    }

    #[test]
    fn test_micronaut_dependency_patterns() {
        let framework = MicronautFramework;
        let deps = framework.dependency_patterns();
        assert!(deps.contains(&"io.micronaut:micronaut-http-server"));
        assert!(deps.contains(&"io.micronaut:micronaut-http-client"));
    }

    #[test]
    fn test_micronaut_import_patterns() {
        let framework = MicronautFramework;
        let imports = framework.import_patterns();
        assert!(imports.contains(&"io.micronaut.http"));
        assert!(imports.contains(&"io.micronaut.security"));
    }

    #[test]
    fn test_micronaut_signature_annotations() {
        let framework = MicronautFramework;
        let annotations = framework.signature_annotations();
        assert!(annotations.contains(&"Controller"));
        assert!(annotations.contains(&"Get"));
        assert!(annotations.contains(&"Post"));
    }

    #[test]
    fn test_micronaut_sources() {
        let framework = MicronautFramework;
        let mut registry = SourceRegistry::empty();
        framework.register_sources(&mut registry);

        // HttpRequest methods should be registered
        assert_eq!(
            registry.is_source(Some("HttpRequest"), "getParameters"),
            Some(SourceKind::HttpRequest)
        );
        assert_eq!(
            registry.is_source(Some("request"), "getHeaders"),
            Some(SourceKind::HttpRequest)
        );

        // HttpParameters methods
        assert_eq!(
            registry.is_source(Some("HttpParameters"), "get"),
            Some(SourceKind::HttpRequest)
        );

        // HttpClient methods
        assert_eq!(
            registry.is_source(Some("HttpClient"), "retrieve"),
            Some(SourceKind::ExternalApi)
        );
    }

    #[test]
    fn test_micronaut_sinks() {
        let framework = MicronautFramework;
        let mut registry = SinkRegistry::empty();
        framework.register_sinks(&mut registry);

        // HttpResponse methods should be registered
        assert!(registry.is_sink(Some("HttpResponse"), "ok").is_some());
        assert!(registry
            .is_sink(Some("MutableHttpResponse"), "body")
            .is_some());

        // Redirect methods
        assert!(registry.is_sink(Some("HttpResponse"), "redirect").is_some());

        // JDBC methods
        assert!(registry
            .is_sink(Some("JdbcOperations"), "execute")
            .is_some());
    }

    #[test]
    fn test_micronaut_source_annotations() {
        let framework = MicronautFramework;

        assert!(framework.is_source_annotation("QueryValue"));
        assert!(framework.is_source_annotation("PathVariable"));
        assert!(framework.is_source_annotation("Header"));
        assert!(framework.is_source_annotation("Body"));

        assert!(!framework.is_source_annotation("Controller"));
        assert!(!framework.is_source_annotation("Get"));
    }
}
