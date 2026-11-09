// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Java 9 JEP 213: Milling Project Coin
//!
//! JEP 213 introduced several small language changes, including:
//! - Try-with-resources with effectively-final variables
//! - Private methods in interfaces
//! - Diamond operator with anonymous classes
//! - Underscore as identifier removed
//! - @SafeVarargs on private instance methods
//!
//! This test suite focuses on try-with-resources enhancements.

use hyperion_parser::{Lexer, Parser};

// ============================================================================
// Try-with-Resources with Effectively-Final Variables (Java 9)
// ============================================================================

#[test]
fn test_try_with_resources_existing_variable() {
    // Java 9+ allows using existing variables in try-with-resources
    // Previously required new declaration: try (FileReader fr = new FileReader(...))
    // Now allows: try (fr) where fr is effectively final
    let source = r#"
        class Test {
            void method() throws Exception {
                FileReader fr = new FileReader("file.txt");
                try (fr) {
                    // use fr
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with existing variable. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_multiple_existing_variables() {
    // Multiple existing variables in try-with-resources
    let source = r#"
        class Test {
            void method() throws Exception {
                Admin admin = cluster.admin();
                Producer producer = cluster.producer();
                try (admin; producer) {
                    admin.listTopics();
                    producer.send(record);
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with multiple existing variables. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_field_reference() {
    // Using a field in try-with-resources
    let source = r#"
        class Test {
            private final Connection connection;

            Test() throws Exception {
                this.connection = DriverManager.getConnection("jdbc:...");
            }

            void method() throws Exception {
                try (connection) {
                    connection.createStatement();
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with field reference. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_method_call_result() {
    // Using method call result directly in try-with-resources
    let source = r#"
        class Test {
            void method() throws Exception {
                try (getConnection()) {
                    // use connection
                }
            }

            Connection getConnection() throws Exception {
                return DriverManager.getConnection("jdbc:...");
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with method call. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_trailing_semicolon() {
    // Java 9 allows trailing semicolon in resource list
    let source = r#"
        class Test {
            void method() throws Exception {
                FileReader fr = new FileReader("file.txt");
                try (fr;) {
                    // use fr
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with trailing semicolon. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_contextual_keyword_variable() {
    // Using a variable named 'record' (contextual keyword) in try-with-resources
    let source = r#"
        class Test {
            void method() throws Exception {
                FileReader record = new FileReader("file.txt");
                try (record) {
                    // use record
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with contextual keyword variable. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_qualified_reference() {
    // Using qualified reference in try-with-resources
    let source = r#"
        class Test {
            static class Resources {
                static FileReader reader;
            }

            void method() throws Exception {
                try (Resources.reader) {
                    // use reader
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with qualified reference. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Backward Compatibility: Traditional Try-with-Resources Still Works
// ============================================================================

#[test]
fn test_try_with_resources_traditional_declaration() {
    // Ensure traditional syntax still works
    let source = r#"
        class Test {
            void method() throws Exception {
                try (FileReader fr = new FileReader("file.txt")) {
                    // use fr
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse traditional try-with-resources. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_multiple_traditional_declarations() {
    let source = r#"
        class Test {
            void method() throws Exception {
                try (FileReader fr = new FileReader("file.txt");
                     BufferedReader br = new BufferedReader(fr)) {
                    String line = br.readLine();
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse multiple traditional try-with-resources. Errors: {:?}",
        errors
    );
}

#[test]
fn test_try_with_resources_final_modifier() {
    let source = r#"
        class Test {
            void method() throws Exception {
                try (final FileReader fr = new FileReader("file.txt")) {
                    // use fr
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse try-with-resources with final modifier. Errors: {:?}",
        errors
    );
}
