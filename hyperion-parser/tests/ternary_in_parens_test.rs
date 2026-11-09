// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Test ternary operator inside parentheses in string concatenation
/// This was failing before the fix because parse_ternary_expr() was calling
/// parse_expression() for the false branch instead of parse_ternary_expr().
///
/// QUESTION Token Disambiguation Tests:
/// The parser must distinguish between:
/// - Wildcard type arguments in casts: (List<?>) x
/// - Ternary operators with comparison: (a < b ? c : d)
///   This is handled by checking if QUESTION appears inside unmatched type args (<>)
use hyperion_parser::{Lexer, Parser};

#[test]
fn test_ternary_in_parentheses_string_concat() {
    let source = r#"
public class TestTernary {
    public String test(boolean isEmpty, String value) {
        return "value=" + (isEmpty ? "untitled" : value);
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    // Should parse without errors
    assert_eq!(
        errors.len(),
        0,
        "Parser should handle ternary in parentheses. Errors: {:?}",
        errors
    );
}

#[test]
fn test_nested_ternary_in_parentheses() {
    let source = r#"
public class TestNested {
    public String test(boolean a, boolean b, String x, String y, String z) {
        return "result=" + (a ? (b ? x : y) : z);
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle nested ternary in parentheses. Errors: {:?}",
        errors
    );
}

#[test]
fn test_catalog_label_ternary_concatenation() {
    // A conditional expression remains nested inside a concatenation.
    let source = r#"
public class LabelBuilder {
    private String shelf;
    private String value;
    private boolean isEmpty;

    @Override
    public String toString() {
        return "LabelBuilder(" +
                "shelf=" + shelf +
                ", value=" + (isEmpty ? "untitled" : value) +
                ")";
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle catalog label concatenation. Errors: {:?}",
        errors
    );
}

// ============================================================================
// QUESTION Token Disambiguation Tests
// ============================================================================

#[test]
fn test_cast_with_wildcard_type() {
    // Cast with wildcard: (List<?>) should be recognized as CAST_EXPR, not ternary
    let source = r#"
public class TestCast {
    public void test(Object x) {
        List<?> list = (List<?>) x;
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle cast with wildcard (List<?>). Errors: {:?}",
        errors
    );
}

#[test]
fn test_cast_with_wildcard_in_generics() {
    // Cast with nested wildcards: (Map<?, String>) should be cast
    let source = r#"
public class TestCast {
    public void test(Object y) {
        Map<?, String> map = (Map<?, String>) y;
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle cast with wildcard in generics (Map<?, String>). Errors: {:?}",
        errors
    );
}

#[test]
fn test_ternary_with_comparison() {
    // Ternary with comparison: (a < b ? c : d) should be ternary, not cast
    let source = r#"
public class TestTernary {
    public int test(int a, int b, int c, int d) {
        return (a < b ? c : d);
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle ternary with comparison (a < b ? c : d). Errors: {:?}",
        errors
    );
}

#[test]
fn test_ternary_containing_cast_with_wildcard() {
    // Complex: ternary containing a cast with wildcard
    // (a < b ? (List<?>) x : y)
    let source = r#"
public class TestComplex {
    public List<?> test(int a, int b, Object x, List<?> y) {
        return (a < b ? (List<?>) x : y);
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle ternary containing cast with wildcard. Errors: {:?}",
        errors
    );
}

#[test]
fn test_nested_wildcard_types() {
    // Nested generics with multiple wildcards
    // Fixed by persistent type_arg_depth tracking in is_cast()
    let source = r#"
public class TestNested {
    public void test(Object obj) {
        List<Map<?, Integer>> list = (List<Map<?, Integer>>) obj;
    }
}
"#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert_eq!(
        errors.len(),
        0,
        "Parser should handle nested wildcards List<Map<?, Integer>>. Errors: {:?}",
        errors
    );
}
