// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: binary expression inference through the
//! real CST path. The parser bumps trivia (whitespace/comments) inside
//! BINARY_EXPR, and same-precedence chains produce continuation nodes
//! (`a + b + c` → sibling `BINARY_EXPR [PLUS, NAME_EXPR(c)]` whose left
//! operand is the preceding sibling expression).

use std::sync::Arc;

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{
    Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase, WorkspaceIndexHandle,
};
use hyperion_semantics::{ClassType, PrimitiveType, Type};
use hyperion_workspace::{Language, WorkspaceIndex};

/// Infer the type of the BINARY_EXPR whose trimmed text equals `expr_text`.
fn inferred_binary_expr_type(source: &str, expr_text: &str) -> Type {
    let file_id = FileId::new(0);
    let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from(
        "/tmp/hyperion-binary-expression-inference",
    )));
    let mut db = Database::default();
    db.set_file_text(file_id, Arc::new(source.to_owned()));
    db.set_file_language(file_id, Language::Java);
    db.set_workspace_index_handle(WorkspaceIndexHandle(workspace_index));
    db.set_workspace_version(0);

    let parse = db.parse_file(file_id);
    let range = parse
        .syntax()
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::BINARY_EXPR && node.text().to_string().trim() == expr_text
        })
        .unwrap_or_else(|| panic!("expected a BINARY_EXPR with text {expr_text:?}"))
        .text_range();

    db.snapshot()
        .file_semantics(file_id)
        .expression_type_at(range)
        .cloned()
        .unwrap_or(Type::Unknown)
}

#[test]
fn spaced_int_addition_infers_int() {
    let source = r#"
class C {
    void test() {
        int a = 1;
        int b = 2;
        int sum = a + b;
    }
}
"#;

    assert_eq!(
        inferred_binary_expr_type(source, "a + b"),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn spaced_string_concatenation_infers_string() {
    let source = r#"
class C {
    void test() {
        int x = 42;
        String s = "value: " + x;
    }
}
"#;

    assert_eq!(
        inferred_binary_expr_type(source, "\"value: \" + x"),
        Type::Class(ClassType::string())
    );
}

#[test]
fn spaced_comparison_infers_boolean() {
    let source = r#"
class C {
    void test() {
        int a = 1;
        int b = 2;
        boolean eq = a == b;
    }
}
"#;

    assert_eq!(
        inferred_binary_expr_type(source, "a == b"),
        Type::Primitive(PrimitiveType::Boolean)
    );
}

#[test]
fn same_precedence_chain_infers_through_continuation_node() {
    // `a + b + c` parses as sibling BINARY_EXPR nodes:
    //   BINARY_EXPR [NAME_EXPR(a), PLUS, NAME_EXPR(b)]
    //   BINARY_EXPR [PLUS, NAME_EXPR(c)]  (left operand = preceding sibling)
    let source = r#"
class C {
    void test() {
        int a = 1;
        int b = 2;
        int c = 3;
        int sum = a + b + c;
    }
}
"#;

    // Head node: a + b
    assert_eq!(
        inferred_binary_expr_type(source, "a + b"),
        Type::Primitive(PrimitiveType::Int)
    );

    // Continuation node: + c (left operand is the preceding sibling)
    assert_eq!(
        inferred_binary_expr_type(source, "+ c"),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn chain_with_string_head_infers_string_in_continuation() {
    let source = r#"
class C {
    void test() {
        int a = 1;
        int b = 2;
        String s = "n=" + a + b;
    }
}
"#;

    assert_eq!(
        inferred_binary_expr_type(source, "+ b"),
        Type::Class(ClassType::string())
    );
}

#[test]
fn comment_before_operator_infers_int() {
    let source = r#"
class C {
    void test() {
        int a = 1;
        int b = 2;
        int sum = a /*x*/ + b;
    }
}
"#;

    assert_eq!(
        inferred_binary_expr_type(source, "a /*x*/ + b"),
        Type::Primitive(PrimitiveType::Int)
    );
}
