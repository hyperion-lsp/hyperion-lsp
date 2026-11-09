// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Invocation inference must check each lambda against its candidate's SAM.

use hyperion_parser::SyntaxKind;
use hyperion_semantics::db::{
    Database, ParseDatabase, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::symbol::{ClassType, Type};
use hyperion_workspace::{FileId, Language, WorkspaceIndex};
use std::sync::Arc;

fn infer_call(source: &str, call_text: &str) -> Type {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    workspace_index
        .index_file_with_source(&temp.path().join("example/Calls.java"), source)
        .expect("fixture should index");

    let file_id = FileId::new(0);
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let tree = db.parse_file(file_id);
    let call = tree
        .syntax()
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_CALL_EXPR
                && node.text().to_string().trim() == call_text
        })
        .unwrap_or_else(|| panic!("missing call {call_text:?}"));
    db.file_semantics(file_id)
        .expression_type_at(call.text_range())
        .cloned()
        .unwrap_or(Type::Unknown)
}

fn example_type(name: &str) -> Type {
    Type::Class(ClassType::new(name.into(), Some("example".into())))
}

#[test]
fn varargs_functional_parameter_accepts_multiple_lambdas() {
    let source = r#"
package example;

interface Action { void run(); }
class Result {}
class Runner {
    Result run(Action... actions) { return new Result(); }
}
class Calls {
    Result test(Runner runner) {
        return runner.run(() -> {}, () -> {});
    }
}
"#;

    assert_eq!(
        infer_call(source, "runner.run(() -> {}, () -> {})"),
        example_type("Result")
    );
}

#[test]
fn void_sam_accepts_value_producing_statement_expression() {
    let source = r#"
package example;

interface Action { void run(); }
class Result {}
class Runner {
    Result run(Action action) { return new Result(); }
}
class Calls {
    int compute() { return 1; }
    Result test(Runner runner) {
        return runner.run(() -> compute());
    }
}
"#;

    assert_eq!(
        infer_call(source, "runner.run(() -> compute())"),
        example_type("Result")
    );
}

#[test]
fn lambda_lower_bounds_are_independent_of_argument_order() {
    let source = r#"
package example;

interface Supplier<T> { T get(); }
class Animal {}
class Dog extends Animal {}
class Calls {
    <T> T combine(Supplier<T> first, Supplier<T> second) { return null; }

    void test() {
        Animal a = combine(() -> new Dog(), () -> new Animal());
        Animal b = combine(() -> new Animal(), () -> new Dog());
    }
}
"#;

    for call in [
        "combine(() -> new Dog(), () -> new Animal())",
        "combine(() -> new Animal(), () -> new Dog())",
    ] {
        assert_eq!(infer_call(source, call), example_type("Animal"), "{call}");
    }
}

#[test]
fn lambda_arity_rejects_incompatible_overload() {
    let source = r#"
package example;

interface Unary { String apply(String value); }
interface Binary { String apply(String left, String right); }
class UnaryResult {}
class BinaryResult {}
class Calls {
    UnaryResult choose(Unary function) { return new UnaryResult(); }
    BinaryResult choose(Binary function) { return new BinaryResult(); }

    BinaryResult test() {
        return choose((left, right) -> left);
    }
}
"#;

    assert_eq!(
        infer_call(source, "choose((left, right) -> left)"),
        example_type("BinaryResult")
    );
}
