// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for all contextual keywords in all valid contexts
//!
//! Per JLS §3.9 and sealed classes spec, contextual keywords can be used as:
//! - Parameter names
//! - Local variable names
//! - Field names
//! - Method names (after DOT operator)
//!
//! Type declarations use the narrower JLS `TypeIdentifier` exclusions. Module
//! contextual words and `when` remain legal there; `record`, `var`, `yield`,
//! `sealed`, and `permits` do not.

use hyperion_parser::{Lexer, Parser};

// ============================================================================
// Test Matrix: All Keywords × All Contexts
// ============================================================================

const CONTEXTUAL_KEYWORDS: &[&str] = &[
    "record",  // Java 14+
    "var",     // Java 10+
    "yield",   // Java 13+
    "sealed",  // Java 17+
    "permits", // Java 17+
];

// ============================================================================
// Parameter Names
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_parameter_names() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method(String {}) {{}}
            }}
            "#,
            keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as parameter name. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_contextual_keyword_with_type() {
    // Use actual type names to make it realistic
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            package com.example;

            class ReleaseDelta {{
                public void replay(ReleaseRecord {}) {{
                    System.out.println({});
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as parameter with type. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_multiple_contextual_keyword_parameters() {
    let source = r#"
        class Test {
            void method(String record, int var, boolean yield, Object sealed) {}
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse multiple contextual keywords as parameters. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Local Variable Names
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_local_variables() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    String {} = "value";
                    System.out.println({});
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as local variable. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_contextual_keyword_with_initialization() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    ReleaseRecord {} = getRecord();
                    if ({} != null) {{
                        process({});
                    }}
                }}
            }}
            "#,
            keyword, keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as initialized variable. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// Field Names
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_field_names() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                private String {};

                public void set(String value) {{
                    this.{} = value;
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as field name. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_contextual_keyword_field_with_modifiers() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                private final ReleaseRecord {};

                public Test(ReleaseRecord {}) {{
                    this.{} = {};
                }}
            }}
            "#,
            keyword, keyword, keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as field with modifiers. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// Lambda Parameters
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_lambda_parameters() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    Function<String, String> fn = {} -> {}.toUpperCase();
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as lambda parameter. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_contextual_keyword_typed_lambda_parameter() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    BiFunction<String, Integer, String> fn =
                        (String {}, Integer x) -> {} + x;
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as typed lambda parameter. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// Record Components (for 'record' keyword only - special case)
// ============================================================================

#[test]
fn test_record_as_record_component() {
    // This is a valid edge case: using 'record' as a component name
    // in a record declaration named something else
    let source = r#"
        record Container(String record, int value) {
            public Container {
                if (record == null) {
                    throw new IllegalArgumentException("record cannot be null");
                }
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'record' as record component. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Catch Parameters
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_catch_parameters() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    try {{
                        doSomething();
                    }} catch (Exception {}) {{
                        {}.printStackTrace();
                    }}
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as catch parameter. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// For-Each Loop Variables
// ============================================================================

#[test]
fn test_all_contextual_keywords_as_foreach_variables() {
    for keyword in CONTEXTUAL_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method(List<String> items) {{
                    for (String {} : items) {{
                        System.out.println({});
                    }}
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as for-each variable. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// Method Names After DOT (already tested in keyword_method_names_tests.rs)
// These are just sanity checks
// ============================================================================

#[test]
fn test_sealed_permits_as_method_names() {
    let source = r#"
        class Test {
            void method() {
                builder.sealed().permits(value);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse 'sealed'/'permits' as method names. Errors: {:?}",
        errors
    );
}

// ============================================================================
// Module System Keywords (Java 9+) as Identifiers
// ============================================================================

const MODULE_KEYWORDS: &[&str] = &[
    "module",
    "requires",
    "exports",
    "opens",
    "to",
    "uses",
    "provides",
    "with",
    "transitive",
    "open",
];

#[test]
fn test_module_keywords_as_local_variables() {
    for keyword in MODULE_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    String {} = "value";
                    System.out.println({});
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as local variable. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_module_keywords_as_parameters() {
    for keyword in MODULE_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method(String {}) {{
                    System.out.println({});
                }}
            }}
            "#,
            keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as parameter. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_module_keywords_in_expressions() {
    // This is the critical test - module keywords used as variable references in expressions
    for keyword in MODULE_KEYWORDS {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    String {} = "value";

                    // Simple name expression
                    System.out.println({});

                    // Method argument
                    doSomething({});

                    // Return value (in assignment)
                    String result = {};

                    // Binary operation
                    String combined = {} + "suffix";

                    // Comparison
                    boolean eq = {} == "test";
                }}

                void doSomething(String arg) {{}}
            }}
            "#,
            keyword, keyword, keyword, keyword, keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' in expressions. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_all_contextual_keywords_as_array_indices() {
    let all_keywords: Vec<&str> = CONTEXTUAL_KEYWORDS
        .iter()
        .chain(MODULE_KEYWORDS.iter())
        .copied()
        .collect();

    for keyword in &all_keywords {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    int[] arr = new int[10];
                    int {} = 5;

                    // Use as array index
                    int value = arr[{}];
                    arr[{}] = 42;
                }}
            }}
            "#,
            keyword, keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' as array index. Errors: {:?}",
            keyword,
            errors
        );
    }
}

#[test]
fn test_all_contextual_keywords_in_field_access() {
    let all_keywords: Vec<&str> = CONTEXTUAL_KEYWORDS
        .iter()
        .chain(MODULE_KEYWORDS.iter())
        .copied()
        .collect();

    for keyword in &all_keywords {
        let source = format!(
            r#"
            class Test {{
                void method() {{
                    Object {} = new Object();

                    // Field access on contextual keyword variable
                    String result = {}.toString();
                    int hash = {}.hashCode();
                }}
            }}
            "#,
            keyword, keyword, keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            errors.is_empty(),
            "Failed to parse '{}' in field access. Errors: {:?}",
            keyword,
            errors
        );
    }
}

// ============================================================================
// Real-World Pattern: All Keywords Together
// ============================================================================

// ============================================================================
// Negative Tests: Type Names (Should Still Fail)
// ============================================================================

#[test]
fn test_contextual_keywords_cannot_be_class_names() {
    // These should all fail - contextual keywords cannot be type names
    let keywords_that_fail = &["record", "var", "yield", "sealed", "permits"];

    for keyword in keywords_that_fail {
        let source = format!(
            r#"
            class {} {{}}
            "#,
            keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            !errors.is_empty(),
            "Should NOT parse '{}' as class name, but it did!",
            keyword
        );
    }
}

#[test]
fn test_contextual_keywords_cannot_be_interface_names() {
    let keywords_that_fail = &["record", "var", "yield", "sealed", "permits"];

    for keyword in keywords_that_fail {
        let source = format!(
            r#"
            interface {} {{}}
            "#,
            keyword
        );

        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);

        assert!(
            !errors.is_empty(),
            "Should NOT parse '{}' as interface name, but it did!",
            keyword
        );
    }
}

#[test]
fn module_and_guard_contextual_words_are_legal_type_identifiers() {
    for name in [
        "open",
        "module",
        "requires",
        "exports",
        "opens",
        "to",
        "uses",
        "provides",
        "with",
        "transitive",
        "when",
    ] {
        let source = format!(
            "class {name}<T> {{ {name}() {{}} {name} field; }} class Box<{name}> {{ {name} value; }}"
        );
        let tokens = Lexer::new(&source).collect();
        let (_, errors) = Parser::parse(tokens);
        assert!(
            errors.is_empty(),
            "{name} should be a legal type identifier: {errors:?}"
        );
    }
}

#[test]
fn restricted_type_identifiers_are_rejected_in_declarations_and_references() {
    for name in ["record", "var", "yield", "sealed", "permits"] {
        let declaration = format!("class {name} {{}}");
        let (_, declaration_errors) = Parser::parse(Lexer::new(&declaration).collect());
        assert!(
            !declaration_errors.is_empty(),
            "class {name} must be rejected"
        );

        if name != "var" {
            let reference = format!("class Holder {{ {name} value; }}");
            let (_, reference_errors) = Parser::parse(Lexer::new(&reference).collect());
            assert!(
                !reference_errors.is_empty(),
                "type reference {name} must be rejected"
            );
        }
    }
}

#[test]
fn restricted_words_remain_legal_package_qualifiers() {
    let source = "class Holder { var.example.Widget value; record.example.Widget other; }";
    let (_, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(
        errors.is_empty(),
        "restricted words may qualify a legal final type: {errors:?}"
    );
}

// ============================================================================
// Edge Case: non-sealed (hyphenated keyword)
// ============================================================================

#[test]
fn test_non_sealed_as_identifier_contexts() {
    // The contextual keyword contains a hyphen and therefore cannot be one
    // identifier outside its modifier context. Underscored/camel-case names
    // remain ordinary identifiers.

    let source = r#"
        class Test {
            private String non_sealed;  // Note: parser will reject hyphen, expects underscore

            void method(String nonSealed) {
                String non_sealed_var = nonSealed;
                System.out.println(non_sealed_var);
            }
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (_, errors) = Parser::parse(tokens);

    assert!(
        errors.is_empty(),
        "Failed to parse non-sealed variations. Errors: {:?}",
        errors
    );
}

#[test]
fn non_sealed_is_a_modifier_not_an_identifier() {
    let valid = "sealed class Base permits Child {} non-sealed class Child extends Base {}";
    let (_, valid_errors) = Parser::parse(Lexer::new(valid).collect());
    assert!(
        valid_errors.is_empty(),
        "valid modifier rejected: {valid_errors:?}"
    );

    let invalid = "class Bad { void run(String non-sealed) {} }";
    let (_, invalid_errors) = Parser::parse(Lexer::new(invalid).collect());
    assert!(
        !invalid_errors.is_empty(),
        "hyphenated non-sealed cannot be one parameter identifier"
    );
}

#[test]
fn non_sealed_spelling_is_subtraction_outside_modifier_context() {
    let source = "class Values { int subtract(int non, int sealed) { return non-sealed; } }";
    let (_, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(
        errors.is_empty(),
        "subtraction misparsed as a modifier: {errors:?}"
    );
}
