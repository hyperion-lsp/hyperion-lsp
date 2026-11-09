// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Text Block Regex
//!
//! Java text blocks (""" ... """) were introduced in Java 15 as a preview feature
//! and made permanent in Java 15. They allow multi-line string literals.
//!
//! The parser's regex for text blocks had several issues:
//! 1. Escape sequences not handled correctly
//! 2. Trailing whitespace handling incorrect
//! 3. Empty lines not preserved properly
//! 4. Dollar signs ($) incorrectly interpreted as interpolation markers
//!
//! Key rules for text blocks (JLS §3.10.6):
//! - Opening delimiter: """ followed by whitespace/newline
//! - Closing delimiter: """ on its own or after content
//! - Incidental whitespace is removed based on indentation
//! - Escape sequences: \n, \t, \", \\, \s (preserve space), \ (line continuation)
//! - No string interpolation in Java ($ is literal)

use hyperion_parser::{Lexer, Parser};

#[test]
fn test_simple_text_block() {
    // Basic text block
    let source = r####"
        class Test {
            String sql = """
                SELECT * FROM users
                WHERE age > 18
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse simple text block, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_quotes() {
    // Text block containing quote characters
    let source = r####"
        class Test {
            String json = """
                {
                    "name": "John",
                    "age": 30
                }
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with quotes, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_escape_sequences() {
    // Text block with \n, \t escapes
    let source = r####"
        class Test {
            String text = """
                Line 1\n
                \tIndented line\n
                Line 3
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with escape sequences, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_trailing_whitespace() {
    // Trailing whitespace handling
    let source = "
        class Test {
            String text = \"\"\"
                Line with trailing spaces
                Another line
                \"\"\";
        }
    ";
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with trailing whitespace, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_empty_lines() {
    // Empty lines should be preserved
    let source = r####"
        class Test {
            String text = """
                First line

                Third line
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with empty lines, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_dollar_signs() {
    // $ should be literal (no interpolation in Java)
    let source = r####"
        class Test {
            String template = """
                Price: $100
                Total: $${total}
                Placeholder: ${name}
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with dollar signs, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_backslash() {
    // Backslash handling
    let source = r####"
        class Test {
            String path = """
                C:\\Users\\John\\Documents
                \\\\server\\share
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with backslashes, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_line_continuation() {
    // Line continuation with trailing backslash
    let source = r####"
        class Test {
            String text = """
                This is a long line \
                that continues here
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with line continuation, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_space_escape() {
    // \s escape to preserve trailing space
    let source = r####"
        class Test {
            String text = """
                Line with preserved space:\s
                Next line
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with \\s escape, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_sql_query() {
    // Real-world SQL query pattern
    let source = r####"
        class Test {
            String query = """
                SELECT u.name, u.email, o.total
                FROM users u
                JOIN orders o ON u.id = o.user_id
                WHERE o.status = 'completed'
                  AND o.total > 100
                ORDER BY o.total DESC
                LIMIT 10
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block SQL query, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_json() {
    // Real-world JSON pattern
    let source = r####"
        class Test {
            String json = """
                {
                    "name": "John Doe",
                    "email": "john@example.com",
                    "age": 30,
                    "address": {
                        "street": "123 Main St",
                        "city": "Springfield"
                    },
                    "tags": ["developer", "java"]
                }
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block JSON, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_html() {
    // HTML content in text block
    let source = r####"
        class Test {
            String html = """
                <html>
                    <body>
                        <h1>Hello, World!</h1>
                        <p>This is a "quoted" paragraph.</p>
                    </body>
                </html>
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block HTML, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_triple_quotes_inside() {
    // Text block containing triple quotes in content
    let source = r#####"
        class Test {
            String text = """
                This text contains """ inside it
                """;
        }
    "#####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    // This is actually invalid Java - triple quotes must be escaped
    assert!(
        !errors.is_empty(),
        "Triple quotes inside text block is invalid Java - parser should produce errors"
    );
}

#[test]
fn test_text_block_minimal() {
    // Minimal text block
    let source = r####"
        class Test {
            String text = """
                x
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse minimal text block, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_empty() {
    // Empty text block
    let source = r####"
        class Test {
            String text = """
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse empty text block, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_in_method_call() {
    // Text block as method argument
    let source = r####"
        class Test {
            void test() {
                System.out.println("""
                    Multi-line
                    text
                    """);
            }
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block in method call, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_concatenation() {
    // Text blocks in concatenation
    let source = r####"
        class Test {
            String text = """
                Part 1
                """ + """
                Part 2
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block concatenation, got: {:?}",
        errors
    );
}

#[test]
fn test_multiple_text_blocks() {
    // Multiple text blocks in same class
    let source = r####"
        class Test {
            String query1 = """
                SELECT * FROM users
                """;

            String query2 = """
                SELECT * FROM orders
                """;

            String json = """
                {"key": "value"}
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse multiple text blocks, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_unicode() {
    // Unicode characters in text block
    let source = r####"
        class Test {
            String text = """
                Hello 世界
                Привет мир
                مرحبا العالم
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with unicode, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_regex_pattern() {
    // Text block containing regex pattern
    let source = r####"
        class Test {
            String regex = """
                \\d{3}-\\d{2}-\\d{4}
                [a-zA-Z]+@[a-zA-Z]+\\.[a-zA-Z]+
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with regex, got: {:?}",
        errors
    );
}

#[test]
fn test_text_block_with_all_escapes() {
    // All escape sequences together
    let source = r####"
        class Test {
            String text = """
                Newline: \n
                Tab: \t
                Quote: \"
                Backslash: \\
                Space: \s
                Continuation: \
                continued
                """;
        }
    "####;
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    let _ = &green;

    assert!(
        errors.is_empty(),
        "Should parse text block with all escapes, got: {:?}",
        errors
    );
}
