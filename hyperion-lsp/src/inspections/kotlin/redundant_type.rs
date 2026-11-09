// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `KT-REDUNDANT-EXPLICIT-TYPE` — `val x: Int = 5`, where the annotation restates exactly
//! what a literal initializer already says.
//!
//! Scope is deliberately narrow. Only *literal* initializers are considered, and only for
//! locals: `val x: Long = 5` is not redundant (the literal is an `Int`), and an explicit
//! type on an API surface is a documentation choice, not a redundancy.

use hyperion_kotlin::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{NodeOrToken, TextRange};
use tower_lsp::lsp_types::Diagnostic;

use super::{is_local_scope, KotlinInspection, KotlinInspectionContext};

pub(crate) struct RedundantExplicitType;

pub(crate) const CODE: &str = "KT-REDUNDANT-EXPLICIT-TYPE";

impl KotlinInspection for RedundantExplicitType {
    fn code(&self) -> &'static str {
        CODE
    }

    fn run(&self, ctx: &KotlinInspectionContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for property in ctx
            .root()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::PROPERTY_DECL)
        {
            if !property
                .parent()
                .is_some_and(|parent| is_local_scope(parent.kind()))
            {
                continue;
            }

            let Some((colon, type_ref)) = annotated_type(&property) else {
                continue;
            };
            let Some(initializer) = literal_initializer(&property) else {
                continue;
            };
            let Some(inferred) = literal_type_name(initializer.kind()) else {
                continue;
            };

            // Textual equality only. `Int?`, `Number`, `Comparable<Int>` and friends all
            // differ textually and are left alone rather than reasoned about.
            let annotated = type_ref.text().to_string();
            if annotated.trim() != inferred {
                continue;
            }

            let range = TextRange::new(colon.text_range().start(), trimmed_end(&type_ref));
            diagnostics.push(ctx.diagnostic(
                range,
                CODE,
                format!("Explicitly given type '{inferred}' is redundant here"),
                None,
            ));
        }

        diagnostics
    }
}

/// The `: Type` annotation, if present, as `(colon token, type node)`.
fn annotated_type(property: &SyntaxNode) -> Option<(SyntaxToken, SyntaxNode)> {
    let mut colon = None;
    for element in property.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::COLON => colon = Some(token),
                SyntaxKind::EQ => return None,
                _ => {}
            },
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::TYPE_REF => {
                let colon = colon.take()?;
                return Some((colon, node));
            }
            NodeOrToken::Node(_) => {}
        }
    }
    None
}

/// The initializer, when it is a single literal token wrapped in `LITERAL_EXPR`.
fn literal_initializer(property: &SyntaxNode) -> Option<SyntaxToken> {
    let mut seen_eq = false;
    for element in property.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ => seen_eq = true,
            NodeOrToken::Node(node) if seen_eq => {
                if node.kind() != SyntaxKind::LITERAL_EXPR {
                    return None;
                }
                let mut tokens = node
                    .children_with_tokens()
                    .filter_map(|element| match element {
                        NodeOrToken::Token(token) if !token.kind().is_trivia() => Some(token),
                        _ => None,
                    });
                let only = tokens.next()?;
                if tokens.next().is_some() {
                    return None;
                }
                return Some(only);
            }
            // A bare token initializer (e.g. `= other`) is not a literal.
            NodeOrToken::Token(token) if seen_eq && !token.kind().is_trivia() => return None,
            _ => {}
        }
    }
    None
}

/// The type a literal token trivially infers to.
///
/// `null` is intentionally absent: its type is `Nothing?`, and any annotation on it is
/// load-bearing.
const fn literal_type_name(kind: SyntaxKind) -> Option<&'static str> {
    Some(match kind {
        SyntaxKind::INT_LITERAL => "Int",
        SyntaxKind::LONG_LITERAL => "Long",
        SyntaxKind::FLOAT_LITERAL => "Float",
        SyntaxKind::DOUBLE_LITERAL => "Double",
        SyntaxKind::STRING_LITERAL | SyntaxKind::RAW_STRING_LITERAL => "String",
        SyntaxKind::CHAR_LITERAL => "Char",
        SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => "Boolean",
        _ => return None,
    })
}

fn trimmed_end(node: &SyntaxNode) -> rowan::TextSize {
    node.descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                Some(token.text_range().end())
            }
            _ => None,
        })
        .last()
        .unwrap_or_else(|| node.text_range().end())
}

#[cfg(test)]
mod tests {
    use super::super::inspect_code;
    use super::CODE;

    fn redundant_types(source: &str) -> Vec<String> {
        inspect_code(source, CODE)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn flags_int_annotation_on_int_literal() {
        let source = r"
fun f() {
    val x: Int = 5
    println(x)
}
";
        let messages = redundant_types(source);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("Int"));
    }

    #[test]
    fn flags_every_trivially_inferable_literal_kind() {
        let source = r#"
fun f() {
    val a: Int = 5
    val b: Long = 5L
    val c: Double = 1.0
    val d: String = "hi"
    val e: Boolean = true
    val g: Char = 'c'
    println("$a $b $c $d $e $g")
}
"#;
        assert_eq!(
            redundant_types(source).len(),
            6,
            "{:?}",
            redundant_types(source)
        );
    }

    #[test]
    fn does_not_flag_a_widening_annotation() {
        // The literal is an `Int`; the annotation genuinely changes the type.
        let source = r#"
fun f() {
    val x: Long = 5
    val y: Number = 5
    println("$x $y")
}
"#;
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_nullable_annotation() {
        let source = r"
fun f() {
    val x: Int? = 5
    println(x)
}
";
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn does_not_flag_null_initializers() {
        let source = r"
fun f() {
    val x: String? = null
    println(x)
}
";
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn does_not_flag_non_literal_initializers() {
        let source = r#"
fun f() {
    val x: Int = compute()
    val y: Int = 1 + 2
    println("$x $y")
}
"#;
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_there_is_no_annotation() {
        let source = r"
fun f() {
    val x = 5
    println(x)
}
";
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn does_not_flag_members_or_top_level_properties() {
        // Explicit types on an API surface are a deliberate choice, not redundancy.
        let source = r#"
val topLevel: Int = 5

class C {
    val member: String = "hi"
}
"#;
        assert!(redundant_types(source).is_empty());
    }

    #[test]
    fn reported_range_covers_the_colon_and_the_type() {
        let source = "fun f() {\n    val x: Int = 5\n    println(x)\n}\n";
        let diagnostics = inspect_code(source, CODE);
        assert_eq!(diagnostics.len(), 1);
        let range = diagnostics[0].range;
        assert_eq!(range.start.line, 1);
        // `    val x: Int = 5` — colon at column 9, `Int` ends at column 14.
        assert_eq!(range.start.character, 9);
        assert_eq!(range.end.character, 14);
    }
}
