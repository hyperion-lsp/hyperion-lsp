// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Signature help edge case tests

use crate::lsp_test_framework::LspTest;

#[test]
fn test_signature_help_overloaded_methods() {
    let mut test = LspTest::new();

    test.add_file(
        "Overload.java",
        r#"
        class Overload {
            void process(String value, int count) {}
            void process(int value, boolean flag) {}

            void test() {
                process(1,<|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("Overload.java", "<|>")
        .expect("signature request should succeed")
        .expect("signature help should be available");

    assert_eq!(sig_help.signatures.len(), 2);
    assert_eq!(sig_help.active_parameter, Some(1));
    let active = sig_help.active_signature.expect("active overload") as usize;
    assert!(
        sig_help.signatures[active]
            .label
            .contains("process(int value, boolean flag)"),
        "argument types should select the int overload: {sig_help:?}"
    );
}

#[test]
fn test_signature_help_varargs() {
    let mut test = LspTest::new();

    test.add_file(
        "Varargs.java",
        r#"
        class Varargs {
            void printf(String format, Object... args) {}

            void test() {
                printf("Hello %s %d", "world",<|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("Varargs.java", "<|>")
        .expect("signature request should succeed")
        .expect("varargs signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(2));
    assert_eq!(sig_help.signatures.len(), 1);
    assert!(sig_help.signatures[0].label.contains("Object... args"));
}

#[test]
fn test_signature_help_generic_method() {
    let mut test = LspTest::new();

    test.add_file(
        "Generic.java",
        r"
        import java.util.List;
        import java.util.Collections;

        class Generic {
            void test() {
                List<String> list = Collections.singletonList(<|>
            }
        }
        ",
    );

    let sig_help = test
        .signature_help("Generic.java", "<|>")
        .expect("signature request should succeed")
        .expect("generic static method signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(0));
    assert!(sig_help
        .signatures
        .iter()
        .any(|signature| signature.label.contains("singletonList")
            && signature.label.contains("<T>")));
}

#[test]
fn test_signature_help_on_static_imported_member_type() {
    let mut test = LspTest::new();

    test.add_file(
        "p/Outer.java",
        r#"
        package p;

        public class Outer {
            public static class Nested {
                public static void pick(String value) {}
            }
        }
        "#,
    );
    test.add_file(
        "consumer/Main.java",
        r#"
        package consumer;

        import static p.Outer.Nested;

        class Main {
            void run() {
                Nested.pick(<|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("consumer/Main.java", "<|>")
        .expect("signature request should succeed")
        .expect("static imported member type should resolve as the receiver");

    assert!(
        sig_help
            .signatures
            .iter()
            .any(|signature| signature.label.contains("pick(")
                && signature.label.contains("String value")),
        "unexpected signatures: {sig_help:?}"
    );
}

#[test]
fn test_signature_help_constructor() {
    let mut test = LspTest::new();

    test.add_file(
        "Constructor.java",
        r"
        import java.util.ArrayList;

        class Constructor {
            void test() {
                var list = new ArrayList<String>(<|>
            }
        }
        ",
    );

    let sig_help = test
        .signature_help("Constructor.java", "<|>")
        .expect("signature request should succeed")
        .expect("constructor signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(0));
    assert!(!sig_help.signatures.is_empty());
    assert!(sig_help
        .signatures
        .iter()
        .all(|signature| signature.label.contains("ArrayList")));
}

#[test]
fn test_signature_help_nested_calls() {
    let mut test = LspTest::new();

    test.add_file(
        "Nested.java",
        r#"
        class Nested {
            String format(String fmt, Object... args) { return ""; }
            int parseInt(String value) { return 0; }

            void test() {
                String result = format("Value: %d", parseInt(<|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("Nested.java", "<|>")
        .expect("signature request should succeed")
        .expect("nested call signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(0));
    assert!(sig_help.signatures[0].label.contains("parseInt"));
}

#[test]
fn test_signature_help_chained_methods() {
    let mut test = LspTest::new();

    test.add_file(
        "Chained.java",
        r#"
        class Builder {
            Builder setValue(String value) { return this; }
            Builder setCount(int count) { return this; }
            void build() {}

            void test() {
                new Builder()
                    .setValue("test")
                    .setCount(<|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("Chained.java", "<|>")
        .expect("signature request should succeed")
        .expect("chained call signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(0));
    assert!(sig_help.signatures[0].label.contains("setCount"));
}

#[test]
fn test_signature_help_lambda_parameter() {
    let mut test = LspTest::new();

    test.add_file(
        "Lambda.java",
        r#"
        import java.util.function.BiFunction;

        class Lambda {
            void apply(String a, String b, BiFunction<String, String, String> fn) {}

            void test() {
                apply("a", "b", (x, y) -><|>
            }
        }
        "#,
    );

    let sig_help = test
        .signature_help("Lambda.java", "<|>")
        .expect("signature request should succeed")
        .expect("lambda argument signature help should be available");

    assert_eq!(sig_help.active_parameter, Some(2));
    assert!(sig_help.signatures[0].label.contains("apply"));
    assert!(sig_help.signatures[0].label.contains("BiFunction"));
}
