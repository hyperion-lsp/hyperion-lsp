// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Switch Context Flags
//!
//! The parser uses context flags to track whether we're inside a switch expression.
//! This is critical for correctly parsing lambdas, which can appear in switch cases.
//!
//! Bug: When parsing a switch expression containing lambdas, the parser would
//! incorrectly handle the context flags, leading to:
//! - Lambdas being misparsed inside switch cases
//! - Nested switch expressions not being handled correctly
//! - Switch expressions in complex contexts (ternary, method args) failing
//!
//! The fix ensures:
//! 1. in_switch_expression flag is properly set/unset
//! 2. Nested switches maintain separate context
//! 3. Lambdas in switch cases parse correctly
//! 4. Switch expressions work in all expression contexts

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_simple_switch_expression() {
    // Basic switch expression
    let source = r#"
        class Test {
            String test(int x) {
                return switch (x) {
                    case 1 -> "one";
                    case 2 -> "two";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse simple switch expression, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_expression_with_lambda() {
    // Lambda in switch case - the core bug case
    let source = r#"
        class Test {
            void test(int x) {
                Runnable r = switch (x) {
                    case 1 -> () -> System.out.println("one");
                    case 2 -> () -> System.out.println("two");
                    default -> () -> System.out.println("other");
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with lambda cases, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_lambda_block() {
    // Lambda with block body in switch
    let source = r#"
        class Test {
            void test(int x) {
                Runnable r = switch (x) {
                    case 1 -> () -> {
                        System.out.println("one");
                    };
                    default -> () -> {
                        System.out.println("other");
                    };
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with lambda blocks, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_function_lambda() {
    // Function (single-param) lambda in switch
    let source = r#"
        class Test {
            void test(int x) {
                Function<String, Integer> f = switch (x) {
                    case 1 -> s -> s.length();
                    case 2 -> s -> s.hashCode();
                    default -> s -> 0;
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with function lambdas, got: {:?}",
        errors
    );
}

#[test]
fn test_nested_switch_expressions() {
    // Switch inside switch
    let source = r#"
        class Test {
            String test(int x, int y) {
                return switch (x) {
                    case 1 -> switch (y) {
                        case 1 -> "1,1";
                        default -> "1,other";
                    };
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse nested switch expressions, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_ternary() {
    // Switch expression as part of ternary
    let source = r#"
        class Test {
            String test(boolean flag, int x) {
                return flag ? switch (x) {
                    case 1 -> "one";
                    default -> "other";
                } : "none";
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in ternary, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_method_argument() {
    // Switch expression as method argument
    let source = r#"
        class Test {
            void test(int x) {
                System.out.println(switch (x) {
                    case 1 -> "one";
                    case 2 -> "two";
                    default -> "other";
                });
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in method argument, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_block_statements() {
    // Switch case with block containing multiple statements
    let source = r#"
        class Test {
            int test(int x) {
                return switch (x) {
                    case 1 -> {
                        System.out.println("one");
                        yield 1;
                    }
                    case 2 -> {
                        System.out.println("two");
                        yield 2;
                    }
                    default -> {
                        System.out.println("other");
                        yield 0;
                    }
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with yield blocks, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_statement_vs_expression() {
    // Both statement and expression forms
    let source = r#"
        class Test {
            void test(int x) {
                // Statement form
                switch (x) {
                    case 1:
                        System.out.println("one");
                        break;
                    default:
                        System.out.println("other");
                }

                // Expression form
                String result = switch (x) {
                    case 1 -> "one";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse both switch forms, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_pattern_matching() {
    // Modern pattern matching in switch
    let source = r#"
        class Test {
            String test(Object obj) {
                return switch (obj) {
                    case Integer i -> "int: " + i;
                    case String s -> "string: " + s;
                    case null -> "null";
                    default -> "unknown";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with patterns, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_combined_null_default_label() {
    let source = r#"
        class Test {
            String test(Object value) {
                return switch (value) {
                    case String text -> text;
                    case null, default -> "fallback";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (_green, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Should parse a combined null/default label, got: {errors:?}"
    );
}

#[test]
fn test_switch_rejects_invalid_combined_default_labels() {
    for label in ["1, default", "String text, default", "null, null, default"] {
        let source = format!(
            "class Test {{ int test(Object value) {{ return switch (value) {{ case {label} -> 1; default -> 0; }}; }} }}"
        );
        let tokens = Lexer::new(&source).collect();
        let (_green, errors) = Parser::parse(tokens);
        assert!(
            !errors.is_empty(),
            "invalid combined label parsed without an error: {label}"
        );
    }
}

#[test]
fn test_switch_with_when_guard() {
    // Pattern with when guard
    let source = r#"
        class Test {
            String test(Object obj) {
                return switch (obj) {
                    case String s when s.length() > 5 -> "long";
                    case String s -> "short";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with when guard, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_assignment() {
    // Switch in variable assignment
    let source = r#"
        class Test {
            void test(int x) {
                String result;
                result = switch (x) {
                    case 1 -> "one";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in assignment, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_return() {
    // Switch directly in return statement
    let source = r#"
        class Test {
            String test(int x) {
                return switch (x) {
                    case 1 -> "one";
                    case 2 -> "two";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in return, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_multiple_lambdas() {
    // Multiple different lambda types in switch
    let source = r#"
        class Test {
            void test(int x) {
                Object obj = switch (x) {
                    case 1 -> (Runnable) () -> System.out.println("run");
                    case 2 -> (Supplier<String>) () -> "supply";
                    case 3 -> (Consumer<String>) s -> System.out.println(s);
                    default -> null;
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with multiple lambda types, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_field_initializer() {
    // Switch expression in field initialization
    let source = r#"
        class Test {
            private final String value = switch (getType()) {
                case 1 -> "one";
                default -> "other";
            };

            private int getType() {
                return 0;
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in field initializer, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_in_array_initializer() {
    // Switch in array element
    let source = r#"
        class Test {
            void test(int x) {
                String[] array = new String[] {
                    "prefix",
                    switch (x) {
                        case 1 -> "one";
                        default -> "other";
                    },
                    "suffix"
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch in array initializer, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_null_case() {
    // null as a case label
    let source = r#"
        class Test {
            String test(String s) {
                return switch (s) {
                    case null -> "was null";
                    case "hello" -> "greeting";
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with null case, got: {:?}",
        errors
    );
}

#[test]
fn test_switch_with_multiple_case_labels() {
    // Multiple labels for one case
    let source = r#"
        class Test {
            String test(int x) {
                return switch (x) {
                    case 1, 2, 3 -> "low";
                    case 4, 5, 6 -> "mid";
                    default -> "high";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse switch with multiple labels, got: {:?}",
        errors
    );
}

#[test]
fn test_deeply_nested_switches() {
    // Three levels of nesting
    let source = r#"
        class Test {
            String test(int x, int y, int z) {
                return switch (x) {
                    case 1 -> switch (y) {
                        case 1 -> switch (z) {
                            case 1 -> "1,1,1";
                            default -> "1,1,other";
                        };
                        default -> "1,other";
                    };
                    default -> "other";
                };
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse deeply nested switches, got: {:?}",
        errors
    );
}
