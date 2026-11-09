// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Annotation reference resolution for goto-definition navigation.

use hyperion_parser::syntax::SyntaxKind;
use hyperion_workspace::{DeclarationNamespace, SymbolTarget, TypeNameResolution};
use rowan::SyntaxToken;
use tower_lsp::lsp_types::Location;
use tracing::debug;

use crate::navigation::context::NavigationContext;
use crate::navigation::provider::NavigationProvider;

impl NavigationProvider<'_> {
    /// Resolve annotation references (e.g., @Deprecated, @Override, @MyCustomAnnotation)
    ///
    /// When the cursor is on an annotation name, navigate to the annotation's definition.
    /// Annotations are defined as `@interface AnnotationName { ... }` in Java.
    pub(crate) fn resolve_annotation_reference(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> Result<Option<Location>, String> {
        // Check if the token is the annotation name (in QUALIFIED_NAME, directly under ANNOTATION)
        // We should only handle the annotation name, not parameters inside the annotation
        let Some(parent) = token.parent() else {
            return Ok(None);
        };

        // The token must be inside QUALIFIED_NAME which is directly under ANNOTATION
        // This excludes tokens inside annotation parameters (ARG_LIST)
        if parent.kind() != SyntaxKind::QUALIFIED_NAME {
            return Ok(None);
        }

        let annotation_node = parent
            .parent()
            .filter(|p| p.kind() == SyntaxKind::ANNOTATION);
        let Some(annotation_node) = annotation_node else {
            return Ok(None);
        };

        // Extract the annotation name from the QUALIFIED_NAME child
        // Annotation structure: ANNOTATION -> AT + QUALIFIED_NAME + (optional args)
        let mut annotation_name = String::new();

        for child in annotation_node.children() {
            if child.kind() == SyntaxKind::QUALIFIED_NAME {
                // Collect all IDENT tokens to form the qualified name
                let mut parts = Vec::new();
                for token_or_node in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token_or_node {
                        if t.kind() == SyntaxKind::IDENT {
                            parts.push(t.text().to_string());
                        }
                    }
                }
                annotation_name = parts.join(".");
                break;
            }
        }

        if annotation_name.is_empty() {
            return Ok(None);
        }

        debug!("NAV: Looking up annotation: {}", annotation_name);
        let file_id = ctx.file_id;

        let lexical_owners = self
            .lexical_type_owners_at_offset(&ctx.parse_tree.syntax(), token.text_range().start());
        let TypeNameResolution::Exact(fqn) =
            self.workspace_index
                .resolve_type_name(file_id, &lexical_owners, &annotation_name)
        else {
            debug!(
                "NAV: Annotation '{}' is missing or ambiguous",
                annotation_name
            );
            return Ok(None);
        };
        let Some(definition) = self
            .workspace_index
            .find_symbol_definition(&SymbolTarget::new(fqn.as_str(), DeclarationNamespace::Type))
        else {
            return Ok(None);
        };
        self.workspace_index.record_symbol_usage(&fqn);
        self.symbol_location_to_lsp_location(&definition.location)
    }
}
