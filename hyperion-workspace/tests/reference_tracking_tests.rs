// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for cross-file reference tracking
//!
//! These tests verify that the workspace index properly tracks symbol usages
//! (where symbols are referenced), enabling find-references functionality.

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::query::QueryEngine;
use std::fs;
use tempfile::TempDir;

/// Test that workspace index can store and retrieve symbol usages
#[tokio::test]
async fn test_index_stores_symbol_usages() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a simple enum definition
    let enum_file = root.join("com/example/ResourceType.java");
    fs::create_dir_all(enum_file.parent().unwrap()).unwrap();
    fs::write(
        &enum_file,
        r#"package com.example;

public enum ResourceType {
    UNKNOWN,
    USER,
    POST
}
"#,
    )
    .unwrap();

    // Create a file that uses the enum
    let usage_file = root.join("com/example/Service.java");
    fs::write(
        &usage_file,
        r#"package com.example;

public class Service {
    public void process() {
        ResourceType type = ResourceType.UNKNOWN;
        if (type == ResourceType.USER) {
            System.out.println("User");
        }
    }
}
"#,
    )
    .unwrap();

    // Build the index
    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    // Should have indexed 2 files
    assert!(stats.files_indexed >= 2, "Should index at least 2 files");

    // Should have found the enum and class
    assert!(
        stats.symbols_indexed >= 4,
        "Should find enum constants and class"
    );

    // Find usages of ResourceType.UNKNOWN
    let fqn = "com.example.ResourceType.UNKNOWN";
    let usages = index.find_usages(fqn);

    // Should find at least 1 usage in Service.java
    assert!(
        !usages.is_empty(),
        "Should find at least one usage of ResourceType.UNKNOWN in Service.java"
    );

    // Verify the usage is in the correct file
    let usage_file_id = index
        .get_file_id(&usage_file)
        .expect("Service.java should be indexed");
    let found_in_usage_file = usages.iter().any(|loc| loc.file_id == usage_file_id);

    assert!(found_in_usage_file, "Usage should be found in Service.java");
}

/// Test that QueryEngine.find_references returns both definition and usages
#[tokio::test]
async fn test_query_engine_find_references() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create enum
    let enum_file = root.join("ResourceType.java");
    fs::write(
        &enum_file,
        r#"public enum ResourceType {
    UNKNOWN,
    USER
}
"#,
    )
    .unwrap();

    // Create usage file
    let client_file = root.join("Client.java");
    fs::write(
        &client_file,
        r#"public class Client {
    public void test() {
        ResourceType type = ResourceType.UNKNOWN;
        service.process(ResourceType.UNKNOWN);
    }
}
"#,
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    // Create query engine
    let query = QueryEngine::new(index.clone());

    // Find references to UNKNOWN
    let refs = query.find_references("ResourceType.UNKNOWN").unwrap();

    // Should find:
    // 1. The definition in ResourceType.java
    // 2. First usage in Client.java (line 3)
    // 3. Second usage in Client.java (line 4)
    assert!(
        refs.len() >= 3,
        "Should find definition + 2 usages, found {}",
        refs.len()
    );

    // Verify we have references in both files
    let unique_files: std::collections::HashSet<_> = refs.iter().map(|r| r.file_id).collect();
    assert_eq!(unique_files.len(), 2, "Should have references in 2 files");
}

/// Test extraction of qualified member access (EnumType.CONSTANT)
///
/// A usage record's key is the FQN of the declaration it resolves to, so this
/// fixture must actually declare `ResourceType`. It did not: the file below was
/// the only file in the workspace, `ResourceType` was undeclared, and the test
/// passed only while `extract_field_access` fabricated an identity for an
/// unresolved receiver -- it guessed the current package for the receiver
/// (removed in 51ea05b5, "retire fabricated Java identities") and then textually
/// concatenated `receiver.field` into an FQN with a synthesised `SymbolKind::FIELD`
/// (removed in ee73ad36, "resolve every field chain owner"). Both removals are
/// correct: an FQN that no declaration answers to poisons find-references, rename
/// and safe-delete, which is the class of defect  catalogued. The
/// contract under test -- that all three qualified-access syntactic positions
/// (initializer, method argument, `==` operand) yield usages -- is unchanged;
/// only the missing declaration is added. `undeclared_receiver_is_not_a_usage`
/// below pins the removal itself so it cannot silently regress.
#[tokio::test]
async fn test_qualified_access_extraction() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let enum_file = root.join("ResourceType.java");
    fs::write(
        &enum_file,
        r#"public enum ResourceType {
    UNKNOWN,
    USER,
    POST
}
"#,
    )
    .unwrap();

    // Create a file with various qualified access patterns
    let test_file = root.join("Test.java");
    fs::write(
        &test_file,
        r#"public class Test {
    public void test() {
        // Qualified enum access
        ResourceType type1 = ResourceType.UNKNOWN;

        // Method call with qualified access
        process(ResourceType.USER);

        // Comparison with qualified access
        if (type1 == ResourceType.POST) {
            System.out.println("Post");
        }

        // Nested qualified access
        Map.Entry<String, String> entry = null;
    }
}
"#,
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    // Should extract usages of ResourceType.UNKNOWN, ResourceType.USER, ResourceType.POST
    let unknown_usages = index.find_usages("ResourceType.UNKNOWN");
    let user_usages = index.find_usages("ResourceType.USER");
    let post_usages = index.find_usages("ResourceType.POST");

    assert!(!unknown_usages.is_empty(), "Should find UNKNOWN usage");
    assert!(!user_usages.is_empty(), "Should find USER usage");
    assert!(!post_usages.is_empty(), "Should find POST usage");
}

/// A qualified access whose receiver resolves to nothing must record no usage at
/// all, rather than a usage keyed by the textual `Receiver.member` spelling.
///
/// This is the deliberate contract established by ee73ad36 and 51ea05b5. A
/// fabricated key is worse than a missing one: nothing declares it, so it can
/// only ever collide with an unrelated declaration that happens to share the
/// spelling, and every consumer of `find_usages` (find-references, rename,
/// safe-delete) would then act on a location that is not a use of the symbol.
#[tokio::test]
async fn undeclared_receiver_is_not_a_usage() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // `Undeclared` exists nowhere in this workspace.
    fs::write(
        root.join("Test.java"),
        r#"public class Test {
    public void test() {
        Object value = Undeclared.CONSTANT;
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    assert!(
        index.find_usages("Undeclared.CONSTANT").is_empty(),
        "an unresolved receiver must not fabricate a usage identity"
    );
}

/// Test that method references work across files
/// This is a regression test for the FQN mismatch bug where:
/// - Definitions use: `com.example.User.getName()` (with parens)
/// - Usages use: `com.example.User.getName` (without parens)
#[tokio::test]
async fn test_method_references_cross_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a class with a method
    let user_file = root.join("com/example/model/User.java");
    fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    fs::write(
        &user_file,
        r#"package com.example.model;

public class User {
    private String name;

    public String getName() {
        return name;
    }
}
"#,
    )
    .unwrap();

    // Create a file that uses the method
    let service_file = root.join("com/example/Service.java");
    fs::create_dir_all(service_file.parent().unwrap()).unwrap();
    fs::write(
        &service_file,
        r#"package com.example;

import com.example.model.User;

public class Service {
    public void process(User user) {
        String name = user.getName();
        System.out.println(user.getName());
    }
}
"#,
    )
    .unwrap();

    // Build the index
    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    // Should have indexed 2 files
    assert!(stats.files_indexed >= 2, "Should index at least 2 files");

    // Create query engine
    let query = QueryEngine::new(index.clone());

    // Check what FQN is used for the method definition
    // The definition should be stored with FQN like "com.example.model.User.getName()"
    let method_def = index.find_symbol("com.example.model.User.getName()");
    println!("Method definition with parens: {:?}", method_def.is_some());

    let method_def_no_parens = index.find_symbol("com.example.model.User.getName");
    println!(
        "Method definition without parens: {:?}",
        method_def_no_parens.is_some()
    );

    // Check what FQN is used for usages
    let usages_with_parens = index.find_usages("com.example.model.User.getName()");
    println!("Usages with parens: {}", usages_with_parens.len());

    let usages_no_parens = index.find_usages("com.example.model.User.getName");
    println!("Usages without parens: {}", usages_no_parens.len());

    // The bug is that definitions use parens but usages don't, causing mismatch
    // FQN format should be consistent:
    // - Either both use parens: "com.example.model.User.getName()"
    // - Or both use no parens: "com.example.model.User.getName"

    // Find references - this should return both definition and usages
    // Using the FQN with parens (as stored in definitions)
    let refs = query
        .find_references("com.example.model.User.getName()")
        .unwrap();

    println!(
        "References found: {} (expected >= 3: definition + 2 usages)",
        refs.len()
    );

    // Should find:
    // 1. The definition in User.java
    // 2. First usage in Service.java (user.getName())
    // 3. Second usage in Service.java (user.getName())
    assert!(
        refs.len() >= 3,
        "Should find definition + 2 usages for method, found {}. \
         This indicates FQN mismatch between definitions and usages.",
        refs.len()
    );
}

/// Test bidirectional FQN lookup in find_symbol and find_usages
/// This ensures the index can find symbols/usages regardless of whether
/// the query uses parameterized or base FQN format.
#[tokio::test]
async fn test_bidirectional_fqn_lookup() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a class with methods (with parameters)
    let user_file = root.join("com/example/User.java");
    fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    fs::write(
        &user_file,
        r#"package com.example;

public class User {
    public String getName() { return ""; }
    public void setName(String name) { this.name = name; }
    public void process(String a, int b) {}
}
"#,
    )
    .unwrap();

    // Create a file that uses the methods
    let service_file = root.join("com/example/Service.java");
    fs::write(
        &service_file,
        r#"package com.example;

public class Service {
    public void test(User user) {
        user.getName();
        user.setName("John");
        user.process("test", 42);
    }
}
"#,
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    // Test find_symbol bidirectional lookup:
    // Query WITH params should find symbol (or fall back to base if stored without)
    let sym_with_params = index.find_symbol("com.example.User.getName()");
    let sym_no_params = index.find_symbol("com.example.User.getName");

    // At least one should find the symbol
    assert!(
        sym_with_params.is_some() || sym_no_params.is_some(),
        "Should find getName symbol with either FQN format"
    );

    // Test find_usages bidirectional lookup:
    // Query WITH params should find usages stored WITHOUT params (and vice versa)
    let usages_with_params = index.find_usages("com.example.User.getName()");
    let usages_no_params = index.find_usages("com.example.User.getName");

    // The bidirectional lookup should ensure both return results
    // (whichever format usages are stored in, both queries should find them)
    let total_usages = usages_with_params.len().max(usages_no_params.len());
    assert!(
        total_usages >= 1,
        "Should find usages with either FQN format. With params: {}, without: {}",
        usages_with_params.len(),
        usages_no_params.len()
    );

    // Test with parameterized method
    let set_name_with_params = index.find_usages("com.example.User.setName(String)");
    let set_name_no_params = index.find_usages("com.example.User.setName");

    let set_name_total = set_name_with_params.len().max(set_name_no_params.len());
    assert!(
        set_name_total >= 1,
        "Should find setName usages with either FQN format"
    );
}

#[tokio::test]
async fn test_method_chain_resolves_nested_builder_usage() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let order_file = root.join("com/example/domain/Order.java");
    fs::create_dir_all(order_file.parent().unwrap()).unwrap();
    fs::write(
        &order_file,
        r#"package com.example.domain;

import java.util.ArrayList;
import java.util.List;

public class Order {
    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        public Builder lineItem(String name, int priceCents) {
            return this;
        }
    }
}
"#,
    )
    .unwrap();

    let service_file = root.join("com/example/service/OrderService.java");
    fs::create_dir_all(service_file.parent().unwrap()).unwrap();
    fs::write(
        &service_file,
        r#"package com.example.service;

import com.example.domain.Order;

public class OrderService {
    public Order createOrder() {
        return Order.builder().lineItem("Coffee", 450);
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index
        .index_file_with_source(&order_file, &fs::read_to_string(&order_file).unwrap())
        .unwrap();
    index
        .index_file_with_source(&service_file, &fs::read_to_string(&service_file).unwrap())
        .unwrap();

    let usages = index.find_usages("com.example.domain.Order.Builder.lineItem");
    let alt_builder = index.find_usages("com.example.domain.Builder.lineItem");
    let alt_order = index.find_usages("com.example.domain.Order.lineItem");
    assert!(
        !usages.is_empty(),
        "Expected chained usage to resolve to nested builder method. Alt Builder: {}, Alt Order: {}",
        alt_builder.len(),
        alt_order.len()
    );
}

/// Test that usage statistics are tracked during indexing
/// This enables smart completion ranking by frequency
#[tokio::test]
async fn test_usage_statistics_tracked_during_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a class with a method
    let user_file = root.join("com/example/User.java");
    fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    fs::write(
        &user_file,
        r#"package com.example;

public class User {
    public String getName() { return ""; }
    public String getEmail() { return ""; }
}
"#,
    )
    .unwrap();

    // Create a file that uses getName() multiple times and getEmail() once
    let service_file = root.join("com/example/Service.java");
    fs::write(
        &service_file,
        r#"package com.example;

public class Service {
    public void process(User user) {
        // getName used 3 times
        String n1 = user.getName();
        String n2 = user.getName();
        String n3 = user.getName();

        // getEmail used 1 time
        String e = user.getEmail();
    }
}
"#,
    )
    .unwrap();

    // Build the index
    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    // Get usage scores - getName should have a higher score than getEmail
    let get_name_score = index.get_usage_score("com.example.User.getName");
    let get_email_score = index.get_usage_score("com.example.User.getEmail");

    println!("getName usage score: {}", get_name_score);
    println!("getEmail usage score: {}", get_email_score);

    // getName is used 3x, getEmail is used 1x
    // getName should score higher
    assert!(
        get_name_score > get_email_score,
        "getName (used 3x) should score higher than getEmail (used 1x). \
         getName: {}, getEmail: {}",
        get_name_score,
        get_email_score
    );

    // Verify top symbols include frequently used ones
    let top = index.get_top_symbols(10);
    println!("Top symbols: {:?}", top);

    // Should have some usage data
    assert!(
        !top.is_empty(),
        "Should have tracked some symbol usages for ranking"
    );
}
