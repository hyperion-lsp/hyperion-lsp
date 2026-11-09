// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Safe Delete refactoring

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, Url};

fn safe_delete_action_at(
    harness: &mut LspTest,
    file: &str,
    source: &str,
    needle: &str,
) -> Option<CodeAction> {
    let offset = source.find(needle).expect("safe-delete cursor needle");
    let position = hyperion_lsp::position_utils::offset_to_position(source, offset);
    harness
        .code_action(file, Range::new(position, position), vec![])
        .expect("code action request")
        .unwrap_or_default()
        .into_iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Safe Delete") => {
                Some(action)
            }
            _ => None,
        })
}

fn apply_safe_delete(source: &str, uri: &Url, action: &CodeAction) -> String {
    let edit = action.edit.as_ref().expect("enabled safe delete edit");
    let mut edits = edit
        .changes
        .as_ref()
        .and_then(|changes| changes.get(uri))
        .cloned()
        .expect("single-file safe delete changes");
    edits.sort_unstable_by_key(|edit| std::cmp::Reverse(edit.range.start));

    let mut edited = source.to_string();
    for edit in edits {
        let start = hyperion_lsp::position_utils::position_to_offset(source, edit.range.start)
            .expect("edit start");
        let end = hyperion_lsp::position_utils::position_to_offset(source, edit.range.end)
            .expect("edit end");
        edited.replace_range(start..end, &edit.new_text);
    }
    let (_, errors) =
        hyperion_parser::Parser::parse(hyperion_parser::Lexer::new(&edited).collect());
    assert!(
        errors.is_empty(),
        "edited source must parse: {errors:?}\n{edited}"
    );
    edited
}

fn assert_safe_delete_not_enabled(action: Option<CodeAction>) {
    if let Some(action) = action {
        assert!(
            action.disabled.is_some() && action.edit.is_none(),
            "unsafe safe-delete action was enabled: {action:?}"
        );
    }
}

/// Test that Safe Delete is offered for unused method
#[test]
fn test_safe_delete_unused_method() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Unused.java",
        r#"
package com.example;

public class Unused {
    public void unusedMethod() {
        // This method is never called
    }

    public void usedMethod() {
        System.out.println("Used!");
    }
}
"#,
    );
    harness.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    public void test() {
        Unused u = new Unused();
        u.usedMethod();  // Only usedMethod is called
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    // Request code actions at the unusedMethod declaration
    // The cursor is on "unusedMethod"
    let result = harness
        .code_action(
            "com/example/Unused.java",
            Range::new(Position::new(4, 16), Position::new(4, 16)),
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Safe Delete actions should be returned");

    // Should have a Safe Delete action
    let safe_delete_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.starts_with("Safe Delete")
        } else {
            false
        }
    });

    assert!(
        safe_delete_action.is_some(),
        "Expected Safe Delete action for unused method, got actions: {:?}",
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

    // Verify it shows "(no usages)" since the method isn't called
    let CodeActionOrCommand::CodeAction(action) = safe_delete_action.expect("Safe Delete action")
    else {
        panic!("Safe Delete must be a code action");
    };
    assert!(action.title.contains("no usages"));
    assert!(action.disabled.is_none());
    assert!(
        action.edit.is_some(),
        "enabled Safe Delete must include an edit"
    );
}

/// Test that Safe Delete shows usage count for used method
#[test]
fn test_safe_delete_used_method_shows_usage_count() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Service.java",
        r#"
package com.example;

public class Service {
    public void doWork() {
        System.out.println("Working...");
    }
}
"#,
    );
    harness.add_file(
        "com/example/Client.java",
        r"
package com.example;

public class Client {
    public void test() {
        Service s = new Service();
        s.doWork();  // First usage
        s.doWork();  // Second usage
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    // Request code actions at the doWork declaration
    let result = harness
        .code_action(
            "com/example/Service.java",
            Range::new(Position::new(4, 16), Position::new(4, 16)),
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Safe Delete actions should be returned");

    // Look for Safe Delete action
    let safe_delete_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.starts_with("Safe Delete")
        } else {
            false
        }
    });

    let CodeActionOrCommand::CodeAction(action) =
        safe_delete_action.expect("Safe Delete action for used method")
    else {
        panic!("Safe Delete must be a code action");
    };
    assert!(
        action.title.contains("2 usages"),
        "expected exact workspace usage count, got: {}",
        action.title
    );
    assert!(
        action.disabled.is_some(),
        "used declarations must not produce an enabled delete"
    );
    assert!(
        action.edit.is_none(),
        "disabled delete must not include an edit"
    );
}

/// Test Safe Delete for unused class
#[test]
fn test_safe_delete_unused_class() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/UnusedClass.java",
        r"
package com.example;

public class UnusedClass {
    // This class is never instantiated
}
",
    );
    harness.initialize().expect("Failed to initialize");

    // Request code actions at the class name
    let result = harness
        .code_action(
            "com/example/UnusedClass.java",
            Range::new(Position::new(3, 13), Position::new(3, 13)),
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Safe Delete actions should be returned");

    // Should have a Safe Delete action
    let safe_delete_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.starts_with("Safe Delete")
        } else {
            false
        }
    });

    assert!(
        safe_delete_action.is_some(),
        "Expected Safe Delete action for unused class"
    );
    let CodeActionOrCommand::CodeAction(action) = safe_delete_action.unwrap() else {
        panic!("Safe Delete must be a code action");
    };
    assert!(action.disabled.is_none());
    assert!(action.edit.is_some());
}

/// Test Safe Delete for unused field
#[test]
fn test_safe_delete_unused_field() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/Entity.java",
        r"
package com.example;

public class Entity {
    private String unusedField;
    private String usedField;

    public String getUsedField() {
        return usedField;
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    // Request code actions at the unusedField declaration
    let result = harness
        .code_action(
            "com/example/Entity.java",
            Range::new(Position::new(4, 19), Position::new(4, 19)),
            vec![],
        )
        .expect("Code action failed");

    let actions = result.expect("Safe Delete actions should be returned");

    // Should have a Safe Delete action
    let safe_delete_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.starts_with("Safe Delete")
        } else {
            false
        }
    });

    assert!(
        safe_delete_action.is_some(),
        "Expected Safe Delete action for unused field"
    );
    let CodeActionOrCommand::CodeAction(action) = safe_delete_action.unwrap() else {
        panic!("Safe Delete must be a code action");
    };
    assert!(action.disabled.is_none());
    assert!(action.edit.is_some());
}

/// Test that Safe Delete action has correct kind
#[test]
fn test_safe_delete_action_kind() {
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

    let actions = result.expect("Safe Delete actions should be returned");

    let safe_delete_action = actions.iter().find(|action| {
        if let CodeActionOrCommand::CodeAction(ca) = action {
            ca.title.starts_with("Safe Delete")
        } else {
            false
        }
    });

    let CodeActionOrCommand::CodeAction(action) =
        safe_delete_action.expect("Safe Delete action should be present")
    else {
        panic!("Safe Delete must be a code action");
    };
    assert_eq!(
        action.kind,
        Some(CodeActionKind::REFACTOR),
        "Safe Delete should have REFACTOR kind"
    );
}

/// Same-named members in another class must not affect the selected declaration.
#[test]
fn test_safe_delete_scopes_usages_to_the_declaring_class() {
    let mut harness = LspTest::new();
    harness.add_file(
        "com/example/First.java",
        r"
package com.example;

public class First {
    public void shared() {}
}
",
    );
    harness.add_file(
        "com/example/Second.java",
        r"
package com.example;

public class Second {
    public void shared() {}
}
",
    );
    harness.add_file(
        "com/example/Client.java",
        r"
package com.example;

public class Client {
    public void use(First first) {
        first.shared();
    }
}
",
    );
    harness.initialize().expect("Failed to initialize");

    let actions = harness
        .code_action(
            "com/example/Second.java",
            Range::new(Position::new(4, 16), Position::new(4, 16)),
            vec![],
        )
        .expect("Code action failed")
        .expect("Safe Delete actions should be returned");

    let action = actions
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Safe Delete") => {
                Some(action)
            }
            _ => None,
        })
        .expect("Safe Delete action for Second.shared");

    assert!(
        action.title.contains("no usages"),
        "First.shared usages must not leak into Second.shared: {}",
        action.title
    );
    assert!(action.disabled.is_none());
    assert!(action.edit.is_some());
}

#[test]
fn safe_delete_binds_the_exact_overload() {
    let service = r#"package com.example;
public class Service {
    public void pick() {}
    public void pick(int value) {}
}
"#;
    let client = r#"package com.example;
public class Client {
    void use(Service service) { service.pick(1); }
}
"#;
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Service.java", service)
        .add_file("com/example/Client.java", client);

    let unused = safe_delete_action_at(&mut harness, "com/example/Service.java", service, "pick()")
        .expect("unused overload action");
    assert!(unused.disabled.is_none());
    assert!(unused.edit.is_some());

    let used = safe_delete_action_at(
        &mut harness,
        "com/example/Service.java",
        service,
        "pick(int",
    )
    .expect("used overload action");
    assert!(used.title.contains("1 usage"), "{}", used.title);
    assert!(used.disabled.is_some());
    assert!(used.edit.is_none());
}

#[test]
fn safe_delete_rejects_non_declaration_identifiers_inside_a_field() {
    let source = r#"package com.example;
class Helper {}
class Entity {
    private Helper value = make();
    private Helper make() { return null; }
}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);

    assert!(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        source,
        "Helper value"
    )
    .is_none());
    assert!(
        safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "make();").is_none()
    );
}

#[test]
fn safe_delete_rejects_multi_declarator_fields() {
    let source = "package com.example;\nclass Entity { int dead, live; }\n";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);

    assert!(
        safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead").is_none()
    );
}

#[test]
fn safe_delete_owns_javadoc_and_modifiers_but_not_the_next_member() {
    let source = r#"package com.example;
class Entity {
    /** obsolete */
    public
    void dead() {}
    void live() {}
}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead()")
        .expect("safe delete action");
    assert!(action.disabled.is_none());

    let uri = harness.file_uri("com/example/Entity.java").unwrap();
    let edited = apply_safe_delete(source, &uri, &action);
    assert!(!edited.contains("obsolete"), "{edited}");
    assert!(!edited.contains("dead"));
    assert!(edited.contains("void live() {}"));
}

#[test]
fn safe_delete_never_attaches_an_older_javadoc_across_an_ordinary_comment() {
    let source = r#"package com.example;
class Entity {
    /** docs for live */
    void live() {}
    /* ordinary comment */
    void dead() {}
}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead()")
        .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);

    let edited = apply_safe_delete(
        source,
        &harness.file_uri("com/example/Entity.java").unwrap(),
        &action,
    );
    assert!(edited.contains("docs for live"), "{edited}");
    assert!(edited.contains("void live() {}"), "{edited}");
    assert!(edited.contains("ordinary comment"), "{edited}");
    assert!(!edited.contains("void dead()"), "{edited}");
}

#[test]
fn safe_delete_never_crosses_an_adjacent_same_line_declaration() {
    let source = "package com.example;\nclass Entity { void dead() {} void live() {} }\n";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead()")
        .expect("safe delete action");
    assert!(action.disabled.is_none());

    let uri = harness.file_uri("com/example/Entity.java").unwrap();
    let edited = apply_safe_delete(source, &uri, &action);
    assert_eq!(
        edited,
        "package com.example;\nclass Entity {  void live() {} }\n"
    );
}

#[test]
fn safe_delete_disables_zero_call_interface_obligations() {
    let contract = "package com.example; public interface Task { void run(); }\n";
    let implementation =
        "package com.example; public class Worker implements Task { public void run() {} }\n";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Task.java", contract)
        .add_file("com/example/Worker.java", implementation);

    let action = safe_delete_action_at(
        &mut harness,
        "com/example/Worker.java",
        implementation,
        "run()",
    )
    .expect("family-aware action");
    assert!(action.title.contains("dispatch family"), "{}", action.title);
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_disables_inherited_class_interface_join_obligations() {
    let base = "package com.example; public class Base { public void run() {} }\n";
    let contract = "package com.example; public interface Task { void run(); }\n";
    let implementation =
        "package com.example; public class Worker extends Base implements Task {}\n";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Base.java", base)
        .add_file("com/example/Task.java", contract)
        .add_file("com/example/Worker.java", implementation);

    let action = safe_delete_action_at(&mut harness, "com/example/Base.java", base, "run()")
        .expect("join-aware action");
    assert!(action.title.contains("dispatch family"), "{}", action.title);
    assert!(action.disabled.is_some());
}

#[test]
fn safe_delete_allows_a_recursive_only_method() {
    let source = r#"package com.example;
class Entity {
    private void dead() { dead(); }
    void live() {}
}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead()")
        .expect("recursive-only safe delete");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);

    let edited = apply_safe_delete(
        source,
        &harness.file_uri("com/example/Entity.java").unwrap(),
        &action,
    );
    assert!(!edited.contains("dead"));
    assert!(edited.contains("live"));
}

#[test]
fn safe_delete_scans_the_complete_type_declaration_closure() {
    let unused = r#"package com.example;
public class Unused {
    public static void run() {}
}
"#;
    let client = r#"package com.example;
import static com.example.Unused.run;
public class Client {}
"#;
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Unused.java", unused)
        .add_file("com/example/Client.java", client);

    let action = safe_delete_action_at(&mut harness, "com/example/Unused.java", unused, "Unused")
        .expect("type safe delete action");
    assert!(
        action.title.contains("import dependency"),
        "{}",
        action.title
    );
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_ignores_self_contained_type_references() {
    let source = r#"package com.example;
class Dead {
    Dead self;
    Dead make() { return new Dead(); }
}
class Live {}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Dead.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Dead.java", source, "Dead")
        .expect("self-contained type action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);

    let edited = apply_safe_delete(
        source,
        &harness.file_uri("com/example/Dead.java").unwrap(),
        &action,
    );
    assert!(!edited.contains("class Dead"));
    assert!(edited.contains("class Live"));
}

#[test]
fn safe_delete_disables_fields_with_initializers() {
    let source = r#"package com.example;
class Entity {
    private int dead = sideEffect();
    private int sideEffect() { return 1; }
}
"#;
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("initializer-aware action");
    assert!(
        action.title.contains("unsafe initializer"),
        "{}",
        action.title
    );
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_preserves_crlf_and_eof_neighbors() {
    let source =
        "package com.example;\r\nclass Entity {\r\n    void dead() {}\r\n    void live() {}\r\n}";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead()")
        .expect("CRLF safe delete action");
    let edited = apply_safe_delete(
        source,
        &harness.file_uri("com/example/Entity.java").unwrap(),
        &action,
    );
    assert_eq!(
        edited,
        "package com.example;\r\nclass Entity {\r\n    void live() {}\r\n}"
    );
}

#[test]
fn safe_delete_fails_closed_for_class_literal_references() {
    let dead = "package com.example; public class Dead {}";
    let client = "package com.example; class Client { Class<?> token = Dead.class; }";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Dead.java", dead)
        .add_file("com/example/Client.java", client);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Dead.java",
        dead,
        "Dead",
    ));
}

#[test]
fn safe_delete_fails_closed_for_super_method_references() {
    let base = "package com.example; class Base { void dead() {} }";
    let child = concat!(
        "package com.example; class Child extends Base { ",
        "void use() { super.dead(); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Base.java", base)
        .add_file("com/example/Child.java", child);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Base.java",
        base,
        "dead",
    ));
}

#[test]
fn safe_delete_fails_closed_for_static_wildcard_calls() {
    let util = "package com.example; class Util { static void dead() {} }";
    let client = concat!(
        "package com.example; import static com.example.Util.*; ",
        "class Client { void use() { dead(); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Util.java", util)
        .add_file("com/example/Client.java", client);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Util.java",
        util,
        "dead",
    ));
}

#[test]
fn safe_delete_fails_closed_for_anonymous_overrides() {
    let base = "package com.example; class Base { void dead() {} }";
    let client = concat!(
        "package com.example; class Client { Base value = new Base() { ",
        "@Override void dead() {} }; }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Base.java", base)
        .add_file("com/example/Client.java", client);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Base.java",
        base,
        "dead",
    ));
}

#[test]
fn safe_delete_disables_abstract_sam_methods_without_named_usages() {
    let source = "package com.example; interface Task { void run(); }";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Task.java", source);

    let action = safe_delete_action_at(&mut harness, "com/example/Task.java", source, "run")
        .expect("abstract method should have an explanatory disabled action");
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_disables_fields_with_annotated_api_generation_context() {
    let source = concat!(
        "package com.example; @lombok.Getter class Entity { ",
        "private int dead; }"
    );
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);

    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("annotated field should have an explanatory disabled action");
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_disables_annotated_and_runtime_discovered_declarations() {
    for (source, needle) in [
        (
            "package com.example; class Entity { @Deprecated void dead() {} }",
            "dead",
        ),
        ("package com.example; @Deprecated class Dead {}", "Dead"),
        (
            "package com.example; class Entity { native void dead(); }",
            "dead",
        ),
        (
            "package com.example; class Entity { public static void main(String[] args) {} }",
            "main",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { private Object readResolve() { return this; } }",
            "readResolve",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { private static final long serialVersionUID = 1L; }",
            "serialVersionUID",
        ),
        (
            "package com.example; class Base { protected Object readResolve() { return this; } } class Entity extends Base implements java.io.Serializable {}",
            "readResolve",
        ),
    ] {
        let mut harness = LspTest::new();
        harness.add_file("com/example/Entity.java", source);
        let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, needle)
            .expect("runtime-discovered declaration should explain why deletion is disabled");
        assert!(action.disabled.is_some(), "{source}: {action:?}");
        assert!(action.edit.is_none(), "{source}: {action:?}");
    }
}

#[test]
fn convention_guards_match_signatures_instead_of_names_alone() {
    for (source, needle) in [
        (
            "package com.example; class Entity { public static void main() {} }",
            "main",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { Object readResolve(int ignored) { return this; } }",
            "readResolve",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { private long serialVersionUID; }",
            "serialVersionUID",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { private static Object readResolve() { return null; } }",
            "readResolve",
        ),
        (
            "package com.example; class Entity implements java.io.Serializable { public void readObject(java.io.ObjectInputStream in) {} }",
            "readObject",
        ),
        (
            "package com.example; class Entity { private Object readResolve() { return this; } }",
            "readResolve",
        ),
        (
            "package com.example; class Entity implements java.io.Externalizable { private void readObject(java.io.ObjectInputStream in) {} }",
            "readObject",
        ),
    ] {
        let mut harness = LspTest::new();
        harness.add_file("com/example/Entity.java", source);
        let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, needle)
            .expect("same-named ordinary declaration should have a safe-delete action");
        assert!(action.disabled.is_none(), "{source}: {:?}", action.disabled);
        assert!(action.edit.is_some(), "{source}: {action:?}");
    }
}

#[test]
fn dynamic_lookup_detection_checks_only_the_call_callee() {
    let declaration = "package com.example; class Entity { void dead() {} }";
    let unrelated = concat!(
        "package com.example; class Client { Object getMethod; ",
        "void consume(Object value) {} void run() { consume(getMethod); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Client.java", unrelated);

    let action =
        safe_delete_action_at(&mut harness, "com/example/Entity.java", declaration, "dead")
            .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn constant_reflection_name_proven_unrelated_does_not_block() {
    let declaration = "package com.example; class Entity { void dead() {} }";
    let reflection = concat!(
        "package com.example; class Client { void run() throws Exception { ",
        "Entity.class.getDeclaredMethod(\"live\"); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Client.java", reflection);

    let action =
        safe_delete_action_at(&mut harness, "com/example/Entity.java", declaration, "dead")
            .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn same_named_user_method_is_not_a_reflection_barrier() {
    let declaration = "package com.example; class Entity { void dead() {} }";
    let unrelated = concat!(
        "package com.example; class Client { void getMethod(String value) {} ",
        "void run(String configured) { getMethod(configured); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Client.java", unrelated);

    let action =
        safe_delete_action_at(&mut harness, "com/example/Entity.java", declaration, "dead")
            .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn method_handles_field_lookup_is_a_dynamic_usage_barrier() {
    let declaration = "package com.example; class Entity { int dead; }";
    let reflection = concat!(
        "package com.example; import java.lang.invoke.MethodHandles; class Client { ",
        "void run() throws Exception { MethodHandles.lookup().findGetter(",
        "Entity.class, \"de\" + \"ad\", int.class); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Client.java", reflection);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        declaration,
        "dead",
    ));
}

#[test]
fn method_handles_bind_is_a_dynamic_usage_barrier() {
    let declaration = "package com.example; class Entity { void dead() {} }";
    let reflection = concat!(
        "package com.example; import java.lang.invoke.MethodHandles; class Client { ",
        "void run(Entity entity, String configured) throws Exception { ",
        "MethodHandles.lookup().bind(entity, configured, ",
        "java.lang.invoke.MethodType.methodType(void.class)); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Client.java", reflection);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        declaration,
        "dead",
    ));
}

#[test]
fn reflection_method_reference_is_a_dynamic_usage_barrier() {
    let declaration = "package com.example; class Dead {}";
    let reflection = concat!(
        "package com.example; interface Loader { Class<?> load(String name) throws Exception; } ",
        "class Client { Loader loader = Class::forName; }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Dead.java", declaration)
        .add_file("com/example/Client.java", reflection);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Dead.java",
        declaration,
        "Dead",
    ));
}

#[test]
fn disappearing_declaration_does_not_block_on_its_own_dynamic_lookup() {
    let source = concat!(
        "package com.example; class Entity { ",
        "void dead() throws Exception { getClass().getDeclaredMethod(\"anything\"); } }"
    );
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);

    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn ordinary_comment_with_target_name_is_not_a_runtime_usage() {
    let declaration = "package com.example; class Entity { void dead() {} }";
    let prose = "package com.example; // dead is intentionally unused\nclass Notes {}";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Notes.java", prose);

    let action =
        safe_delete_action_at(&mut harness, "com/example/Entity.java", declaration, "dead")
            .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn same_named_java_local_binding_is_not_a_target_usage() {
    let declaration = "package com.example; class Entity { int dead; }";
    let local = concat!(
        "package com.example; class Other { void run() { ",
        "int dead = 1; System.out.println(dead); } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Other.java", local);

    let action =
        safe_delete_action_at(&mut harness, "com/example/Entity.java", declaration, "dead")
            .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn safe_delete_fails_closed_for_reflective_strings_and_javadoc_links() {
    for client in [
        concat!(
            "package com.example; class Client { void use() throws Exception { ",
            "Entity.class.getDeclaredMethod(\"dead\"); } }"
        ),
        concat!(
            "package com.example; /** See {@link Entity#dead()} */ ",
            "class Client {}"
        ),
    ] {
        let source = "package com.example; class Entity { void dead() {} }";
        let mut harness = LspTest::new();
        harness
            .add_file("com/example/Entity.java", source)
            .add_file("com/example/Client.java", client);
        assert_safe_delete_not_enabled(safe_delete_action_at(
            &mut harness,
            "com/example/Entity.java",
            source,
            "dead",
        ));
    }
}

#[test]
fn safe_delete_fails_closed_for_computed_reflection_names() {
    let declaration = r#"package com.example;
class Entity { private void dead() {} }
"#;
    let reflection = r#"package com.example;
class ReflectionClient {
    void run() throws Exception {
        Entity.class.getDeclaredMethod("de" + "ad").invoke(new Entity());
    }
}
"#;
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/ReflectionClient.java", reflection);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        declaration,
        "dead()",
    ));
}

#[test]
fn safe_delete_comment_and_string_guard_matches_identifier_boundaries() {
    let declaration = r#"package com.example;
class Entity {
    private void get() {}
}
"#;
    let unrelated = r#"package com.example;
class Other {
    // A target and a budget are ordinary prose, not method references.
    String text = "target budget";
}
"#;
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", declaration)
        .add_file("com/example/Other.java", unrelated);

    let action = safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        declaration,
        "get()",
    )
    .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn safe_delete_fails_closed_for_runtime_resource_references() {
    let declaration = "package com.example; public class PluginImpl {}";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/PluginImpl.java", declaration)
        .add_file(
            "src/main/resources/META-INF/services/com.example.Plugin",
            "com.example.PluginImpl\n",
        );

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/PluginImpl.java",
        declaration,
        "PluginImpl",
    ));
}

#[test]
fn safe_delete_counts_static_imports_of_nested_types() {
    let outer = "package com.example; class Outer { static class Dead {} }";
    let client = concat!(
        "package com.example; import static com.example.Outer.Dead; ",
        "class Client {}"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Outer.java", outer)
        .add_file("com/example/Client.java", client);

    let action = safe_delete_action_at(&mut harness, "com/example/Outer.java", outer, "Dead")
        .expect("nested type import should have an explanatory disabled action");
    assert!(
        action.title.contains("import dependency"),
        "{}",
        action.title
    );
    assert!(action.disabled.is_some());
    assert!(action.edit.is_none());
}

#[test]
fn safe_delete_is_suppressed_when_another_java_source_has_parse_errors() {
    let source = "package com.example; class Entity { void dead() {} }";
    let broken = "package com.example; class Broken { void nope( }";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", source)
        .add_file("com/example/Broken.java", broken);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        source,
        "dead",
    ));
}

#[test]
fn safe_delete_allows_unrelated_mixed_language_sources() {
    let source = "package com.example; class Entity { void dead() {} }";
    let kotlin = "package com.example\nclass KotlinPeer";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", source)
        .add_file("com/example/KotlinPeer.kt", kotlin);

    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("safe delete action");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn safe_delete_distinguishes_same_named_non_java_declarations() {
    let source = "package com.example; class Entity { void dead() {} }";
    let kotlin = "package com.example\nclass KotlinPeer { fun dead() = Unit }";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", source)
        .add_file("com/example/KotlinPeer.kt", kotlin);

    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("the unrelated Kotlin declaration is semantically accounted for");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn safe_delete_distinguishes_kotlin_local_string_template_binding() {
    let source = "package com.example; class Entity { void dead() {} }";
    let kotlin = concat!(
        "package com.example\nclass KotlinPeer { fun run() { ",
        "val dead = 1; println(\"$dead\") } }"
    );
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", source)
        .add_file("com/example/KotlinPeer.kt", kotlin);

    let action = safe_delete_action_at(&mut harness, "com/example/Entity.java", source, "dead")
        .expect("the Kotlin template binds its same-named local");
    assert!(action.disabled.is_none(), "{:?}", action.disabled);
}

#[test]
fn safe_delete_fails_closed_for_same_spelled_mixed_language_sources() {
    let source = "package com.example; class Entity { void dead() {} }";
    let kotlin = "package com.example\nclass KotlinPeer { fun run() = dead() }";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Entity.java", source)
        .add_file("com/example/KotlinPeer.kt", kotlin);

    assert_safe_delete_not_enabled(safe_delete_action_at(
        &mut harness,
        "com/example/Entity.java",
        source,
        "dead",
    ));
}

#[test]
fn safe_delete_rejects_unindexed_request_bytes() {
    let indexed = "package com.example; class Entity { void dead() {} }\n";
    let request = "package com.example; class Entity { void live() {} }\n";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", indexed);
    let offset = request.find("live").unwrap();
    let position = hyperion_lsp::position_utils::offset_to_position(request, offset);
    let actions = harness
        .code_action_with_unindexed_source(
            "com/example/Entity.java",
            Range::new(position, position),
            request,
        )
        .expect("stale code action request")
        .unwrap_or_default();
    assert!(!actions.iter().any(|action| matches!(
        action,
        CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Safe Delete")
    )));
}

#[test]
fn safe_delete_is_suppressed_until_the_workspace_is_ready() {
    let source = "package com.example; class Entity { void dead() {} }\n";
    let mut harness = LspTest::new();
    harness.add_file("com/example/Entity.java", source);
    let offset = source.find("dead").unwrap();
    let position = hyperion_lsp::position_utils::offset_to_position(source, offset);
    let actions = harness
        .code_action_with_workspace_ready(
            "com/example/Entity.java",
            Range::new(position, position),
            false,
        )
        .expect("not-ready code action request")
        .unwrap_or_default();
    assert!(!actions.iter().any(|action| matches!(
        action,
        CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Safe Delete")
    )));
}

#[test]
fn safe_delete_disables_types_with_external_subtype_declarations() {
    let base = "package com.example; public class Base {}\n";
    let child = "package com.example; public class Child extends Base {}\n";
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Base.java", base)
        .add_file("com/example/Child.java", child);
    let action = safe_delete_action_at(&mut harness, "com/example/Base.java", base, "Base")
        .expect("structural type dependency action");
    assert!(action.disabled.is_some());
    assert!(
        action
            .disabled
            .as_ref()
            .is_some_and(|disabled| disabled.reason.contains("subtype")),
        "{:?}",
        action.disabled
    );
}

#[test]
fn safe_delete_fails_closed_for_inherited_overloaded_method_references() {
    let base = r#"package com.example;
public class Base {
    public void pick(String value) {}
    public void pick(int value) {}
}
"#;
    let child = "package com.example; public class Child extends Base {}\n";
    let client = r#"package com.example;
import java.util.function.BiConsumer;
class Client {
    BiConsumer<Child, String> reference = Child::pick;
}
"#;
    let mut harness = LspTest::new();
    harness
        .add_file("com/example/Base.java", base)
        .add_file("com/example/Child.java", child)
        .add_file("com/example/Client.java", client);

    assert!(
        safe_delete_action_at(&mut harness, "com/example/Base.java", base, "pick(String").is_none()
    );
}
