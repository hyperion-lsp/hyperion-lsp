// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests verifying that framework implementations register their expected patterns

use hyperion_security::finding::VulnType;
use hyperion_security::frameworks::{Framework, FrameworkRegistry};
use hyperion_security::sanitizers::SanitizerRegistry;
use hyperion_security::sinks::SinkRegistry;
use hyperion_security::sources::{SourceKind, SourceRegistry};
use hyperion_security::taint::VulnTypes;

#[test]
fn test_servlet_framework_sources() {
    let mut sources = SourceRegistry::empty();
    let servlet = hyperion_security::frameworks::servlet::ServletFramework;
    servlet.register_sources(&mut sources);

    // HttpServletRequest methods should be registered
    assert_eq!(
        sources.is_source(Some("HttpServletRequest"), "getParameter"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("HttpServletRequest"), "getHeader"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("HttpServletRequest"), "getCookies"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("request"), "getParameter"),
        Some(SourceKind::HttpRequest)
    );
}

#[test]
fn test_servlet_framework_sinks() {
    let mut sinks = SinkRegistry::empty();
    let servlet = hyperion_security::frameworks::servlet::ServletFramework;
    servlet.register_sinks(&mut sinks);

    // PrintWriter methods should be registered
    assert_eq!(
        sinks.is_sink(Some("PrintWriter"), "write"),
        Some(VulnType::Xss)
    );
    assert_eq!(
        sinks.is_sink(Some("PrintWriter"), "println"),
        Some(VulnType::Xss)
    );

    // Redirect should be registered
    assert_eq!(
        sinks.is_sink(Some("HttpServletResponse"), "sendRedirect"),
        Some(VulnType::OpenRedirect)
    );

    // Dispatcher should be registered
    assert_eq!(
        sinks.is_sink(Some("ServletContext"), "getRequestDispatcher"),
        Some(VulnType::PathTraversal)
    );
}

#[test]
fn test_spring_framework_sources() {
    let mut sources = SourceRegistry::empty();
    let spring = hyperion_security::frameworks::spring::SpringFramework;
    spring.register_sources(&mut sources);

    // WebRequest methods should be registered
    assert_eq!(
        sources.is_source(Some("WebRequest"), "getParameter"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("WebRequest"), "getHeader"),
        Some(SourceKind::HttpRequest)
    );

    // MultipartFile methods should be registered
    assert_eq!(
        sources.is_source(Some("MultipartFile"), "getOriginalFilename"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("MultipartFile"), "getBytes"),
        Some(SourceKind::HttpRequest)
    );

    // RestTemplate methods should be registered
    assert_eq!(
        sources.is_source(Some("RestTemplate"), "getForObject"),
        Some(SourceKind::ExternalApi)
    );
}

#[test]
fn test_spring_framework_sinks() {
    let mut sinks = SinkRegistry::empty();
    let spring = hyperion_security::frameworks::spring::SpringFramework;
    spring.register_sinks(&mut sinks);

    // Model methods should be registered
    assert_eq!(
        sinks.is_sink(Some("Model"), "addAttribute"),
        Some(VulnType::Xss)
    );

    // ResponseEntity methods should be registered
    assert_eq!(
        sinks.is_sink(Some("ResponseEntity"), "ok"),
        Some(VulnType::Xss)
    );
    assert_eq!(
        sinks.is_sink(Some("ResponseEntity"), "body"),
        Some(VulnType::Xss)
    );

    // JdbcTemplate methods should be registered
    assert_eq!(
        sinks.is_sink(Some("JdbcTemplate"), "execute"),
        Some(VulnType::SqlInjection)
    );
    assert_eq!(
        sinks.is_sink(Some("JdbcTemplate"), "query"),
        Some(VulnType::SqlInjection)
    );

    // RedirectView methods should be registered
    assert_eq!(
        sinks.is_sink(Some("RedirectView"), "setUrl"),
        Some(VulnType::OpenRedirect)
    );
}

#[test]
fn test_spring_framework_sanitizers() {
    let mut sanitizers = SanitizerRegistry::empty();
    let spring = hyperion_security::frameworks::spring::SpringFramework;
    spring.register_sanitizers(&mut sanitizers);

    // HtmlUtils methods should be registered
    let sanitizes = sanitizers.is_sanitizer(Some("HtmlUtils"), "htmlEscape");
    assert!(sanitizes.is_some());
    assert!(sanitizes.unwrap().contains(VulnTypes::XSS));
}

#[test]
fn test_spring_framework_annotations() {
    let spring = hyperion_security::frameworks::spring::SpringFramework;

    assert!(spring.is_source_annotation("RequestParam"));
    assert!(spring.is_source_annotation("PathVariable"));
    assert!(spring.is_source_annotation("RequestHeader"));
    assert!(spring.is_source_annotation("CookieValue"));
    assert!(spring.is_source_annotation("RequestBody"));
    assert!(spring.is_source_annotation("ModelAttribute"));

    assert!(!spring.is_source_annotation("Autowired"));
    assert!(!spring.is_source_annotation("Component"));
}

#[test]
fn test_jaxrs_framework_sources() {
    let mut sources = SourceRegistry::empty();
    let jaxrs = hyperion_security::frameworks::jaxrs::JaxRsFramework;
    jaxrs.register_sources(&mut sources);

    // UriInfo methods should be registered
    assert_eq!(
        sources.is_source(Some("UriInfo"), "getQueryParameters"),
        Some(SourceKind::HttpRequest)
    );
    assert_eq!(
        sources.is_source(Some("UriInfo"), "getPathParameters"),
        Some(SourceKind::HttpRequest)
    );
}

#[test]
fn test_jaxrs_framework_sinks() {
    let mut sinks = SinkRegistry::empty();
    let jaxrs = hyperion_security::frameworks::jaxrs::JaxRsFramework;
    jaxrs.register_sinks(&mut sinks);

    // Response.entity() should be registered
    assert_eq!(
        sinks.is_sink(Some("Response"), "entity"),
        Some(VulnType::Xss)
    );

    // Redirect methods should be registered
    assert_eq!(
        sinks.is_sink(Some("Response"), "seeOther"),
        Some(VulnType::OpenRedirect)
    );
    assert_eq!(
        sinks.is_sink(Some("Response"), "temporaryRedirect"),
        Some(VulnType::OpenRedirect)
    );
}

#[test]
fn test_jaxrs_framework_annotations() {
    let jaxrs = hyperion_security::frameworks::jaxrs::JaxRsFramework;

    assert!(jaxrs.is_source_annotation("QueryParam"));
    assert!(jaxrs.is_source_annotation("PathParam"));
    assert!(jaxrs.is_source_annotation("HeaderParam"));
    assert!(jaxrs.is_source_annotation("CookieParam"));
    assert!(jaxrs.is_source_annotation("FormParam"));
    assert!(jaxrs.is_source_annotation("BeanParam"));

    assert!(!jaxrs.is_source_annotation("Path"));
    assert!(!jaxrs.is_source_annotation("GET"));
}

#[test]
fn test_framework_registry_integration() {
    let mut registry = FrameworkRegistry::with_builtin();

    // Should have all frameworks registered
    assert!(registry.has_framework("servlet"));
    assert!(registry.has_framework("spring"));
    assert!(registry.has_framework("jaxrs"));
    assert!(registry.has_framework("dropwizard"));

    // All should be enabled by default
    assert!(registry.is_enabled("servlet"));
    assert!(registry.is_enabled("spring"));
    assert!(registry.is_enabled("jaxrs"));
    assert!(registry.is_enabled("dropwizard"));

    // Rebuild registries
    let (sources, sinks, sanitizers) = registry.rebuild_registries();

    // Test that patterns from multiple frameworks are combined

    // Servlet sources
    assert!(sources
        .is_source(Some("HttpServletRequest"), "getParameter")
        .is_some());

    // Spring sources
    assert!(sources
        .is_source(Some("WebRequest"), "getParameter")
        .is_some());

    // JAX-RS sources
    assert!(sources
        .is_source(Some("UriInfo"), "getQueryParameters")
        .is_some());

    // Servlet sinks
    assert!(sinks.is_sink(Some("PrintWriter"), "write").is_some());

    // Spring sinks
    assert!(sinks.is_sink(Some("Model"), "addAttribute").is_some());

    // JAX-RS sinks
    assert!(sinks.is_sink(Some("Response"), "entity").is_some());

    // Spring sanitizers
    assert!(sanitizers
        .is_sanitizer(Some("HtmlUtils"), "htmlEscape")
        .is_some());
}

#[test]
fn test_selective_framework_enabling() {
    let mut registry = FrameworkRegistry::with_builtin();

    // Only enable Spring
    registry.enable_frameworks(&["spring"]);

    let (sources, sinks, sanitizers) = registry.rebuild_registries();

    // Spring patterns should be present
    assert!(sources
        .is_source(Some("WebRequest"), "getParameter")
        .is_some());
    assert!(sinks.is_sink(Some("Model"), "addAttribute").is_some());
    assert!(sanitizers
        .is_sanitizer(Some("HtmlUtils"), "htmlEscape")
        .is_some());

    // Servlet patterns should NOT be present (not enabled)
    assert!(sources
        .is_source(Some("HttpServletRequest"), "getParameter")
        .is_none());
    assert!(sinks.is_sink(Some("PrintWriter"), "write").is_none());

    // JAX-RS patterns should NOT be present (not enabled)
    assert!(sources
        .is_source(Some("UriInfo"), "getQueryParameters")
        .is_none());
    assert!(sinks.is_sink(Some("Response"), "entity").is_none());
}

#[test]
fn test_annotation_detection_across_frameworks() {
    let registry = FrameworkRegistry::with_builtin();

    // Spring annotations
    assert!(registry.is_source_annotation("RequestParam"));
    assert!(registry.is_source_annotation("PathVariable"));

    // JAX-RS annotations
    assert!(registry.is_source_annotation("QueryParam"));
    assert!(registry.is_source_annotation("PathParam"));

    // Non-source annotations
    assert!(!registry.is_source_annotation("Autowired"));
    assert!(!registry.is_source_annotation("Path"));
}

#[test]
fn test_dropwizard_framework_metadata() {
    let dropwizard = hyperion_security::frameworks::dropwizard::DropwizardFramework;

    assert_eq!(dropwizard.id(), "dropwizard");
    assert_eq!(dropwizard.name(), "Dropwizard");

    // Should have Dropwizard dependencies
    let deps = dropwizard.dependency_patterns();
    assert!(deps.contains(&"io.dropwizard:dropwizard-core"));

    // Should have Dropwizard import patterns
    let imports = dropwizard.import_patterns();
    assert!(imports.contains(&"io.dropwizard"));
}
