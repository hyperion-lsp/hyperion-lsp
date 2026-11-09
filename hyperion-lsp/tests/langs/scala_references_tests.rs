// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Fixture paths intentionally use case-sensitive language extensions.
#![allow(clippy::case_sensitive_file_extension_comparisons)]

//! Find references tests for Scala language support
//!
//! These tests verify that find-references works for Scala files, including:
//! - Finding references to Scala methods
//! - Finding references to Scala classes
//! - Finding references to Scala vals/vars
//! - Cross-language references (Java ↔ Scala)

use crate::common_workspace::semantic_workspace as create_test_workspace;
use hyperion_semantics::db::Language;

// ========== Basic Scala References Tests ==========

#[test]
fn test_scala_class_symbol_indexed() {
    let (_temp, index, _db, _files) = create_test_workspace(vec![(
        "com/example/User.scala",
        r#"
package com.example

class User {
    val name: String = "test"
}
        "#,
        Language::Scala,
    )]);

    // Verify the class is indexed
    let symbols = index.find_by_simple_name("User");
    assert!(!symbols.is_empty(), "Expected User class to be indexed");

    println!("Found {} symbols for 'User'", symbols.len());
    for sym in &symbols {
        println!("  - {} (file_id: {:?})", sym.name, sym.file_id);
    }
}

#[test]
fn test_scala_method_symbol_indexed() {
    let (_temp, index, _db, _files) = create_test_workspace(vec![(
        "com/example/User.scala",
        r#"
package com.example

class User {
    def getName(): String = "test"
}
        "#,
        Language::Scala,
    )]);

    // Verify the method is indexed
    let symbols = index.find_by_simple_name("getName");
    assert!(!symbols.is_empty(), "Expected getName method to be indexed");
}

// ========== Cross-Language Symbol Lookup Tests ==========

#[test]
fn test_java_class_accessible_from_scala_index() {
    let (_temp, index, _db, _files) = create_test_workspace(vec![
        (
            "com/example/Helper.java",
            r#"
package com.example;

public class Helper {
    public String getValue() {
        return "test";
    }
}
            "#,
            Language::Java,
        ),
        (
            "com/example/Main.scala",
            r"
package com.example

object Main {
    val helper = new Helper()
}
            ",
            Language::Scala,
        ),
    ]);

    // Verify both Java and Scala symbols are indexed
    let helper_symbols = index.find_by_simple_name("Helper");
    assert!(
        !helper_symbols.is_empty(),
        "Expected Helper class to be indexed"
    );

    let main_symbols = index.find_by_simple_name("Main");
    assert!(
        !main_symbols.is_empty(),
        "Expected Main object to be indexed"
    );
}

#[test]
fn test_scala_class_accessible_from_java_index() {
    let (_temp, index, _db, _files) = create_test_workspace(vec![
        (
            "com/example/User.scala",
            r#"
package com.example

class User {
    val name: String = "test"
}
            "#,
            Language::Scala,
        ),
        (
            "com/example/Main.java",
            r"
package com.example;

public class Main {
    public void test() {
        User user = new User();
    }
}
            ",
            Language::Java,
        ),
    ]);

    // Verify Scala class is accessible from Java context
    let user_symbols = index.find_by_simple_name("User");
    assert!(
        !user_symbols.is_empty(),
        "Expected User class to be indexed and findable"
    );
}

// ========== Find References Tests ==========
// Note: Full find-references support for Scala requires tree-sitter CST traversal
// For now, we verify that:
// 1. Scala symbols are properly indexed
// 2. Cross-language symbol lookup works
// These are the foundation for find-references to work

// TODO: Implement full find-references once tree-sitter CST traversal is available
