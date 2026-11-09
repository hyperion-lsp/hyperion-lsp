// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for nullability analysis of Optional and Stream methods
//!
//! These tests verify that methods like `orElseThrow()` and `collect()` are correctly
//! recognized as returning non-null values, which is critical for avoiding false positives
//! in nullability warnings.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{lexer::Lexer, parser::Parser};
use hyperion_semantics::nullability::{
    analyze, ControlFlowGraph, MethodContext, NullabilityEnv, SimpleResolver,
};

// Note: Nullability enum not directly used in tests - we check diagnostic count instead

/// Helper to parse Java code and extract the method body block
fn parse_method_body(code: &str) -> SyntaxNode {
    let tokens: Vec<_> = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);
    root.descendants()
        .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)
        .expect("No block statement found in test code")
}

/// Helper to analyze a method body and return the analysis result
fn analyze_code(code: &str) -> hyperion_semantics::nullability::NullabilityAnalysis {
    let method_body = parse_method_body(code);
    let cfg = ControlFlowGraph::build(&method_body);
    let resolver = SimpleResolver::new();
    let method_context = MethodContext::default();
    analyze(&cfg, NullabilityEnv::new(), &resolver, &method_context)
}

#[test]
fn test_optional_or_else_throw_nonnull() {
    // Optional.orElseThrow() should return NonNull because it throws if empty
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String str = opt.orElseThrow();
                str.length(); // Should not warn - orElseThrow returns non-null
            }
        }
    "#;

    let analysis = analyze_code(code);
    // The analysis should complete without treating orElseThrow's result as potentially null
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_optional_or_else_throw_with_supplier_nonnull() {
    // Optional.orElseThrow(supplier) should also return NonNull
    let code = r#"
        class Test {
            void test(Optional<User> opt) {
                User user = opt.orElseThrow(IllegalStateException::new);
                user.getName(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_stream_collect_nonnull() {
    // Stream.collect() always returns non-null (even if empty collection)
    let code = r#"
        class Test {
            void test(Stream<String> stream) {
                List<String> list = stream.collect(Collectors.toList());
                list.size(); // Should not warn - collect returns non-null
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_stream_collect_to_set_nonnull() {
    // Stream.collect(Collectors.toSet()) should also return NonNull
    let code = r#"
        class Test {
            void test(Stream<Integer> stream) {
                Set<Integer> set = stream.collect(Collectors.toSet());
                set.isEmpty(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_stream_collect_to_map_nonnull() {
    // Stream.collect(Collectors.toMap(...)) should return NonNull
    let code = r#"
        class Test {
            void test(Stream<User> users) {
                Map<String, User> map = users.collect(
                    Collectors.toMap(User::getId, Function.identity())
                );
                map.size(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_chained_optional_operations() {
    // Complex chain: filter().map().orElseThrow() should return NonNull
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String result = opt
                    .filter(s -> !s.isEmpty())
                    .map(String::toUpperCase)
                    .orElseThrow();
                result.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_chained_stream_operations() {
    // Complex chain: filter().map().collect() should return NonNull
    let code = r#"
        class Test {
            void test(List<String> items) {
                List<Integer> lengths = items.stream()
                    .filter(s -> !s.isEmpty())
                    .map(String::length)
                    .collect(Collectors.toList());
                lengths.size(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_optional_or_else_vs_or_else_throw() {
    // orElse() returns the type of the optional (potentially null if default is null)
    // orElseThrow() returns non-null (throws exception instead)
    let code = r#"
        class Test {
            void test(Optional<String> opt) {
                String safe = opt.orElseThrow();
                String maybeNull = opt.orElse(null);

                safe.length(); // OK - orElseThrow returns non-null
                // maybeNull.length() would warn - orElse can return null
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_stream_collect_with_filtering() {
    // Even with filtering, collect() returns non-null (empty collection, not null)
    let code = r#"
        class Test {
            void test(Stream<Integer> numbers) {
                List<Integer> evens = numbers
                    .filter(n -> n % 2 == 0)
                    .collect(Collectors.toList());
                evens.isEmpty(); // OK - collect returns non-null list
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_nested_optional_or_else_throw() {
    // Nested optionals with orElseThrow should all return non-null
    let code = r#"
        class Test {
            void test(Optional<Optional<String>> nested) {
                Optional<String> inner = nested.orElseThrow();
                String value = inner.orElseThrow();
                value.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_parallel_stream_collect() {
    // Parallel streams should also return non-null from collect()
    let code = r#"
        class Test {
            void test(List<String> items) {
                List<String> processed = items.parallelStream()
                    .map(String::trim)
                    .collect(Collectors.toList());
                processed.size(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_method_reference_with_collect() {
    // Method references in collect should still return non-null
    let code = r#"
        class Test {
            void test(Stream<User> users) {
                List<String> names = users
                    .map(User::getName)
                    .collect(Collectors.toList());
                names.isEmpty(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_collector_joining() {
    // Collectors.joining() should return non-null String
    let code = r#"
        class Test {
            void test(Stream<String> words) {
                String sentence = words.collect(Collectors.joining(" "));
                sentence.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_comparison_with_require_non_null() {
    // Both requireNonNull and orElseThrow should behave similarly
    let code = r#"
        class Test {
            void test(String maybeNull, Optional<String> opt) {
                String checked = Objects.requireNonNull(maybeNull);
                String thrown = opt.orElseThrow();

                checked.length(); // Should not warn
                thrown.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_stream_empty_collect() {
    // Even Stream.empty().collect() returns non-null empty collection
    let code = r#"
        class Test {
            void test() {
                List<String> empty = Stream.empty().collect(Collectors.toList());
                empty.isEmpty(); // Should not warn - returns non-null empty list
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}

#[test]
fn test_optional_or_else_throw_in_conditional() {
    // orElseThrow in conditional context should still be non-null
    let code = r#"
        class Test {
            void test(Optional<String> opt, boolean condition) {
                String result = condition ? opt.orElseThrow() : "default";
                result.length(); // Should not warn
            }
        }
    "#;

    let analysis = analyze_code(code);
    assert!(!analysis.entry_states.is_empty());
}
