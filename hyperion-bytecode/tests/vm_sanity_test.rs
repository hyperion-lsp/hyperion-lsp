// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! VM Sanity Tests - Comprehensive bytecode interpreter validation
//!
//! These tests execute real Groovy code through the full pipeline:
//! Groovy source → AST → Bytecode → VM execution → Value verification
//!
//! Goal: Identify what actually works vs. what's broken.

use hyperion_bytecode::groovy_producer::GroovyBytecodeProducer;
use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

/// Execute Groovy source and return the VM alongside the result so assertions
/// can inspect boxed values rather than accepting any object reference.
fn execute_groovy_with_vm(source: &str) -> Result<(VirtualMachine, Value), String> {
    let producer = GroovyBytecodeProducer::new();

    // Parse source
    producer
        .parse(source, "Test.groovy")
        .map_err(|e| format!("Parse failed: {:?}", e))?;

    // Compile to bytecode
    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    let mut all_classes: Vec<_> = producer
        .produce_all_classes(&context)
        .map_err(|e| format!("Bytecode generation failed: {:?}", e))?
        .into_iter()
        .collect();
    all_classes.sort_by(|left, right| left.0.cmp(&right.0));

    // Create VM and load all classes
    let mut vm = VirtualMachine::new(MemoryConfig::default());

    let report = vm.load_classes_from_bytes(&all_classes);
    if !report.failures.is_empty() {
        return Err(format!(
            "Failed to load generated classes: {:#?}",
            report.failures
        ));
    }

    // Execute the run() method
    let result = vm
        .invoke_static("Test", "run", "()Ljava/lang/Object;", &[])
        .map_err(|e| format!("Execution failed: {:?}", e))?;
    Ok((vm, result))
}

fn execute_groovy_int(source: &str) -> Result<i32, String> {
    let (vm, result) = execute_groovy_with_vm(source)?;
    vm.unbox_int(&result)
        .ok_or_else(|| format!("Expected an integer result, got {result:?}"))
}

fn execute_groovy_string(source: &str) -> Result<String, String> {
    let (vm, result) = execute_groovy_with_vm(source)?;
    let Value::Reference(reference) = result else {
        return Err(format!("Expected a String reference, got {result:?}"));
    };
    let id = reference
        .heap_id()
        .ok_or_else(|| format!("Expected a non-null String reference, got {reference:?}"))?;
    vm.heap_string(id)
        .map_err(|error| format!("Expected a String result: {error:?}"))
}

// ============================================================================
// TEST 1: Arithmetic - Operator precedence
// ============================================================================

#[test]
fn test_arithmetic_precedence() {
    let source = "return 2 + 3 * 4";
    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 14);
}

// ============================================================================
// TEST 2: Variables - Assignment and retrieval
// ============================================================================

#[test]
fn test_variable_assignment() {
    let source = r#"
        def x = 10
        def y = 20
        return x + y
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 30);
}

// ============================================================================
// TEST 3: Loops - For-in range iteration
// ============================================================================

#[test]
fn test_loop_sum() {
    let source = r#"
        def sum = 0
        for (i in 1..10) {
            sum += i
        }
        return sum
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 55);
}

#[test]
fn test_descending_exclusive_range_loop() {
    let source = r#"
        def sum = 0
        for (i in 5..<1) {
            sum += i
        }
        return sum
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 14);
}

// ============================================================================
// TEST 4: String length - Method call on literal
// ============================================================================

#[test]
fn test_string_length() {
    let source = r#"return "hello".length()"#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 5);
}

// ============================================================================
// TEST 5: String concatenation - Plus operator
// ============================================================================

#[test]
fn test_string_concat() {
    let source = r#"return "Hello" + " " + "World""#;

    assert_eq!(
        execute_groovy_string(source).expect("Execution failed"),
        "Hello World"
    );
}

// ============================================================================
// TEST 6: Conditionals - Ternary operator
// ============================================================================

#[test]
fn test_ternary_operator() {
    let source = r#"
        def x = 10
        return x > 5 ? "big" : "small"
    "#;

    assert_eq!(
        execute_groovy_string(source).expect("Execution failed"),
        "big"
    );
}

// ============================================================================
// TEST 7: Lists - Size method
// ============================================================================

#[test]
fn test_list_size() {
    let source = r#"
        def list = [1, 2, 3]
        return list.size()
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 3);
}

// ============================================================================
// TEST 8: Method definition and call
// ============================================================================

#[test]
fn test_method_definition() {
    let source = r#"
        def add(a, b) {
            return a + b
        }
        return add(5, 7)
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 12);
}

// ============================================================================
// TEST 9: Closures - Basic closure execution
// ============================================================================

#[test]
fn test_closure_basic() {
    let source = r#"
        def add = { a, b -> a + b }
        return add(3, 4)
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 7);
}

// ============================================================================
// TEST 10: Closure variable capture
// ============================================================================

#[test]
fn test_closure_capture() {
    let source = r#"
        def multiplier = 3
        def closure = { x -> x * multiplier }
        return closure(5)
    "#;

    assert_eq!(execute_groovy_int(source).expect("Execution failed"), 15);
}

// ============================================================================
// SUMMARY TEST: Run all tests and report status
// ============================================================================

#[test]
fn test_summary() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║          VM SANITY TEST SUMMARY                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    enum Expected<'a> {
        Int(i32),
        String(&'a str),
    }

    let tests = vec![
        (
            "Arithmetic precedence",
            "return 2 + 3 * 4",
            Expected::Int(14),
        ),
        (
            "Variable assignment",
            "def x = 10; def y = 20; return x + y",
            Expected::Int(30),
        ),
        (
            "Loop iteration",
            "def sum = 0; for (i in 1..10) { sum += i }; return sum",
            Expected::Int(55),
        ),
        (
            "String.length()",
            r#"return "hello".length()"#,
            Expected::Int(5),
        ),
        (
            "String concat",
            r#"return "Hello" + " " + "World""#,
            Expected::String("Hello World"),
        ),
        (
            "Ternary operator",
            "def x = 10; return x > 5 ? \"big\" : \"small\"",
            Expected::String("big"),
        ),
        (
            "List.size()",
            "def list = [1, 2, 3]; return list.size()",
            Expected::Int(3),
        ),
        (
            "Method definition",
            "def add(a, b) { return a + b }; return add(5, 7)",
            Expected::Int(12),
        ),
        (
            "Closure basic",
            "def add = { a, b -> a + b }; return add(3, 4)",
            Expected::Int(7),
        ),
        (
            "Closure capture",
            "def multiplier = 3; def closure = { x -> x * multiplier }; return closure(5)",
            Expected::Int(15),
        ),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (name, source, expected) in tests {
        match execute_groovy_with_vm(source) {
            Ok((vm, result)) => {
                let valid = match expected {
                    Expected::Int(expected) => vm.unbox_int(&result) == Some(expected),
                    Expected::String(expected) => result
                        .as_reference()
                        .ok()
                        .and_then(|reference| reference.heap_id())
                        .and_then(|id| vm.heap_string(id).ok())
                        .is_some_and(|actual| actual == expected),
                };
                if valid {
                    println!("  ✅  {} - PASS", name);
                    passed += 1;
                } else {
                    println!("  ❌  {} - FAIL (wrong value: {:?})", name, result);
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  ❌  {} - ERROR: {}", name, e);
                failed += 1;
            }
        }
    }

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  RESULTS: {} passed, {} failed", passed, failed);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    assert_eq!(failed, 0, "{failed} VM sanity checks failed");
}
