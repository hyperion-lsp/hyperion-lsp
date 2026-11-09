// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `KT-UNREACHABLE-CODE` — statements that follow a `return` or `throw` in the same block.
//!
//! Statement-level only: control flow inside `if`/`when`/loops is not modelled, so only a
//! `return`/`throw` that is a *direct* child of a block terminates it.

use hyperion_kotlin::{SyntaxKind, SyntaxNode};
use rowan::{NodeOrToken, TextRange};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticTag};

use super::{line_of, trimmed_range, KotlinInspection, KotlinInspectionContext};

pub(crate) struct UnreachableCode;

pub(crate) const CODE: &str = "KT-UNREACHABLE-CODE";

impl KotlinInspection for UnreachableCode {
    fn code(&self) -> &'static str {
        CODE
    }

    fn run(&self, ctx: &KotlinInspectionContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for block in ctx
            .root()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::BLOCK)
        {
            let statements: Vec<SyntaxNode> = block.children().collect();
            let Some(terminator_index) = statements
                .iter()
                .position(|statement| is_terminator(statement, ctx.source_text()))
            else {
                continue;
            };

            let unreachable: Vec<&SyntaxNode> = statements[terminator_index + 1..]
                .iter()
                .filter(|statement| !is_declaration(statement.kind()))
                .collect();

            let (Some(first), Some(last)) = (unreachable.first(), unreachable.last()) else {
                continue;
            };
            let (Some(start), Some(end)) = (trimmed_range(first), trimmed_range(last)) else {
                continue;
            };

            diagnostics.push(ctx.diagnostic(
                TextRange::new(start.start(), end.end()),
                CODE,
                "Unreachable code".to_string(),
                Some(vec![DiagnosticTag::UNNECESSARY]),
            ));
        }

        diagnostics
    }
}

/// Whether this statement unconditionally leaves the enclosing block.
///
/// Two shapes are refused even though they look like terminators:
///
/// * **Labelled returns** (`return@forEach`) currently parse as `return @` followed by the
///   label as its own statement, so the "unreachable" span would start on the label.
/// * **Bare `return` followed by a statement on the next line** parses with that statement
///   absorbed as the return's operand. The absorbed statement really is unreachable, but
///   its position in the tree is not trustworthy, so nothing is reported.
fn is_terminator(statement: &SyntaxNode, source_text: &str) -> bool {
    match statement.kind() {
        SyntaxKind::THROW_EXPR => true,
        SyntaxKind::RETURN_EXPR => {
            let has_label = statement.children_with_tokens().any(|element| {
                matches!(element, NodeOrToken::Token(token) if token.kind() == SyntaxKind::AT)
            });
            if has_label {
                return false;
            }
            !operand_starts_on_a_later_line(statement, source_text)
        }
        _ => false,
    }
}

fn operand_starts_on_a_later_line(return_expr: &SyntaxNode, source_text: &str) -> bool {
    let mut significant = return_expr
        .children_with_tokens()
        .filter(|element| !element.kind().is_trivia());

    let Some(keyword) = significant.next() else {
        return false;
    };
    let Some(operand) = significant.next() else {
        return false;
    };

    let keyword_line = line_of(usize::from(keyword.text_range().start()), source_text);
    let operand_line = line_of(usize::from(operand.text_range().start()), source_text);
    operand_line > keyword_line
}

/// Declarations after a terminator are still visible to the compiler in some positions;
/// they are not reported as unreachable *code*.
const fn is_declaration(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FUNCTION_DECL
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::OBJECT_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::TYPEALIAS_DECL
    )
}

#[cfg(test)]
mod tests {
    use super::super::inspect_code;
    use super::CODE;

    fn unreachable(source: &str) -> usize {
        inspect_code(source, CODE).len()
    }

    #[test]
    fn flags_statements_after_return_with_a_value() {
        let source = r#"
fun f(): Int {
    return 1
    println("never")
}
"#;
        assert_eq!(unreachable(source), 1);
    }

    #[test]
    fn flags_statements_after_throw() {
        let source = r#"
fun f() {
    throw IllegalStateException("boom")
    println("never")
}
"#;
        assert_eq!(unreachable(source), 1);
    }

    #[test]
    fn one_diagnostic_spans_all_trailing_statements() {
        let source = "fun f(): Int {\n    return 1\n    println(\"a\")\n    println(\"b\")\n}\n";
        let diagnostics = inspect_code(source, CODE);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start.line, 2);
        assert_eq!(diagnostics[0].range.end.line, 3);
    }

    #[test]
    fn does_not_flag_a_return_at_the_end_of_a_block() {
        let source = r#"
fun f(): Int {
    println("a")
    return 1
}
"#;
        assert_eq!(unreachable(source), 0);
    }

    #[test]
    fn does_not_flag_a_conditional_return() {
        let source = r"
fun f(flag: Boolean): Int {
    if (flag) {
        return 1
    }
    return 2
}
";
        assert_eq!(unreachable(source), 0);
    }

    #[test]
    fn does_not_flag_a_return_inside_a_when_branch() {
        let source = r"
fun f(x: Int): Int {
    when (x) {
        1 -> {
            return 1
        }
        else -> {
        }
    }
    return 2
}
";
        assert_eq!(unreachable(source), 0);
    }

    #[test]
    fn does_not_flag_labelled_returns() {
        let source = r"
fun f(items: List<Int>) {
    items.forEach {
        return@forEach
    }
}
";
        assert_eq!(unreachable(source), 0);
    }

    #[test]
    fn does_not_flag_declarations_after_a_terminator() {
        let source = r"
fun f(): Int {
    return 1
    class Helper
}
";
        assert_eq!(unreachable(source), 0);
    }

    #[test]
    fn each_block_is_judged_independently() {
        let source = r#"
fun f(): Int {
    if (true) {
        throw IllegalStateException("a")
        println("dead one")
    }
    return 1
}

fun g(): Int {
    return 2
    println("dead two")
}
"#;
        assert_eq!(unreachable(source), 2);
    }
}
