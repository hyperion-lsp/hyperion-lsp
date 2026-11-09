// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Semantic highlight extraction owned by the Scala parser layer.

use hyperion_types::{SemanticHighlight, SemanticHighlightKind, TextRange};
use tree_sitter::Node;

pub fn semantic_highlights(source: &str) -> anyhow::Result<Vec<SemanticHighlight>> {
    let parsed = crate::parser::parse(source)?;
    let mut highlights = Vec::new();
    collect(parsed.tree.root_node(), &mut highlights);
    highlights.sort_by_key(|highlight| (highlight.range.start, highlight.range.end));
    Ok(highlights)
}

fn collect(node: Node<'_>, highlights: &mut Vec<SemanticHighlight>) {
    if let Some(kind) = whole_node_kind(node.kind()) {
        push(highlights, node, kind, false, false);
        return;
    }

    if matches!(
        node.kind(),
        "identifier" | "operator_identifier" | "type_identifier"
    ) {
        if let Some((kind, declaration, readonly)) = identifier_kind(node) {
            push(highlights, node, kind, declaration, readonly);
        }
    } else if !node.is_named() && node.child_count() == 0 {
        if let Some(kind) = keyword_kind(node.kind()) {
            push(highlights, node, kind, false, false);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, highlights);
    }
}

fn whole_node_kind(kind: &str) -> Option<SemanticHighlightKind> {
    match kind {
        "comment" | "block_comment" => Some(SemanticHighlightKind::Comment),
        "string" | "interpolated_string" | "character_literal" => {
            Some(SemanticHighlightKind::String)
        }
        "integer_literal" | "floating_point_literal" => Some(SemanticHighlightKind::Number),
        _ => None,
    }
}

fn identifier_kind(node: Node<'_>) -> Option<(SemanticHighlightKind, bool, bool)> {
    if node.kind() == "type_identifier" {
        return Some((SemanticHighlightKind::Class, false, false));
    }

    let parent = node.parent()?;
    if is_field(node, parent, "name") {
        let kind = match parent.kind() {
            "class_definition" | "object_definition" => Some(SemanticHighlightKind::Class),
            "trait_definition" => Some(SemanticHighlightKind::Interface),
            "enum_definition" => Some(SemanticHighlightKind::Enum),
            "function_definition" | "function_declaration" => Some(SemanticHighlightKind::Method),
            "parameter" | "class_parameter" => Some(SemanticHighlightKind::Parameter),
            "type_parameter" | "covariant_type_parameter" | "contravariant_type_parameter" => {
                Some(SemanticHighlightKind::TypeParameter)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            return Some((kind, true, false));
        }
    }

    if is_field(node, parent, "function") && parent.kind() == "call_expression" {
        return Some((SemanticHighlightKind::Method, false, false));
    }

    let mut ancestor = Some(parent);
    while let Some(current) = ancestor {
        if matches!(current.kind(), "val_definition" | "var_definition")
            && current
                .child_by_field_name("pattern")
                .is_some_and(|pattern| contains(pattern, node))
        {
            return Some((
                SemanticHighlightKind::Variable,
                true,
                current.kind() == "val_definition",
            ));
        }
        ancestor = current.parent();
    }

    None
}

fn is_field(node: Node<'_>, parent: Node<'_>, field: &str) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|candidate| candidate == node)
}

fn contains(container: Node<'_>, node: Node<'_>) -> bool {
    container.start_byte() <= node.start_byte() && node.end_byte() <= container.end_byte()
}

fn keyword_kind(kind: &str) -> Option<SemanticHighlightKind> {
    if matches!(
        kind,
        "private"
            | "protected"
            | "final"
            | "sealed"
            | "abstract"
            | "override"
            | "implicit"
            | "lazy"
            | "inline"
            | "open"
            | "transparent"
    ) {
        return Some(SemanticHighlightKind::Modifier);
    }

    matches!(
        kind,
        "class"
            | "trait"
            | "object"
            | "enum"
            | "def"
            | "val"
            | "var"
            | "extends"
            | "with"
            | "derives"
            | "package"
            | "import"
            | "export"
            | "return"
            | "if"
            | "then"
            | "else"
            | "for"
            | "while"
            | "do"
            | "match"
            | "case"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "new"
            | "this"
            | "super"
            | "given"
            | "using"
            | "extension"
            | "end"
    )
    .then_some(SemanticHighlightKind::Keyword)
}

fn push(
    highlights: &mut Vec<SemanticHighlight>,
    node: Node<'_>,
    kind: SemanticHighlightKind,
    declaration: bool,
    readonly: bool,
) {
    let (Ok(start), Ok(end)) = (
        u32::try_from(node.start_byte()),
        u32::try_from(node.end_byte()),
    ) else {
        return;
    };
    if start == end {
        return;
    }
    let mut highlight = SemanticHighlight::new(TextRange::new(start, end), kind);
    if declaration {
        highlight = highlight.declaration();
    }
    if readonly {
        highlight = highlight.readonly();
    }
    highlights.push(highlight);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_declarations_literals_and_comments() {
        let source = r#"trait Greeter {
  val greeting = "hello"
  def greet(name: String): String
  // note
}"#;
        let highlights = semantic_highlights(source).expect("valid Scala");
        let facts: Vec<_> = highlights
            .iter()
            .map(|highlight| {
                (
                    &source[highlight.range.start as usize..highlight.range.end as usize],
                    highlight.kind,
                    highlight.modifiers.declaration,
                )
            })
            .collect();

        assert!(facts.contains(&("Greeter", SemanticHighlightKind::Interface, true)));
        assert!(facts.contains(&("greeting", SemanticHighlightKind::Variable, true)));
        assert!(facts.contains(&("greet", SemanticHighlightKind::Method, true)));
        assert!(facts.contains(&("name", SemanticHighlightKind::Parameter, true)));
        assert!(facts.contains(&("\"hello\"", SemanticHighlightKind::String, false)));
        assert!(facts.contains(&("// note", SemanticHighlightKind::Comment, false)));
    }
}
