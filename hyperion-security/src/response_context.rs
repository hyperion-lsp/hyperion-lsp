// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Response context analysis for reducing XSS false positives
//!
//! This module determines whether a method is in a REST API context
//! where responses are JSON (not HTML), which makes XSS impossible.

use crate::annotation_utils::{has_produces_media_type, is_rest_controller};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode, TypeDecl};
use tracing::debug;

/// Context information about a response sink
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseContext {
    /// JSON response - XSS not applicable
    Json,
    /// HTML response - XSS applicable
    Html,
    /// Unknown response type - assume HTML for safety
    Unknown,
}

/// Analyzer for determining response context
pub struct ResponseContextAnalyzer;

impl ResponseContextAnalyzer {
    /// Create a new response context analyzer
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Determine the response context for a method
    ///
    /// # Arguments
    /// * `method_node` - The syntax node of the method containing the response
    ///
    /// # Returns
    /// The response context (JSON, HTML, or Unknown)
    pub fn analyze_response_context(&self, method_node: &SyntaxNode) -> ResponseContext {
        // Find the containing class
        let class_node = Self::find_containing_class(method_node);

        if let Some(ref class) = class_node {
            // Check if this is a REST controller
            if let Some(type_decl) = TypeDecl::cast(class.clone()) {
                if is_rest_controller(&type_decl) {
                    let class_name = type_decl.name().map(|n| n.text().to_string());
                    debug!(
                        class = ?class_name,
                        "Class is REST controller - JSON context"
                    );
                    return ResponseContext::Json;
                }
            }

            // Check if the method or class has produces = "application/json"
            if has_produces_media_type(class, "application/json") {
                debug!("Class has JSON produces annotation");
                return ResponseContext::Json;
            }
        }

        // Check the method itself for produces annotation
        if has_produces_media_type(method_node, "application/json") {
            debug!("Method has JSON produces annotation");
            return ResponseContext::Json;
        }

        // Check if the method or class has produces = "text/html"
        if let Some(ref class) = class_node {
            if has_produces_media_type(class, "text/html") {
                debug!("Class has HTML produces annotation");
                return ResponseContext::Html;
            }
        }

        if has_produces_media_type(method_node, "text/html") {
            debug!("Method has HTML produces annotation");
            return ResponseContext::Html;
        }

        // Default to unknown (conservative - treat as HTML)
        debug!("Unknown response context - assuming HTML");
        ResponseContext::Unknown
    }

    /// Check if a response context should skip XSS checks
    ///
    /// JSON responses cannot have XSS vulnerabilities because:
    /// 1. Spring @RestController returns JSON by default
    /// 2. Jackson escapes special characters automatically
    /// 3. JSON responses aren't rendered as HTML in the browser
    ///
    /// # Arguments
    /// * `context` - The response context
    ///
    /// # Returns
    /// `true` if XSS checks should be skipped, `false` otherwise
    pub const fn should_skip_xss(&self, context: &ResponseContext) -> bool {
        matches!(context, ResponseContext::Json)
    }

    /// Find the containing class for a method node
    fn find_containing_class(node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut current = node.parent()?;

        loop {
            if current.kind() == SyntaxKind::CLASS_DECL
                || current.kind() == SyntaxKind::INTERFACE_DECL
                || current.kind() == SyntaxKind::ENUM_DECL
            {
                return Some(current);
            }

            current = current.parent()?;
        }
    }
}

impl Default for ResponseContextAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
