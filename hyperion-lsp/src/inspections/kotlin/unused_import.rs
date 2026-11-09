// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `KT-UNUSED-IMPORT` — an import whose simple name (or alias) never occurs in the file.

use hyperion_kotlin::SyntaxKind;
use rowan::{NodeOrToken, TextRange};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticTag};

use super::{
    identifier_text, is_convention_name, is_identifier_kind, KotlinInspection,
    KotlinInspectionContext,
};

pub(crate) struct UnusedImport;

pub(crate) const CODE: &str = "KT-UNUSED-IMPORT";

impl KotlinInspection for UnusedImport {
    fn code(&self) -> &'static str {
        CODE
    }

    fn run(&self, ctx: &KotlinInspectionContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for import in ctx
            .root()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::IMPORT_DECL)
        {
            let tokens: Vec<_> = import
                .children_with_tokens()
                .filter_map(|element| match element {
                    NodeOrToken::Token(token) if !token.kind().is_trivia() => Some(token),
                    _ => None,
                })
                .collect();

            // Star imports bring in an unknown set of names — never flagged.
            if tokens.iter().any(|token| token.kind() == SyntaxKind::STAR) {
                continue;
            }

            // `import a.b.C as D` is used through `D`, not `C`.
            let alias_position = tokens
                .iter()
                .position(|token| token.kind() == SyntaxKind::AS_KW);
            let name_token = match alias_position {
                Some(index) => tokens
                    .get(index + 1)
                    .filter(|token| is_identifier_kind(token.kind())),
                None => tokens
                    .iter()
                    .rev()
                    .find(|token| is_identifier_kind(token.kind())),
            };
            let Some(name_token) = name_token else {
                continue;
            };
            let name = identifier_text(name_token);
            if name.is_empty() {
                continue;
            }

            // Operator/delegation/destructuring conventions are invoked without ever
            // spelling the imported name, so absence of the text proves nothing.
            if is_convention_name(&name) {
                continue;
            }

            if ctx.name_occurs_anywhere(&name) {
                continue;
            }

            let Some(first) = tokens.first() else {
                continue;
            };
            let Some(last) = tokens.last() else {
                continue;
            };
            let range = TextRange::new(first.text_range().start(), last.text_range().end());

            diagnostics.push(ctx.diagnostic(
                range,
                CODE,
                format!("Unused import: '{name}' is never used"),
                Some(vec![DiagnosticTag::UNNECESSARY]),
            ));
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::super::inspect_code;
    use super::CODE;

    fn unused_import_messages(source: &str) -> Vec<String> {
        inspect_code(source, CODE)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn flags_import_that_is_never_referenced() {
        let source = r#"
package p

import a.b.Widget

fun f() {
    println("hi")
}
"#;
        let messages = unused_import_messages(source);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("Widget"), "{messages:?}");
    }

    #[test]
    fn does_not_flag_import_used_as_a_type() {
        let source = r"
import a.b.Widget

fun f(w: Widget) {
    println(w)
}
";
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn never_flags_star_imports() {
        let source = r#"
import a.b.*
import kotlinx.coroutines.flow.*

fun f() {
    println("hi")
}
"#;
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn aliased_import_is_judged_by_its_alias() {
        // The alias is used, the original simple name is not — must stay silent.
        let source = r"
import java.util.Map as JMap

fun f(m: JMap) {
    println(m)
}
";
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn aliased_import_is_flagged_when_the_alias_is_unused() {
        // `Map` appears in the file, but the binding introduced is `JMap`.
        let source = r"
import java.util.Map as JMap

fun f(m: Map) {
    println(m)
}
";
        let messages = unused_import_messages(source);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("JMap"), "{messages:?}");
    }

    #[test]
    fn import_used_only_from_a_kdoc_link_is_not_flagged() {
        let source = r"
import a.b.Widget

/**
 * Companion to [Widget].
 */
class Holder
";
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn import_used_only_inside_a_string_template_is_not_flagged() {
        let source = r#"
import a.b.CONSTANT

fun f() {
    println("value=$CONSTANT")
}
"#;
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn operator_convention_imports_are_never_flagged() {
        // `x + y` resolves to the imported `plus` without naming it.
        let source = r"
import a.b.plus
import a.b.component1
import a.b.getValue

fun f(x: Int, y: Int): Int = x + y
";
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn nested_class_import_used_through_its_outer_class_is_not_flagged() {
        // `Inner` only ever appears as a member selector.
        let source = r"
import a.b.Outer.Inner

fun f() {
    println(Outer.Inner)
}
";
        assert!(unused_import_messages(source).is_empty());
    }

    #[test]
    fn the_import_statement_itself_does_not_count_as_a_use() {
        let source = r"
import a.b.Widget
";
        assert_eq!(unused_import_messages(source).len(), 1);
    }

    #[test]
    fn package_declaration_does_not_count_as_a_use() {
        let source = r"
package widget

import a.b.widget
";
        assert_eq!(unused_import_messages(source).len(), 1);
    }
}
