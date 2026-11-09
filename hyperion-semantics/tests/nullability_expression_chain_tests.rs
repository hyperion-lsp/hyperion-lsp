// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::DiagnosticKind;

#[test]
fn return_chain_checks_every_sibling_operand_in_source_order() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            @Nullable String first;
            @Nullable String last;

            String test() {
                return first.trim() + "middle" + "middle" + last.trim();
            }
        }
        "#,
    );
    let warnings: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| diagnostic.variable_name.as_str())
        .collect();

    assert_eq!(warnings, ["first", "last"]);
}
