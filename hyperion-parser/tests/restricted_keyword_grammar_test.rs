// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Grammar-boundary regressions for Java contextual and restricted keywords.
//!
//! The positive and negative cases in this file were checked against javac 21
//! (with preview enabled for guarded switch labels) before being admitted.

use hyperion_parser::syntax::{AstNode, SourceFile, SyntaxKind, SyntaxNode};
use hyperion_parser::{Lexer, Parser};

fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::ParseError>) {
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    (SyntaxNode::new_root(green), errors)
}

fn assert_parses(source: &str) -> SyntaxNode {
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "expected a clean parse: {errors:#?}");
    root
}

fn assert_rejected(source: &str) {
    let (_, errors) = parse(source);
    assert!(!errors.is_empty(), "expected syntax errors for: {source}");
}

#[test]
fn parses_a_complete_module_declaration_without_erasing_contextual_tokens() {
    let root = assert_parses(
        r#"
        @module open module module.open {
            requires transitive;
            requires static transitive when.module;
            exports exports.to to to, module;
            opens opens.to to transitive;
            uses when;
            provides module.Service with record.impl.Provider, when.Provider;
        }
        "#,
    );

    let source = SourceFile::new(root.clone());
    let module = source.module().expect("module declaration");
    assert!(module.is_open());
    assert_eq!(
        module.name().expect("module name").as_string(),
        "module.open"
    );

    let syntax = module.syntax();
    assert_eq!(
        syntax
            .children()
            .filter(|node| node.kind() == SyntaxKind::REQUIRES_DIRECTIVE)
            .count(),
        2
    );
    for kind in [
        SyntaxKind::EXPORTS_DIRECTIVE,
        SyntaxKind::OPENS_DIRECTIVE,
        SyntaxKind::USES_DIRECTIVE,
        SyntaxKind::PROVIDES_DIRECTIVE,
    ] {
        assert!(
            syntax.children().any(|node| node.kind() == kind),
            "{kind:?}"
        );
    }

    assert!(root
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .any(|token| token.kind() == SyntaxKind::MODULE_KW));
}

#[test]
fn rejects_invalid_modular_compilation_units_and_directives() {
    assert_rejected("package p; module m {}");
    assert_rejected("module m {} class Extra {}");
    assert_rejected("module m { uses record; }");
    assert_rejected("module m { nonsense value; }");
}

#[test]
fn annotated_package_declarations_are_reachable() {
    assert_parses("@module package module.open; class C {}");
}

#[test]
fn contextual_words_work_as_type_and_value_identifiers() {
    let root = assert_parses(
        r#"
        class module {
            module field;
            void when(module module) {}

            void test(Object value, Iterable<module> values) {
                module local = null;
                when guarded = null;
                record.pkg.Widget qualified = null;
                for (module item : values) {}
                try (module resource = open()) {}
                Object cast = (@module module) value;
                java.util.function.Function<module, module> fn = (module item) -> item;
                if (value instanceof when matched) {}
                record module(module component) {}
                record();
                this.var();
            }

            void record() {}
            void var() {}
        }
        "#,
    );

    let class = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
        .expect("class declaration");
    assert_eq!(
        hyperion_parser::syntax::ClassDecl::new(class)
            .name()
            .expect("contextual class name")
            .text(),
        "module"
    );
}

#[test]
fn final_type_segment_still_uses_type_identifier() {
    assert_parses("class C { var.pkg.Type first; record.pkg.Type second; }");
    for invalid in [
        "class C { pkg.var field; }",
        "class C { pkg.record field; }",
        "class C { pkg.yield field; }",
        "class C { pkg.sealed field; }",
        "class C { pkg.permits field; }",
    ] {
        assert_rejected(invalid);
    }
    assert_rejected("class C { Class<?> value = pkg.var.class; }");
    assert_rejected("class C { Class<?> value = record.class; }");
}

#[test]
fn void_is_only_a_method_result_or_class_literal() {
    assert_parses("class C { void method() { Class<?> type = void.class; } }");

    for invalid in [
        "class C { void field; }",
        "class C { void method(void parameter) {} }",
        "class C { java.util.List<void> values; }",
        "class C { void method() { void local; } }",
        "class C { void method() { Object value = new void(); } }",
        "record R(void component) {}",
    ] {
        assert_rejected(invalid);
    }
}

#[test]
fn annotation_values_accept_character_and_class_literals() {
    assert_parses(
        r#"
        @interface TypeValue { Class<?> value(); }
        @interface CharValue { char value(); }
        @TypeValue(int.class) class Primitive {}
        @TypeValue(void.class) class VoidType {}
        @TypeValue(String[].class) class ReferenceArray {}
        @TypeValue(int[][].class) class PrimitiveArray {}
        @CharValue('x') class Character {}
        "#,
    );
    assert_rejected("@interface A { Class<?> value(); } @A(void[].class) class C {}");
    assert_rejected("class C { Class<?> value = void[].class; }");
}

#[test]
fn a_record_header_is_mandatory() {
    assert_parses("record Empty() {}");
    assert_parses(
        "class C { record Nested<T>(T value) {} void f() { record Local<T>(T value) {} } }",
    );
    assert_rejected("record Empty {}");
}

#[test]
fn yield_is_contextual_at_the_statement_boundary() {
    let root = assert_parses(
        r#"
        class C {
            int f(int selector) {
                int yield = 0;
                return switch (selector) {
                    default -> {
                        yield = 2;
                        yield++;
                        this.yield();
                        yield yield;
                    }
                };
            }
            void yield() {}
        }
        "#,
    );
    assert_eq!(
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::YIELD_STMT)
            .count(),
        1
    );

    assert_rejected("class C { void f(int x) { switch (x) { default: yield 1; } } }");
    assert_rejected("class C { void yield() {} void f() { yield(); } }");
    assert_parses("class C { void yield() {} void f() { this.yield(); } }");

    // An ordinary nested switch statement does not block a yield targeting
    // the enclosing switch expression.
    assert_parses(
        "class C { int f(int x, int y) { return switch (x) { default -> { switch (y) { default: yield 1; } } }; } }",
    );
}

#[test]
fn yield_cannot_cross_lambda_or_class_boundaries() {
    assert_rejected(
        "class C { int f(int x) { return switch (x) { default -> { Runnable r = () -> { yield 1; }; yield 2; } }; } }",
    );
    assert_rejected(
        "class C { int f(int x) { return switch (x) { default -> { class Local { int m() { yield 1; } } yield 2; } }; } }",
    );
    assert_rejected(
        "class C { int f(int x) { return switch (x) { default -> { Object o = new Object() { { yield 1; } }; yield 2; } }; } }",
    );

    assert_parses(
        "class C { int f(int x) { return switch (x) { default -> { Runnable r = () -> { int yield = 1; yield++; }; yield 2; } }; } }",
    );
}

#[test]
fn when_guards_require_a_pattern_label() {
    assert_parses(
        "class C { int f(Object o) { return switch (o) { case String s when !s.isEmpty() -> 1; default -> 0; }; } }",
    );
    assert_rejected(
        "class C { int f(Integer i) { return switch (i) { case 1 when true -> 1; default -> 0; }; } }",
    );
    assert_rejected(
        "class C { int f(Object o) { return switch (o) { case null when true -> 1; default -> 0; }; } }",
    );
}

#[test]
fn reserved_keywords_do_not_become_member_names_after_a_dot() {
    assert_rejected("class C { void f(C value) { value.if(); } }");
    assert_parses("class C { void f(C value) { value.var(); value.record(); } }");
}
