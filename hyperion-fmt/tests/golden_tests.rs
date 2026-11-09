// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Golden tests for hyperion-fmt
//!
//! These tests verify that the formatter produces expected output for various
//! Java constructs.

use hyperion_fmt::{format_source, FormatConfig};

/// Helper to format and compare output
fn assert_format(input: &str, expected: &str) {
    let config = FormatConfig::default();
    let result = format_source(input, &config).expect("format should succeed");
    assert_eq!(
        result.trim(),
        expected.trim(),
        "\n\nInput:\n{input}\n\nExpected:\n{expected}\n\nGot:\n{result}"
    );
}

/// Helper that just checks formatting doesn't panic
fn assert_formats(input: &str) {
    let config = FormatConfig::default();
    let result = format_source(input, &config);
    let err = result.as_ref().err();
    assert!(result.is_ok(), "Failed to format: {err:?}");
}

#[test]
fn test_empty_class() {
    assert_format("class Foo {}", "class Foo {}");
}

#[test]
fn test_class_with_field() {
    assert_formats(
        r"
class Foo {
    private int x;
}
",
    );
}

#[test]
fn test_class_with_method() {
    assert_formats(
        r"
class Foo {
    public void bar() {
        return;
    }
}
",
    );
}

#[test]
fn test_bare_return_comment_spacing() {
    assert_format(
        "class X { void f() { return /* c */; } }",
        "class X {\n    void f() {\n        return /* c */;\n    }\n}",
    );
}

#[test]
fn test_if_statement() {
    assert_formats(
        r"
class Foo {
    void test() {
        if (x > 0) {
            return;
        }
    }
}
",
    );
}

#[test]
fn test_if_else_statement() {
    assert_formats(
        r"
class Foo {
    void test() {
        if (x > 0) {
            return;
        } else {
            throw new Exception();
        }
    }
}
",
    );
}

#[test]
fn test_for_loop() {
    assert_formats(
        r"
class Foo {
    void test() {
        for (int i = 0; i < 10; i++) {
            System.out.println(i);
        }
    }
}
",
    );
}

#[test]
fn test_while_loop() {
    assert_formats(
        r"
class Foo {
    void test() {
        while (running) {
            process();
        }
    }
}
",
    );
}

#[test]
fn test_try_catch() {
    assert_formats(
        r"
class Foo {
    void test() {
        try {
            riskyOperation();
        } catch (Exception e) {
            log(e);
        }
    }
}
",
    );
}

#[test]
fn test_switch_statement() {
    assert_formats(
        r"
class Foo {
    void test(int x) {
        switch (x) {
            case 1:
                doOne();
                break;
            case 2:
                doTwo();
                break;
            default:
                doDefault();
        }
    }
}
",
    );
}

#[test]
fn test_package_and_imports() {
    assert_formats(
        r"
package com.example;

import java.util.List;
import java.util.Map;

class Foo {
}
",
    );
}

#[test]
fn test_import_sorting() {
    // With sort_imports enabled: static imports first, then regular imports,
    // both groups sorted alphabetically
    let config = FormatConfig {
        sort_imports: true,
        ..FormatConfig::default()
    };
    let result = format_source(
        r"
package com.example;

import java.util.Map;
import static java.util.Collections.emptyList;
import java.util.List;
import static java.lang.Math.max;
import java.io.File;

class Foo {
}
",
        &config,
    )
    .expect("format should succeed");
    assert_eq!(
        result.trim(),
        r"
package com.example;

import static java.lang.Math.max;
import static java.util.Collections.emptyList;

import java.io.File;
import java.util.List;
import java.util.Map;

class Foo {}
"
        .trim()
    );
}

#[test]
fn test_import_order_preserved_by_default() {
    // Default (palantir-compatible) behavior: formatSource never reorders
    // imports, and the author's blank-line grouping is preserved.
    assert_format(
        r"
package com.example;

import java.util.Map;
import static java.util.Collections.emptyList;

import java.util.List;

class Foo {
}
",
        r"
package com.example;

import java.util.Map;
import static java.util.Collections.emptyList;

import java.util.List;

class Foo {}
"
        .trim(),
    );
}

#[test]
fn test_interface() {
    assert_formats(
        r"
interface Foo {
    void bar();
    int baz(String s);
}
",
    );
}

#[test]
fn test_enum() {
    assert_formats(
        r"
enum Color {
    RED,
    GREEN,
    BLUE
}
",
    );
}

#[test]
fn test_record() {
    assert_formats(
        r"
record Point(int x, int y) {
}
",
    );
}

#[test]
fn test_annotation() {
    assert_formats(
        r"
@interface MyAnnotation {
    String value();
}
",
    );
}

#[test]
fn test_javadoc_preserved() {
    assert_formats(
        r"
/**
 * This is a Javadoc comment.
 */
class Foo {
}
",
    );
}

#[test]
fn test_line_comment_preserved() {
    assert_formats(
        r"
class Foo {
    // This is a line comment
    int x;
}
",
    );
}

#[test]
fn test_method_with_parameters() {
    assert_formats(
        r"
class Foo {
    public String process(int a, String b, List<Integer> c) {
        return b;
    }
}
",
    );
}

#[test]
fn test_constructor() {
    assert_formats(
        r"
class Foo {
    private int x;

    public Foo(int x) {
        this.x = x;
    }
}
",
    );
}

#[test]
fn test_generics() {
    assert_formats(
        r"
class Container<T extends Comparable<T>> {
    private T value;

    public T getValue() {
        return value;
    }
}
",
    );
}

#[test]
fn test_lambda_expression() {
    assert_formats(
        r"
class Foo {
    void test() {
        list.forEach(x -> System.out.println(x));
    }
}
",
    );
}

#[test]
fn test_method_chain() {
    assert_formats(
        r"
class Foo {
    void test() {
        list.stream()
            .filter(x -> x > 0)
            .map(x -> x * 2)
            .collect(Collectors.toList());
    }
}
",
    );
}

#[test]
fn test_builder_pattern() {
    // Builder patterns should use continuation indent (8 spaces)
    assert_format(
        r"
class Foo {
    void test() {
        var result = SomeType.builder().name(name).value(42).enabled(true).build();
    }
}
",
        r"
class Foo {
    void test() {
        var result = SomeType.builder()
                .name(name)
                .value(42)
                .enabled(true)
                .build();
    }
}
",
    );
}

#[test]
fn test_builder_pattern_short_fits_one_line() {
    // Short builders should stay on one line
    assert_formats(
        r"
class Foo {
    void test() {
        var x = Foo.builder().build();
    }
}
",
    );
}

// =========================================================================
// Phase 1: Expression Visitor Tests
// =========================================================================

#[test]
fn test_unary_expr_prefix() {
    assert_formats(
        r"
class Foo {
    void test() {
        int x = -5;
        boolean b = !flag;
        int y = ++i;
        int z = --j;
        int bits = ~mask;
    }
}
",
    );
}

#[test]
fn test_unary_expr_postfix() {
    assert_formats(
        r"
class Foo {
    void test() {
        int x = i++;
        int y = j--;
    }
}
",
    );
}

#[test]
fn test_assignment_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        x = 5;
        y += 10;
        z -= 3;
        a *= 2;
        b /= 4;
        c %= 7;
        d &= 0xFF;
        e |= 0x01;
        f ^= 0x10;
    }
}
",
    );
}

#[test]
fn test_cast_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        String s = (String) obj;
        int i = (int) longValue;
        List<String> list = (List<String>) rawList;
    }
}
",
    );
}

#[test]
fn test_instanceof_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        if (obj instanceof String) {
            return;
        }
    }
}
",
    );
}

#[test]
fn test_pattern_instanceof_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        if (obj instanceof String s) {
            System.out.println(s);
        }
    }
}
",
    );
}

#[test]
fn test_array_access_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        int x = arr[0];
        int y = matrix[row][col];
    }
}
",
    );
}

#[test]
fn test_method_reference() {
    assert_formats(
        r"
class Foo {
    void test() {
        Function<String, Integer> f = String::length;
        Supplier<List<String>> s = ArrayList::new;
    }
}
",
    );
}

#[test]
fn test_this_super_expr() {
    assert_formats(
        r"
class Foo extends Bar {
    void test() {
        this.x = 5;
        super.doSomething();
    }
}
",
    );
}

#[test]
fn test_class_literal_expr() {
    assert_formats(
        r"
class Foo {
    void test() {
        Class<?> c1 = String.class;
        Class<?> c2 = int.class;
    }
}
",
    );
}

// =========================================================================
// Phase 2: Statement Visitor Tests
// =========================================================================

#[test]
fn test_synchronized_stmt() {
    assert_formats(
        r"
class Foo {
    void test() {
        synchronized (lock) {
            count++;
        }
    }
}
",
    );
}

#[test]
fn test_break_with_label() {
    assert_formats(
        r"
class Foo {
    void test() {
        outer:
        for (int i = 0; i < 10; i++) {
            for (int j = 0; j < 10; j++) {
                if (condition) {
                    break outer;
                }
            }
        }
    }
}
",
    );
}

#[test]
fn test_continue_with_label() {
    assert_formats(
        r"
class Foo {
    void test() {
        outer:
        for (int i = 0; i < 10; i++) {
            for (int j = 0; j < 10; j++) {
                if (condition) {
                    continue outer;
                }
            }
        }
    }
}
",
    );
}

#[test]
fn test_assert_stmt() {
    assert_formats(
        r#"
class Foo {
    void test() {
        assert x > 0;
        assert y != null : "y cannot be null";
    }
}
"#,
    );
}

#[test]
fn test_labeled_stmt() {
    assert_formats(
        r"
class Foo {
    void test() {
        loop:
        while (true) {
            break loop;
        }
    }
}
",
    );
}

#[test]
fn test_yield_stmt() {
    assert_formats(
        r#"
class Foo {
    String test(int x) {
        return switch (x) {
            case 1 -> "one";
            case 2 -> {
                yield "two";
            }
            default -> "other";
        };
    }
}
"#,
    );
}

#[test]
fn test_empty_stmt() {
    assert_formats(
        r"
class Foo {
    void test() {
        ;
    }
}
",
    );
}

#[test]
fn test_try_with_resources() {
    assert_formats(
        r#"
class Foo {
    void test() throws IOException {
        try (InputStream is = new FileInputStream("file")) {
            is.read();
        }
    }
}
"#,
    );
}

#[test]
fn test_switch_expr() {
    assert_formats(
        r#"
class Foo {
    String test(int x) {
        return switch (x) {
            case 1 -> "one";
            case 2 -> "two";
            default -> "other";
        };
    }
}
"#,
    );
}

// =========================================================================
// Phase 3: Type Visitor Tests
// =========================================================================

#[test]
fn test_wildcard_type_unbounded() {
    assert_formats(
        r"
class Foo {
    void test(List<?> list) {
    }
}
",
    );
}

#[test]
fn test_wildcard_type_extends() {
    assert_formats(
        r"
class Foo {
    void test(List<? extends Number> list) {
    }
}
",
    );
}

#[test]
fn test_wildcard_type_super() {
    assert_formats(
        r"
class Foo {
    void test(List<? super Integer> list) {
    }
}
",
    );
}

#[test]
fn test_intersection_type() {
    assert_formats(
        r"
class Foo {
    <T extends Comparable<T> & Serializable> void test(T value) {
    }
}
",
    );
}

#[test]
fn test_union_type_multicatch() {
    assert_formats(
        r"
class Foo {
    void test() {
        try {
            risky();
        } catch (IOException | SQLException e) {
            log(e);
        }
    }
}
",
    );
}

#[test]
fn test_array_type() {
    assert_formats(
        r"
class Foo {
    int[] arr;
    String[][] matrix;

    void test(int[] param) {
    }
}
",
    );
}

#[test]
fn test_complex_generic_type() {
    assert_formats(
        r"
class Foo {
    Map<String, List<Integer>> map;
    Function<? super String, ? extends Number> func;
}
",
    );
}

// =============================================================================
// Exact-match tests for expression spacing
// These verify that operators and keywords don't get split across lines
// =============================================================================

#[test]
fn test_binary_expr_exact() {
    assert_format(
        "class Foo { int x = 1 + 2; }",
        "class Foo {\n    int x = 1 + 2;\n}",
    );
}

#[test]
fn test_binary_expr_comparison_exact() {
    assert_format(
        "class Foo { boolean b = x == y; }",
        "class Foo {\n    boolean b = x == y;\n}",
    );
}

#[test]
fn test_binary_expr_logical_exact() {
    assert_format(
        "class Foo { boolean b = a && b || c; }",
        "class Foo {\n    boolean b = a && b || c;\n}",
    );
}

#[test]
fn test_ternary_expr_exact() {
    assert_format(
        "class Foo { int x = cond ? 1 : 2; }",
        "class Foo {\n    int x = cond ? 1 : 2;\n}",
    );
}

#[test]
fn test_lambda_expr_exact() {
    assert_format(
        "class Foo { Runnable r = () -> run(); }",
        "class Foo {\n    Runnable r = () -> run();\n}",
    );
}

#[test]
fn test_lambda_expr_with_param_exact() {
    assert_format(
        "class Foo { Consumer<String> c = s -> print(s); }",
        "class Foo {\n    Consumer<String> c = s -> print(s);\n}",
    );
}

#[test]
fn test_lambda_expr_with_block_exact() {
    assert_format(
        "class Foo { Runnable r = () -> { run(); }; }",
        "class Foo {\n    Runnable r = () -> {\n        run();\n    };\n}",
    );
}

#[test]
fn test_cast_expr_exact() {
    assert_format(
        "class Foo { void test() { String s = (String) obj; } }",
        "class Foo {\n    void test() {\n        String s = (String) obj;\n    }\n}",
    );
}

#[test]
fn test_instanceof_expr_exact() {
    assert_format(
        "class Foo { boolean b = obj instanceof String; }",
        "class Foo {\n    boolean b = obj instanceof String;\n}",
    );
}

#[test]
fn test_assignment_expr_exact() {
    assert_format(
        "class Foo { void test() { x = 1; } }",
        "class Foo {\n    void test() {\n        x = 1;\n    }\n}",
    );
}

#[test]
fn test_compound_assignment_exact() {
    assert_format(
        "class Foo { void test() { x += 1; } }",
        "class Foo {\n    void test() {\n        x += 1;\n    }\n}",
    );
}

#[test]
fn test_wildcard_extends_exact() {
    assert_format(
        "class Foo { List<? extends Number> list; }",
        "class Foo {\n    List<? extends Number> list;\n}",
    );
}

#[test]
fn test_wildcard_super_exact() {
    assert_format(
        "class Foo { List<? super Integer> list; }",
        "class Foo {\n    List<? super Integer> list;\n}",
    );
}

#[test]
fn test_intersection_type_exact() {
    assert_format(
        "class Foo { <T extends Comparable<T> & Serializable> void test() { } }",
        "class Foo {\n    <T extends Comparable<T> & Serializable> void test() {}\n}",
    );
}

#[test]
fn test_union_type_exact() {
    assert_format(
        "class Foo { void test() { try { } catch (IOException | SQLException e) { } } }",
        "class Foo {\n    void test() {\n        try {} catch (IOException | SQLException e) {}\n    }\n}",
    );
}

#[test]
fn test_varargs_exact() {
    assert_format(
        "class Foo { void test(String... args) { } }",
        "class Foo {\n    void test(String... args) {}\n}",
    );
}

#[test]
fn test_break_with_label_exact() {
    assert_format(
        "class Foo { void test() { outer: while (true) { break outer; } } }",
        "class Foo {\n    void test() {\n        outer: while (true) {\n            break outer;\n        }\n    }\n}",
    );
}

#[test]
fn test_continue_with_label_exact() {
    assert_format(
        "class Foo { void test() { outer: while (true) { continue outer; } } }",
        "class Foo {\n    void test() {\n        outer: while (true) {\n            continue outer;\n        }\n    }\n}",
    );
}

#[test]
fn test_assert_stmt_exact() {
    assert_format(
        "class Foo { void test() { assert x > 0; } }",
        "class Foo {\n    void test() {\n        assert x > 0;\n    }\n}",
    );
}

#[test]
fn test_assert_with_message_exact() {
    assert_format(
        "class Foo { void test() { assert x > 0 : \"x must be positive\"; } }",
        "class Foo {\n    void test() {\n        assert x > 0 : \"x must be positive\";\n    }\n}",
    );
}

#[test]
fn test_while_stmt_exact() {
    assert_format(
        "class Foo { void test() { while (true) { } } }",
        "class Foo {\n    void test() {\n        while (true) {}\n    }\n}",
    );
}

#[test]
fn test_for_stmt_exact() {
    assert_format(
        "class Foo { void test() { for (int i = 0; i < 10; i++) { } } }",
        "class Foo {\n    void test() {\n        for (int i = 0; i < 10; i++) {}\n    }\n}",
    );
}

#[test]
fn test_switch_stmt_exact() {
    assert_format(
        "class Foo { void test() { switch (x) { case 1: break; } } }",
        "class Foo {\n    void test() {\n        switch (x) {\n            case 1:\n                break;\n        }\n    }\n}",
    );
}

#[test]
fn test_synchronized_stmt_exact() {
    assert_format(
        "class Foo { void test() { synchronized (lock) { } } }",
        "class Foo {\n    void test() {\n        synchronized (lock) {}\n    }\n}",
    );
}

#[test]
fn test_enum_decl_exact() {
    assert_format(
        "enum Color { RED, GREEN, BLUE }",
        "enum Color {\n    RED,\n    GREEN,\n    BLUE\n}",
    );
}

#[test]
fn test_var_keyword_spacing() {
    assert_format(
        "class Foo { void test() { var x = 1; } }",
        "class Foo {\n    void test() {\n        var x = 1;\n    }\n}",
    );
}

#[test]
fn test_license_header_placement() {
    assert_format(
        "/*\n * License\n */\npackage foo;\n\nimport java.util.List;\n\npublic class Foo {\n}",
        "/*\n * License\n */\npackage foo;\n\nimport java.util.List;\n\npublic class Foo {}",
    );
}

#[test]
fn test_throws_clause_spacing() {
    assert_format(
        "class Foo { void test() throws Exception { } }",
        "class Foo {\n    void test() throws Exception {}\n}",
    );
}

#[test]
fn test_line_comments_preserved() {
    assert_format(
        "class Foo {\n// comment 1\n// comment 2\nvoid test() { }\n}",
        "class Foo {\n    // comment 1\n    // comment 2\n    void test() {}\n}",
    );
}

#[test]
fn test_var_in_for_loop() {
    assert_format(
        "class Foo { void test() { for (var i = 0; i < 10; i++) { var x = 1; } } }",
        "class Foo {\n    void test() {\n        for (var i = 0; i < 10; i++) {\n            var x = 1;\n        }\n    }\n}",
    );
}

#[test]
fn test_var_record_keyword_as_variable_name() {
    // `record` is a contextual keyword that can be used as variable name
    assert_format(
        "class Foo { void test() { var record = 1; } }",
        "class Foo {\n    void test() {\n        var record = 1;\n    }\n}",
    );
}

#[test]
fn test_enhanced_for_with_var() {
    assert_format(
        "class Foo { void test(List<Integer> list) { for (var x : list) { } } }",
        "class Foo {\n    void test(List<Integer> list) {\n        for (var x : list) {}\n    }\n}",
    );
}

#[test]
fn test_enhanced_for_with_record_keyword() {
    // `record` is a contextual keyword that can be used as variable name
    assert_format(
        "class Foo { void test(List<Integer> list) { for (var record : list) { } } }",
        "class Foo {\n    void test(List<Integer> list) {\n        for (var record : list) {}\n    }\n}",
    );
}

#[test]
fn test_try_with_resources_followed_by_comment() {
    let input = r"
class Test {
    void test() {
        try (var x = foo()) {
            bar();
        }
        // Comment after try
        baz();
    }
}
";
    let expected = r"
class Test {
    void test() {
        try (var x = foo()) {
            bar();
        }
        // Comment after try
        baz();
    }
}
";
    assert_format(input, expected);
}

#[test]
fn test_void_method_space_preserved() {
    // Regression test: ensure space between "void" and method name
    let input = r#"
class Test {
    public void testFoo() {
        String x = "test";
    }
}
"#;
    let config = hyperion_fmt::FormatConfig::default();
    let result = hyperion_fmt::format_source(input, &config).expect("format should succeed");
    // Check that "void testFoo" has space preserved, not "voidtestFoo"
    assert!(
        result.contains("void testFoo") || result.contains("void\n"),
        "Expected space between 'void' and method name, got:\n{result}"
    );
    assert!(
        !result.contains("voidtestFoo"),
        "Found 'voidtestFoo' without space:\n{result}"
    );
}

#[test]
fn test_array_initializer_in_method_call() {
    // Simpler test: array initializer as method argument
    let input = r"
class Test {
    void test() {
        foo(new byte[] {2});
        bar();
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Check that bar() is still on its own line with space
    assert!(
        !formatted.contains("});bar()") && !formatted.contains("})bar()"),
        "Array initializer corrupted formatting:\n{formatted}"
    );
    // Check proper spacing
    assert!(
        !formatted.contains("voidtest"),
        "void and test should have space:\n{formatted}"
    );
}

#[test]
fn test_array_init_in_lambda() {
    // Array initializer inside a lambda
    let input = r"
class Test {
    void test() {
        IntStream.range(0, 2).forEach(i -> {
            foo(new byte[] {2});
        });
        bar();
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Check that bar() is still on its own line with space
    assert!(
        !formatted.contains(");bar()"),
        "Statement after lambda corrupted:\n{formatted}"
    );
}

#[test]
fn test_lambda_with_multiarg_call_with_array() {
    // Multi-argument call with array inside lambda
    let input = r#"
class Test {
    void test() {
        IntStream.range(0, 2).forEach(i -> {
            members.add(new Member("a", "b", "c", new byte[] {2}));
        });
        bar();
    }
}
"#;
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    assert!(
        !formatted.contains(");bar()"),
        "Statement after lambda corrupted:\n{formatted}"
    );
}

#[test]
fn test_multiarg_call_with_array_no_lambda() {
    // Multi-argument call with array (no lambda to isolate)
    let input = r#"
class Test {
    void test() {
        members.add(new Member("a", "b", "c", new byte[] {2}));
        bar();
    }
}
"#;
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Debug: print the output with visible whitespace
    eprintln!("=== Formatted output ===");
    for (i, line) in formatted.lines().enumerate() {
        eprintln!("{:3}: {:?}", i, line);
    }
    eprintln!("=== End ===");

    // Check that bar() is on its own line after the previous statement
    assert!(
        !formatted.contains(";bar()") && !formatted.contains(");bar()"),
        "bar() should be on its own line:\n{formatted}"
    );
}

#[test]
fn test_simple_array_in_call() {
    // Just array initializer in a call - simplest case
    let input = r"
class Test {
    void test() {
        foo(new byte[]{2});
        bar();
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    eprintln!("=== simple array (no spaces) ===");
    for (i, line) in formatted.lines().enumerate() {
        eprintln!("{:3}: {:?}", i, line);
    }
    eprintln!("=== End ===");

    // Just check it formats without crashing for now
    assert!(formatted.contains("foo(") && formatted.contains("{2}"));
}

#[test]
fn test_new_expr_with_array_arg() {
    // new Foo() with array argument
    let input = r"
class Test {
    void test() {
        foo(new Member(new byte[] {2}));
        bar();
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    eprintln!("=== new with array ===");
    for (i, line) in formatted.lines().enumerate() {
        eprintln!("{:3}: {:?}", i, line);
    }
    eprintln!("=== End ===");

    // bar() should be on its own line
    assert!(
        !formatted.contains(");bar()"),
        "bar() should be on its own line:\n{formatted}"
    );
}

#[test]
fn test_contextual_keyword_field_names() {
    // Regression test: contextual keywords should have space before them in field names
    let input = r"
class Test {
    private String record;
    private int var;
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Check field declarations
    assert!(
        formatted.contains("String record"),
        "Expected space between 'String' and 'record' in field:\n{formatted}"
    );
    assert!(
        formatted.contains("int var"),
        "Expected space between 'int' and 'var' in field:\n{formatted}"
    );
}

#[test]
fn test_for_loop_spacing_with_single_statement() {
    // Regression test: space after closing paren in regular for loop should be preserved
    let input = r"
class Test {
    void test() {
        for (int i = 0; i < 10; i++) System.out.println(i);
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Check that there's a space between ) and the statement
    assert!(
        formatted.contains(") System.out.println"),
        "Expected space between ')' and 'System.out.println':\n{formatted}"
    );
}

#[test]
fn test_while_loop_spacing_with_single_statement() {
    // Regression test: space after closing paren in while loop should be preserved
    let input = r"
class Test {
    void test() {
        while (x < 10) x++;
    }
}
";
    let config = hyperion_fmt::FormatConfig::default();
    let formatted = hyperion_fmt::format_source(input, &config).expect("format should succeed");

    // Check that there's a space between ) and the statement
    assert!(
        formatted.contains(") x++"),
        "Expected space between ')' and 'x++':\n{formatted}"
    );
}

#[test]
fn line_comment_before_a_class_without_a_package_keeps_the_class() {
    // A line comment has no newline; the class after it must still be code.
    let input = "// first\n// second\nclass Foo {\n}\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(output.contains("// first\n"), "{output}");
    assert!(output.contains("// second\n"), "{output}");
    assert!(output.contains("class Foo"), "{output}");
    assert!(
        output
            .lines()
            .all(|line| !line.starts_with("// ") || !line.contains("class Foo")),
        "the class must not be inside a comment: {output}"
    );
}

#[test]
fn line_comment_before_a_package_stays_on_the_line_above_it() {
    let input = "// header\npackage foo;\n\nclass Foo {\n}\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(output.starts_with("// header\npackage foo;"), "{output}");
    assert!(output.contains("class Foo"), "{output}");
}

#[test]
fn javadoc_before_a_package_stays_above_it() {
    // A package's Javadoc is written before the package, as a line comment is
    let input = "/**\n * Package docs.\n */\npackage foo;\n\nimport java.util.List;\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(
        output.starts_with("/**\n * Package docs.\n */\npackage foo;"),
        "{output}"
    );
    assert!(output.contains("import java.util.List;"), "{output}");
}

#[test]
fn javadoc_before_a_class_without_a_package_stays_on_the_class() {
    let input = "/**\n * A class.\n */\npublic class Foo {\n}\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(output.starts_with("/**\n * A class.\n */"), "{output}");
    assert!(output.contains("public class Foo"), "{output}");
    assert!(!output.contains("package"), "{output}");
}

#[test]
fn use_tabs_indents_with_tabs() {
    let input = "class Foo {\n    void test() {\n        int x = 1;\n    }\n}\n";
    let config = FormatConfig::default()
        .with_use_tabs(true)
        .with_tab_width(4);
    let output = format_source(input, &config).expect("formats");
    assert!(
        output.contains("\n\tvoid test() {\n\t\tint x = 1;\n\t}\n"),
        "{output:?}"
    );
    assert!(
        !output.contains("\n    "),
        "no space indentation remains: {output:?}"
    );
}

#[test]
fn range_format_of_a_nested_method_indents_its_body_from_its_column() {
    // The method is selected at its nested indentation; its body must follow that column.
    let source = "class Foo {\n    void test() { int x = 1; }\n}\n";
    let start = source.find("void").expect("method");
    let end = source.find("}\n}").expect("method end") + 1;
    let output =
        hyperion_fmt::format_range(source, start..end, &FormatConfig::default()).expect("formats");
    assert!(
        output.contains("\n    void test() {\n        int x = 1;\n    }\n"),
        "{output}"
    );
}

#[test]
fn comment_between_finally_and_its_block_is_kept() {
    let input = "class Foo {\n    void test() {\n        try {\n            run();\n        } finally /* cleanup */ {\n            close();\n        }\n    }\n}\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(output.contains("finally /* cleanup */ {"), "{output}");
}

#[test]
fn annotation_on_a_method_reference_is_kept() {
    let input = "class Foo {\n    void test() {\n        Supplier<String> s = @Nullable String::new;\n    }\n}\n";
    let output = format_source(input, &FormatConfig::default()).expect("formats");
    assert!(output.contains("@Nullable String::new"), "{output}");
}
