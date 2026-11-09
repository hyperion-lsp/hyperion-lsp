// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::{
    lexer::Lexer,
    parser::Parser,
    syntax::{SyntaxKind, SyntaxNode},
};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green), errors)
}

fn assert_rejects(label: &str, source: &str) {
    let (_, errors) = parse(source);
    assert!(
        !errors.is_empty(),
        "{label} must reject `var` outside an inferred-type context"
    );
}

#[test]
fn var_is_accepted_only_in_inferred_type_contexts() {
    let source = r#"
        record Pair(Object left, Object right) {}

        class Test {
            void run(Iterable<String> values) throws Exception {
                var local = values.iterator();
                @Deprecated final var annotated = local;
                for (var index = 0; index < 1; index++) {}
                for (var value : values) {}
                try (var resource = open()) {}
                java.util.function.Function<String, String> identity =
                    (@Deprecated final var value) -> value;

                if (local instanceof Pair(var left, var right)) {}
            }

            AutoCloseable open() { return null; }
        }
    "#;

    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "valid inferred types: {errors:#?}");
    assert_eq!(
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::VAR_TYPE)
            .count(),
        8,
        "every accepted bare `var` should have an explicit VAR_TYPE node"
    );
}

#[test]
fn var_is_rejected_as_an_ordinary_type() {
    let invalid = [
        ("field type", "class Test { var value; }"),
        ("return type", "class Test { var value() { return null; } }"),
        ("formal parameter type", "class Test { void use(var value) {} }"),
        (
            "catch parameter type",
            "class Test { void use() { try {} catch (var error) {} } }",
        ),
        ("type argument", "class Test { java.util.List<var> values; }"),
        ("type bound", "class Test<T extends var> {}"),
        (
            "cast type",
            "class Test { Object use(Object value) { return (var) value; } }",
        ),
        (
            "constructed type",
            "class Test { Object use() { return new var(); } }",
        ),
        (
            "array element type",
            "class Test { void use() { var[] values = new Object[0]; } }",
        ),
        (
            "top-level instanceof pattern",
            "class Test { boolean use(Object value) { return value instanceof var match; } }",
        ),
        (
            "top-level switch pattern",
            "class Test { int use(Object value) { return switch (value) { case var match -> 1; }; } }",
        ),
    ];

    for (label, source) in invalid {
        assert_rejects(label, source);
    }
}

#[test]
fn lambda_parameter_lists_do_not_mix_parameter_grammars() {
    let invalid = [
        (
            "inferred and declared types",
            "class Test { Object f = (var left, String right) -> left; }",
        ),
        (
            "concise then var",
            "class Test { Object f = (left, var right) -> left; }",
        ),
        (
            "var then concise",
            "class Test { Object f = (var left, right) -> left; }",
        ),
        (
            "declared then concise",
            "class Test { Object f = (String left, right) -> left; }",
        ),
    ];

    for (label, source) in invalid {
        let (_, errors) = parse(source);
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("Lambda parameters cannot mix")),
            "{label} must report the mixed lambda parameter grammar: {errors:#?}"
        );
    }
}

#[test]
fn var_remains_legal_as_a_package_qualifier() {
    let source = r#"
        class Test {
            var.example.Widget field;
            java.util.List<var.example.Widget> fields;
            java.util.function.Function<var.example.Widget, var.example.Widget> identity =
                (var.example.Widget value) -> value;

            var.example.Widget method(var.example.Widget parameter) {
                var.example.Widget local = (var.example.Widget) parameter;
                return new var.example.Widget();
            }

            boolean matches(Object value) {
                return value instanceof var.example.Widget match;
            }
        }
    "#;

    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "qualified package segment: {errors:#?}");
    assert_eq!(
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::VAR_TYPE)
            .count(),
        0,
        "qualified `var` is an identifier, not an inferred type"
    );
}
