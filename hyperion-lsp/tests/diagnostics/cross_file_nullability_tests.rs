// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cross-File Nullability Cache Invalidation Tests
//!
//! These tests verify that the nullability analysis correctly invalidates
//! across file boundaries when method signatures change. This is critical for
//! maintaining correctness when:
//! - External edits change method annotations (git checkout, vim, build system)
//! - Developer modifies method signatures in one file
//! - Dependent files need to re-analyze with new signature information
//!
//! ## Architecture
//!
//! Phase 1: MethodSignatureCache tracks method signatures globally
//! Phase 2: Salsa integration via method_signature_version input query
//! Phase 3: File watcher updates signatures and bumps version
//! Phase 4: These tests verify the whole chain works
//!
//! ## Test Strategy
//!
//! 1. Unit tests for signature extraction edge cases
//! 2. Integration tests for Salsa invalidation chain
//! 3. Cross-file scenario tests (File A change -> File B invalidation)

use hyperion_semantics::db::{
    Database, FileId, Language, ParseDatabase, SemanticDatabase, SourceDatabase,
};
use hyperion_semantics::nullability::{extract_method_signatures, Nullability};
use std::sync::Arc;

// =============================================================================
// Helper Functions
// =============================================================================

/// Create a test database with two files: a service file defining a method,
/// and a client file that calls that method.
///
/// Returns (db, service_file_id, client_file_id)
fn setup_two_file_scenario(
    service_source: &str,
    client_source: &str,
) -> (Database, FileId, FileId) {
    let mut db = Database::new();

    let service_file = FileId::new(1);
    let client_file = FileId::new(2);

    db.register_file(
        service_file,
        Arc::new(service_source.to_string()),
        Language::Java,
    );
    db.register_file(
        client_file,
        Arc::new(client_source.to_string()),
        Language::Java,
    );

    (db, service_file, client_file)
}

// =============================================================================
// Unit Tests: Signature Extraction Edge Cases
// =============================================================================

#[test]
fn test_extract_signatures_with_nullable_param() {
    let source = r"
package com.example;

import javax.annotation.Nullable;

public class Service {
    public void process(@Nullable String input) {
        // ...
    }
}
";

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let signatures = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(signatures.len(), 1);
    let (fqn, sig) = &signatures[0];
    assert_eq!(fqn, "com.example.Service.process(String)");
    assert_eq!(sig.param_nullabilities.len(), 1);
    assert_eq!(sig.param_nullabilities[0], Nullability::Nullable);
}

#[test]
fn test_extract_signatures_with_nonnull_return() {
    let source = r#"
package com.example;

import javax.annotation.Nonnull;

public class Service {
    @Nonnull
    public String getData() {
        return "";
    }
}
"#;

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let signatures = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(signatures.len(), 1);
    let (fqn, sig) = &signatures[0];
    assert_eq!(fqn, "com.example.Service.getData()");
    assert_eq!(sig.return_nullability, Nullability::NonNull);
}

#[test]
fn test_extract_signatures_multiple_params_mixed_nullability() {
    let source = r"
package com.example;

import javax.annotation.Nullable;
import javax.annotation.Nonnull;

public class Service {
    public void update(@Nonnull String id, @Nullable String value, int count) {
        // ...
    }
}
";

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let signatures = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(signatures.len(), 1);
    let (fqn, sig) = &signatures[0];
    assert_eq!(fqn, "com.example.Service.update(String,String,int)");
    assert_eq!(sig.param_nullabilities.len(), 3);
    assert_eq!(sig.param_nullabilities[0], Nullability::NonNull);
    assert_eq!(sig.param_nullabilities[1], Nullability::Nullable);
    assert_eq!(sig.param_nullabilities[2], Nullability::NonNull); // primitives are non-null
}

#[test]
fn test_extract_signatures_array_parameters() {
    let source = r"
package com.example;

import javax.annotation.Nullable;

public class Service {
    public void process(
        @Nullable String[] items,
        @Nullable int[] optionalCounts,
        int[] counts
    ) {
        // ...
    }
}
";

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let signatures = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(signatures.len(), 1);
    let (fqn, sig) = &signatures[0];
    assert_eq!(fqn, "com.example.Service.process(String[],int[],int[])");
    assert_eq!(sig.param_nullabilities.len(), 3);
    assert_eq!(sig.param_nullabilities[0], Nullability::Nullable);
    assert_eq!(sig.param_nullabilities[1], Nullability::Nullable);
    assert_eq!(sig.param_nullabilities[2], Nullability::Unknown); // arrays are nullable references
}

#[test]
fn test_extract_signatures_generic_types() {
    let source = r"
package com.example;

import javax.annotation.Nullable;
import java.util.List;

public class Service {
    public void process(@Nullable List<String> items) {
        // ...
    }
}
";

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let signatures = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(signatures.len(), 1);
    let (fqn, sig) = &signatures[0];
    // Generic type arguments are not included in the FQN (uses simple name "List")
    assert_eq!(fqn, "com.example.Service.process(List)");
    assert_eq!(sig.param_nullabilities.len(), 1);
    assert_eq!(sig.param_nullabilities[0], Nullability::Nullable);
}

// =============================================================================
// Integration Tests: Cache Update Detection
// =============================================================================

#[test]
fn test_cache_update_detects_signature_changes() {
    // Test that update_file_signatures correctly returns true when signatures change
    let mut db = Database::new();
    let file_id = FileId::new(1);

    // Initial signature: Unknown nullability
    let source_v1 = r#"
package com.example;

public class Service {
    public String getData() {
        return "";
    }
}
"#;

    db.register_file(file_id, Arc::new(source_v1.to_string()), Language::Java);
    let root_v1 = db.parse_file(file_id);
    let sigs_v1 = extract_method_signatures(&root_v1.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v1);
    assert!(changed, "First update should always report changed");

    // Same signature again - should report no change
    let root_v2 = db.parse_file(file_id);
    let sigs_v2 = extract_method_signatures(&root_v2.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v2);
    assert!(!changed, "Same signature should not report changed");

    // Add @Nonnull annotation - should detect change
    let source_v3 = r#"
package com.example;

import javax.annotation.Nonnull;

public class Service {
    @Nonnull
    public String getData() {
        return "";
    }
}
"#;

    db.set_file_text(file_id, Arc::new(source_v3.to_string()));
    let root_v3 = db.parse_file(file_id);
    let sigs_v3 = extract_method_signatures(&root_v3.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v3);
    assert!(
        changed,
        "Adding @Nonnull annotation should be detected as change"
    );
}

#[test]
fn test_cache_update_detects_parameter_nullability_changes() {
    let mut db = Database::new();
    let file_id = FileId::new(1);

    // Version 1: no annotations
    let source_v1 = r"
package com.example;

public class Service {
    public void process(String input) {
    }
}
";

    db.register_file(file_id, Arc::new(source_v1.to_string()), Language::Java);
    let root_v1 = db.parse_file(file_id);
    let sigs_v1 = extract_method_signatures(&root_v1.syntax(), file_id);
    db.method_signature_cache()
        .update_file_signatures(file_id, sigs_v1);

    // Version 2: add @Nullable to parameter
    let source_v2 = r"
package com.example;

import javax.annotation.Nullable;

public class Service {
    public void process(@Nullable String input) {
    }
}
";

    db.set_file_text(file_id, Arc::new(source_v2.to_string()));
    let root_v2 = db.parse_file(file_id);
    let sigs_v2 = extract_method_signatures(&root_v2.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v2);
    assert!(changed, "Parameter nullability change should be detected");
}

#[test]
fn test_cache_update_detects_new_method() {
    let mut db = Database::new();
    let file_id = FileId::new(1);

    // Version 1: one method
    let source_v1 = r"
package com.example;

public class Service {
    public void foo() {
    }
}
";

    db.register_file(file_id, Arc::new(source_v1.to_string()), Language::Java);
    let root_v1 = db.parse_file(file_id);
    let sigs_v1 = extract_method_signatures(&root_v1.syntax(), file_id);
    db.method_signature_cache()
        .update_file_signatures(file_id, sigs_v1);

    // Version 2: add another method
    let source_v2 = r"
package com.example;

public class Service {
    public void foo() {
    }

    public void bar() {
    }
}
";

    db.set_file_text(file_id, Arc::new(source_v2.to_string()));
    let root_v2 = db.parse_file(file_id);
    let sigs_v2 = extract_method_signatures(&root_v2.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v2);
    assert!(changed, "Adding a new method should be detected");
}

#[test]
fn test_cache_update_detects_removed_method() {
    let mut db = Database::new();
    let file_id = FileId::new(1);

    // Version 1: two methods
    let source_v1 = r"
package com.example;

public class Service {
    public void foo() {
    }

    public void bar() {
    }
}
";

    db.register_file(file_id, Arc::new(source_v1.to_string()), Language::Java);
    let root_v1 = db.parse_file(file_id);
    let sigs_v1 = extract_method_signatures(&root_v1.syntax(), file_id);
    db.method_signature_cache()
        .update_file_signatures(file_id, sigs_v1);

    // Version 2: remove one method
    let source_v2 = r"
package com.example;

public class Service {
    public void foo() {
    }
}
";

    db.set_file_text(file_id, Arc::new(source_v2.to_string()));
    let root_v2 = db.parse_file(file_id);
    let sigs_v2 = extract_method_signatures(&root_v2.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs_v2);
    assert!(changed, "Removing a method should be detected");
}

// =============================================================================
// Integration Tests: Version Bumping
// =============================================================================

#[test]
fn test_version_bumps_on_signature_change() {
    let mut db = Database::new();
    let file_id = FileId::new(1);

    let initial_version = db.method_signature_version();

    // Add a method
    let source = r"
package com.example;

public class Service {
    public void method() {
    }
}
";

    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);
    let root = db.parse_file(file_id);
    let sigs = extract_method_signatures(&root.syntax(), file_id);

    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs);
    if changed {
        db.bump_method_signature_version();
    }

    assert!(changed, "First signature addition should report changed");
    assert_eq!(
        db.method_signature_version(),
        initial_version + 1,
        "Version should have incremented"
    );
}

#[test]
fn test_version_stable_when_no_change() {
    let mut db = Database::new();
    let file_id = FileId::new(1);

    let source = r"
package com.example;

public class Service {
    public void method() {
    }
}
";

    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);
    let root = db.parse_file(file_id);
    let sigs = extract_method_signatures(&root.syntax(), file_id);

    // First update
    db.method_signature_cache()
        .update_file_signatures(file_id, sigs.clone());
    db.bump_method_signature_version();
    let version_after_first = db.method_signature_version();

    // Second update with same signatures
    let changed = db
        .method_signature_cache()
        .update_file_signatures(file_id, sigs);
    if changed {
        db.bump_method_signature_version();
    }

    assert!(!changed, "Same signatures should not report changed");
    assert_eq!(
        db.method_signature_version(),
        version_after_first,
        "Version should remain stable when signatures don't change"
    );
}

// =============================================================================
// Integration Tests: Cross-File Invalidation
// =============================================================================

#[test]
fn test_cross_file_invalidation_on_signature_change() {
    // This test verifies the core invalidation flow:
    // 1. File A defines a method with Unknown nullability
    // 2. File B calls that method
    // 3. File A is updated to add @Nullable annotation
    // 4. File B's nullability analysis should be invalidated and recomputed

    let service_v1 = r#"
package com.example;

public class Service {
    public String getData() {
        return "";
    }
}
"#;

    let client = r"
package com.example;

public class Client {
    void test(Service service) {
        String data = service.getData();
        data.length(); // Should this warn? Depends on Service.getData() nullability
    }
}
";

    let (mut db, service_file, client_file) = setup_two_file_scenario(service_v1, client);

    // Update service signatures in the cache
    let root = db.parse_file(service_file);
    let sigs = extract_method_signatures(&root.syntax(), service_file);
    db.method_signature_cache()
        .update_file_signatures(service_file, sigs);
    db.bump_method_signature_version();

    // Analyze client file (first time)
    let analysis1 = db.file_nullability(client_file);

    // Second query should be cached
    let analysis2 = db.file_nullability(client_file);
    assert!(
        Arc::ptr_eq(&analysis1, &analysis2),
        "Second query should return cached result"
    );

    // Now update service to add @Nullable annotation
    let service_v2 = r"
package com.example;

import javax.annotation.Nullable;

public class Service {
    @Nullable
    public String getData() {
        return null;
    }
}
";

    db.set_file_text(service_file, Arc::new(service_v2.to_string()));
    let root = db.parse_file(service_file);
    let sigs = extract_method_signatures(&root.syntax(), service_file);
    let changed = db
        .method_signature_cache()
        .update_file_signatures(service_file, sigs);

    assert!(changed, "Service signature should have changed");

    // Bump version to trigger invalidation
    db.bump_method_signature_version();

    // Query client file again - should be invalidated and recomputed
    let analysis3 = db.file_nullability(client_file);

    assert!(
        !Arc::ptr_eq(&analysis1, &analysis3),
        "Client analysis should be invalidated when service signature changes"
    );

    // Fourth query should be cached again
    let analysis4 = db.file_nullability(client_file);
    assert!(
        Arc::ptr_eq(&analysis3, &analysis4),
        "Fourth query should return cached result"
    );
}

#[test]
fn test_multiple_dependents_invalidated() {
    // Test that multiple files depending on the same changed signature all get invalidated

    let service = r#"
package com.example;

public class Service {
    public String getData() {
        return "";
    }
}
"#;

    let client1 = r"
package com.example;

public class Client1 {
    void test(Service service) {
        String data = service.getData();
    }
}
";

    let client2 = r"
package com.example;

public class Client2 {
    void test(Service service) {
        String data = service.getData();
    }
}
";

    let mut db = Database::new();
    let service_file = FileId::new(1);
    let client1_file = FileId::new(2);
    let client2_file = FileId::new(3);

    db.register_file(service_file, Arc::new(service.to_string()), Language::Java);
    db.register_file(client1_file, Arc::new(client1.to_string()), Language::Java);
    db.register_file(client2_file, Arc::new(client2.to_string()), Language::Java);

    // Update service signatures
    let root = db.parse_file(service_file);
    let sigs = extract_method_signatures(&root.syntax(), service_file);
    db.method_signature_cache()
        .update_file_signatures(service_file, sigs);
    db.bump_method_signature_version();

    // Analyze both clients
    let client1_analysis1 = db.file_nullability(client1_file);
    let client2_analysis1 = db.file_nullability(client2_file);

    // Update service
    let service_v2 = r"
package com.example;

import javax.annotation.Nullable;

public class Service {
    @Nullable
    public String getData() {
        return null;
    }
}
";

    db.set_file_text(service_file, Arc::new(service_v2.to_string()));
    let root = db.parse_file(service_file);
    let sigs = extract_method_signatures(&root.syntax(), service_file);
    let changed = db
        .method_signature_cache()
        .update_file_signatures(service_file, sigs);

    if changed {
        db.bump_method_signature_version();
    }

    // Both clients should be invalidated
    let client1_analysis2 = db.file_nullability(client1_file);
    let client2_analysis2 = db.file_nullability(client2_file);

    assert!(
        !Arc::ptr_eq(&client1_analysis1, &client1_analysis2),
        "Client1 should be invalidated"
    );
    assert!(
        !Arc::ptr_eq(&client2_analysis1, &client2_analysis2),
        "Client2 should be invalidated"
    );
}

#[test]
fn test_no_invalidation_when_unrelated_file_changes() {
    // Test that changing an unrelated file doesn't invalidate analysis unnecessarily

    let service = r#"
package com.example;

public class Service {
    public String getData() {
        return "";
    }
}
"#;

    let client = r"
package com.example;

public class Client {
    void test(Service service) {
        String data = service.getData();
    }
}
";

    let unrelated = r"
package com.other;

public class Unrelated {
    public void foo() {
    }
}
";

    let mut db = Database::new();
    let service_file = FileId::new(1);
    let client_file = FileId::new(2);
    let unrelated_file = FileId::new(3);

    db.register_file(service_file, Arc::new(service.to_string()), Language::Java);
    db.register_file(client_file, Arc::new(client.to_string()), Language::Java);
    db.register_file(
        unrelated_file,
        Arc::new(unrelated.to_string()),
        Language::Java,
    );

    // Update service and unrelated signatures
    let service_root = db.parse_file(service_file);
    let service_sigs = extract_method_signatures(&service_root.syntax(), service_file);
    db.method_signature_cache()
        .update_file_signatures(service_file, service_sigs);

    let unrelated_root = db.parse_file(unrelated_file);
    let unrelated_sigs = extract_method_signatures(&unrelated_root.syntax(), unrelated_file);
    db.method_signature_cache()
        .update_file_signatures(unrelated_file, unrelated_sigs);

    db.bump_method_signature_version();

    // Analyze client
    let client_analysis1 = db.file_nullability(client_file);

    // Update only the unrelated file
    let unrelated_v2 = r"
package com.other;

import javax.annotation.Nullable;

public class Unrelated {
    @Nullable
    public String bar() {
        return null;
    }
}
";

    db.set_file_text(unrelated_file, Arc::new(unrelated_v2.to_string()));
    let unrelated_root = db.parse_file(unrelated_file);
    let unrelated_sigs = extract_method_signatures(&unrelated_root.syntax(), unrelated_file);
    let changed = db
        .method_signature_cache()
        .update_file_signatures(unrelated_file, unrelated_sigs);

    if changed {
        db.bump_method_signature_version();
    }

    // Client analysis WILL be invalidated because we bump the global version
    // This is a limitation of the current approach - we don't have fine-grained
    // tracking of which files depend on which methods.
    //
    // However, re-analysis should be fast due to Salsa's internal caching of
    // intermediate results. This test documents current behavior.
    let client_analysis2 = db.file_nullability(client_file);

    // The analysis will be recomputed, but should produce the same diagnostics
    // (since client doesn't depend on Unrelated class)
    assert_eq!(
        client_analysis1.diagnostics.len(),
        client_analysis2.diagnostics.len(),
        "Diagnostics should remain the same even though Arc changed"
    );
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_method_signature_cache_with_overloaded_methods() {
    // Verify that overloaded methods are tracked separately
    let source = r#"
package com.example;

import javax.annotation.Nullable;
import javax.annotation.Nonnull;

public class Service {
    @Nullable
    public String process(String input) {
        return null;
    }

    @Nonnull
    public String process(String input, int count) {
        return "";
    }
}
"#;

    let mut db = Database::new();
    let file_id = FileId::new(1);
    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);

    let root = db.parse_file(file_id);
    let sigs = extract_method_signatures(&root.syntax(), file_id);

    assert_eq!(sigs.len(), 2, "Should extract both overloads");

    db.method_signature_cache()
        .update_file_signatures(file_id, sigs.clone());

    // Verify we can retrieve both signatures by their FQN
    let sig1 = db
        .method_signature_cache()
        .get_signature("com.example.Service.process(String)");
    let sig2 = db
        .method_signature_cache()
        .get_signature("com.example.Service.process(String,int)");

    assert!(sig1.is_some(), "Should find first overload");
    assert!(sig2.is_some(), "Should find second overload");

    assert_eq!(sig1.unwrap().return_nullability, Nullability::Nullable);
    assert_eq!(sig2.unwrap().return_nullability, Nullability::NonNull);
}

#[test]
fn test_signature_cache_remove_file() {
    // Verify that removing a file cleans up its signatures
    let mut db = Database::new();
    let file_id = FileId::new(1);

    let source = r"
package com.example;

public class Service {
    public void method() {
    }
}
";

    db.register_file(file_id, Arc::new(source.to_string()), Language::Java);
    let root = db.parse_file(file_id);
    let sigs = extract_method_signatures(&root.syntax(), file_id);

    db.method_signature_cache()
        .update_file_signatures(file_id, sigs);
    db.bump_method_signature_version();

    let version_before_remove = db.method_signature_version();

    // Verify signature exists
    let sig = db
        .method_signature_cache()
        .get_signature("com.example.Service.method()");
    assert!(sig.is_some(), "Signature should exist before removal");

    // Remove the file
    let removed = db.method_signature_cache().remove_file_signatures(file_id);
    if removed {
        db.bump_method_signature_version();
    }

    // Verify signature is gone
    let sig = db
        .method_signature_cache()
        .get_signature("com.example.Service.method()");
    assert!(sig.is_none(), "Signature should be removed");

    // Verify version was bumped (removal is a change)
    assert!(
        removed,
        "Removal should report that signatures were removed"
    );
    assert!(
        db.method_signature_version() > version_before_remove,
        "DB version should bump when file is removed"
    );
}
