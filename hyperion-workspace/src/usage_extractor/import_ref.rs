// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type references that live outside executable code: `import` declarations
//! and Javadoc reference tags (`{@link Type}`, `@see Type`, `@throws Type`).
//!
//! They are not part of the general usage cache (they must not keep an import
//! "used" or pollute call graphs); refactorings that must rewrite every
//! spelling of a type request them explicitly.

use crate::{import_resolver::ImportResolver, FileId, SymbolLocation, TextRange};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use lsp_types::SymbolKind;

use super::Usage;

/// One identifier of a dotted type chain with its absolute byte range.
struct Segment {
    text: String,
    start: u32,
    end: u32,
}

fn push_type_chain(
    segments: &[Segment],
    file_id: FileId,
    resolve: &impl Fn(&str) -> Option<String>,
    usages: &mut Vec<Usage>,
) {
    let mut prefix = String::new();
    for segment in segments {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(&segment.text);
        let Some(fqn) = resolve(&prefix) else {
            continue;
        };
        usages.push(Usage {
            fqn: fqn.clone(),
            location: SymbolLocation {
                file_id,
                source_revision_id: None,
                range: TextRange::new(segment.start, segment.end),
                kind: SymbolKind::CLASS,
                name: segment.text.as_str().into(),
                fqn: fqn.into(),
                container_name: None,
                documentation: None,
            },
        });
    }
}

/// Byte spans `(start, end)` of the reference expression of each Javadoc
/// reference tag in `text` (`Type#member(args)` without the tag itself).
pub fn javadoc_reference_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    for tag in ["@linkplain", "@link", "@see", "@throws", "@exception"] {
        for (at, _) in text.match_indices(tag) {
            let mut cursor = at + tag.len();
            // `@link` is a prefix of `@linkplain`; require a delimiter.
            if bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            {
                continue;
            }
            while bytes
                .get(cursor)
                .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'*'))
            {
                cursor += 1;
            }
            let start = cursor;
            // The type/member part ends at the first whitespace, `}` or `(`.
            while bytes.get(cursor).is_some_and(|byte| {
                !matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'}' | b'(' | b'*')
            }) {
                cursor += 1;
            }
            if cursor > start && !matches!(bytes[start], b'"' | b'<') {
                spans.push((start, cursor));
            }
        }
    }
    spans.sort_unstable();
    spans
}

/// Type usages spelled in import declarations and Javadoc references.
pub(crate) fn extract_import_and_doc_type_usages(
    root: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
) -> Vec<Usage> {
    let mut usages = Vec::new();
    let resolve_import = |prefix: &str| resolver.has_class(prefix).then(|| prefix.to_string());
    let resolve_doc = |prefix: &str| match resolver.resolve_type_identity(prefix) {
        crate::TypeNameResolution::Exact(fqn) => Some(fqn),
        _ => None,
    };

    for element in root.descendants_with_tokens() {
        match element {
            rowan::NodeOrToken::Node(node) if node.kind() == SyntaxKind::IMPORT_DECL => {
                let is_static = node
                    .children_with_tokens()
                    .any(|child| child.kind() == SyntaxKind::STATIC_KW);
                let is_wildcard = node
                    .children_with_tokens()
                    .any(|child| child.kind() == SyntaxKind::STAR);
                let Some(path) = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
                else {
                    continue;
                };
                let mut segments = path
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .map(|token| Segment {
                        text: token.text().to_string(),
                        start: u32::from(token.text_range().start()),
                        end: u32::from(token.text_range().end()),
                    })
                    .collect::<Vec<_>>();
                if is_static && !is_wildcard {
                    // The final segment is a member, not a type.
                    segments.pop();
                }
                push_type_chain(&segments, file_id, &resolve_import, &mut usages);
            }
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::DOC_COMMENT => {
                let text = token.text();
                let base = u32::from(token.text_range().start());
                for (start, end) in javadoc_reference_spans(text) {
                    let reference = &text[start..end];
                    let type_part = reference.split('#').next().unwrap_or_default();
                    let mut segments = Vec::new();
                    let mut offset = start;
                    for piece in type_part.split('.') {
                        if !piece.is_empty()
                            && piece
                                .chars()
                                .all(|c| c == '_' || c == '$' || c.is_alphanumeric())
                        {
                            segments.push(Segment {
                                text: piece.to_string(),
                                start: base + offset as u32,
                                end: base + (offset + piece.len()) as u32,
                            });
                        } else {
                            segments.clear();
                            break;
                        }
                        offset += piece.len() + 1;
                    }
                    push_type_chain(&segments, file_id, &resolve_doc, &mut usages);
                }
            }
            _ => {}
        }
    }
    usages
}

/// Type usages spelled in Kotlin `import a.b.C` / `import a.b.C as D` /
/// `import a.b.C.*` declarations. Only the imported (not the alias) spelling
/// names the type.
pub(crate) fn extract_kotlin_import_type_usages(
    root: &hyperion_kotlin::SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver<'_>,
) -> Vec<Usage> {
    use hyperion_kotlin::SyntaxKind as Kt;
    let mut usages = Vec::new();
    let resolve = |prefix: &str| resolver.has_class(prefix).then(|| prefix.to_string());
    for node in root
        .descendants()
        .filter(|node| node.kind() == Kt::IMPORT_DECL)
    {
        let mut segments = Vec::new();
        for token in node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
        {
            if token.kind() == Kt::AS_KW {
                break;
            }
            if token.kind() == Kt::IDENT {
                let text = token.text().trim_matches('`').to_string();
                let range = token.text_range();
                segments.push(Segment {
                    text,
                    start: u32::from(range.start()),
                    end: u32::from(range.end()),
                });
            }
        }
        push_type_chain(&segments, file_id, &resolve, &mut usages);
    }
    usages
}

#[cfg(test)]
mod tests {
    use super::javadoc_reference_spans;

    fn spans(text: &str) -> Vec<&str> {
        javadoc_reference_spans(text)
            .into_iter()
            .map(|(start, end)| &text[start..end])
            .collect()
    }

    #[test]
    fn reference_spans_cover_link_see_and_throws_targets_only() {
        let doc = "/** Uses {@link Circle#area()} and {@linkplain com.x.Shape the shape}.\n * @see Round\n * @throws IllegalStateException if broken\n * Prose about Circle stays inert, {@code Circle} too. */";
        assert_eq!(
            spans(doc),
            vec![
                "Circle#area",
                "com.x.Shape",
                "Round",
                "IllegalStateException"
            ]
        );
    }

    #[test]
    fn quoted_and_html_see_targets_are_not_references() {
        assert!(spans("/** @see \"Effective Java\" and @see <a href=\"x\">x</a> */").is_empty());
    }
}
