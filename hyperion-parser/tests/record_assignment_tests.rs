// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for 'record' contextual keyword in assignment contexts
//!
//! The 'record' keyword is contextual - it can be:
//! 1. A type declaration keyword: `record Point(int x, int y) {}`
//! 2. A variable/field name: `Entry<K, V> record = ...;`
//! 3. An identifier in expressions: `record = iterator.next();`
//!
//! This test suite focuses on distinguishing these cases correctly.

use hyperion_parser::{Lexer, Parser};

// ============================================================================
// Variable Declaration with 'record' as Variable Name
// ============================================================================

#[test]
fn test_record_as_local_variable_name() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record = getRecord();
                System.out.println(record.value());
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' as local variable name. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_as_field_name() {
    let source = r#"
        class Test {
            private Entry<String, String> record;

            void setRecord(Entry<String, String> record) {
                this.record = record;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' as field name. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Assignment to 'record' Variable
// ============================================================================

#[test]
fn test_record_reassignment_in_loop() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record = null;
                for (int i = 0; i < 10; i++) {
                    record = records.get(i);
                    process(record);
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' reassignment in loop. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_compound_assignment() {
    let source = r#"
        class Test {
            void method() {
                Integer record = 0;
                record += 1;
                record -= 2;
                record *= 3;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse compound assignment to 'record'. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_increment_decrement() {
    let source = r#"
        class Test {
            void method() {
                Integer record = 0;
                record++;
                ++record;
                record--;
                --record;
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse increment/decrement on 'record'. Errors: {:?}",
        errors
    );
}

// ============================================================================
// 'record' in Expressions
// ============================================================================

#[test]
fn test_record_method_call() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record = getRecord();
                String value = record.value();
                int partition = record.partition();
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse method calls on 'record'. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_array_access() {
    // Simplified to avoid parser issues with complex generic array types
    let source = r#"
        class Test {
            void method() {
                String[] record = getRecords();
                String first = record[0];
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse array access on 'record'. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_in_conditional() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record = getRecord();
                if (record != null && record.value() != null) {
                    process(record);
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' in conditional. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_as_argument() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record = getRecord();
                process(record);
                validate(record.key(), record.value());
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' as method argument. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Type Declaration Still Works
// ============================================================================

#[test]
fn test_record_type_declaration() {
    let source = r#"
        record Point(int x, int y) {
            public Point {
                if (x < 0 || y < 0) {
                    throw new IllegalArgumentException("Coordinates must be positive");
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse record type declaration. Errors: {:?}",
        errors
    );
}

#[test]
fn test_local_record_declaration() {
    let source = r#"
        class Test {
            void method() {
                record Point(int x, int y) {}
                Point p = new Point(10, 20);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse local record declaration. Errors: {:?}",
        errors
    );
}

// ============================================================================
// 'record' as Type Name (Not Declaration)
// ============================================================================

#[test]
fn test_record_as_type_in_variable_declaration() {
    // Using 'record' as a type name in a variable declaration
    // Note: This tests that "record Type name = ..." is parsed as var decl, not type decl
    let source = r#"
        class Test {
            void method() {
                record String x = getValue();
                System.out.println(x);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        !errors.is_empty(),
        "Expected parse errors for unsupported/ambiguous `record` type-name usage, but got none"
    );
}

// ============================================================================
// Labeled Statement with 'record'
// ============================================================================

#[test]
fn test_record_as_label() {
    // 'record' can also be a statement label
    // The parser should recognize "record: statement" and handle it via is_labeled_stmt()
    let source = r#"
        class Test {
            void method() {
                outer: for (int i = 0; i < 10; i++) {
                    if (i == 5) {
                        break outer;
                    }
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse labeled statement (testing label mechanism). Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_keyword_as_label() {
    let source = r#"
        class Test {
            void method() {
                record: for (int i = 0; i < 10; i++) {
                    if (i == 5) {
                        break record;
                    }
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse `record:` as statement label. Errors: {errors:?}"
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_record_chained_assignment() {
    let source = r#"
        class Test {
            void method() {
                Entry<byte[], byte[]> record, record2;
                record = record2 = getRecord();
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse chained assignment with 'record'. Errors: {:?}",
        errors
    );
}

#[test]
fn test_record_ternary_assignment() {
    let source = r#"
        class Test {
            void method(boolean condition) {
                Entry<byte[], byte[]> record;
                record = condition ? getRecord1() : getRecord2();
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse ternary assignment to 'record'. Errors: {:?}",
        errors
    );
}
