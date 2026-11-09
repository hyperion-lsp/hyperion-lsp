// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for labeled break/continue reachability.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::DiagnosticKind;

#[test]
fn labeled_block_break_keeps_following_code_reachable() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                @Nullable Service service = null;
                scope: { break scope; }
                service.run();
            }
        }
        class Service { void run() {} }
        "#,
    );

    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.kind == DiagnosticKind::PotentialNullDereference
            && diagnostic.variable_name == "service"
    }));
}
