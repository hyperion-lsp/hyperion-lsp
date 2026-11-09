// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Spring Framework security patterns
//!
//! Patterns for Spring MVC, Spring Boot, and Spring Web.

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::ContextValidator;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use crate::taint::VulnTypes;
use std::collections::HashSet;

/// Spring Framework (Spring MVC, Spring Boot, Spring Web)
pub struct SpringFramework;

impl Framework for SpringFramework {
    fn id(&self) -> &'static str {
        "spring"
    }

    fn name(&self) -> &'static str {
        "Spring Framework"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "org.springframework:spring-web",
            "org.springframework:spring-webmvc",
            "org.springframework.boot:spring-boot-starter-web",
            "org.springframework:spring-jdbc",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec![
            "org.springframework.web",
            "org.springframework.http",
            "org.springframework.stereotype",
            "org.springframework.jdbc",
        ]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        vec![
            "RestController",
            "Controller",
            "RequestMapping",
            "GetMapping",
            "PostMapping",
            "PutMapping",
            "DeleteMapping",
            "PatchMapping",
        ]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // Spring WebRequest
        let spring_methods: HashSet<String> = [
            "getParameter",
            "getHeader",
            "getParameterValues",
            "getParameterMap",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let spring_receivers: HashSet<String> = ["WebRequest", "NativeWebRequest", "webRequest"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(spring_receivers, spring_methods, SourceKind::HttpRequest);

        // Multipart file uploads (untrusted filenames and content)
        let upload_methods: HashSet<String> = ["getOriginalFilename", "getInputStream", "getBytes"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let upload_receivers: HashSet<String> = ["MultipartFile", "multipartFile"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(upload_receivers, upload_methods, SourceKind::HttpRequest);

        // Spring RestTemplate - external API sources
        let rest_methods: HashSet<String> = [
            "getForObject",
            "getForEntity",
            "postForObject",
            "exchange",
            "getBody",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let rest_receivers: HashSet<String> = ["RestTemplate", "restTemplate", "ResponseEntity"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(rest_receivers, rest_methods, SourceKind::ExternalApi);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // Spring Model - XSS when rendering to views
        let model_methods: HashSet<String> = ["addAttribute", "addObject", "put"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let model_receivers: HashSet<String> = ["Model", "model", "ModelAndView", "modelAndView"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            model_receivers,
            model_methods,
            VulnType::Xss,
            vec![1], // Second param is the value
            false,
        );

        // Spring ResponseEntity - body() and ok() methods
        // These need context validation - JSON responses aren't XSS-vulnerable
        let response_entity_methods: HashSet<String> =
            ["body", "ok"].iter().map(|s| (*s).to_string()).collect();

        let response_entity_receivers: HashSet<String> = [
            "ResponseEntity",
            "responseEntity",
            "BodyBuilder",
            "bodyBuilder",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        // Register sink pattern with context validation
        registry.register_pattern_with_context(
            response_entity_receivers,
            response_entity_methods,
            VulnType::Xss,
            vec![0],
            false,
            true,
            Some(ContextValidator::RestControllerContext),
        );

        // OWASP WebGoat's Spring lessons render AttackResultBuilder.output() as HTML.
        // Chained factory calls are represented by their root receiver name, so include both
        // the builder types/aliases and the static factory methods used by WebGoat.
        let webgoat_methods: HashSet<String> = std::iter::once("output".to_string()).collect();
        let webgoat_receivers: HashSet<String> = [
            "AttackResult",
            "attackResult",
            "AttackResultBuilder",
            "attackResultBuilder",
            "Builder",
            "builder",
            "success",
            "failed",
            "informationMessage",
        ]
        .iter()
        .map(|receiver| (*receiver).to_string())
        .collect();

        registry.register_pattern(
            webgoat_receivers,
            webgoat_methods,
            VulnType::Xss,
            vec![0],
            false,
        );

        // Spring JdbcTemplate - SQL injection
        let jdbc_methods: HashSet<String> = [
            "execute",
            "query",
            "update",
            "batchUpdate",
            "queryForObject",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let jdbc_receivers: HashSet<String> = [
            "JdbcTemplate",
            "jdbcTemplate",
            "NamedParameterJdbcTemplate",
            "namedParameterJdbcTemplate",
        ]
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

        // Spring RedirectView - open redirect
        let spring_redirect_methods: HashSet<String> = ["setUrl", "setRedirectUrl"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let spring_redirect_receivers: HashSet<String> = [
            "RedirectView",
            "redirectView",
            "ModelAndView",
            "modelAndView",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(
            spring_redirect_receivers,
            spring_redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, registry: &mut SanitizerRegistry) {
        // Spring HtmlUtils - HTML escaping
        let spring_methods: HashSet<String> = ["htmlEscape", "htmlEscapeDecimal", "htmlEscapeHex"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let spring_receivers: HashSet<String> = std::iter::once("HtmlUtils".to_string()).collect();

        registry.register_pattern(spring_receivers, spring_methods, VulnTypes::XSS);
    }

    fn is_source_annotation(&self, annotation_name: &str) -> bool {
        matches!(
            annotation_name,
            "RequestParam"
                | "RequestHeader"
                | "CookieValue"
                | "PathVariable"
                | "RequestBody"
                | "ModelAttribute"
        )
    }
}
