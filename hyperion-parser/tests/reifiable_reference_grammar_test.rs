// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! javac-21-oracle regressions for class literals and method references.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
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
fn reifiable_array_class_literals_accept_unbounded_wildcards() {
    let root = assert_parses(
        r#"
        class C {
            Class<?> a = java.util.List<?>[].class;
            Class<?> b = java.util.Map<?, ?>[][].class;
            Class<?> c = record.pkg.Type[][].class;
            Class<?> d = module.class;
            Class<?> e = int[][][].class;
            Class<?> f = void.class;
        }
        "#,
    );
    assert_eq!(
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::CLASS_LITERAL_EXPR)
            .count(),
        6
    );
}

#[test]
fn class_literals_reject_non_reifiable_or_non_type_receivers() {
    for source in [
        "class C { Class<?> x = java.util.List<String>.class; }",
        "class C { Class<?> x = java.util.List<?>.class; }",
        "class C { Class<?> x = java.util.List<String>[].class; }",
        "class C { Class<?> x = void[].class; }",
        "class C { Class<?> x = record.class; }",
        "class C { Class<?> x = pkg.var.class; }",
        "class C { Class<?> x = this.class; }",
        "class C { Class<?> x = (new C()).class; }",
        "class C { Class<?> x = int.foo.class; }",
        "class C { Class<?> x = int<?>[].class; }",
        "class C { Class<?> x = void.foo.class; }",
    ] {
        assert_rejected(source);
    }
}

#[test]
fn array_constructor_references_follow_reifiable_type_rules() {
    assert_parses(
        r#"
        class C {
            void f() {
                Object a = java.util.List<?>[]::new;
                Object b = java.util.Map<?, ?>[][]::new;
                Object c = record.pkg.Type[]::new;
                Object d = int[][]::new;
            }
        }
        "#,
    );

    for source in [
        "class C { Object x = java.util.List<String>[]::new; }",
        "class C { Object x = void[]::new; }",
        "class C { Object x = record[]::new; }",
        "class C { Object x = int<?>[]::new; }",
    ] {
        assert_rejected(source);
    }
}

#[test]
fn parameterized_and_explicit_type_argument_method_references_parse() {
    let root = assert_parses(
        r#"
        class Base { <X> int generic() { return 1; } }
        class C<T> extends Base {
            <X> C() {}
            int when() { return 1; }
            void f(C<String> value) {
                Object a = java.util.List<String>::size;
                Object b = C<String>::new;
                Object c = C::<Integer>new;
                Object d = String::<Object>valueOf;
                Object e = value::when;
                Object f = value::record;
                Object g = C<java.util.List<?>>::new;
                Object h = C::<java.util.List<?>>new;
                Object i = C::<java.util.List<?>>when;
                Object j = this::when;
                Object k = super::generic;
            }
            class Inner {
                Object l = C.this::when;
                Object m = C.super::generic;
                Object n = C.super::<String>generic;
                Object o = String::<int[]>valueOf;
                Object p = C<int[]>::when;
                Object q = C::<int[]>new;
                Object r = String.class::getName;
                Object s = value.<String>when()::toString;
            }
        }
        "#,
    );
    assert_eq!(
        root.descendants()
            .filter(|node| matches!(
                node.kind(),
                SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR
            ))
            .count(),
        19
    );
}

#[test]
fn method_references_reject_illegal_receiver_and_type_argument_forms() {
    for source in [
        "class C { Object x = int::new; }",
        "class C { Object x = void::new; }",
        "class C { Object x = this::new; }",
        "class C { Object x = new C()::new; }",
        "class C { Object x = String::if; }",
        "class C { Object x = String::<>new; }",
        "class C { Object x = String::<?>valueOf; }",
        "class C { Object x = String::<int>valueOf; }",
        "class C<T> { Object x = C<int>::new; }",
        "class C<T> { Object x = C<int>::toString; }",
        "class C<T> { Object x = C<void[]>::new; }",
        "class C<T> { Object x = C::<void[]>new; }",
        "class C { Object x = String[]::<Object>new; }",
        "class C { Object x = void.foo::bar; }",
        "class C { Object x = int.super::bar; }",
        "class C { Object x = record.super::bar; }",
        "class C { Object x = void.this::bar; }",
        "class C<T> { class I { Object x = C<String>.this::toString; } }",
        "class C<T> { class I { Object x = C<String>.super::toString; } }",
        "class C { Object x = C[].this::toString; }",
        "class C { Object x = String.<Object>valueOf(null).class; }",
        "class C { Object x = int.<String>m(); }",
        "class C { Object x = void.<String>m(); }",
        "class C { Object value; Object x = value.<String>field; }",
    ] {
        assert_rejected(source);
    }
}

#[test]
fn annotation_constant_expressions_cover_casts_and_recursive_unary_ops() {
    assert_parses(
        r#"
        @interface ByteValue { byte value(); }
        @interface BoolValue { boolean value(); }
        @interface TypeValue { Class<?> value(); }
        @ByteValue((byte) (-(~+1))) class Casted {}
        @BoolValue(!!true ? true : false) class RecursiveUnary {}
        @BoolValue((boolean) true) class BooleanCast {}
        @interface StringValue { String value(); }
        @StringValue((String) ("a" + "b")) class SimpleStringCast {}
        @StringValue((java.lang.String) "value") class QualifiedStringCast {}
        @TypeValue(java.util.List<?>[].class) class ReifiableArray {}
        "#,
    );

    assert_rejected(
        "@interface A { Class<?> value(); } @A(java.util.List<String>[].class) class C {}",
    );
    assert_rejected("@interface A { Class<?> value(); } @A(java.util.List<?>.class) class C {}");
    for source in [
        "@interface A { String value(); } @A(null) class C {}",
        "@interface A { String value(); } @A((Object) \"x\") class C {}",
        "@interface A { int value(); } @A((Integer) 1) class C {}",
        "@interface A { int value(); } @A((int[]) null) class C {}",
        "@interface A { String value(); } @A((String[]) null) class C {}",
        "@interface A { int value(); } @A(true ? 1) class C {}",
    ] {
        assert_rejected(source);
    }
}

#[test]
fn annotated_reference_types_match_javac_boundaries() {
    assert_parses(
        r#"
        @interface A {}
        class C {
            Object a = @A String[]::new;
            Object b = String @A []::new;
            Object c = java.util.List<@A ?>[]::new;
            Object d = @A String::valueOf;
        }
        "#,
    );

    for source in [
        "@interface A {} class C { Class<?> x = String @A [].class; }",
        "@interface A {} class C { Class<?> x = java.util.List<@A ?>[].class; }",
        "@interface A {} class C { Object x = java.util.List<?> @A []::new; }",
        "@interface A {} class O<T> { class I<U> {} } class C { Object x = O<String>.@A I<Integer>::new; }",
        "@interface A {} class C { Object f() { return @A value; } }",
        "@interface A {} class C { Object x = @A String; }",
        "@interface A {} class C { Object x = @A String.foo; }",
    ] {
        assert_rejected(source);
    }
}
