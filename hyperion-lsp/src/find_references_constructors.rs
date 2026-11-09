// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constructor-specific reference finding.
//!
//! Handles finding references to constructor definitions:
//! - Signature-aware constructor overload matching
//! - Constructor call (`new Foo()`) scanning in files
//! - Constructor reference (`Foo::new`) scanning
//!
//! General-purpose signature utilities (matching, inference, type resolution)
//! live in [`crate::signature_matching`].

use crate::navigation::NavigationProvider;
use hyperion_semantics::db::FileId;
use hyperion_workspace::{DeclarationNamespace, SymbolTarget};
use rowan::TextSize;
use std::collections::HashSet;
use tower_lsp::lsp_types::{Location, Url};
use tracing::debug;

impl NavigationProvider<'_> {
    pub(crate) fn find_constructor_references(
        &self,
        _file_id: FileId,
        token: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        _source_text: &str,
        _uri: &Url,
        include_declaration: bool,
        root: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<Vec<Location>, String> {
        let offset = u32::from(token.text_range().start());
        let class_fqn = match self.find_enclosing_class(root, TextSize::from(offset)) {
            Some(name) => name,
            None => return Ok(Vec::new()),
        };

        let token_start = u32::from(token.text_range().start());
        let token_end = u32::from(token.text_range().end());

        let mut references = Vec::new();
        // O(1) dedup: track (uri-as-string, start-line, start-char, end-line, end-char)
        let mut seen: HashSet<(String, u32, u32, u32, u32)> = HashSet::new();

        // Phase 2: Extract parameter types from constructor definition
        // Find the constructor definition node that contains this token
        let constructor_fqn =
            if let Some(ctor_node) = self.find_constructor_node_at_position(root, token) {
                // Extract parameter types from the constructor definition
                let param_types = self.extract_constructor_parameter_types(&ctor_node);

                // Get simple class name from class_fqn (last part after last dot)
                let simple_name = class_fqn.rsplit('.').next().unwrap_or(&class_fqn);

                // Build full constructor FQN: com.example.Person.Person(String,int)
                format!("{}.{}({})", class_fqn, simple_name, param_types)
            } else {
                // Fallback to class FQN if we can't find the constructor node
                // This maintains backward compatibility with old behavior
                class_fqn.clone()
            };

        debug!(
            "Finding references for constructor: {} (token range: {}..{})",
            constructor_fqn, token_start, token_end
        );

        if include_declaration {
            if let Some(ctor_fqn) = self
                .workspace_index
                .get_constructors_for_class(&class_fqn)
                .into_iter()
                .find(|ctor| ctor.range.start == token_start && ctor.range.end == token_end)
                .map(|ctor| ctor.fqn.clone())
            {
                if let Some(revisioned) = self
                    .workspace_index
                    .find_symbol_definition(&SymbolTarget::new(
                        ctor_fqn.clone(),
                        DeclarationNamespace::Constructor,
                    ))
                    .and_then(|definition| {
                        self.workspace_index
                            .revisioned_location(&definition.location)
                    })
                {
                    let symbol_loc = &revisioned.location;
                    if let Some(indexed) = self
                        .workspace_index
                        .get_indexed_revision_content(revisioned.revision)
                    {
                        let lsp_range = Self::validate_reference_range(
                            &indexed.content,
                            symbol_loc.range.start,
                            symbol_loc.range.end,
                            &symbol_loc.name,
                        )
                        .then(|| {
                            crate::navigation_util::byte_range_to_lsp_range(
                                &indexed.content,
                                symbol_loc.range.start,
                                symbol_loc.range.end,
                            )
                        })
                        .flatten();
                        if let (Ok(symbol_uri), Some(range)) =
                            (Url::from_file_path(&indexed.file_path), lsp_range)
                        {
                            seen.insert((
                                symbol_uri.as_str().to_string(),
                                range.start.line,
                                range.start.character,
                                range.end.line,
                                range.end.character,
                            ));
                            references.push(Location {
                                uri: symbol_uri,
                                range,
                            });
                        }
                    }
                }
            }
        }

        // Constructor references: find instantiations matching the specific overload.
        // We use the parameter signature from the constructor definition to discriminate
        // between overloads (e.g., Person() vs Person(String) vs Person(String,int)).
        {
            let expected_sig =
                Self::extract_signature_from_fqn(&constructor_fqn).map(|s| s.to_string());
            let current = self
                .workspace_index
                .find_current_usages_for_symbol_target(&SymbolTarget::new(
                    class_fqn.clone(),
                    DeclarationNamespace::Type,
                ))
                .map_err(|error| format!("Constructor references unavailable: {error}"))?;

            for file in &current.files {
                let indexed = self
                    .workspace_index
                    .get_indexed_revision_content(file.revision)
                    .ok_or_else(|| {
                        "Constructor source changed while references were assembled".to_string()
                    })?;
                let language = hyperion_workspace::Language::from_path(&indexed.file_path)
                    .ok_or_else(|| "Constructor source language is unavailable".to_string())?;
                let candidate_uri = Url::from_file_path(&indexed.file_path)
                    .map_err(|()| "Constructor reference path is not a file URI".to_string())?;
                for usage in &file.locations {
                    match language {
                        hyperion_workspace::Language::Java => {
                            if usage.kind != tower_lsp::lsp_types::SymbolKind::CONSTRUCTOR {
                                continue;
                            }
                            if let Some(expected) = expected_sig.as_deref() {
                                let actual = Self::extract_signature_from_fqn(&usage.fqn)
                                    .ok_or_else(|| {
                                        "Constructor call signature is unavailable".to_string()
                                    })?;
                                if !self.signatures_match(expected, actual) {
                                    continue;
                                }
                            }
                        }
                        hyperion_workspace::Language::Kotlin => {
                            let mut offset = usage.range.end as usize;
                            let bytes = indexed.content.as_bytes();
                            while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
                                offset += 1;
                            }
                            if bytes.get(offset) == Some(&b'<') {
                                let mut depth = 0_u32;
                                while let Some(byte) = bytes.get(offset) {
                                    match byte {
                                        b'<' => depth += 1,
                                        b'>' => {
                                            depth -= 1;
                                            if depth == 0 {
                                                offset += 1;
                                                break;
                                            }
                                        }
                                        _ => {}
                                    }
                                    offset += 1;
                                }
                                while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
                                    offset += 1;
                                }
                            }
                            let constructor_reference = indexed.content
                                [..usage.range.start as usize]
                                .trim_end()
                                .ends_with("::");
                            if bytes.get(offset) != Some(&b'(') && !constructor_reference {
                                continue;
                            }
                        }
                        hyperion_workspace::Language::Groovy
                        | hyperion_workspace::Language::Scala => {
                            return Err(format!(
                                "Constructor references from {:?} are not yet exact",
                                language
                            ));
                        }
                    }
                    let Some(range) = crate::navigation_util::byte_range_to_lsp_range(
                        &indexed.content,
                        usage.range.start,
                        usage.range.end,
                    ) else {
                        return Err("Constructor usage range is invalid".to_string());
                    };
                    let location = Location {
                        uri: candidate_uri.clone(),
                        range,
                    };
                    let key = (
                        location.uri.as_str().to_string(),
                        location.range.start.line,
                        location.range.start.character,
                        location.range.end.line,
                        location.range.end.character,
                    );
                    if seen.insert(key) {
                        references.push(location);
                    }
                }
            }
            if !self
                .workspace_index
                .current_usage_locations_are_current(&current)
            {
                return Err(
                    "Workspace changed while constructor references were assembled".to_string(),
                );
            }
        }

        Ok(crate::reference_results::normalize(references))
    }
}
