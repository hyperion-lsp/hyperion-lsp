// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for code lens functionality
//!
//! Code lenses provide inline actionable information above code elements:
//! - "Run" buttons above executable Java entry points
//!
//! Implementation-count lenses are suppressed until their command can perform
//! a portable client action.

use hyperion_lsp::code_lens::CodeLensProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::FileId;
use std::sync::Arc;
use tower_lsp::lsp_types::CodeLens;

/// Helper to get code lenses for Java source code
fn get_code_lenses(source: &str) -> Vec<CodeLens> {
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let provider = CodeLensProvider::new();

    let result = provider.code_lens(&host.read_now(), file_id, source);
    match result {
        Ok(lenses) => lenses,
        Err(e) => {
            eprintln!("Code lens error: {e}");
            vec![]
        }
    }
}

#[test]
fn implementation_counts_do_not_advertise_a_nonfunctional_command() {
    let interface_source = r"package com.example;

public interface Service {
    void run();
}";
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(interface_source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let provider = CodeLensProvider::new();
    let lenses = provider
        .code_lens(&host.read_now(), file_id, interface_source)
        .unwrap();
    assert!(
        get_lens_titles(&lenses).is_empty(),
        "a code lens must not advertise an action that only logs when invoked"
    );
}

/// Extract command titles from code lenses
fn get_lens_titles(lenses: &[CodeLens]) -> Vec<String> {
    lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref())
        .map(|cmd| cmd.title.clone())
        .collect()
}

// An action is a product promise. Until Hyperion has a real test
// framework runner, neither annotations nor naming heuristics may create a
// command that looks executable.

#[test]
fn test_junit_method_does_not_advertise_unsupported_run_test() {
    let source = r"class MyTest {
    @org.junit.jupiter.api.Test
    void testCalculation() {}
}";

    let titles = get_lens_titles(&get_code_lenses(source));
    assert!(
        titles.is_empty(),
        "unsupported test execution must not be advertised: {titles:?}"
    );
}

#[test]
fn test_test_naming_heuristics_do_not_advertise_unsupported_actions() {
    let source = r"class MyTest {
    void testFirst() {}
    void calculate_should_work() {}
}";

    let titles = get_lens_titles(&get_code_lenses(source));
    assert!(
        titles.is_empty(),
        "method names alone must not create unsupported actions: {titles:?}"
    );
}

#[test]
fn test_test_class_with_main_only_advertises_real_main_action() {
    let source = r"class MyTest {
    public static void main(String[] args) {}

    @org.junit.jupiter.api.Test
    void shouldDoSomething() {}
}";

    let titles = get_lens_titles(&get_code_lenses(source));
    assert_eq!(titles, vec!["▶ Run"]);
}

// ============================================================================
// "▶ Run" lens (public static void main) -- end-to-end through
// the public `CodeLensProvider::code_lens` entry point (see `code_lens.rs`'s
// own unit tests for the exhaustive `is_main_method` signature-rejection
// matrix).
// ============================================================================

#[test]
fn test_run_lens_for_main_method() {
    let source = r#"public class Main {
    public static void main(String[] args) {
        System.out.println("hi");
    }
}"#;
    let lenses = get_code_lenses(source);
    let titles = get_lens_titles(&lenses);
    assert!(
        titles.iter().any(|t| t == "▶ Run"),
        "Expected a '▶ Run' lens for `public static void main`, got: {titles:?}"
    );
}

#[test]
fn test_no_run_lens_for_non_main_method() {
    let source = r"public class Helper {
    public static void notMain(String[] args) {}
}";
    let lenses = get_code_lenses(source);
    let titles = get_lens_titles(&lenses);
    assert!(
        !titles.iter().any(|t| t == "▶ Run"),
        "Should not emit a '▶ Run' lens for a method named something other than `main`, got: {titles:?}"
    );
}

#[test]
fn test_no_run_lens_for_wrong_main_signature() {
    // `main(String args)` (no array) is the classic near-miss: it compiles
    // but the JVM (and this lens) must not treat it as an entry point.
    let source = r"public class Main {
    public static void main(String args) {}
}";
    let lenses = get_code_lenses(source);
    let titles = get_lens_titles(&lenses);
    assert!(
        !titles.iter().any(|t| t == "▶ Run"),
        "main(String args) (non-array) must not produce a '▶ Run' lens, got: {titles:?}"
    );
}
