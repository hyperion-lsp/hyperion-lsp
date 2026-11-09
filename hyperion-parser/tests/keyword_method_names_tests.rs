// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Tests for Keywords as method names
///
/// Java allows contextual keywords to be used as method names (e.g.,
/// `obj.with()` and `obj.to()`). Reserved keywords such as `if` remain illegal.
///
/// This is valid per JLS §3.9 (Keywords).
/// Fluent builder APIs commonly use this pattern.
use hyperion_parser::{Lexer, Parser};

#[test]
fn test_keyword_method_to() {
    let source = r#"
        class Test {
            void method() {
                stream.to("topic");
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse 'to' as method name after dot, got: {:?}",
        errors
    );
}

#[test]
fn test_keyword_method_from() {
    let source = r#"
        class Test {
            void method() {
                Stream.from(list);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse 'from' as method name after dot, got: {:?}",
        errors
    );
}

#[test]
fn test_chained_keyword_methods() {
    let source = r#"
        class Test {
            void method() {
                stream.with(serde1, serde2)
                       .to("topic");
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse chained keyword methods, got: {:?}",
        errors
    );
}

#[test]
fn test_keyword_method_in_call_expression() {
    let source = r#"
        class Test {
            void method() {
                consumer.poll(Duration.ofMillis(100))
                        .with(deserializer)
                        .to(records);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse keyword methods in expression chains, got: {:?}",
        errors
    );
}

#[test]
fn test_keyword_method_with_arguments() {
    let source = r#"
        class Test {
            void method() {
                obj.with(arg1, arg2, arg3);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse keyword method with multiple arguments, got: {:?}",
        errors
    );
}

// NOTE: The following pattern is not yet supported - it requires static generic method call support
// #[test]
// fn test_keyword_method_with_static_generic_args() {
//     // This pattern: Config.<String, Integer>with(...)
//     // Requires support for static generic method calls which is separate from
//     // keywords as method names.
//     let source = r#"
//         class Test {
//             void method() {
//                 Config.<String, Integer>with(stringSerde, intSerde);
//             }
//         }
//     "#;
//     let tokens = Lexer::new(source).collect();
//     let (green, errors) = Parser::parse(tokens);
//     let _ = &green;
//
//     assert!(errors.is_empty(), "Should parse keyword method with generic type arguments, got: {:?}", errors);
// }

#[test]
fn test_keyword_method_chained_complex() {
    let source = r#"
        class Test {
            void method() {
                builder
                    .with(config1)
                    .to(output)
                    .from(input);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse complex chained keyword methods, got: {:?}",
        errors
    );
}

#[test]
fn test_keyword_not_method_in_declaration() {
    // Keywords should still be rejected in method declarations
    let source = r#"
        class Test {
            void if() {  // This should be invalid
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    // This SHOULD have errors - keywords are not allowed in method declarations
    // (unlike method calls where they are allowed after DOT)
    assert!(
        !errors.is_empty(),
        "Should NOT parse 'if' as method name in declaration"
    );
}

#[test]
fn test_keyword_method_in_assignment() {
    let source = r#"
        class Test {
            void method() {
                Configuration config = builder.with(properties)
                                              .to(file);
            }
        }
    "#;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse keyword methods in assignment, got: {:?}",
        errors
    );
}
