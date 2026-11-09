// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Basic Gradle Executor Test

use hyperion_bytecode::gradle_executor::GradleExecutor;

#[test]
fn test_create_executor() {
    println!("Creating executor...");
    let _executor = GradleExecutor::new();
    println!("✅ Executor created successfully");
}

#[test]
fn test_execute_empty_source() {
    println!("Testing empty source...");
    let mut executor = GradleExecutor::new();
    let result = executor.execute_source("");
    println!("Result: {:?}", result);
    assert!(result.is_ok());
    println!("✅ Empty source executed");
}

#[test]
fn test_execute_simple_plugin() {
    println!("Testing simple plugin...");
    let mut executor = GradleExecutor::new();

    let source = r#"
        plugins {
            id 'java'
        }
    "#;

    println!("Executing source...");
    let result = executor.execute_source(source);
    println!("Result: {:?}", result);

    assert!(result.is_ok());
    let config = result.unwrap();
    println!(
        "Config: plugins={}, repos={}, deps={}",
        config.plugin_definitions.len(),
        config.repository_list.len(),
        config.dependency_list.len()
    );

    assert!(config.has_plugin("java"));
    println!("✅ Simple plugin test passed");
}
