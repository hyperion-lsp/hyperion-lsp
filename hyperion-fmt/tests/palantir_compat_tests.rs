// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Palantir-java-format compatibility
//!
//! These tests verify specific formatting behaviors that match palantir-java-format,
//! particularly around binary expression continuation and assignment breaking.
//!
//! ## Test Organization
//!
//! This file contains both passing tests (baseline behavior) and ignored tests (known mismatches).
//!
//! ### Passing Tests (Baseline Behavior)
//! - `test_binary_expr_continuation_short` - Short expressions stay on one line
//! - `test_binary_expr_continuation_medium` - Medium expressions stay on one line
//! - `test_binary_expr_continuation_long` - Long expressions break with +8 continuation
//! - `test_nested_binary_expr_continuation` - Nested binary expressions use +8 continuation
//! - `test_builder_pattern_always_breaks` - Builder patterns break at each method
//!
//! ### Known Mismatches (Ignored Tests)
//!
//! #### Category 1: Extremely Long First Operands (100+ char literals)
//! When the first operand in a binary expression is 100+ characters:
//! - **Palantir**: Breaks before first operand with double indent (+12)
//!   ```java
//!   String x =
//!           "100+ char string..."
//!                   + "second";
//!   ```
//! - **Hyperion**: Keeps assignment on one line, breaks at operator with single indent (+8)
//!   ```java
//!   String x = "100+ char string..."
//!           + "second";
//!   ```
//!
//! #### Category 2: Method Call Argument Wrapping
//! When method call arguments contain long binary expressions or exceed line width:
//! - **Palantir**: Keeps method receiver on same line, wraps only arguments
//!   ```java
//!   System.out.println("long string..."
//!           + "continuation");
//!   ```
//! - **Hyperion**: Breaks method call itself onto separate line
//!   ```java
//!   System.out.println(
//!           "long string..."
//!                   + "continuation");
//!   ```
//!
//! #### Category 3: Method Chain Receiver Breaking
//! When method chains exceed line width (Palantir uses chain_wrap_column=80):
//! - **Palantir**: First method stays with receiver, subsequent methods break
//!   ```java
//!   list.stream()
//!           .filter(x -> x > 0)
//!           .map(x -> x * 2);
//!   ```
//! - **Hyperion**: Breaks receiver onto separate line from first method
//!   ```java
//!   list
//!           .stream()
//!           .filter(x -> x > 0)
//!           .map(x -> x * 2);
//!   ```

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

#[test]
fn test_binary_expr_continuation_short() {
    // Short binary expressions stay on one line with assignment
    assert_format(
        r#"
class T {
    void test() {
        String s = "short" + "concat";
    }
}
"#,
        r#"
class T {
    void test() {
        String s = "short" + "concat";
    }
}
"#,
    );
}

#[test]
fn test_binary_expr_continuation_medium() {
    // Medium binary expressions stay on one line with assignment
    assert_format(
        r#"
class T {
    void test() {
        String m = "This is a medium length string that fits" + " on one line";
    }
}
"#,
        r#"
class T {
    void test() {
        String m = "This is a medium length string that fits" + " on one line";
    }
}
"#,
    );
}

#[test]
fn test_binary_expr_continuation_long() {
    // Long binary expressions: assignment stays on one line, binary breaks with +8 continuation
    assert_format(
        r#"
class T {
    void test() {
        String l = "This is a very long string that will definitely need to be wrapped because it exceeds the maximum" + " line width limit second part";
    }
}
"#,
        r#"
class T {
    void test() {
        String l = "This is a very long string that will definitely need to be wrapped because it exceeds the maximum"
                + " line width limit second part";
    }
}
"#,
    );
}

#[test]
fn test_nested_binary_expr_continuation() {
    // Verified against palantir-java-format 2.83.0 (120-col limit):
    // concat chain fills to the limit, then continues at +8
    assert_format(
        r#"
class T {
    void test() {
        String x = "aaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbb" + "cccccccccccccccccccccccccccc" + "dddddddddddddddddddddddddddd";
    }
}
"#,
        r#"
class T {
    void test() {
        String x = "aaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbb" + "cccccccccccccccccccccccccccc"
                + "dddddddddddddddddddddddddddd";
    }
}
"#,
    );
}

#[test]
fn test_binary_in_method_call_continuation() {
    // Binary expressions in method calls use +8 continuation
    // Current behavior: breaks the method call onto separate line
    assert_format(
        r#"
class T {
    void test() {
        System.out.println("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
        r#"
class T {
    void test() {
        System.out.println("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
    );
}

#[test]
fn test_builder_pattern_always_breaks() {
    // Builder patterns should always break with +8 continuation
    assert_format(
        r#"
class T {
    void test() {
        var result = SomeType.builder().name("test").value(42).build();
    }
}
"#,
        r#"
class T {
    void test() {
        var result = SomeType.builder()
                .name("test")
                .value(42)
                .build();
    }
}
"#,
    );
}

#[test]
fn test_method_chain_breaks_at_80() {
    // Method chains should break at 80 chars (chain_wrap_column), not 120
    // Current behavior: breaks receiver onto separate line
    assert_format(
        r"
class T {
    void test() {
        list.stream().filter(x -> x > 0).map(x -> x * 2).collect(Collectors.toList());
    }
}
",
        r"
class T {
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
fn test_extremely_long_first_operand() {
    // When assignment header + first operand exceeds line width,
    // Palantir breaks before the first operand
    // Current behavior: keeps assignment on one line, breaks binary operator
    assert_format(
        r#"
class T {
    void test() {
        String m = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "b";
    }
}
"#,
        r#"
class T {
    void test() {
        String m =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        + "b";
    }
}
"#,
    );
}

// ============================================================================
// MISMATCH CATEGORY 1: Extremely Long First Operands (100+ char literals)
// ============================================================================

#[test]
fn test_long_first_operand_simple_binary() {
    // Palantir: When first operand is 100+ chars, break before it with double indent (+12 for continuation)
    // Hyperion: Keeps assignment on one line, breaks at operator with single continuation (+8)
    assert_format(
        r#"
class T {
    void test() {
        String x = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbb";
    }
}
"#,
        r#"
class T {
    void test() {
        String x =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        + "bbbbbb";
    }
}
"#,
    );
}

#[test]
fn test_long_first_operand_multiple_operands() {
    // Palantir: Extremely long first operand forces initial break, subsequent operands use double indent
    // Hyperion: Breaks at operators with single continuation indent
    assert_format(
        r#"
class T {
    void test() {
        String result = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "b" + "c" + "d";
    }
}
"#,
        r#"
class T {
    void test() {
        String result =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        + "b"
                        + "c"
                        + "d";
    }
}
"#,
    );
}

#[test]
fn test_long_first_operand_at_boundary() {
    // Palantir: Exactly 100-char first operand triggers the break-before behavior
    // Test the boundary condition
    assert_format(
        r#"
class T {
    void test() {
        String boundary = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "short";
    }
}
"#,
        r#"
class T {
    void test() {
        String boundary =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        + "short";
    }
}
"#,
    );
}

#[test]
fn test_long_first_operand_in_return() {
    // Palantir: Same behavior applies to return statements
    // Hyperion: May handle differently in non-assignment contexts
    assert_format(
        r#"
class T {
    String test() {
        return "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "suffix";
    }
}
"#,
        r#"
class T {
    String test() {
        return "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                + "suffix";
    }
}
"#,
    );
}

// ============================================================================
// MISMATCH CATEGORY 2: Method Call Argument Wrapping Behavior
// ============================================================================

#[test]
fn test_method_call_with_long_binary_arg() {
    // Palantir: Method call stays on one line, only the argument binary expr breaks
    // Hyperion: Breaks the method call itself onto a separate line
    assert_format(
        r#"
class T {
    void test() {
        System.out.println("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
        r#"
class T {
    void test() {
        System.out.println("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
    );
}

#[test]
fn test_method_call_multiple_long_args() {
    // Palantir: When multiple args exceed line width, each arg breaks but receiver stays
    // Hyperion: May break receiver or use different indent strategy
    assert_format(
        r#"
class T {
    void test() {
        obj.someMethod("first_arg_that_is_quite_long_aaaaaaaaaaaaaa", "second_arg_also_long_bbbbbbbbbbbbbb", "third_ccccccccccccccccc");
    }
}
"#,
        r#"
class T {
    void test() {
        obj.someMethod(
                "first_arg_that_is_quite_long_aaaaaaaaaaaaaa",
                "second_arg_also_long_bbbbbbbbbbbbbb",
                "third_ccccccccccccccccc");
    }
}
"#,
    );
}

#[test]
fn test_nested_method_calls_with_long_args() {
    // Verified against palantir-java-format 2.83.0: 117 cols fits in 120, stays flat
    assert_format(
        r#"
class T {
    void test() {
        outer.call(inner.method("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }
}
"#,
        r#"
class T {
    void test() {
        outer.call(inner.method("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }
}
"#,
    );
}

#[test]
fn test_constructor_call_with_long_args() {
    // Oracle (palantir 2.83.0) breaks after `=` so the RHS fits on one line:
    //     var obj =
    //             new SomeClass("..." + "...");
    assert_format(
        r#"
class T {
    void test() {
        var obj = new SomeClass("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
        r#"
class T {
    void test() {
        var obj =
                new SomeClass("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    }
}
"#,
    );
}

#[test]
fn test_varargs_method_call_wrapping() {
    // Oracle (palantir 2.83.0): the trailing `;` counts toward the call's level
    // (121 > 120), so the arg list breaks after `(` with all args on one line:
    //     String.format(
    //             "template", ...);
    assert_format(
        r#"
class T {
    void test() {
        String.format("template", "arg1_is_long_aaaaaaaaaaaaa", "arg2_is_long_bbbbbbbbbbbb", "arg3_is_long_cccccccccccc");
    }
}
"#,
        r#"
class T {
    void test() {
        String.format(
                "template", "arg1_is_long_aaaaaaaaaaaaa", "arg2_is_long_bbbbbbbbbbbb", "arg3_is_long_cccccccccccc");
    }
}
"#,
    );
}

// ============================================================================
// MISMATCH CATEGORY 3: Method Chain Receiver Breaking Differences
// ============================================================================

#[test]
fn test_method_chain_receiver_position() {
    // Palantir: First method in chain stays with receiver, subsequent methods break
    // Hyperion: Breaks receiver onto separate line from first method
    assert_format(
        r"
class T {
    void test() {
        list.stream().filter(x -> x > 0).map(x -> x * 2).collect(Collectors.toList());
    }
}
",
        r"
class T {
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
fn test_method_chain_breaks_at_80_not_120() {
    // Palantir: Method chains break at 80 chars even when line_width is 120
    // Hyperion: May use line_width for all breaking decisions
    assert_format(
        r"
class T {
    void test() {
        veryLongVariableName.firstMethod().secondMethod().thirdMethod().fourthMethod();
    }
}
",
        r"
class T {
    void test() {
        veryLongVariableName.firstMethod()
                .secondMethod()
                .thirdMethod()
                .fourthMethod();
    }
}
",
    );
}

#[test]
fn test_builder_chain_receiver_short() {
    // Palantir: Short builder chains keep first method with receiver
    // Hyperion: Always breaks builder patterns
    assert_format(
        r#"
class T {
    void test() {
        var x = Obj.builder().name("test").build();
    }
}
"#,
        r#"
class T {
    void test() {
        var x = Obj.builder()
                .name("test")
                .build();
    }
}
"#,
    );
}

#[test]
fn test_nested_method_chains() {
    // Palantir: Nested chains each maintain their own receiver positioning rules
    // Hyperion: May break differently when chains are nested
    assert_format(
        r"
class T {
    void test() {
        outer.getInner().doSomething().getThing().process().getResult().chain().more();
    }
}
",
        r"
class T {
    void test() {
        outer.getInner()
                .doSomething()
                .getThing()
                .process()
                .getResult()
                .chain()
                .more();
    }
}
",
    );
}

#[test]
fn test_method_chain_with_qualified_receiver() {
    // Palantir: Qualified receivers (package.Class.method()) keep qualification with first method
    // Hyperion: May break qualified receiver differently
    assert_format(
        r"
class T {
    void test() {
        com.example.Utils.getInstance().configure().build().execute();
    }
}
",
        r"
class T {
    void test() {
        com.example.Utils.getInstance()
                .configure()
                .build()
                .execute();
    }
}
",
    );
}

#[test]
fn test_method_chain_with_lambda_args() {
    // Palantir: Lambda args in method chains don't affect receiver positioning
    // Hyperion: May break differently when lambdas are present
    assert_format(
        r"
class T {
    void test() {
        list.stream().filter(x -> x.isValid()).map(x -> x.transform()).collect(Collectors.toList());
    }
}
",
        r"
class T {
    void test() {
        list.stream()
                .filter(x -> x.isValid())
                .map(x -> x.transform())
                .collect(Collectors.toList());
    }
}
",
    );
}

#[test]
fn test_single_method_no_chain() {
    // Verified against palantir-java-format 2.83.0: a lone call whose line fits
    // (98 chars here) stays on one line — no break before the dot.
    assert_format(
        r"
class T {
    void test() {
        veryLongReceiverNameThatMightCauseIssues.singleMethodCallThatIsAlsoQuiteReasonablyLong();
    }
}
",
        r"
class T {
    void test() {
        veryLongReceiverNameThatMightCauseIssues.singleMethodCallThatIsAlsoQuiteReasonablyLong();
    }
}
",
    );
}

// ============================================================================
// COMPREHENSIVE TESTS: Multiple Mismatches Combined
// ============================================================================

#[test]
fn test_combined_mismatches_realistic_scenario() {
    // Real-world scenario combining:
    // 1. Long first operands in binary expressions
    // 2. Method call arg wrapping
    // 3. Method chain receiver positioning
    //
    // This test shows how these issues interact in realistic code
    assert_format(
        r#"
class DataProcessor {
    void process() {
        // Category 1: Long first operand
        String errorMsg = "An extremely long error message that exceeds 100 characters and should trigger special formatting!!!" + " with continuation";

        // Category 2: Method call with long binary arg
        logger.error("Database connection failed with extremely long message aaaaaaaaaaaaaaa" + " and additional context bbbbbbbbbb");

        // Category 3: Method chain
        results.stream().filter(x -> x.isValid()).map(x -> x.transform()).collect(Collectors.toList());

        // Combined: Method chain with long arg
        api.client().request().setBody("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbb").execute();
    }
}
"#,
        r#"
class DataProcessor {
    void process() {
        // Category 1: Long first operand
        String errorMsg =
                "An extremely long error message that exceeds 100 characters and should trigger special formatting!!!"
                        + " with continuation";

        // Category 2: Method call with long binary arg
        logger.error("Database connection failed with extremely long message aaaaaaaaaaaaaaa"
                + " and additional context bbbbbbbbbb");

        // Category 3: Method chain
        results.stream()
                .filter(x -> x.isValid())
                .map(x -> x.transform())
                .collect(Collectors.toList());

        // Combined: Method chain with long arg
        api.client()
                .request()
                .setBody("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "bbbbbbbbbbbbbbbbbbbb")
                .execute();
    }
}
"#,
    );
}

#[test]
fn test_boundary_conditions_100_char() {
    // Test exact 100-char threshold where Palantir changes behavior
    // Strings with exactly 100 chars should trigger break-before formatting
    assert_format(
        r#"
class T {
    void test() {
        // Exactly 100 chars: "a" repeated 100 times
        String exactly100 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "b";
        // 99 chars should use normal formatting
        String lessThan100 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" + "b";
    }
}
"#,
        r#"
class T {
    void test() {
        // Exactly 100 chars: "a" repeated 100 times
        String exactly100 =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        + "b";
        // 99 chars should use normal formatting
        String lessThan100 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                + "b";
    }
}
"#,
    );
}
