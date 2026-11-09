// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Basic usage example of the Hyperion semantic database.
//!
//! This example demonstrates:
//! - Creating a database
//! - Adding file contents
//! - Querying for symbols
//! - Incremental updates

use hyperion_semantics::db::{Database, SemanticDatabase, SourceDatabase};
use hyperion_semantics::FileId;
use std::sync::Arc;

fn main() {
    // Initialize the database
    println!("Creating Hyperion semantic database...\n");
    let mut db = Database::new();

    // Example 1: Add a simple Java file
    let file1 = FileId::new(1);
    let source1 = r#"
public class HelloWorld {
    public static void main(String[] args) {
        System.out.println("Hello, World!");
    }
}
"#;

    println!("=== Example 1: Simple Class ===");
    println!("Source:\n{source1}");

    db.set_file_text(file1, Arc::new(source1.to_string()));

    let symbols1 = db.file_symbols(file1);
    println!("\nFound {} symbol(s):", symbols1.symbols.len());
    for symbol in symbols1.symbols.iter() {
        println!("  - {:?}: {}", symbol.kind, symbol.name);
    }

    // Example 2: Add a file with multiple classes
    let file2 = FileId::new(2);
    let source2 = r"
public class Vehicle {
    private String brand;
}

interface Drivable {
    void drive();
}

class Car {
    // Car implementation
}
";

    println!("\n=== Example 2: Multiple Types ===");
    println!("Source:\n{source2}");

    db.set_file_text(file2, Arc::new(source2.to_string()));

    let symbols2 = db.file_symbols(file2);
    println!("\nFound {} symbol(s):", symbols2.symbols.len());
    for symbol in symbols2.symbols.iter() {
        println!("  - {:?}: {}", symbol.kind, symbol.name);
    }

    // Example 3: Incremental update
    println!("\n=== Example 3: Incremental Update ===");
    println!("Modifying file1...");

    let updated_source1 = r#"
public class HelloWorld {
    public static void main(String[] args) {
        System.out.println("Hello, Hyperion!");
    }
}

class AnotherClass {
    // New class added
}
"#;

    db.set_file_text(file1, Arc::new(updated_source1.to_string()));

    let updated_symbols1 = db.file_symbols(file1);
    println!(
        "\nFile1 now has {} symbol(s):",
        updated_symbols1.symbols.len()
    );
    for symbol in updated_symbols1.symbols.iter() {
        println!("  - {:?}: {}", symbol.kind, symbol.name);
    }

    // Verify that file2's symbols are still cached (not recomputed)
    let cached_symbols2 = db.file_symbols(file2);
    println!(
        "\nFile2 still has {} symbol(s) (from cache):",
        cached_symbols2.symbols.len()
    );

    // Demonstrate that Arc pointers are the same (proving it's cached)
    if Arc::ptr_eq(&symbols2, &cached_symbols2) {
        println!("✓ File2 symbols were retrieved from cache (no recomputation)");
    }

    // Example 4: Empty file
    println!("\n=== Example 4: Empty File ===");
    let file3 = FileId::new(3);
    db.set_file_text(file3, Arc::new(String::new()));

    let symbols3 = db.file_symbols(file3);
    println!("Empty file has {} symbol(s)", symbols3.symbols.len());

    println!("\n=== Summary ===");
    println!("Demonstrated:");
    println!("  ✓ Database creation and file management");
    println!("  ✓ Symbol extraction from source code");
    println!("  ✓ Incremental updates (only modified files recompute)");
    println!("  ✓ Query memoization (cached results)");
    println!("\nThis is a foundation for building a full-featured LSP!");
}
