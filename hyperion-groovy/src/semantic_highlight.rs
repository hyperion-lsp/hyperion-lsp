// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Semantic highlight extraction owned by the Groovy parser layer.

use hyperion_types::{SemanticHighlight, SemanticHighlightKind, TextRange};
use tree_sitter::Node;

use crate::tree_sitter_wrapper::{GroovyParser, TreeSitterResult};

pub fn semantic_highlights(source: &str) -> TreeSitterResult<Vec<SemanticHighlight>> {
    let tree = GroovyParser::new()?.parse_tree(source)?;
    let mut highlights = Vec::new();
    collect(tree.root_node(), &mut highlights);
    highlights.sort_by_key(|highlight| (highlight.range.start, highlight.range.end));
    Ok(highlights)
}

fn collect(node: Node<'_>, highlights: &mut Vec<SemanticHighlight>) {
    if let Some(kind) = whole_node_kind(node.kind()) {
        push(highlights, node, kind, false, false);
        return;
    }

    if matches!(node.kind(), "identifier" | "type_identifier") {
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
        "line_comment" | "block_comment" => Some(SemanticHighlightKind::Comment),
        "string_literal" | "multiline_string_fragment" => Some(SemanticHighlightKind::String),
        "decimal_integer_literal"
        | "hex_integer_literal"
        | "octal_integer_literal"
        | "binary_integer_literal"
        | "decimal_floating_point_literal"
        | "hex_floating_point_literal" => Some(SemanticHighlightKind::Number),
        _ => None,
    }
}

fn identifier_kind(node: Node<'_>) -> Option<(SemanticHighlightKind, bool, bool)> {
    if node.kind() == "type_identifier" {
        return Some((SemanticHighlightKind::Class, false, false));
    }

    let parent = node.parent()?;
    if is_name_field(node, parent) {
        let kind = match parent.kind() {
            "class_declaration" => Some(SemanticHighlightKind::Class),
            "interface_declaration" => Some(SemanticHighlightKind::Interface),
            "enum_declaration" => Some(SemanticHighlightKind::Enum),
            "method_declaration"
            | "constructor_declaration"
            | "compact_constructor_declaration" => Some(SemanticHighlightKind::Method),
            "formal_parameter"
            | "catch_formal_parameter"
            | "spread_parameter"
            | "receiver_parameter" => Some(SemanticHighlightKind::Parameter),
            "method_invocation" => Some(SemanticHighlightKind::Method),
            "type_parameter" => Some(SemanticHighlightKind::TypeParameter),
            _ => None,
        };
        if let Some(kind) = kind {
            let declaration = !matches!(parent.kind(), "method_invocation");
            return Some((kind, declaration, false));
        }
    }

    if parent.kind() == "variable_declarator" && is_name_field(node, parent) {
        let mut ancestor = parent.parent();
        while let Some(current) = ancestor {
            match current.kind() {
                "field_declaration" => {
                    return Some((SemanticHighlightKind::Property, true, is_final(current)));
                }
                "local_variable_declaration" => {
                    return Some((SemanticHighlightKind::Variable, true, is_final(current)));
                }
                _ => ancestor = current.parent(),
            }
        }
    }

    None
}

fn is_name_field(node: Node<'_>, parent: Node<'_>) -> bool {
    parent
        .child_by_field_name("name")
        .is_some_and(|name| name == node)
}

fn is_final(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == "final");
    found
}

fn keyword_kind(kind: &str) -> Option<SemanticHighlightKind> {
    if matches!(
        kind,
        "public"
            | "protected"
            | "private"
            | "static"
            | "final"
            | "abstract"
            | "synchronized"
            | "native"
            | "transient"
            | "volatile"
            | "strictfp"
    ) {
        return Some(SemanticHighlightKind::Modifier);
    }

    matches!(
        kind,
        "class"
            | "interface"
            | "enum"
            | "def"
            | "extends"
            | "implements"
            | "package"
            | "import"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "switch"
            | "case"
            | "default"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "throws"
            | "new"
            | "this"
            | "super"
            | "instanceof"
            | "in"
            | "as"
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
        let source = r#"class Person {
    final String name = "Ada"
    def greet(String who) { println(who) }
    // note
}"#;
        let highlights = semantic_highlights(source).expect("valid Groovy");
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

        assert!(facts.contains(&("Person", SemanticHighlightKind::Class, true)));
        assert!(facts.contains(&("greet", SemanticHighlightKind::Method, true)));
        assert!(facts.contains(&("who", SemanticHighlightKind::Parameter, true)));
        assert!(facts.contains(&("\"Ada\"", SemanticHighlightKind::String, false)));
        assert!(facts.contains(&("// note", SemanticHighlightKind::Comment, false)));
    }
}
