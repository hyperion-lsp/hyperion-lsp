// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Inline Variable must not change what the program means: a reassigned variable is
//! not inlined, an operator initializer keeps its grouping, and the declaration's
//! removal leaves any other statement on its line alone.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{CodeActionOrCommand, Position, Range, TextEdit};

/// Byte offset of an LSP position in ASCII source.
fn offset(source: &str, position: Position) -> usize {
    let mut line_start = 0;
    for _ in 0..position.line {
        line_start += source[line_start..].find('\n').expect("line") + 1;
    }
    line_start + position.character as usize
}

fn apply(source: &str, edits: &[TextEdit]) -> String {
    let mut ordered: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            (
                offset(source, edit.range.start),
                offset(source, edit.range.end),
                edit.new_text.as_str(),
            )
        })
        .collect();
    ordered.sort_by_key(|item| std::cmp::Reverse(item.0));
    let mut out = source.to_string();
    for (start, end, text) in ordered {
        out.replace_range(start..end, text);
    }
    out
}

/// The edits of the Inline Variable action for the variable at `needle`, if offered.
fn inline_edits(source: &str, needle: &str) -> Option<Vec<TextEdit>> {
    let mut test = LspTest::new();
    test.add_file("A.java", source);
    let start = source.find(needle).expect("needle in source");
    let before = &source[..start];
    let line = before.matches('\n').count() as u32;
    let column = (start - before.rfind('\n').map_or(0, |p| p + 1)) as u32;
    let position = Position::new(line, column);
    let actions = test
        .code_action("A.java", Range::new(position, position), vec![])
        .expect("code action request")
        .unwrap_or_default();
    actions.into_iter().find_map(|action| match action {
        CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Inline variable") => {
            action.edit?.changes?.into_values().next()
        }
        _ => None,
    })
}

#[test]
fn reassigned_variable_is_not_inlined() {
    let source = "class A {\n    void m() {\n        int x = 5;\n        x = 3;\n        System.out.println(x);\n    }\n}\n";
    assert!(inline_edits(source, "x = 5").is_none());
}

#[test]
fn operator_initializer_keeps_its_grouping() {
    let source = "class A {\n    void m(int a, int b) {\n        int r = a % b;\n        int z = 2 * r;\n    }\n}\n";
    let edits = inline_edits(source, "r = a % b").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int z = 2 * (a % b);"), "{result}");
    assert!(!result.contains("int r"), "{result}");
}

#[test]
fn declaration_sharing_its_line_keeps_the_other_statement() {
    let source = "class A {\n    void m() {\n        int x = 5; int y = x + 1;\n    }\n}\n";
    let edits = inline_edits(source, "x = 5").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int y = 5 + 1;"), "{result}");
    assert!(!result.contains("int x"), "{result}");
}

#[test]
fn member_access_of_the_same_name_is_not_a_usage() {
    let source = "class A {\n    void m(B b) {\n        int x = 5;\n        int y = b.x + x;\n    }\n}\nclass B { int x; }\n";
    let edits = inline_edits(source, "x = 5").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int y = b.x + 5;"), "{result}");
}

/// The edits of the Inline Method action for the call or declaration at `needle`, if offered.
fn inline_method_edits(source: &str, needle: &str) -> Option<Vec<TextEdit>> {
    let mut test = LspTest::new();
    test.add_file("A.java", source);
    let start = source.find(needle).expect("needle in source");
    let before = &source[..start];
    let line = before.matches('\n').count() as u32;
    let column = (start - before.rfind('\n').map_or(0, |p| p + 1)) as u32;
    let position = Position::new(line, column);
    let actions = test
        .code_action("A.java", Range::new(position, position), vec![])
        .expect("code action request")
        .unwrap_or_default();
    actions.into_iter().find_map(|action| match action {
        CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Inline method") => {
            action.edit?.changes?.into_values().next()
        }
        _ => None,
    })
}

#[test]
fn single_return_method_is_inlined() {
    let source = "class A {\n    private int add(int a, int b) { return a + b; }\n    void m() {\n        int r = add(1, 2);\n    }\n}\n";
    let edits = inline_method_edits(source, "add(int a").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("1 + 2"), "{result}");
    assert!(!result.contains("add(1, 2)"), "{result}");
}

#[test]
fn inlined_operator_body_keeps_its_grouping_in_a_larger_expression() {
    let source = "class A {\n    private int add(int a, int b) { return a + b; }\n    void m() {\n        int q = 2 * add(1, 2);\n    }\n}\n";
    let edits = inline_method_edits(source, "add(int a").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("2 * (1 + 2)"), "{result}");
}

#[test]
fn statement_body_is_not_spliced_as_an_expression() {
    let source = "class A {\n    void log(String s) { System.out.println(s); }\n    void m() {\n        log(\"x\");\n    }\n}\n";
    assert!(inline_method_edits(source, "log(\"x\")").is_none());
}

#[test]
fn argument_naming_a_later_parameter_is_not_substituted_again() {
    // add(b, 1) must give b + 1, not 1 + 1: the first argument is the name b.
    let source = "class A {\n    private int add(int a, int b) { return a + b; }\n    void m(int b) {\n        int q = add(b, 1);\n    }\n}\n";
    let edits = inline_method_edits(source, "add(int a").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int q = (b + 1);"), "{result}");
}

#[test]
fn a_method_that_is_not_private_is_not_inlined() {
    // Its callers may be in other files, which this action does not search
    let source = "class A {\n    int add(int a, int b) { return a + b; }\n    void m() {\n        int r = add(1, 2);\n    }\n}\n";
    assert!(inline_method_edits(source, "add(int a").is_none());
}

#[test]
fn an_overloaded_method_is_not_inlined() {
    // The one-argument add is a different method, and its calls would be rewritten too
    let source = "class A {\n    private int add(int a, int b) { return a + b; }\n    private int add(int a) { return a; }\n    void m() {\n        int r = add(1);\n    }\n}\n";
    assert!(inline_method_edits(source, "add(int a, int b)").is_none());
}

#[test]
fn usage_shadowed_by_a_nested_class_field_is_not_inlined() {
    // The x read inside the anonymous class is the class's own field, not the local
    let source = "class A {\n    void m() {\n        int x = 5;\n        Runnable r = new Runnable() {\n            int x = 9;\n            public void run() { System.out.println(x); }\n        };\n        System.out.println(x);\n    }\n}\n";
    let edits = inline_edits(source, "x = 5").expect("inline offered");
    let result = apply(source, &edits);
    assert!(
        result.contains("public void run() { System.out.println(x); }"),
        "{result}"
    );
    assert!(result.contains("System.out.println(5);"), "{result}");
}

#[test]
fn usage_shadowed_by_a_method_parameter_of_a_nested_class_is_not_inlined() {
    // A method of an anonymous class may name its parameter x, which shadows the local there
    let source = "class A {\n    void m() {\n        int x = 5;\n        Runnable r = new Runnable() {\n            public void run(int x) { System.out.println(x); }\n        };\n        System.out.println(x);\n    }\n}\n";
    let edits = inline_edits(source, "x = 5").expect("inline offered");
    let result = apply(source, &edits);
    assert!(
        result.contains("public void run(int x) { System.out.println(x); }"),
        "{result}"
    );
    assert!(result.contains("System.out.println(5);"), "{result}");
}

#[test]
fn a_read_before_the_declaration_refers_to_the_field_and_is_not_inlined() {
    // The local x begins at its declaration, so the first read is of the field
    let source = "class A {\n    int x = 1;\n    void m() {\n        System.out.println(x);\n        int x = 5;\n        System.out.println(x);\n    }\n}\n";
    let edits = inline_edits(source, "x = 5").expect("inline offered");
    let result = apply(source, &edits);
    assert!(
        result.contains("        System.out.println(x);\n"),
        "{result}"
    );
    assert!(result.contains("System.out.println(5);"), "{result}");
}

#[test]
fn a_call_on_another_receiver_is_not_inlined() {
    // other.add runs on another object, whose add may be a different method
    let source = "class A {\n    private int add(int a, int b) { return a + b; }\n    void m(A other) {\n        int r = add(1, 2);\n        int s = other.add(3, 4);\n    }\n}\n";
    let edits = inline_method_edits(source, "add(int a").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int r = (1 + 2);"), "{result}");
    assert!(result.contains("int s = other.add(3, 4);"), "{result}");
}

#[test]
fn an_operator_argument_keeps_its_grouping_in_the_body() {
    let source = "class A {\n    private int twice(int x) { return x * 2; }\n    void m(int a, int b) {\n        int r = twice(a + b);\n    }\n}\n";
    let edits = inline_method_edits(source, "twice(int x").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int r = ((a + b) * 2);"), "{result}");
}

#[test]
fn an_operator_argument_to_a_body_that_is_the_parameter_gets_one_pair_of_parentheses() {
    let source = "class A {\n    private int same(int x) { return x; }\n    void m(int a, int b) {\n        int r = 3 * same(a + b);\n    }\n}\n";
    let edits = inline_method_edits(source, "same(int x").expect("inline offered");
    let result = apply(source, &edits);
    assert!(result.contains("int r = 3 * (a + b);"), "{result}");
}
