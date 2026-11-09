// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Ergonomic macros for LSP testing
//!
//! Provides a declarative way to write LSP tests with minimal boilerplate.

/// Internal helper: normalize a single marker-map entry to `(file, markers)` tuple.
#[doc(hidden)]
#[macro_export]
macro_rules! __lsp_marker_map_entry {
    ($file:tt : [ $( $marker:expr ),* $(,)? ]) => {
        ($file.to_string(), vec![ $( $marker.to_string() ),* ])
    };
    ($file:tt : $marker:expr) => {
        ($file.to_string(), vec![ $marker.to_string() ])
    };
}

/// Internal helper: build marker-map vector from compact DSL syntax.
#[doc(hidden)]
#[macro_export]
macro_rules! __lsp_marker_map_vec {
    ( $( $file:tt : $value:tt ),* $(,)? ) => {{
        let __map: Vec<(String, Vec<String>)> = vec![
            $( $crate::__lsp_marker_map_entry!($file : $value) ),*
        ];
        __map
    }};
}

/// Strict goto-definition test macro.
///
/// Syntax:
/// ```ignore
/// goto_test! {
///     test_name {
///         file "A.java" r#"..."#;
///         target "A.java" "use";
///         expect "B.java" "decl";
///     }
/// }
/// ```
/// or:
/// ```ignore
/// goto_test! {
///     test_name {
///         file "A.java" r#"..."#;
///         target "A.java" "marker";
///         expect none;
///     }
/// }
/// ```
#[macro_export]
macro_rules! goto_test {
    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect $expected_file:tt $expected_marker:expr;
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __response = test
                .goto_definition($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("goto_definition failed: {e}"));

            let __actual_locations = $crate::lsp_test_framework::goto_response_to_locations(__response);
            if __actual_locations.len() != 1 {
                let __actual_keys = test
                    .locations_to_keys(&__actual_locations)
                    .unwrap_or_else(|e| panic!("failed to normalize goto locations: {e}"));
                panic!(
                    "[goto_test:{}] expected exactly one target, got {}\nactual:\n{}",
                    stringify!($test_name),
                    __actual_locations.len(),
                    $crate::lsp_test_framework::format_location_keys(&__actual_keys),
                );
            }

            let __actual_keys = test
                .locations_to_keys(&__actual_locations)
                .unwrap_or_else(|e| panic!("failed to normalize goto locations: {e}"));
            let __expected_map = vec![($expected_file.to_string(), vec![$expected_marker.to_string()])];
            let __expected_keys = test
                .marker_map_to_keys(&__expected_map)
                .unwrap_or_else(|e| panic!("expected marker resolution failed: {e}"));

            if __actual_keys != __expected_keys {
                let __missing: std::collections::BTreeSet<_> =
                    __expected_keys.difference(&__actual_keys).cloned().collect();
                let __unexpected: std::collections::BTreeSet<_> =
                    __actual_keys.difference(&__expected_keys).cloned().collect();
                panic!(
                    "[goto_test:{}] exact match failed\nexpected:\n{}\nactual:\n{}\nmissing:\n{}\nunexpected:\n{}",
                    stringify!($test_name),
                    $crate::lsp_test_framework::format_location_keys(&__expected_keys),
                    $crate::lsp_test_framework::format_location_keys(&__actual_keys),
                    $crate::lsp_test_framework::format_location_keys(&__missing),
                    $crate::lsp_test_framework::format_location_keys(&__unexpected),
                );
            }
        }
    };

    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect none;
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __response = test
                .goto_definition($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("goto_definition failed: {e}"));
            let __actual_locations = $crate::lsp_test_framework::goto_response_to_locations(__response);
            let __actual_keys = test
                .locations_to_keys(&__actual_locations)
                .unwrap_or_else(|e| panic!("failed to normalize goto locations: {e}"));

            if !__actual_keys.is_empty() {
                panic!(
                    "[goto_test:{}] expected no target, but got:\n{}",
                    stringify!($test_name),
                    $crate::lsp_test_framework::format_location_keys(&__actual_keys),
                );
            }
        }
    };
}

/// Strict references test macro.
///
/// - `expect { ... }` is exact by default.
/// - `expect contains { ... }` asserts subset.
/// - `expect not { ... }` asserts forbidden markers are absent.
#[macro_export]
macro_rules! references_test {
    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect { $( $expected_file:tt : $expected_value:tt ),* $(,)? };
            $( expect not { $( $not_file:tt : $not_value:tt ),* $(,)? }; )*
            $( expect contains { $( $contains_file:tt : $contains_value:tt ),* $(,)? }; )*
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __locations = test
                .find_references($target_file, &__resolved_target, false)
                .unwrap_or_else(|e| panic!("find_references failed: {e}"));
            let __actual_keys = test
                .locations_to_keys(&__locations)
                .unwrap_or_else(|e| panic!("failed to normalize reference locations: {e}"));

            let __expected_exact_map = $crate::__lsp_marker_map_vec! {
                $( $expected_file : $expected_value ),*
            };
            let __expected_exact_keys = test
                .marker_map_to_keys(&__expected_exact_map)
                .unwrap_or_else(|e| panic!("expected marker resolution failed: {e}"));
            if __actual_keys != __expected_exact_keys {
                let __missing: std::collections::BTreeSet<_> =
                    __expected_exact_keys.difference(&__actual_keys).cloned().collect();
                let __unexpected: std::collections::BTreeSet<_> =
                    __actual_keys.difference(&__expected_exact_keys).cloned().collect();
                panic!(
                    "[references_test:{}] exact match failed\nexpected:\n{}\nactual:\n{}\nmissing:\n{}\nunexpected:\n{}",
                    stringify!($test_name),
                    $crate::lsp_test_framework::format_location_keys(&__expected_exact_keys),
                    $crate::lsp_test_framework::format_location_keys(&__actual_keys),
                    $crate::lsp_test_framework::format_location_keys(&__missing),
                    $crate::lsp_test_framework::format_location_keys(&__unexpected),
                );
            }

            $(
                {
                    let __forbidden_map = $crate::__lsp_marker_map_vec! {
                        $( $not_file : $not_value ),*
                    };
                    let __forbidden_keys = test
                        .marker_map_to_keys(&__forbidden_map)
                        .unwrap_or_else(|e| panic!("forbidden marker resolution failed: {e}"));
                    let __hits: std::collections::BTreeSet<_> =
                        __actual_keys.intersection(&__forbidden_keys).cloned().collect();
                    if !__hits.is_empty() {
                        panic!(
                            "[references_test:{}] forbidden locations were returned:\n{}",
                            stringify!($test_name),
                            $crate::lsp_test_framework::format_location_keys(&__hits),
                        );
                    }
                }
            )*

            $(
                {
                    let __contains_map = $crate::__lsp_marker_map_vec! {
                        $( $contains_file : $contains_value ),*
                    };
                    let __contains_keys = test
                        .marker_map_to_keys(&__contains_map)
                        .unwrap_or_else(|e| panic!("contains marker resolution failed: {e}"));
                    let __missing_contains: std::collections::BTreeSet<_> =
                        __contains_keys.difference(&__actual_keys).cloned().collect();
                    if !__missing_contains.is_empty() {
                        panic!(
                            "[references_test:{}] required subset missing:\n{}",
                            stringify!($test_name),
                            $crate::lsp_test_framework::format_location_keys(&__missing_contains),
                        );
                    }
                }
            )*
        }
    };
}

/// Hover test macro.
#[macro_export]
macro_rules! hover_test {
    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect contains [ $( $expected_text:expr ),+ $(,)? ];
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __hover = test
                .hover($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("hover failed: {e}"))
                .unwrap_or_else(|| panic!("[hover_test:{}] expected hover, got none", stringify!($test_name)));
            let __hover_text = $crate::lsp_test_framework::hover_to_text(&__hover);

            $(
                assert!(
                    __hover_text.contains($expected_text),
                    "[hover_test:{}] expected hover text to contain '{}', got:\n{}",
                    stringify!($test_name),
                    $expected_text,
                    __hover_text
                );
            )+
        }
    };

    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect none;
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __hover = test
                .hover($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("hover failed: {e}"));
            assert!(
                __hover.is_none(),
                "[hover_test:{}] expected no hover, got some",
                stringify!($test_name)
            );
        }
    };
}

/// Completions test macro.
#[macro_export]
macro_rules! completions_test {
    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect contains [ $( $expected_label:expr ),+ $(,)? ];
            $( expect not [ $( $not_label:expr ),+ $(,)? ]; )?
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __completions = test
                .completion($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("completion failed: {e}"));
            let __labels: std::collections::BTreeSet<_> =
                __completions.iter().map(|c| c.label.clone()).collect();

            $(
                assert!(
                    __labels.contains($expected_label),
                    "[completions_test:{}] expected completion label '{}' not found. labels={:?}",
                    stringify!($test_name),
                    $expected_label,
                    __labels
                );
            )+

            $(
                $(
                    assert!(
                        !__labels.contains($not_label),
                        "[completions_test:{}] unexpected completion label '{}' found. labels={:?}",
                        stringify!($test_name),
                        $not_label,
                        __labels
                    );
                )+
            )?
        }
    };

    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect none;
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __completions = test
                .completion($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("completion failed: {e}"));
            assert!(
                __completions.is_empty(),
                "[completions_test:{}] expected no completions, got {}",
                stringify!($test_name),
                __completions.len()
            );
        }
    };
}

/// Signature-help test macro.
#[macro_export]
macro_rules! signature_test {
    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect contains [ $( $expected_text:expr ),+ $(,)? ];
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __sig_help = test
                .signature_help($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("signature_help failed: {e}"))
                .unwrap_or_else(|| panic!("[signature_test:{}] expected signature help, got none", stringify!($test_name)));

            assert!(
                !__sig_help.signatures.is_empty(),
                "[signature_test:{}] expected at least one signature, got none",
                stringify!($test_name)
            );
            let __label = &__sig_help.signatures[0].label;
            $(
                assert!(
                    __label.contains($expected_text),
                    "[signature_test:{}] expected signature label to contain '{}', got '{}'",
                    stringify!($test_name),
                    $expected_text,
                    __label
                );
            )+
        }
    };

    (
        $test_name:ident {
            $( file $file_path:tt $file_content:expr; )+
            target $target_file:tt $target_marker:expr;
            expect none;
        }
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let __resolved_target = test
                .resolve_marker_name($target_file, $target_marker)
                .unwrap_or_else(|e| panic!("target marker resolution failed: {e}"));
            let __sig_help = test
                .signature_help($target_file, &__resolved_target)
                .unwrap_or_else(|e| panic!("signature_help failed: {e}"));
            assert!(
                __sig_help.is_none(),
                "[signature_test:{}] expected no signature help, got some",
                stringify!($test_name)
            );
        }
    };
}

// Define a goto-definition test with inline file definitions
//
// # Example
//
// ```no_run
/// lsp_goto_test! {
///     name: method_call_on_field,
///
///     "com/example/Foo.java" => r#"
///         package com.example;
///         import com.example.Bar;
///
///         public class Foo {
///             private Bar bar;
///             public void test() {
///                 bar.ba<|>z();
///             }
///         }
///     "#,
///
///     "com/example/Bar.java" => r#"
///         package com.example;
///         public class Bar {
///             public void baz() {}
///         }
///     "#,
///
///     goto: "com/example/Foo.java" @ "<|>" => "Bar.java"
/// }
/// ```
#[macro_export]
macro_rules! lsp_goto_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        nav: $test_file:expr, $cursor:expr => $expected_file:expr
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform goto_definition
            let result = test.goto_definition($test_file, $cursor);

            // Check expectation
            // If $expected_file starts with '<' and ends with '>', it's a marker for position check
            let is_position_check = $expected_file.starts_with('<') && $expected_file.ends_with('>');

            match result {
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
                    if is_position_check {
                        // Get expected position from marker
                        let expected_pos = test.get_marker_position($test_file, $expected_file)
                            .expect(&format!("Marker {} not found in {}", $expected_file, $test_file));

                        let actual = loc.range.start;
                        assert_eq!(
                            (actual.line, actual.character),
                            (expected_pos.line, expected_pos.character),
                            "Position mismatch: expected {}:{}, got {}:{}",
                            expected_pos.line, expected_pos.character,
                            actual.line, actual.character
                        );
                        println!("✅ {}: Navigated to {} at exact position {}:{}",
                            stringify!($test_name), $expected_file, actual.line, actual.character);
                    } else {
                        assert!(
                            loc.uri.as_str().contains($expected_file),
                            "Expected navigation to file containing '{}', but got: {} at {:?}",
                            $expected_file,
                            loc.uri,
                            loc.range
                        );
                        println!("✅ {}: Navigated to {} at {:?}",
                            stringify!($test_name), $expected_file, loc.range.start);
                    }
                }
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
                    assert!(!locs.is_empty(), "Expected non-empty location array");

                    if is_position_check {
                        let expected_pos = test.get_marker_position($test_file, $expected_file)
                            .expect(&format!("Marker {} not found in {}", $expected_file, $test_file));

                        let actual = locs[0].range.start;
                        assert_eq!(
                            (actual.line, actual.character),
                            (expected_pos.line, expected_pos.character),
                            "Position mismatch: expected {}:{}, got {}:{}",
                            expected_pos.line, expected_pos.character,
                            actual.line, actual.character
                        );
                        println!("✅ {}: Navigated to {} at exact position {}:{}",
                            stringify!($test_name), $expected_file, actual.line, actual.character);
                    } else {
                        assert!(
                            locs[0].uri.as_str().contains($expected_file),
                            "Expected navigation to file containing '{}', but got: {} at {:?}",
                            $expected_file,
                            locs[0].uri,
                            locs[0].range
                        );
                        println!("✅ {}: Navigated to {} at {:?}",
                            stringify!($test_name), $expected_file, locs[0].range.start);
                    }
                }
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Link(_))) => {
                    panic!("Unexpected link response");
                }
                Ok(None) => panic!("Expected definition in {}, got None", $expected_file),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };
}

// Define a goto-definition test expecting no result
//
// # Example
//
// ```no_run
/// lsp_goto_none_test! {
///     name: primitive_type_variable,
///
///     "com/example/Test.java" => r#"
///         package com.example;
///         public class Test {
///             public void test() {
///                 int count<|> = 0;
///             }
///         }
///     "#,
///
///     goto: "com/example/Test.java" @ "<|>"
/// }
/// ```
#[macro_export]
macro_rules! lsp_goto_none_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        nav: $test_file:expr, $cursor:expr
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform goto_definition
            let result = test.goto_definition($test_file, $cursor);

            // Expect None
            match result {
                Ok(None) => {
                    println!("✅ {}: No definition (expected)", stringify!($test_name));
                }
                Ok(Some(resp)) => {
                    panic!("Expected no definition, but got: {:?}", resp);
                }
                Err(e) => panic!("Error: {}", e),
            }
        }
    };
}

// Group multiple goto-definition tests into a test suite
//
// # Example
//
// ```no_run
/// lsp_test_suite! {
///     name: "Qualified Member Access",
///
///     test method_call_on_field {
///         "Foo.java" => "class Foo { Bar b; void test() { b.baz<|>(); } }",
///         "Bar.java" => "class Bar { void baz() {} }",
///         goto: "Foo.java" @ "<|>" => "Bar.java"
///     }
///
///     test method_call_on_param {
///         "Test.java" => "class Test { void test(Bar b) { b.baz<|>(); } }",
///         "Bar.java" => "class Bar { void baz() {} }",
///         goto: "Test.java" @ "<|>" => "Bar.java"
///     }
/// }
/// ```
#[macro_export]
macro_rules! lsp_test_suite {
    (
        name: $suite_name:expr,
        $(
            test $test_name:ident {
                files: {
                    $( $file_path:expr => $file_content:expr ),+ $(,)?
                },
                nav: $test_file:expr, $cursor:expr => $expected_file:expr
            }
        )+
    ) => {
        // Generate individual tests
        $(
            lsp_goto_test! {
                name: $test_name,
                files: {
                    $( $file_path => $file_content ),+
                },
                nav: $test_file, $cursor => $expected_file
            }
        )+

        // Note: Suite runner removed to avoid function name collisions
        // when multiple test suites are defined in the same file.
        // Each test runs independently via `cargo test`.
    };
}

/// Compact syntax for simple same-package tests
///
/// # Example
///
/// ```no_run
/// simple_goto_test!(
///     method_call,
///     "Foo.java" => "class Foo { Bar b; void test() { b.baz<|>(); } }",
///     "Bar.java" => "class Bar { void baz() {} }",
///     "Foo.java" @ "<|>" => "Bar.java"
/// );
/// ```
#[macro_export]
macro_rules! simple_goto_test {
    (
        $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        $test_file:expr, $cursor:expr => $expected_file:expr
    ) => {
        lsp_goto_test! {
            name: $test_name,
            files: {
                $( $file_path => $file_content ),+
            },
            nav: $test_file, $cursor => $expected_file
        }
    };
}

// Multi-file test with multiple cursors and expectations
//
// Useful for testing multiple navigation points in the same setup
//
// # Example
//
// ```no_run
/// multi_cursor_test! {
///     name: multiple_method_calls,
///
///     files: {
///         "Foo.java" => r#"
///             class Foo {
///                 Bar bar;
///                 void test() {
///                     bar.baz<cursor1>();
///                     bar.qux<cursor2>();
///                 }
///             }
///         "#,
///         "Bar.java" => r#"
///             class Bar {
///                 void baz() {}
///                 void qux() {}
///             }
///         "#
///     },
///
///     checks: [
///         "Foo.java" @ "<cursor1>" => "Bar.java",
///         "Foo.java" @ "<cursor2>" => "Bar.java"
///     ]
/// }
/// ```
#[macro_export]
macro_rules! multi_cursor_test {
    (
        name: $test_name:ident,

        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },

        checks: [
            $( $check_file:expr, $check_cursor:expr => $check_expected:expr ),+ $(,)?
        ]
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Run all checks
            let mut check_num = 0;
            $(
                check_num += 1;
                print!("  Check {}: {} @ {} ... ", check_num, $check_file, $check_cursor);

                let result = test.goto_definition($check_file, $check_cursor);

                match &result {
                    Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
                        assert!(
                            loc.uri.as_str().contains($check_expected),
                            "Expected {}, got {}",
                            $check_expected,
                            loc.uri
                        );
                        println!("✅");
                    }
                    Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) if !locs.is_empty() => {
                        assert!(
                            locs[0].uri.as_str().contains($check_expected),
                            "Expected {}, got {}",
                            $check_expected,
                            locs[0].uri
                        );
                        println!("✅");
                    }
                    Ok(None | Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(_))) => {
                        panic!("Expected definition in {}, got None or empty array", $check_expected);
                    }
                    Err(e) => panic!("Error: {}", e),
                    _ => panic!("Unexpected response type"),
                }
            )+

            println!("✅ {}: All {} checks passed", stringify!($test_name), check_num);
        }
    };
}

// Test with expected compilation/semantic errors
//
// Useful for negative testing
//
// # Example
//
// ```no_run
/// error_test! {
///     name: undefined_method,
///
///     "Test.java" => r#"
///         class Test {
///             void test() {
///                 Bar bar = new Bar();
///                 bar.undefined<|>();  // Should not resolve
///             }
///         }
///     "#,
///
///     "Bar.java" => "class Bar { void defined() {} }",
///
///     goto: "Test.java" @ "<|>" => expect_none
/// }
/// ```
#[macro_export]
macro_rules! error_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        nav: $test_file:expr, $cursor:expr => expect_none
    ) => {
        lsp_goto_none_test! {
            name: $test_name,
            files: {
                $( $file_path => $file_content ),+
            },
            nav: $test_file, $cursor
        }
    };
}

// Define a hover test with inline file definitions
//
// # Example
//
// ```no_run
/// lsp_hover_test! {
///     name: hover_on_method_call,
///
///     files: {
///         "com/example/Main.java" => r#"
///             package com.example;
///             import com.example.model.User;
///
///             public class Main {
///                 public void test() {
///                     getUser().getName();
///                     ^
///                 }
///
///                 private User getUser() {
///                     return new User();
///                 }
///             }
///         "#,
///         "com/example/model/User.java" => r#"
///             package com.example.model;
///
///             public class User {
///                 public String getName() {
///                     return "";
///                 }
///             }
///         "#
///     },
///
///     hover: "com/example/Main.java", "^",
///     contains: ["User", "getUser"]
/// }
/// ```
#[macro_export]
macro_rules! lsp_hover_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        hover: $test_file:expr, $cursor:expr,
        contains: [ $( $expected_text:expr ),+ $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform hover
            let result = test.hover($test_file, $cursor);

            // Check expectation
            match result {
                Ok(Some(hover)) => {
                    let hover_text = match hover.contents {
                        tower_lsp::lsp_types::HoverContents::Markup(markup) => markup.value,
                        tower_lsp::lsp_types::HoverContents::Scalar(scalar) => match scalar {
                            tower_lsp::lsp_types::MarkedString::String(s) => s,
                            tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
                        },
                        tower_lsp::lsp_types::HoverContents::Array(arr) => {
                            arr.into_iter()
                                .map(|s| match s {
                                    tower_lsp::lsp_types::MarkedString::String(s) => s,
                                    tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                    };

                    // Check that all expected strings are present
                    $(
                        assert!(
                            hover_text.contains($expected_text),
                            "Expected hover text to contain '{}', but got: {}",
                            $expected_text,
                            hover_text
                        );
                    )+

                    println!("✅ {}: Hover text contains all expected strings", stringify!($test_name));
                    println!("   Hover text: {}", hover_text);
                }
                Ok(None) => panic!("Expected hover information, got None"),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };

    // Variant without expected text - just checks that hover returns something
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        hover: $test_file:expr, $cursor:expr
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform hover
            let result = test.hover($test_file, $cursor);

            // Check expectation
            match result {
                Ok(Some(hover)) => {
                    let hover_text = match hover.contents {
                        tower_lsp::lsp_types::HoverContents::Markup(markup) => markup.value,
                        tower_lsp::lsp_types::HoverContents::Scalar(scalar) => match scalar {
                            tower_lsp::lsp_types::MarkedString::String(s) => s,
                            tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
                        },
                        tower_lsp::lsp_types::HoverContents::Array(arr) => {
                            arr.into_iter()
                                .map(|s| match s {
                                    tower_lsp::lsp_types::MarkedString::String(s) => s,
                                    tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                    };

                    println!("✅ {}: Hover succeeded", stringify!($test_name));
                    println!("   Hover text: {}", hover_text);
                }
                Ok(None) => panic!("Expected hover information, got None"),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };
}

// Define a find-references test with inline file definitions
//
// # Example
//
// ```no_run
/// Defines a find-references test with inline file definitions.
///
/// lsp_references_test! {
///     name: references_for_method,
///     files: {
///         "com/example/Main.java" => r#"
///             package com.example;
///             public class Main {
///                 public void test() {
///                     new User().getNa<|>me();
///                 }
///             }
///         "#,
///         "com/example/User.java" => r#"
///             package com.example;
///             public class User {
///                 public String getName() { return ""; }
///             }
///         "#
///     },
///     refs: "com/example/Main.java", "<|>",
///     include_declaration: true,
///     min_count: 2,
///     expect_files: ["User.java", "Main.java"]
/// }
/// ```
#[macro_export]
macro_rules! lsp_references_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        refs: $test_file:expr, $cursor:expr,
        include_declaration: $include_declaration:expr,
        min_count: $min_count:expr,
        expect_files: [ $( $expected_file:expr ),+ $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            $(
                test.add_file($file_path, $file_content);
            )+

            let refs = test
                .find_references($test_file, $cursor, $include_declaration)
                .expect("find_references failed");

            assert!(
                refs.len() >= $min_count,
                "Expected at least {} references, found {}",
                $min_count,
                refs.len()
            );

            $(
                assert!(
                    refs.iter().any(|loc| loc.uri.as_str().contains($expected_file)),
                    "Expected at least one reference in file '{}', but got: {:?}",
                    $expected_file,
                    refs
                );
            )+

            println!(
                "✅ {}: Found {} references (min {})",
                stringify!($test_name),
                refs.len(),
                $min_count
            );
        }
    };

    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        refs: $test_file:expr, $cursor:expr,
        include_declaration: $include_declaration:expr,
        min_count: $min_count:expr
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            $(
                test.add_file($file_path, $file_content);
            )+

            let refs = test
                .find_references($test_file, $cursor, $include_declaration)
                .expect("find_references failed");

            assert!(
                refs.len() >= $min_count,
                "Expected at least {} references, found {}",
                $min_count,
                refs.len()
            );

            println!(
                "✅ {}: Found {} references (min {})",
                stringify!($test_name),
                refs.len(),
                $min_count
            );
        }
    };
}

// Define a signature help test with inline file definitions
//
// # Example
//
// ```no_run
/// Defines a signature-help test with inline file definitions.
///
/// lsp_signature_help_test! {
///     name: method_signature_help,
///     files: {
///         "com/example/Main.java" => r#"
///             package com.example;
///             import com.example.model.User;
///             public class Main {
///                 public void test() {
///                     User user = new User();
///                     user.setName(<|>
///                 }
///             }
///         "#,
///         "com/example/model/User.java" => r#"
///             package com.example.model;
///             public class User {
///                 public void setName(String name) {}
///             }
///         "#
///     },
///     signature_help: "com/example/Main.java", "<|>",
///     contains: ["setName", "String name"]
/// }
/// ```
#[macro_export]
macro_rules! lsp_signature_help_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        signature_help: $test_file:expr, $cursor:expr,
        contains: [ $( $expected_text:expr ),+ $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform signature help
            let result = test.signature_help($test_file, $cursor);

            // Check expectation
            match result {
                Ok(Some(sig_help)) => {
                    assert!(!sig_help.signatures.is_empty(), "Expected signatures, got empty list");

                    // Get the signature text from the first signature
                    let sig_text = &sig_help.signatures[0].label;

                    // Check that all expected strings are present
                    $(
                        assert!(
                            sig_text.contains($expected_text),
                            "Expected signature to contain '{}', but got: {}",
                            $expected_text,
                            sig_text
                        );
                    )+

                    println!("✅ {}: Signature help contains all expected strings", stringify!($test_name));
                    println!("   Signature: {}", sig_text);
                    if let Some(active_param) = sig_help.active_parameter {
                        println!("   Active parameter: {}", active_param);
                    }
                }
                Ok(None) => panic!("Expected signature help, got None"),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };

    // Variant without expected text - just checks that signature help returns something
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        signature_help: $test_file:expr, $cursor:expr
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform signature help
            let result = test.signature_help($test_file, $cursor);

            // Check expectation
            match result {
                Ok(Some(sig_help)) => {
                    assert!(!sig_help.signatures.is_empty(), "Expected signatures, got empty list");
                    let sig_text = &sig_help.signatures[0].label;
                    println!("✅ {}: Signature help succeeded", stringify!($test_name));
                    println!("   Signature: {}", sig_text);
                    if let Some(active_param) = sig_help.active_parameter {
                        println!("   Active parameter: {}", active_param);
                    }
                }
                Ok(None) => panic!("Expected signature help, got None"),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };

    // Variant for testing "no signature help" case (e.g., not in a method call)
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        signature_help: $test_file:expr, $cursor:expr,
        expect_none: true
    ) => {
        #[test]
        fn $test_name() {
            let mut test = $crate::lsp_test_framework::LspTest::new();

            // Add all files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform signature help
            let result = test.signature_help($test_file, $cursor);

            // Check expectation
            match result {
                Ok(None) => {
                    println!("✅ {}: Correctly returned None (not in method call)", stringify!($test_name));
                }
                Ok(Some(sig_help)) => {
                    panic!("Expected None, but got signature help: {:?}", sig_help.signatures[0].label);
                }
                Err(e) => panic!("Error: {}", e),
            }
        }
    };
}

// Macro for testing code completion
//
// # Basic Usage (just check completion works)
//
// ```ignore
// lsp_completion_test! {
//     name: basic_completion,
//     files: {
//         "Test.java" => "class Test { void test() { int x<|> = 0; } }"
//     },
//     complete: "Test.java", "<|>"
// }
// ```
//
// # With Assertions
//
// ```ignore
/// Defines a completion test with inline file definitions.
///
/// lsp_completion_test! {
///     name: member_access,
///     files: {
///         "User.java" => "class User { String getName() {} String getEmail() {} }",
///         "Main.java" => "class Main { void test(User u) { u.get<|> } }"
///     },
///     complete: "Main.java", "<|>",
///     contains: ["getName", "getEmail"],
///     not_contains: ["toString"]  // optional
/// }
/// ```
#[macro_export]
macro_rules! lsp_completion_test {
    // Basic version: just check that completion returns results
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        complete: $test_file:expr, $cursor:expr
    ) => {
        #[test]
        fn $test_name() {
            use $crate::lsp_test_framework::LspTest;

            let mut test = LspTest::new();

            // Add files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform completion
            let result = test.completion($test_file, $cursor);
            assert!(result.is_ok(), "Completion failed: {:?}", result.err());

            let completions = result.unwrap();
            eprintln!("Completions returned: {}", completions.len());
            for item in &completions {
                eprintln!("  - {} ({:?})", item.label, item.kind);
            }

            // Just check that we got some completions
            assert!(!completions.is_empty(), "Expected some completions");
        }
    };

    // With contains assertions
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        complete: $test_file:expr, $cursor:expr,
        contains: [ $( $expected:expr ),+ $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            use $crate::lsp_test_framework::LspTest;

            let mut test = LspTest::new();

            // Add files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform completion
            let result = test.completion($test_file, $cursor);
            assert!(result.is_ok(), "Completion failed: {:?}", result.err());

            let completions = result.unwrap();
            eprintln!("Completions returned: {}", completions.len());
            for item in &completions {
                eprintln!("  - {} ({:?})", item.label, item.kind);
            }

            // Check that expected items are present
            $(
                let found = completions.iter().any(|c| c.label == $expected);
                assert!(
                    found,
                    "Expected completion item '{}' not found. Available: {:?}",
                    $expected,
                    completions.iter().map(|c| &c.label).collect::<Vec<_>>()
                );
            )+
        }
    };

    // With contains and not_contains assertions
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        complete: $test_file:expr, $cursor:expr,
        contains: [ $( $expected:expr ),+ $(,)? ],
        not_contains: [ $( $not_expected:expr ),+ $(,)? ]
    ) => {
        #[test]
        fn $test_name() {
            use $crate::lsp_test_framework::LspTest;

            let mut test = LspTest::new();

            // Add files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform completion
            let result = test.completion($test_file, $cursor);
            assert!(result.is_ok(), "Completion failed: {:?}", result.err());

            let completions = result.unwrap();
            eprintln!("Completions returned: {}", completions.len());
            for item in &completions {
                eprintln!("  - {} ({:?})", item.label, item.kind);
            }

            // Check that expected items are present
            $(
                let found = completions.iter().any(|c| c.label == $expected);
                assert!(
                    found,
                    "Expected completion item '{}' not found. Available: {:?}",
                    $expected,
                    completions.iter().map(|c| &c.label).collect::<Vec<_>>()
                );
            )+

            // Check that unwanted items are NOT present
            $(
                let found = completions.iter().any(|c| c.label == $not_expected);
                assert!(
                    !found,
                    "Unexpected completion item '{}' found",
                    $not_expected
                );
            )+
        }
    };
}

// Completion test expecting NO results (for primitives, invalid contexts, etc.)
//
// # Example
//
// ```no_run
/// Defines a completion test that expects no results.
///
/// lsp_completion_none_test! {
///     name: test_primitive_no_members,
///     files: {
///         "Test.java" => r#"
///             class Test {
///                 void test() {
///                     int x = 0;
///                     x.<cursor>
///                 }
///             }
///         "#
///     },
///     complete: "Test.java", "<cursor>"
/// }
/// ```
#[macro_export]
macro_rules! lsp_completion_none_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        complete: $test_file:expr, $cursor:expr
    ) => {
        #[test]
        fn $test_name() {
            use $crate::lsp_test_framework::LspTest;

            let mut test = LspTest::new();

            // Add files
            $(
                test.add_file($file_path, $file_content);
            )+

            // Perform completion
            let result = test.completion($test_file, $cursor);
            assert!(result.is_ok(), "Completion failed: {:?}", result.err());

            let completions = result.unwrap();
            eprintln!("Completions returned: {}", completions.len());
            for item in &completions {
                eprintln!("  - {} ({:?})", item.label, item.kind);
            }

            // Expect ZERO completions
            assert!(
                completions.is_empty(),
                "Expected no completions, but got {}: {:?}",
                completions.len(),
                completions.iter().map(|c| &c.label).collect::<Vec<_>>()
            );
        }
    };
}
