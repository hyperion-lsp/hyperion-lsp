// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for record pattern support in Java 21+ switch statements
//!
//! Tests pattern variable extraction and scoping for:
//! - Simple type patterns (String s, Integer i)
//! - Record patterns (Point(int x, int y))
//! - Nested record patterns (Circle(Point(var x, var y), var r))

// Simple type pattern navigation test
lsp_goto_test! {
    name: simple_type_pattern_goto,
    files: {
        "Test.java" => r"
            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case String s ->
                            System.out.println(s<|>.length());
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// Record pattern variables navigation test
lsp_goto_test! {
    name: record_pattern_x_variable,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Point(int x, int y) ->
                            System.out.println(x<|> + y);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

lsp_goto_test! {
    name: record_pattern_y_variable,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Point(int x, int y) ->
                            System.out.println(x + y<|>);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// Nested record pattern test
lsp_goto_test! {
    name: nested_record_pattern_x,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}
            record Circle(Point center, int radius) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Circle(Point(int x, int y), int r) ->
                            System.out.println(x<|> + y + r);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

lsp_goto_test! {
    name: nested_record_pattern_y,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}
            record Circle(Point center, int radius) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Circle(Point(int x, int y), int r) ->
                            System.out.println(x + y<|> + r);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

lsp_goto_test! {
    name: nested_record_pattern_r,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}
            record Circle(Point center, int radius) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Circle(Point(int x, int y), int r) ->
                            System.out.println(x + y + r<|>);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// Pattern with var
lsp_goto_test! {
    name: pattern_with_var_x,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Point(var x, var y) ->
                            System.out.println(x<|> + y);
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// JLS 14.30.1 infers a `var` component's type from the record component in the
// same POSITION, not the one that happens to share its name. Here the binding
// spelled `count` sits in position 0, so it is the `String label` component and
// `toUpperCase()` resolves; a by-name inference would type it `int` and find
// nothing. This pins the inference itself, not merely that the binding exists.
lsp_goto_test! {
    name: var_pattern_component_type_is_positional,
    files: {
        "Test.java" => r"
            record Labelled(String label, int count) {}

            class Test {
                void process(Object obj) {
                    switch (obj) {
                        case Labelled(var count, var label) ->
                            System.out.println(count.toUpperC<|>ase());
                        default -> {}
                    }
                }
            }
        ",
        "java/lang/String.java" => r"
            package java.lang;
            public final class String {
                public String toUpperCase() { return this; }
            }
        "
    },
    nav: "Test.java", "<|>" => "String.java"
}

// Switch expression with pattern
lsp_goto_test! {
    name: switch_expression_pattern,
    files: {
        "Test.java" => r#"
            record Point(int x, int y) {}

            class Test {
                String format(Object obj) {
                    return switch (obj) {
                        case Point(int x, int y) -> x<|> + "," + y;
                        default -> "unknown";
                    };
                }
            }
        "#
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// Pattern scope isolation
lsp_goto_test! {
    name: pattern_scope_isolation,
    files: {
        "Test.java" => r"
            record Point(int x, int y) {}

            class Test {
                void test(Object obj) {
                    switch (obj) {
                        case Point(int x, int y) -> {
                            System.out.println(x<|>);
                        }
                        default -> {}
                    }
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}
