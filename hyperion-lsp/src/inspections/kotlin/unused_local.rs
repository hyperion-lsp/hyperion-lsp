// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `KT-UNUSED-VARIABLE` — a local `val`/`var` that is never read.
//!
//! Writes do not count as reads, matching kotlinc's "variable is never used". Everything
//! ambiguous is skipped: re-declared/shadowed names, destructuring declarations,
//! delegated properties, and anything outside a plain block or lambda body.

use hyperion_kotlin::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticTag};

use super::{
    identifier_text, is_identifier_kind, is_local_scope, KotlinInspection, KotlinInspectionContext,
};

pub(crate) struct UnusedLocalVariable;

pub(crate) const CODE: &str = "KT-UNUSED-VARIABLE";

impl KotlinInspection for UnusedLocalVariable {
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
            let Some(scope) = property.parent().filter(|p| is_local_scope(p.kind())) else {
                // Members and top-level properties are part of an API surface; only
                // locals are reported.
                continue;
            };

            if is_destructuring(&property) || is_delegated(&property) {
                continue;
            }

            let Some(name_token) = local_name_token(&property) else {
                continue;
            };
            let name = identifier_text(&name_token);

            // `_` (and `_`-prefixed names) are the conventional "intentionally unused".
            if name.is_empty() || name.starts_with('_') {
                continue;
            }

            // Shadowing, re-declaration in a sibling block, a same-named parameter or
            // function: any of these make "is this occurrence a read of *this* binding?"
            // unanswerable without full resolution, so stay silent.
            if ctx.is_ambiguously_declared(&name) {
                continue;
            }

            if ctx.name_read_in_scope(&name, scope.text_range(), name_token.text_range()) {
                continue;
            }

            diagnostics.push(ctx.diagnostic(
                name_token.text_range(),
                CODE,
                format!("Variable '{name}' is never used"),
                Some(vec![DiagnosticTag::UNNECESSARY]),
            ));
        }

        diagnostics
    }
}

/// `val (a, b) = pair` — component bindings are out of scope for this rule.
fn is_destructuring(property: &SyntaxNode) -> bool {
    property.children_with_tokens().any(|element| {
        matches!(element, NodeOrToken::Token(token) if token.kind() == SyntaxKind::L_PAREN)
    })
}

/// `val x by lazy { .. }` — the delegate's `getValue` may have effects.
fn is_delegated(property: &SyntaxNode) -> bool {
    property.children_with_tokens().any(
        |element| matches!(element, NodeOrToken::Token(token) if token.kind() == SyntaxKind::BY_KW),
    )
}

/// The declared name: the first identifier token after `val`/`var`.
fn local_name_token(property: &SyntaxNode) -> Option<hyperion_kotlin::SyntaxToken> {
    let mut seen_binder = false;
    for element in property.children_with_tokens() {
        let NodeOrToken::Token(token) = element else {
            continue;
        };
        match token.kind() {
            SyntaxKind::VAL_KW | SyntaxKind::VAR_KW => seen_binder = true,
            SyntaxKind::EQ => break,
            kind if seen_binder && is_identifier_kind(kind) => return Some(token),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::inspect_code;
    use super::CODE;

    fn unused_variables(source: &str) -> Vec<String> {
        inspect_code(source, CODE)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn flags_a_local_val_that_is_never_read() {
        let source = r#"
fun f() {
    val unusedOne = 5
    println("hi")
}
"#;
        let messages = unused_variables(source);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("unusedOne"));
    }

    #[test]
    fn does_not_flag_a_local_that_is_read() {
        let source = r"
fun f(): Int {
    val used = 5
    return used
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn writes_are_not_reads() {
        let source = r"
fun f() {
    var counter = 0
    counter = 3
}
";
        let messages = unused_variables(source);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("counter"));
    }

    #[test]
    fn compound_assignment_counts_as_a_read() {
        let source = r"
fun f() {
    var total = 0
    total += 1
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn increment_counts_as_a_read() {
        let source = r"
fun f() {
    var ticks = 0
    ticks++
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn string_template_interpolation_is_a_read() {
        let source = r#"
fun f() {
    val name = "world"
    val greeting = "hello $name"
    println(greeting)
}
"#;
        assert!(
            unused_variables(source).is_empty(),
            "{:?}",
            unused_variables(source)
        );
    }

    #[test]
    fn braced_string_template_interpolation_is_a_read() {
        let source = r#"
fun f() {
    val count = 1
    println("n=${count + 1}")
}
"#;
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn underscore_names_are_never_flagged() {
        let source = r"
fun f() {
    val _ = compute()
    val _ignored = compute()
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn shadowed_names_are_skipped_entirely() {
        // Whether the inner `x` is read cannot be decided textually — stay silent for both.
        let source = r"
fun f() {
    val x = 1
    if (true) {
        val x = 2
        println(x)
    }
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn a_same_named_parameter_suppresses_the_report() {
        let source = r"
fun f(value: Int) {
    println(value)
}

fun g() {
    val value = 1
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn member_selector_with_the_same_name_is_not_a_read() {
        // `this.total` and `other.total` select members, never the local.
        let source = r"
fun f(other: Holder) {
    val total = 1
    println(other.total)
}
";
        let messages = unused_variables(source);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("total"));
    }

    #[test]
    fn read_from_a_nested_lambda_counts() {
        let source = r"
fun f(items: List<Int>) {
    val factor = 2
    items.forEach { item -> println(item * factor) }
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn named_argument_with_the_same_name_is_not_treated_as_a_write() {
        // `foo(limit = 1)` has the same tree shape as an assignment statement; treating it
        // as a write would suppress nothing here, but treating the local as written-only
        // must not happen for `limit` used as an argument value.
        let source = r"
fun f() {
    val limit = 1
    foo(other = limit)
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn members_and_top_level_properties_are_not_reported() {
        let source = r"
val topLevel = 1

class C {
    val member = 2
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn companion_object_members_are_not_reported() {
        let source = r"
class C {
    companion object {
        val CONST = 1
    }
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn delegated_locals_are_skipped() {
        let source = r"
fun f() {
    val heavy by lazy { compute() }
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn destructuring_locals_are_skipped() {
        let source = r"
fun f(pair: Pair<Int, Int>) {
    val (first, second) = pair
    println(first)
}
";
        assert!(unused_variables(source).is_empty());
    }

    #[test]
    fn extension_receiver_member_of_the_same_name_does_not_count() {
        let source = r"
fun String.f() {
    val length = 1
    println(this.length)
}
";
        let messages = unused_variables(source);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("length"));
    }

    #[test]
    fn kdoc_mention_suppresses_the_report() {
        let source = r"
fun f() {
    /** keeps [scratch] alive for documentation */
    val scratch = 1
}
";
        assert!(unused_variables(source).is_empty());
    }
}
