// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for method-level symbol resolution

use hyperion_workspace::WorkspaceIndex;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

/// Test that method resolution is deterministic across multiple invocations
///
/// This test verifies that querying for overloaded methods returns the same
/// results in the same order every time, preventing non-deterministic HashMap
/// iteration bugs.
///
/// Runs 1000 iterations to catch non-deterministic behavior that might only
/// appear intermittently.
#[tokio::test]
async fn test_method_resolution_determinism() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a class with many overloaded methods
    let file = root.join("Calculator.java");
    fs::write(
        &file,
        r#"
public class Calculator {
    public int add(int a, int b) { return a + b; }
    public double add(double a, double b) { return a + b; }
    public long add(long a, long b) { return a + b; }
    public float add(float a, float b) { return a + b; }
    public short add(short a, short b) { return (short)(a + b); }

    public int multiply(int a, int b) { return a * b; }
    public double multiply(double a, double b) { return a * b; }
    public long multiply(long a, long b) { return a * b; }

    public String format(String fmt) { return fmt; }
    public String format(String fmt, Object... args) { return fmt; }
    public String format(String fmt, int value) { return fmt; }
}
    "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file).await.unwrap();

    // Run the query 1000 times and verify identical results
    let mut previous_add_fqns: Option<Vec<Arc<str>>> = None;
    let mut previous_multiply_fqns: Option<Vec<Arc<str>>> = None;
    let mut previous_format_fqns: Option<Vec<Arc<str>>> = None;

    for iteration in 0..1000 {
        // Query for overloaded "add" methods
        let add_results = index.find_methods_by_name("Calculator", "add");
        let add_fqns: Vec<Arc<str>> = add_results.iter().map(|s| s.fqn.clone()).collect();

        // Query for overloaded "multiply" methods
        let multiply_results = index.find_methods_by_name("Calculator", "multiply");
        let multiply_fqns: Vec<Arc<str>> = multiply_results.iter().map(|s| s.fqn.clone()).collect();

        // Query for overloaded "format" methods
        let format_results = index.find_methods_by_name("Calculator", "format");
        let format_fqns: Vec<Arc<str>> = format_results.iter().map(|s| s.fqn.clone()).collect();

        // First iteration: save results
        if iteration == 0 {
            assert_eq!(add_fqns.len(), 5, "Should find 5 add() overloads");
            assert_eq!(multiply_fqns.len(), 3, "Should find 3 multiply() overloads");
            assert_eq!(format_fqns.len(), 3, "Should find 3 format() overloads");

            previous_add_fqns = Some(add_fqns);
            previous_multiply_fqns = Some(multiply_fqns);
            previous_format_fqns = Some(format_fqns);
        } else {
            // Subsequent iterations: verify identical results
            assert_eq!(
                add_fqns,
                *previous_add_fqns.as_ref().unwrap(),
                "add() methods returned different order on iteration {}",
                iteration
            );
            assert_eq!(
                multiply_fqns,
                *previous_multiply_fqns.as_ref().unwrap(),
                "multiply() methods returned different order on iteration {}",
                iteration
            );
            assert_eq!(
                format_fqns,
                *previous_format_fqns.as_ref().unwrap(),
                "format() methods returned different order on iteration {}",
                iteration
            );
        }
    }
}
