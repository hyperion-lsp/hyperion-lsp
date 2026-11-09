// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Edge case tests to uncover bugs and issues

// ============================================================================
// HOVER EDGE CASES
// ============================================================================

// Edge case: Nested static method calls
lsp_hover_test! {
    name: hover_nested_static_calls,

    files: {
        "Test.java" => r#"
            import java.util.Collections;
            import java.util.Objects;

            class Test {
                void test() {
                    var list = Collections.singletonList(Obje<|>cts.equals("a", "b"));
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["Objects"]
}

// Edge case: Inner class static method
lsp_hover_test! {
    name: hover_inner_class_static,

    files: {
        "Test.java" => r"
            class Outer {
                static class Inner {
                    static void staticMethod() {}
                }

                void test() {
                    Outer.In<|>ner.staticMethod();
                }
            }
        "
    },

    hover: "Test.java", "<|>"
}

lsp_hover_test! {
    name: field_wins_over_same_named_nested_type_in_expression,

    files: {
        "Test.java" => r"
            class Outer {
                static class Inner {}
                static int Inner;

                void test() {
                    int value = Outer.In<|>ner;
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["int", "Inner"]
}

// Edge case: Enum constant access
lsp_hover_test! {
    name: hover_enum_constant,

    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    Thread.St<|>ate state = Thread.State.RUNNING;
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["State", "java.lang.Thread.State"]
}

// Edge case: Generic class static method with type argument
lsp_hover_test! {
    name: hover_generic_static_method,

    files: {
        "Test.java" => r#"
            import java.util.Optional;

            class Test {
                void test() {
                    Optional<String> opt = Optio<|>nal.<String>of("test");
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["Optional"]
}

// ============================================================================
// POSTFIX COMPLETION EDGE CASES
// ============================================================================

#[test]
fn test_postfix_nested_member_access() {
    use crate::lsp_test_framework::LspTest;

    let mut test = LspTest::new();
    test.add_file(
        "Test.java",
        r#"
        class User {
            Address getAddress() { return null; }
        }

        class Address {
            String getStreet() { return ""; }
        }

        class Test {
            void test(User user) {
                user.getAddress().getStreet().null
            }
        }
        "#,
    );

    // Should extract just "getStreet()" as the expression
    use hyperion_lsp::completion::match_postfix_pattern;
    let result = match_postfix_pattern("user.getAddress().getStreet().null");
    println!("Nested member access result: {result:?}");
    assert!(result.is_some());
    if let Some((expr, template)) = result {
        assert_eq!(template, "null");
        // Should be "getStreet()" not the whole chain
        assert!(expr.contains("getStreet"));
    }
}

#[test]
fn test_postfix_parenthesized_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: expression in parentheses
    let result = match_postfix_pattern("(value + 1).if");
    println!("Parenthesized expression: {result:?}");
    assert!(result.is_some());
    if let Some((expr, template)) = result {
        assert_eq!(template, "if");
        assert!(expr.contains("(value + 1)"));
    }
}

#[test]
fn test_postfix_array_access() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: array element access
    let result = match_postfix_pattern("users[0].null");
    println!("Array access: {result:?}");
    assert!(result.is_some());
    if let Some((expr, template)) = result {
        assert_eq!(template, "null");
        assert!(expr.contains("[0]") || expr == "users[0]");
    }
}

#[test]
fn test_postfix_cast_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: cast expression
    let result = match_postfix_pattern("((String) obj).null");
    println!("Cast expression: {result:?}");
    assert!(result.is_some());
}

#[test]
fn test_postfix_qualified_name_not_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: qualified class name (not an expression)
    let result = match_postfix_pattern("com.example.Class");
    println!("Qualified name: {result:?}");
    // This should NOT trigger postfix (no alphabetic template after last dot)
    assert!(result.is_none());
}

#[test]
fn test_postfix_very_long_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: really long expression
    let long_expr = "user.getProfile().getSettings().getPreferences().getTheme().getName().toString().toLowerCase().trim()";
    let result = match_postfix_pattern(&format!("{long_expr}.null"));
    println!("Long expression: {result:?}");
    assert!(result.is_some());
}

#[test]
fn test_postfix_with_generic_type_args() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: method call with generic type arguments
    let result = match_postfix_pattern("Optional.<String>empty().null");
    println!("Generic type args: {result:?}");
    assert!(result.is_some());
}

// ============================================================================
// MORE HOVER EDGE CASES
// ============================================================================

// Edge case: Method reference
lsp_hover_test! {
    name: hover_method_reference,

    files: {
        "Test.java" => r"
            import java.util.function.Function;

            class Test {
                void test() {
                    Function<String, Integer> f = Str<|>ing::valueOf;
                }
            }
        "
    },

    hover: "Test.java", "<|>"
}

// Edge case: Lambda parameter hover
lsp_hover_test! {
    name: hover_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;

            class Test {
                void test(List<String> items) {
                    items.forEach(it<|>em -> System.out.println(item));
                }
            }
        "
    },

    hover: "Test.java", "<|>"
}

// Edge case: Annotation hover
lsp_hover_test! {
    name: hover_annotation,

    files: {
        "Test.java" => r#"
            class Test {
                @Overr<|>ide
                public String toString() {
                    return "";
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["Override", "java.lang.Override"]
}

// Edge case: Package name in import (should NOT show class hover)
lsp_hover_test! {
    name: hover_package_in_import,

    files: {
        "Test.java" => r"
            import ja<|>va.util.List;

            class Test {}
        "
    },

    hover: "Test.java", "<|>"
}

// Edge case: Var with diamond operator
lsp_hover_test! {
    name: hover_var_with_diamond,

    files: {
        "Test.java" => r"
            import java.util.ArrayList;
            import java.util.List;

            class Test {
                void test() {
                    var li<|>st = new ArrayList<String>();
                }
            }
        "
    },

    hover: "Test.java", "<|>"
}

// ============================================================================
// CODE GENERATION EDGE CASES
// ============================================================================

#[test]
fn test_generate_constructor_no_fields() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "EmptyClass.java",
        r"
        class EmptyClass {
            // No fields
        }
        ",
    );

    let result = test.code_action(
        "EmptyClass.java",
        Range::new(Position::new(1, 0), Position::new(1, 0)),
        vec![],
    );

    // Should we offer constructor generation for empty class?
    if let Ok(Some(actions)) = result {
        let has_constructor = actions.iter().any(|a| {
            if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(action) = a {
                action.title.to_lowercase().contains("constructor")
            } else {
                false
            }
        });
        println!("Empty class has constructor action: {has_constructor}");
        // Document the behavior
    }
}

#[test]
fn test_generate_constructor_generic_fields() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "GenericClass.java",
        r"
        import java.util.Map;
        import java.util.List;

        class GenericClass {
            private Map<String, List<Integer>> complexField;
            private List<? extends Number> wildcardField;
        }
        ",
    );

    let result = test.code_action(
        "GenericClass.java",
        Range::new(Position::new(5, 0), Position::new(5, 0)),
        vec![],
    );

    match result {
        Ok(Some(actions)) => {
            let has_constructor = actions.iter().any(|a| {
                if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(action) = a {
                    action.title.to_lowercase().contains("constructor")
                } else {
                    false
                }
            });
            println!("Generic fields has constructor action: {has_constructor}");

            if has_constructor {
                println!("✅ Can generate constructor with complex generic types");
            }
        }
        Err(e) => {
            println!("❌ Error with generic fields: {e}");
            panic!("Should handle generic field types");
        }
        _ => {}
    }
}

#[test]
fn test_generate_constructor_with_static_fields() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "ClassWithStatic.java",
        r#"
        class ClassWithStatic {
            private static final String CONSTANT = "value";
            private String instanceField;
        }
        "#,
    );

    let result = test.code_action(
        "ClassWithStatic.java",
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        println!("Actions for class with static fields: {}", actions.len());
        // Static fields should NOT be included in constructor
        // Only instanceField should be in the constructor
    }
}

#[test]
fn test_generate_constructor_array_fields() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "ArrayFields.java",
        r"
        class ArrayFields {
            private String[] names;
            private int[][] matrix;
        }
        ",
    );

    let result = test.code_action(
        "ArrayFields.java",
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        let has_constructor = actions.iter().any(|a| {
            if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(action) = a {
                action.title.to_lowercase().contains("constructor")
            } else {
                false
            }
        });
        if has_constructor {
            println!("✅ Can generate constructor with array types");
        } else {
            println!("❌ Cannot generate constructor with array types");
        }
    }
}

#[test]
fn test_generate_getters_setters_final_fields() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "FinalFields.java",
        r"
        class FinalFields {
            private final String immutableField;
            private String mutableField;

            FinalFields(String immutableField) {
                this.immutableField = immutableField;
            }
        }
        ",
    );

    let result = test.code_action(
        "FinalFields.java",
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        println!("Actions for final fields: {}", actions.len());
        // Final fields should have getter but no setter
        // Should we offer "generate getters" separately?
    }
}

#[test]
fn test_generate_constructor_nested_class() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "OuterClass.java",
        r"
        class OuterClass {
            private String outerField;

            class InnerClass {
                private String innerField;
            }
        }
        ",
    );

    let result = test.code_action(
        "OuterClass.java",
        Range::new(Position::new(5, 0), Position::new(5, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        println!("Actions for inner class: {}", actions.len());
        // Should offer constructor for inner class with just innerField
    }
}

#[test]
fn test_generate_constructor_already_exists() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "ExistingConstructor.java",
        r"
        class ExistingConstructor {
            private String field1;
            private String field2;

            public ExistingConstructor(String field1) {
                this.field1 = field1;
            }
        }
        ",
    );

    let result = test.code_action(
        "ExistingConstructor.java",
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        let has_constructor = actions.iter().any(|a| {
            if let tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(action) = a {
                action.title.to_lowercase().contains("constructor")
            } else {
                false
            }
        });
        println!("Offers constructor when one exists: {has_constructor}");
        // Should we still offer? Should we detect existing constructors?
    }
}

// ============================================================================
// UNICODE AND SPECIAL CHARACTERS
// ============================================================================

#[test]
fn test_postfix_unicode_identifiers() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Unicode identifiers (valid in Java)
    let result = match_postfix_pattern("über.null");
    println!("Unicode identifier: {result:?}");
    assert!(result.is_some());
    if let Some((expr, template)) = result {
        assert_eq!(template, "null");
        assert_eq!(expr, "über");
    }
}

#[test]
fn test_postfix_emoji_in_string() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Expression with string containing emoji
    let result = match_postfix_pattern(r#"message("Hello 👋").for"#);
    println!("String with emoji: {result:?}");
    assert!(result.is_some());
}

// ============================================================================
// SYNTAX ERROR RESILIENCE
// ============================================================================

lsp_hover_test! {
    name: hover_with_syntax_errors_nearby,

    files: {
        "Broken.java" => r#"
            class Broken {
                void test() {
                    String valu<|>e = "test";
                    // Missing semicolon below
                    int broken =
                }
            }
        "#
    },

    hover: "Broken.java", "<|>"
}

#[test]
fn test_postfix_incomplete_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Incomplete expression (unclosed paren)
    let result = match_postfix_pattern("getValue(.for");
    println!("Incomplete expression: {result:?}");
    // Should handle gracefully
}

// ============================================================================
// COMPLEX REAL-WORLD SCENARIOS
// ============================================================================

lsp_hover_test! {
    name: hover_chained_builder_pattern,

    files: {
        "Builder.java" => r#"
            class User {
                String name;
                int age;

                static Builder builder() {
                    return new Builder();
                }

                static class Builder {
                    String name;
                    int age;

                    Builder name(String name) {
                        this.name = name;
                        return this;
                    }

                    Builder age(int age) {
                        this.age = age;
                        return this;
                    }

                    User build() {
                        return new User();
                    }
                }
            }

            class Test {
                void test() {
                    User user = User.build<|>er()
                        .name("John")
                        .age(30)
                        .build();
                }
            }
        "#
    },

    hover: "Builder.java", "<|>"
}

#[test]
fn test_postfix_stream_api_chain() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Complex Stream API chain
    let result = match_postfix_pattern(
        "items.stream().filter(x -> x > 0).map(x -> x * 2).collect(Collectors.toList()).for",
    );
    println!("Stream API chain: {result:?}");
    assert!(result.is_some());
    if let Some((_expr, template)) = result {
        assert_eq!(template, "for");
    }
}

#[test]
fn test_generate_constructor_with_generics() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::{Position, Range};

    let mut test = LspTest::new();
    test.add_file(
        "GenericClass.java",
        r"
        class GenericClass<T extends Comparable<T>> {
            private T value;
            private List<T> items;
        }
        ",
    );

    let result = test.code_action(
        "GenericClass.java",
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        vec![],
    );

    if let Ok(Some(actions)) = result {
        println!("Actions for generic class with bounds: {}", actions.len());
        // Should handle T and List<T> correctly
    }
}

#[test]
fn test_postfix_with_comments() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Expression with comment before dot
    // (In real usage, comments would be stripped by parser, but test the pattern matcher)
    let result = match_postfix_pattern("value /* comment */ .for");
    println!("Expression with comment: {result:?}");
    // Pattern matcher might see this differently
}

#[test]
fn test_postfix_ternary_expression() {
    use hyperion_lsp::completion::match_postfix_pattern;

    // Edge case: Ternary operator
    let result = match_postfix_pattern("(x > 0 ? x : -x).for");
    println!("Ternary expression: {result:?}");
    assert!(result.is_some());
    if let Some((expr, template)) = result {
        assert_eq!(template, "for");
        assert!(expr.contains('?'));
    }
}
