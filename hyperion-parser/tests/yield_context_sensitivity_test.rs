// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for context-sensitive parsing of the `yield` keyword
//!
//! Per JLS §3.9 and JEP 354 (Switch Expressions), `yield` is:
//! - A KEYWORD inside switch expressions (for yield statements)
//! - An IDENTIFIER everywhere else (parameters, variables, fields, method names)
//!
//! Current limitation: Parser always treats `yield` as a statement keyword,
//! causing failures when used as an identifier outside switch expressions.

use hyperion_parser::{Lexer, Parser};

// ============================================================================
// Expected Behavior: `yield` as identifier OUTSIDE switch expressions
// ============================================================================

#[test]
fn test_yield_as_parameter_name() {
    let source = r#"
        class Test {
            void method(String yield) {
                System.out.println(yield);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as parameter name outside switch expression. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_local_variable() {
    let source = r#"
        class Test {
            void method() {
                String yield = "value";
                System.out.println(yield);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as local variable outside switch expression. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_field_name() {
    let source = r#"
        class Test {
            private String yield;

            public void setYield(String yield) {
                this.yield = yield;
            }

            public String getYield() {
                return yield;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as field name. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_lambda_parameter() {
    let source = r#"
        class Test {
            void method() {
                Function<String, String> fn = yield -> yield.toUpperCase();
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as lambda parameter outside switch expression. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_catch_parameter() {
    let source = r#"
        class Test {
            void method() {
                try {
                    doSomething();
                } catch (Exception yield) {
                    yield.printStackTrace();
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as catch parameter. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_foreach_variable() {
    let source = r#"
        class Test {
            void method(List<String> items) {
                for (String yield : items) {
                    System.out.println(yield);
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as for-each variable. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_method_declaration() {
    let source = r#"
        class Test {
            public void yield() {
                return;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as method declaration name. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_as_method_name() {
    let source = r#"
        class Test {
            void method() {
                builder.yield().doSomething();
            }

            public Builder yield() {
                return this;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as method name (after DOT). Errors: {:?}",
        errors
    );
}

// ============================================================================
// Expected Behavior: `yield` as KEYWORD inside switch expressions
// ============================================================================

#[test]
fn test_yield_statement_in_switch_expression() {
    let source = r#"
        class Test {
            String method(int value) {
                return switch (value) {
                    case 1 -> {
                        System.out.println("one");
                        yield "ONE";
                    }
                    case 2 -> {
                        yield "TWO";
                    }
                    default -> "UNKNOWN";
                };
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as keyword in switch expression. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_in_nested_switch_expression() {
    let source = r#"
        class Test {
            String method(int x, int y) {
                return switch (x) {
                    case 1 -> switch (y) {
                        case 1 -> { yield "1,1"; }
                        default -> { yield "1,other"; }
                    };
                    case 2 -> { yield "TWO"; }
                    default -> "UNKNOWN";
                };
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as keyword in nested switch expressions. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Critical Test: Ambiguity Resolution
// ============================================================================

#[test]
fn test_yield_identifier_and_keyword_in_same_scope() {
    // This is the critical test: `yield` must be context-sensitive within the same method
    let source = r#"
        class Test {
            String method(String yield) {
                // `yield` is parameter name here (identifier)
                String result = switch (yield.length()) {
                    case 0 -> { yield "empty"; }  // `yield` is keyword here
                    case 1 -> { yield "short"; }  // `yield` is keyword here
                    default -> { yield "long"; }  // `yield` is keyword here
                };
                return result + yield;  // `yield` is parameter name here (identifier)
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should handle 'yield' as both identifier and keyword in same scope. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_variable_before_switch_expression() {
    let source = r#"
        class Test {
            String method(int value) {
                String yield = "default";  // identifier
                String result = switch (value) {
                    case 1 -> { yield "ONE"; }  // keyword
                    case 2 -> { yield "TWO"; }  // keyword
                    default -> yield;  // identifier (reference to variable)
                };
                return result;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should handle 'yield' as variable before switch and as keyword inside switch. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_yield_in_traditional_switch_statement() {
    // Traditional switch statements don't have yield statements
    // So `yield` should be treated as identifier even in switch body
    let source = r#"
        class Test {
            void method(int value) {
                switch (value) {
                    case 1:
                        String yield = "one";  // identifier in traditional switch
                        System.out.println(yield);
                        break;
                    case 2:
                        int yield = 2;  // identifier
                        System.out.println(yield);
                        break;
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse 'yield' as identifier in traditional switch statement. Errors: {:?}",
        errors
    );
}

#[test]
fn test_yield_method_call_in_switch_expression() {
    let source = r#"
        class Test {
            String method(Builder builder, int value) {
                return switch (value) {
                    case 1 -> { yield builder.yield().build(); }  // both uses!
                    default -> "UNKNOWN";
                };
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should handle 'yield' keyword followed by method call to 'yield()'. Errors: {:?}",
        errors
    );
}
