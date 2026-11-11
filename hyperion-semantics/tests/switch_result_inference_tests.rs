// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Both the `var` symbol and expression consumers must see all values yielded
//! by this switch, excluding values belonging to nested switches or methods.

use std::sync::Arc;

use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxKind, SyntaxNode};
use hyperion_semantics::{
    inference::TypeInferenceEngine, ClassType, PrimitiveType, ResolverBuilder, Type,
};
use hyperion_workspace::WorkspaceIndex;

fn assert_switch_type(body: &str, expected: Type) {
    let source = format!("class Example {{ void run(int mode, boolean flag) {{ var result = switch (mode) {{ {body} }}; }} }}");
    let root = tempfile::tempdir().expect("workspace directory");
    let index = Arc::new(WorkspaceIndex::new(root.path().to_path_buf()));
    let path = root.path().join("Example.java");
    index
        .index_file_with_source(&path, &source)
        .expect("index source");
    let file_id = index.get_file_id(&path).expect("file ID");
    let (green, errors) = Parser::parse(Lexer::new(&source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let syntax = SyntaxNode::new_root(green);
    let resolver =
        ResolverBuilder::with_workspace(file_id, Arc::clone(&index)).build_from_syntax(&syntax);
    let inferred_var = resolver
        .symbol_table()
        .all_symbols()
        .iter()
        .find(|symbol| symbol.name == "result")
        .expect("result variable")
        .type_info
        .clone();
    let switch = syntax
        .descendants()
        .find(|node| node.kind() == SyntaxKind::SWITCH_EXPR)
        .expect("outer switch expression");
    let engine = TypeInferenceEngine::new(index, file_id);
    let inferred_expression = engine.infer_expression(&switch, &resolver, None);
    assert_eq!(
        (inferred_var, inferred_expression),
        (expected.clone(), expected)
    );
}

#[test]
fn all_yields_in_arrow_block_contribute_to_numeric_result() {
    assert_switch_type(
        "case 0 -> { if (flag) yield 1; yield 2L; } default -> 3;",
        Type::Primitive(PrimitiveType::Long),
    );
}

#[test]
fn all_yields_in_colon_group_contribute_to_numeric_result() {
    assert_switch_type(
        "case 0: if (flag) yield 2L; yield 1; default: yield 3;",
        Type::Primitive(PrimitiveType::Long),
    );
}

#[test]
fn nested_switch_yields_do_not_change_outer_string_result() {
    assert_switch_type(
        r#"case 0 -> { var count = switch (mode) { default -> { yield 1; } }; yield "ready"; } default -> "waiting";"#,
        Type::Class(ClassType::string()),
    );
}

#[test]
fn yielded_nested_switch_keeps_its_complete_result_type() {
    assert_switch_type("case 0 -> { yield switch (mode) { default -> { if (flag) yield 1; yield 2L; } }; } default -> 3;", Type::Primitive(PrimitiveType::Long));
}

#[test]
fn unknown_arrow_result_does_not_invent_known_type() {
    assert_switch_type("case 0 -> missing(); default -> 1;", Type::Unknown);
}

#[test]
fn unknown_yield_does_not_invent_known_type() {
    assert_switch_type(
        "case 0 -> { if (flag) yield 1; yield missing(); } default -> 2;",
        Type::Unknown,
    );
}

#[test]
fn throw_arms_and_fallthrough_labels_do_not_add_result_types() {
    assert_switch_type(
        "case 0: case 1: yield 2L; default: throw new IllegalStateException();",
        Type::Primitive(PrimitiveType::Long),
    );
    assert_switch_type(
        "case 0 -> { throw new IllegalStateException(); } default -> 2L;",
        Type::Primitive(PrimitiveType::Long),
    );
}

#[test]
fn local_class_switch_yields_do_not_change_outer_string_result() {
    assert_switch_type(
        r#"case 0 -> { class Local { int value() { return switch (mode) { default -> { yield 1; } }; } } yield "ready"; } default -> "waiting";"#,
        Type::Class(ClassType::string()),
    );
}

#[test]
fn yielded_local_in_arm_is_available_when_inferring_var() {
    assert_switch_type(
        r#"case 0 -> { String message = "ready"; yield message; } default -> "waiting";"#,
        Type::Class(ClassType::string()),
    );
}

#[test]
fn yield_inside_nested_switch_statement_targets_outer_expression() {
    assert_switch_type(
        "case 0 -> { switch (mode) { case 1: yield 2L; default: break; } yield 1; } default -> 3;",
        Type::Primitive(PrimitiveType::Long),
    );
}

#[test]
fn arm_local_does_not_leak_into_sibling_arm() {
    assert_switch_type(
        r#"case 0 -> { String message = "ready"; yield message; } default -> message;"#,
        Type::Unknown,
    );
}
