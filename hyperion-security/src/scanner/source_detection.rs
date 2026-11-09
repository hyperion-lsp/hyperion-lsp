// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Source detection for taint analysis.
//!
//! Identifies taint sources from annotations (e.g., @RequestParam, @KafkaListener)
//! and known source types (e.g., HttpServletRequest).

use super::visitor::SecurityVisitor;

use crate::sources::SourceRegistry;

use hyperion_parser::syntax::{Annotation, AstNode, MethodDecl, Param};

impl SecurityVisitor<'_> {
    fn method_has_annotation_named(method: &MethodDecl, expected: &[&str]) -> bool {
        let method_node = method.syntax();
        let modifier_list = method_node
            .prev_sibling()
            .filter(|sibling| sibling.kind() == hyperion_parser::syntax::SyntaxKind::MODIFIER_LIST);

        std::iter::once(method_node.clone())
            .chain(modifier_list)
            .flat_map(|node| node.descendants())
            .filter_map(Annotation::cast)
            .filter_map(|annotation| annotation.name())
            .any(|name| {
                let name = name.as_string();
                let simple_name = name.split('.').next_back().unwrap_or(&name);
                expected.contains(&simple_name)
            })
    }

    pub(super) fn is_source_type(type_name: &str) -> bool {
        matches!(
            type_name,
            "HttpServletRequest"
                | "ServletRequest"
                | "HttpRequest"
                | "WebRequest"
                | "NativeWebRequest"
                | "MultipartRequest"
                | "MultipartHttpServletRequest"
        )
    }

    pub(super) fn method_has_listener_annotation(method: &MethodDecl) -> bool {
        // Check all descendants of the method for listener annotations
        #[cfg(test)]
        eprintln!("Checking method for listener annotations");

        Self::method_has_annotation_named(
            method,
            &["KafkaListener", "RabbitListener", "JmsListener"],
        )
    }

    /// Whether this method is directly exposed as an HTTP endpoint.
    ///
    /// Raw request body types such as `InputStream` do not need a parameter
    /// annotation in Spring MVC or JAX-RS, so the method annotation provides
    /// the request boundary for those parameters.
    pub(super) fn method_has_http_endpoint_annotation(method: &MethodDecl) -> bool {
        Self::method_has_annotation_named(
            method,
            &[
                "RequestMapping",
                "GetMapping",
                "PostMapping",
                "PutMapping",
                "DeleteMapping",
                "PatchMapping",
                "GET",
                "POST",
                "PUT",
                "DELETE",
                "PATCH",
                "HEAD",
                "OPTIONS",
                "Get",
                "Post",
                "Put",
                "Delete",
                "Patch",
                "Head",
                "Options",
            ],
        )
    }

    pub(super) fn is_raw_request_body_type(type_name: &str) -> bool {
        matches!(
            type_name,
            "InputStream" | "Reader" | "BufferedReader" | "InputStreamReader"
        )
    }

    pub(super) fn param_is_tainted(param: &Param) -> bool {
        // Check for taint annotations on the parameter
        // Annotations can be inside MODIFIER_LIST or directly on the param
        #[cfg(test)]
        eprintln!("Checking param for taint: {:?}", param.syntax().text());

        // Check all descendants for annotations
        for desc in param.syntax().descendants() {
            if let Some(annotation) = Annotation::cast(desc) {
                if let Some(name) = annotation.name() {
                    let name_str = name.as_string();
                    // Check last part of qualified name
                    let simple_name = name_str.split('.').next_back().unwrap_or(&name_str);
                    #[cfg(test)]
                    eprintln!("  Found annotation: {}", simple_name);
                    if SourceRegistry::is_source_annotation(simple_name) {
                        #[cfg(test)]
                        eprintln!("  -> This is a taint source annotation!");
                        return true;
                    }
                }
            }
        }
        false
    }
}
