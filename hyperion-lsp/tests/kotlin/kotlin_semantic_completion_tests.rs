// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end Kotlin completion contracts for inferred receiver types.

use crate::lsp_test_framework::LspTest;

const JAVA_USER: &str = r#"
package model;

public final class User {
    public String displayName() { return "Ada"; }
    public int score() { return 42; }
}
"#;

fn labels(items: &[tower_lsp::lsp_types::CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

#[test]
fn completes_java_members_for_an_inferred_kotlin_lambda_parameter() {
    let mut test = LspTest::new();
    test.add_file("model/User.java", JAVA_USER);
    test.add_file(
        "demo/Users.kt",
        r#"
package demo

import model.User

fun names(users: List<User>) = users.map { user -> user.<|> }
"#,
    );

    let completions = test.completion("demo/Users.kt", "<|>").unwrap();
    let labels = labels(&completions);

    assert!(
        labels.contains(&"displayName") && labels.contains(&"score"),
        "expected useful Java members for inferred User receiver, got {labels:?}"
    );
}

#[test]
fn completes_members_for_a_value_inferred_from_when_branches() {
    let mut test = LspTest::new();
    test.add_file("model/User.java", JAVA_USER);
    test.add_file(
        "demo/Selection.kt",
        r#"
package demo

import model.User

fun selected(flag: Boolean) {
    val user = when (flag) {
        true -> User()
        false -> User()
    }
    user.<|>
}
"#,
    );

    let completions = test.completion("demo/Selection.kt", "<|>").unwrap();
    let labels = labels(&completions);

    assert!(
        labels.contains(&"displayName") && labels.contains(&"score"),
        "expected User members after a same-type when expression, got {labels:?}"
    );
}
