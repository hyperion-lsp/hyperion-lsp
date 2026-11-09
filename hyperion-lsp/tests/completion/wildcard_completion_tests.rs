// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Member completion through a method whose declared return type comes from an on-demand
//! import. The receiver's type is `java.util.List`, not a class in the file's package.

use crate::lsp_test_framework::LspTest;

#[test]
fn members_of_a_call_result_typed_through_a_wildcard_import_are_offered() {
    let mut test = LspTest::new();
    test.add_file(
        "Test.java",
        r"
            import java.util.*;
            class Test {
                List<String> names() { return null; }
                void m() {
                    names().<cur>
                }
            }
        ",
    );

    let result = test.completion("Test.java", "<cur>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let labels: Vec<String> = result.unwrap().into_iter().map(|item| item.label).collect();
    assert!(
        labels.iter().any(|label| label == "size"),
        "List members should be offered. Got: {labels:?}"
    );
}
