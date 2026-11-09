// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Characterization and regression tests for the hierarchy-walk consolidation
//! as it is observed through the "Implement interface methods" and
//! "Override methods" code actions.
//!
//! Each test states the JLS membership contract the action must honour: the
//! set of methods a type inherits is the *effective* method set of that type,
//! which is transitive through superclasses and superinterfaces alike
//! (JLS 8.4.8, 9.4.1), not the members written on the directly named
//! supertypes.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{CodeAction, CodeActionOrCommand, Position, Range};

fn actions_at(test: &mut LspTest, file: &str, line: u32) -> Vec<CodeActionOrCommand> {
    test.code_action(
        file,
        Range::new(Position::new(line, 0), Position::new(line, 0)),
        vec![],
    )
    .unwrap_or_else(|error| panic!("code action request failed for {file}: {error}"))
    .unwrap_or_default()
}

fn titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => action.title.clone(),
            CodeActionOrCommand::Command(command) => command.title.clone(),
        })
        .collect()
}

fn find_action(
    actions: &[CodeActionOrCommand],
    predicate: impl Fn(&str) -> bool,
) -> Option<&CodeAction> {
    actions.iter().find_map(|action| match action {
        CodeActionOrCommand::CodeAction(action) if predicate(&action.title) => Some(action),
        _ => None,
    })
}

fn edit_text(action: &CodeAction) -> String {
    action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .map(|changes| {
            changes
                .values()
                .flatten()
                .map(|edit| edit.new_text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn implement_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    find_action(actions, |title| title.starts_with("Implement "))
}

fn override_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    find_action(actions, |title| title.starts_with("Override "))
}

/// JLS 9.4.1: a class implementing an interface inherits the abstract methods
/// of that interface's superinterfaces too. Offering only the directly named
/// interface's own members leaves the class uncompilable.
#[test]
fn implement_offers_superinterface_methods_transitively() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Base.java",
        "package com.example;\npublic interface Base {\n    void base();\n}\n",
    );
    test.add_file(
        "com/example/Derived.java",
        "package com.example;\npublic interface Derived extends Base {\n    void derived();\n}\n",
    );
    test.add_file(
        "com/example/Impl.java",
        "package com.example;\npublic class Impl implements Derived {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Impl.java", 1);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action; available: {:?}",
            titles(&actions)
        )
    });
    let text = edit_text(action);
    assert!(
        text.contains("derived("),
        "direct superinterface method missing from stub: {text}"
    );
    assert!(
        text.contains("base("),
        "inherited superinterface method missing from stub: {text}"
    );
}

/// A supertype named by its simple name still denotes a packaged type. Using
/// the written spelling as an FQN index key silently produced no action.
#[test]
fn implement_resolves_a_supertype_in_another_package() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/api/Service.java",
        "package com.example.api;\npublic interface Service {\n    void run();\n}\n",
    );
    test.add_file(
        "com/example/app/Runner.java",
        "package com.example.app;\n\nimport com.example.api.Service;\n\npublic class Runner implements Service {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/app/Runner.java", 4);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action for an imported interface; available: {:?}",
            titles(&actions)
        )
    });
    assert!(edit_text(action).contains("run("));
}

/// `Symbol::parameters` holds `"Type name"` strings. Pasting the
/// whole string where a type belongs generates `void save(User user arg0)`,
/// which does not compile.
#[test]
fn generated_stub_parameters_are_types_not_type_and_name_pairs() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/User.java",
        "package com.example;\npublic class User {\n}\n",
    );
    test.add_file(
        "com/example/Repo.java",
        "package com.example;\npublic interface Repo {\n    void save(User user);\n}\n",
    );
    test.add_file(
        "com/example/RepoImpl.java",
        "package com.example;\npublic class RepoImpl implements Repo {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/RepoImpl.java", 1);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action; available: {:?}",
            titles(&actions)
        )
    });
    let text = edit_text(action);
    assert!(
        !text.contains("User user arg"),
        "stub pasted the raw \"Type name\" parameter string: {text}"
    );
    assert!(
        text.contains("save(User arg0)"),
        "expected a compilable parameter list: {text}"
    );
}

/// JLS 8.4.8: inheritance is transitive through the superclass chain.
#[test]
fn override_offers_grandparent_methods() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Grand.java",
        "package com.example;\npublic class Grand {\n    public void fromGrand() {}\n}\n",
    );
    test.add_file(
        "com/example/Parent.java",
        "package com.example;\npublic class Parent extends Grand {\n    public void fromParent() {}\n}\n",
    );
    test.add_file(
        "com/example/Child.java",
        "package com.example;\npublic class Child extends Parent {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Child.java", 1);
    let action = override_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an override-methods action; available: {:?}",
            titles(&actions)
        )
    });
    let text = edit_text(action);
    assert!(
        text.contains("fromParent("),
        "parent method missing: {text}"
    );
    assert!(
        text.contains("fromGrand("),
        "grandparent method missing: {text}"
    );
}

/// A method the class already declares is not an override candidate, and a
/// `final` or `static` inherited method cannot be overridden at all.
#[test]
fn override_skips_already_declared_final_and_static_methods() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Base2.java",
        "package com.example;\npublic class Base2 {\n    public void open() {}\n    public final void sealed() {}\n    public static void helper() {}\n    private void hidden() {}\n}\n",
    );
    test.add_file(
        "com/example/Sub2.java",
        "package com.example;\npublic class Sub2 extends Base2 {\n    public void open() {}\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Sub2.java", 1);
    let Some(action) = override_action(&actions) else {
        // Nothing overridable is a legal outcome here; the point is that
        // nothing *illegal* is offered.
        return;
    };
    let text = edit_text(action);
    assert!(!text.contains("sealed("), "final method offered: {text}");
    assert!(!text.contains("helper("), "static method offered: {text}");
    assert!(!text.contains("hidden("), "private method offered: {text}");
    assert!(
        !text.contains("open("),
        "already-declared method offered: {text}"
    );
}

/// An enum may implement an interface. `SyntaxKind::CLASS_DECL`-only cursor
/// resolution made these actions dead in enums and records.
#[test]
fn implement_is_offered_inside_an_enum() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Named.java",
        "package com.example;\npublic interface Named {\n    String label();\n}\n",
    );
    test.add_file(
        "com/example/Color.java",
        "package com.example;\npublic enum Color implements Named {\n    RED,\n    GREEN;\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Color.java", 2);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action inside an enum; available: {:?}",
            titles(&actions)
        )
    });
    assert!(edit_text(action).contains("label("));
}

/// A record may implement an interface too.
#[test]
fn implement_is_offered_inside_a_record() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Sized.java",
        "package com.example;\npublic interface Sized {\n    int size();\n}\n",
    );
    test.add_file(
        "com/example/Pair.java",
        "package com.example;\npublic record Pair(int a, int b) implements Sized {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Pair.java", 1);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action inside a record; available: {:?}",
            titles(&actions)
        )
    });
    assert!(edit_text(action).contains("size("));
}

/// An interface `default` method is already implemented for every
/// implementor; offering it as an unimplemented method is wrong.
#[test]
fn implement_does_not_offer_default_methods_as_unimplemented() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/WithDefault.java",
        "package com.example;\npublic interface WithDefault {\n    void required();\n    default void optional() {}\n}\n",
    );
    test.add_file(
        "com/example/UsesDefault.java",
        "package com.example;\npublic class UsesDefault implements WithDefault {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/UsesDefault.java", 1);
    let action = implement_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an implement-methods action; available: {:?}",
            titles(&actions)
        )
    });
    let text = edit_text(action);
    assert!(
        text.contains("required("),
        "abstract method missing: {text}"
    );
    assert!(
        !text.contains("optional("),
        "default method offered as unimplemented: {text}"
    );
}

/// JLS 15.12.1: plain `super` denotes the direct superclass, so a
/// redeclaration of an inherited interface `default` method cannot delegate
/// with `super.greet()` -- it needs `Greeter.super.greet()`.
#[test]
fn overriding_an_inherited_default_method_delegates_through_its_interface() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Greeter.java",
        "package com.example;\npublic interface Greeter {\n    default void greet() {}\n}\n",
    );
    test.add_file(
        "com/example/Polite.java",
        "package com.example;\npublic class Polite implements Greeter {\n}\n",
    );

    let actions = actions_at(&mut test, "com/example/Polite.java", 1);
    let action = override_action(&actions).unwrap_or_else(|| {
        panic!(
            "expected an override-methods action for an inherited default method; available: {:?}",
            titles(&actions)
        )
    });
    let text = edit_text(action);
    assert!(
        text.contains("Greeter.super.greet();"),
        "expected a qualified super delegation: {text}"
    );
}
