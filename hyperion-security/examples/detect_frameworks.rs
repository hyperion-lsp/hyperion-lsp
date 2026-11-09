// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Example: Automatic framework detection
//!
//! Run this example to test framework detection on the example projects:
//!
//! ```bash
//! cargo run --example detect_frameworks -- ../example-projects/shopify-clone
//! cargo run --example detect_frameworks -- ../example-projects/sample-app
//! ```

use hyperion_security::frameworks::FrameworkRegistry;
use std::env;
use std::path::PathBuf;

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Get project root from command line args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <project-root>", args[0]);
        eprintln!();
        eprintln!("Example:");
        eprintln!("  {} /path/to/java-project", args[0]);
        std::process::exit(1);
    }

    let project_root = PathBuf::from(&args[1]);

    if !project_root.exists() {
        eprintln!(
            "Error: Project root does not exist: {}",
            project_root.display()
        );
        std::process::exit(1);
    }

    println!("Detecting frameworks in: {}", project_root.display());
    println!();

    // Create registry with built-in frameworks
    let mut registry = FrameworkRegistry::with_builtin();

    // Run detection
    let results = registry.detect_frameworks(&project_root);

    // Display results
    println!("Detection Results:");
    println!("{:-<80}", "");

    for result in &results {
        let status = if result.confidence.exceeds_threshold() {
            "ENABLED"
        } else {
            "not detected"
        };

        println!(
            "{:<20} | Confidence: {:.2} | {}",
            result.framework_name,
            result.confidence.value(),
            status
        );

        if !result.evidence.dependencies.is_empty() {
            println!("  Dependencies: {:?}", result.evidence.dependencies);
        }
        if !result.evidence.imports.is_empty() {
            println!("  Imports: {} unique", result.evidence.imports.len());
        }
        if !result.evidence.annotations.is_empty() {
            println!("  Annotations: {:?}", result.evidence.annotations);
        }
    }

    println!("{:-<80}", "");
    println!();
    println!("Enabled frameworks: {:?}", registry.enabled_frameworks());
}
