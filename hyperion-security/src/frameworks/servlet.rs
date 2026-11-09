// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java Servlet API framework patterns
//!
//! Core servlet patterns used by multiple frameworks (Spring, JAX-RS, etc.).

use crate::finding::VulnType;
use crate::frameworks::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::SinkRegistry;
use crate::sources::{SourceKind, SourceRegistry};
use std::collections::HashSet;

/// Java Servlet API framework
///
/// This is a "base" framework that provides core servlet API patterns.
/// Many other frameworks (Spring MVC, JAX-RS implementations) build on top of servlets.
pub struct ServletFramework;

impl Framework for ServletFramework {
    fn id(&self) -> &'static str {
        "servlet"
    }

    fn name(&self) -> &'static str {
        "Java Servlet API"
    }

    fn dependency_patterns(&self) -> Vec<&'static str> {
        vec![
            "javax.servlet:javax.servlet-api",
            "jakarta.servlet:jakarta.servlet-api",
        ]
    }

    fn import_patterns(&self) -> Vec<&'static str> {
        vec!["javax.servlet", "jakarta.servlet"]
    }

    fn signature_annotations(&self) -> Vec<&'static str> {
        vec!["WebServlet"]
    }

    fn register_sources(&self, registry: &mut SourceRegistry) {
        // HttpServletRequest methods
        let servlet_methods: HashSet<String> = [
            "getParameter",
            "getParameterValues",
            "getParameterMap",
            "getHeader",
            "getHeaders",
            "getCookies",
            "getInputStream",
            "getReader",
            "getRequestURI",
            "getRequestURL",
            "getQueryString",
            "getPathInfo",
            "getRemoteUser",
            "getAttribute",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let servlet_receivers: HashSet<String> = [
            "HttpServletRequest",
            "ServletRequest",
            "request",
            "req",
            "httpRequest",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(servlet_receivers, servlet_methods, SourceKind::HttpRequest);
    }

    fn register_sinks(&self, registry: &mut SinkRegistry) {
        // Response output
        let response_methods: HashSet<String> = ["write", "print", "println", "append"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let response_receivers: HashSet<String> = [
            "PrintWriter",
            "writer",
            "out",
            "ServletOutputStream",
            "outputStream",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(
            response_receivers,
            response_methods,
            VulnType::Xss,
            vec![0],
            false,
        );

        // Servlet redirect
        let servlet_redirect_methods: HashSet<String> =
            std::iter::once("sendRedirect".to_string()).collect();

        let servlet_redirect_receivers: HashSet<String> = ["HttpServletResponse", "response"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        registry.register_pattern(
            servlet_redirect_receivers,
            servlet_redirect_methods,
            VulnType::OpenRedirect,
            vec![0],
            false,
        );

        // Servlet dispatcher for path traversal
        let dispatch_methods: HashSet<String> = ["getRequestDispatcher", "getResource"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let dispatch_receivers: HashSet<String> = [
            "ServletContext",
            "servletContext",
            "HttpServletRequest",
            "request",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        registry.register_pattern(
            dispatch_receivers,
            dispatch_methods,
            VulnType::PathTraversal,
            vec![0],
            false,
        );
    }

    fn register_sanitizers(&self, _registry: &mut SanitizerRegistry) {
        // No servlet-specific sanitizers
    }

    fn is_source_annotation(&self, _annotation_name: &str) -> bool {
        // Servlets don't use parameter annotations - that's Spring/JAX-RS
        false
    }
}
