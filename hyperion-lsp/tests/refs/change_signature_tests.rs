// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Change Method Signature info action

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{CodeActionOrCommand, Position, Range};

/// Test that Change Signature info is shown for methods
#[test]
fn test_change_signature_info_shows_for_method() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Service.java",
        r#"
package com.example;

public class Service {
    public String processData(int count, String name) {
        return name + ": " + count;
    }
}
"#,
    );
    harness.initialize().expect("Failed to initialize");

    // Request code actions at the method name
    let result = harness
        .code_action(
            "com/example/Service.java",
            Range::new(Position::new(4, 18), Position::new(4, 18)), // on "processData"
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Change Signature actions should be returned");

    // Should have a method signature info action
    let signature_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("Change Signature") && ca.title.contains("call site")
        } else {
            false
        }
    });

    assert!(
        signature_action.is_some(),
        "Expected Change Signature info action for method, got actions: {:?}",
        actions
            .iter()
            .map(|a| {
                if let CodeActionOrCommand::CodeAction(ca) = a {
                    ca.title.clone()
                } else {
                    "command".to_string()
                }
            })
            .collect::<Vec<_>>()
    );
}

/// Test that Change Signature shows parameter info
#[test]
fn test_change_signature_shows_parameters() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Calculator.java",
        r"
package com.example;

public class Calculator {
    public int add(int a, int b) {
        return a + b;
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    let result = harness
        .code_action(
            "com/example/Calculator.java",
            Range::new(Position::new(4, 15), Position::new(4, 15)), // on "add"
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Change Signature actions should be returned");

    let signature_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("Change Signature") && ca.title.contains("add")
        } else {
            false
        }
    });

    let CodeActionOrCommand::CodeAction(action) =
        signature_action.expect("Change Signature action should be present")
    else {
        panic!("Change Signature must be a code action");
    };
    assert!(
        action.title.contains("int add(int a, int b)"),
        "expected method signature and parameters in title: {}",
        action.title
    );
    assert!(action.disabled.is_some());
}

/// Test that void methods are handled
#[test]
fn test_change_signature_void_method() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Logger.java",
        r"
package com.example;

public class Logger {
    public void log(String message) {
        System.out.println(message);
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    let result = harness
        .code_action(
            "com/example/Logger.java",
            Range::new(Position::new(4, 16), Position::new(4, 16)), // on "log"
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Change Signature actions should be returned");

    let signature_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("Change Signature") && ca.title.contains("log")
        } else {
            false
        }
    });

    assert!(
        signature_action.is_some(),
        "Expected Change Signature info for void method"
    );
}

/// Test that Change Signature action is disabled with proper message
#[test]
fn test_change_signature_is_disabled() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Test.java",
        r"
package com.example;

public class Test {
    public void testMethod() {}
}
",
    );
    harness.initialize().expect("Failed to initialize");

    let result = harness
        .code_action(
            "com/example/Test.java",
            Range::new(Position::new(4, 16), Position::new(4, 16)),
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Change Signature actions should be returned");

    let signature_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.contains("Method") && ca.title.contains("call site")
        } else {
            false
        }
    });

    let CodeActionOrCommand::CodeAction(action) =
        signature_action.expect("Change Signature action should be present")
    else {
        panic!("Change Signature must be a code action");
    };
    let disabled = action
        .disabled
        .as_ref()
        .expect("Change Signature must remain disabled without IDE dialog support");
    assert!(disabled.reason.contains("IDE dialog support"));
}

#[test]
fn change_signature_call_count_uses_the_selected_overload() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Overloaded.java",
        r#"
package com.example;

public class Overloaded {
    public void pick(int value) {}
    public void pick(String value) {}
    public void use() { pick(1); pick("value"); }
}
"#,
    );
    harness.initialize().expect("Failed to initialize");

    for line in [4, 5] {
        let result = harness
            .code_action(
                "com/example/Overloaded.java",
                Range::new(Position::new(line, 17), Position::new(line, 17)),
                vec![],
            )
            .expect("Code action failed")
            .expect("Change Signature actions should be returned");
        let title = result.iter().find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title.contains("Change Signature") =>
            {
                Some(action.title.as_str())
            }
            _ => None,
        });
        assert!(
            title.is_some_and(|title| title.contains("1 call site")),
            "selected overload should have exactly one call site: {title:?}"
        );
    }
}
