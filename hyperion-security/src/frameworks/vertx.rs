// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Vert.x Framework security patterns
//!
//! Patterns for Vert.x, a reactive/event-driven framework for the JVM.
//! Vert.x uses a callback-based paradigm quite different from traditional servlet/Spring models.

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::ContextValidator;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use std::collections::HashSet;

/// Vert.x Framework
///
/// Vert.x is a toolkit for building reactive applications on the JVM.
/// It uses an event-driven, non-blocking I/O model with callback-based handlers.
pub struct VertxFramework;

impl Framework for VertxFramework {
    fn id(&self) -> &'static str {
        "vertx"
    }

    fn name(&self) -> &'static str {
        "Vert.x"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "io.vertx:vertx-core",
            "io.vertx:vertx-web",
            "io.vertx:vertx-web-client",
            "io.vertx:vertx-auth-common",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec!["io.vertx.core", "io.vertx.ext.web", "io.vertx.ext.auth"]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        // Vert.x doesn't use annotations for routing (uses fluent API)
        // But return some common markers for detection
        vec![]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // RoutingContext - main entry point for web handlers
        let routing_methods: HashSet<String> = [
            "request",
            "queryParam",
            "pathParam",
            "getBodyAsString",
            "getBodyAsJson",
            "getParam",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let routing_receivers: HashSet<String> =
            ["RoutingContext", "routingContext", "context", "ctx"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern(routing_receivers, routing_methods, SourceKind::HttpRequest);

        // HttpServerRequest - low-level request access
        let request_methods: HashSet<String> = [
            "getParam",
            "params",
            "getHeader",
            "headers",
            "path",
            "query",
            "absoluteURI",
            "formAttributes",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let request_receivers: HashSet<String> =
            ["HttpServerRequest", "httpServerRequest", "request"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern(request_receivers, request_methods, SourceKind::HttpRequest);

        // MultiMap - parameter/header containers
        let multimap_methods: HashSet<String> = ["get", "getAll", "entries"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        // Avoid treating common local variable names like `params`/`headers` as request sources.
        //
        // We prefer matching by declared type (`MultiMap`) via `var_types` inference in the
        // taint tracker, which prevents false-positives for unrelated `List<String> params`
        // or `Map<String, String> params` variables.
        let multimap_receivers: HashSet<String> = ["MultiMap", "multiMap"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            multimap_receivers,
            multimap_methods,
            SourceKind::HttpRequest,
        );

        // WebClient - external API sources
        let client_methods: HashSet<String> = [
            "send",
            "sendJson",
            "sendBuffer",
            "bodyAsString",
            "bodyAsJson",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let client_receivers: HashSet<String> = ["WebClient", "webClient", "HttpResponse"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(client_receivers, client_methods, SourceKind::ExternalApi);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // HttpServerResponse.end() - sends response body
        // Context-aware: JSON responses aren't XSS-vulnerable
        let response_end_methods: HashSet<String> = ["end", "write", "sendFile"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let response_receivers: HashSet<String> =
            ["HttpServerResponse", "httpServerResponse", "response"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern_with_context(
            response_receivers.clone(),
            response_end_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // RoutingContext.response().end() - convenience methods
        let routing_response_methods: HashSet<String> =
            ["end", "json"].iter().map(|s| (*s).to_string()).collect();

        let routing_receivers: HashSet<String> =
            ["RoutingContext", "routingContext", "context", "ctx"]
                .iter()
                .map(|s| (*s).to_string())
                .collect();

        registry.register_pattern_with_context(
            routing_receivers.clone(),
            routing_response_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // Redirect - open redirect
        let redirect_methods: HashSet<String> = [
            "redirect",
            "putHeader", // When used with "Location" header
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(
            response_receivers,
            redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );

        // RoutingContext.redirect() - convenience redirect
        let routing_redirect_methods: HashSet<String> = ["redirect", "reroute"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            routing_receivers,
            routing_redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );

        // Vert.x SQL Client - SQL injection
        let sql_methods: HashSet<String> = ["query", "preparedQuery", "execute"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let sql_receivers: HashSet<String> = [
            "SqlClient",
            "sqlClient",
            "SqlConnection",
            "PgPool",
            "MySQLPool",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(
            sql_receivers,
            sql_methods,
            VulnType::SqlInjection,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // No Vert.x-specific sanitizers (uses standard Java sanitization)
    }

    fn is_source_annotation(&self, _annotation_name: &str) -> bool {
        // Vert.x doesn't use annotations for request parameter binding
        // Parameters are accessed via RoutingContext methods
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceKind;

    #[test]
    fn test_vertx_framework_metadata() {
        let framework = VertxFramework;
        assert_eq!(framework.id(), "vertx");
        assert_eq!(framework.name(), "Vert.x");
    }

    #[test]
    fn test_vertx_dependency_patterns() {
        let framework = VertxFramework;
        let deps = framework.dependency_patterns();
        assert!(deps.contains(&"io.vertx:vertx-core"));
        assert!(deps.contains(&"io.vertx:vertx-web"));
    }

    #[test]
    fn test_vertx_import_patterns() {
        let framework = VertxFramework;
        let imports = framework.import_patterns();
        assert!(imports.contains(&"io.vertx.core"));
        assert!(imports.contains(&"io.vertx.ext.web"));
    }

    #[test]
    fn test_vertx_sources() {
        let framework = VertxFramework;
        let mut registry = SourceRegistry::empty();
        framework.register_sources(&mut registry);

        // RoutingContext methods should be registered
        assert_eq!(
            registry.is_source(Some("RoutingContext"), "queryParam"),
            Some(SourceKind::HttpRequest)
        );
        assert_eq!(
            registry.is_source(Some("context"), "getParam"),
            Some(SourceKind::HttpRequest)
        );

        // HttpServerRequest methods
        assert_eq!(
            registry.is_source(Some("HttpServerRequest"), "getParam"),
            Some(SourceKind::HttpRequest)
        );
        assert_eq!(
            registry.is_source(Some("request"), "params"),
            Some(SourceKind::HttpRequest)
        );

        // MultiMap methods
        assert_eq!(
            registry.is_source(Some("MultiMap"), "get"),
            Some(SourceKind::HttpRequest)
        );

        // WebClient methods
        assert_eq!(
            registry.is_source(Some("WebClient"), "send"),
            Some(SourceKind::ExternalApi)
        );
    }

    #[test]
    fn test_vertx_sinks() {
        let framework = VertxFramework;
        let mut registry = SinkRegistry::empty();
        framework.register_sinks(&mut registry);

        // HttpServerResponse methods should be registered
        assert!(registry
            .is_sink(Some("HttpServerResponse"), "end")
            .is_some());
        assert!(registry.is_sink(Some("response"), "write").is_some());

        // RoutingContext response methods
        assert!(registry.is_sink(Some("RoutingContext"), "end").is_some());
        assert!(registry.is_sink(Some("context"), "json").is_some());

        // Redirect methods
        assert!(registry
            .is_sink(Some("HttpServerResponse"), "redirect")
            .is_some());
        assert!(registry
            .is_sink(Some("RoutingContext"), "redirect")
            .is_some());

        // SQL methods
        assert!(registry.is_sink(Some("SqlClient"), "query").is_some());
        assert!(registry.is_sink(Some("PgPool"), "execute").is_some());
    }

    #[test]
    fn test_vertx_no_source_annotations() {
        let framework = VertxFramework;

        // Vert.x doesn't use annotations for parameter binding
        assert!(!framework.is_source_annotation("QueryParam"));
        assert!(!framework.is_source_annotation("PathParam"));
        assert!(!framework.is_source_annotation("RequestParam"));
    }
}
