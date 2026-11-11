// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::query::selected_owner_member_definitions_shared_in;
use super::*;
use tempfile::TempDir;
use tokio;

#[tokio::test]
async fn test_index_single_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    std::fs::write(&file_path, "package com.example;\n\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    assert_eq!(stats.symbols_added, 1);

    // Check symbol was indexed
    let sym = index.find_symbol("com.example.Test");
    assert!(sym.is_some());
    let sym = sym.unwrap();
    assert_eq!(&*sym.name, "Test");
}

#[test]
fn parallel_skip_cached_indexes_only_changed_sources() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Test.java");
    std::fs::write(
        &file_path,
        "package com.example; public class Test { void before() {} }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let initial = index.index_files_parallel(std::slice::from_ref(&file_path));
    assert_eq!(initial.files_indexed, 1);

    let unchanged = index.index_files_parallel_skip_cached(std::slice::from_ref(&file_path));
    assert_eq!(unchanged.files_indexed, 0);
    assert_eq!(unchanged.files_failed, 0);

    std::fs::write(
        &file_path,
        "package com.example; public class Test { void after() {} }",
    )
    .unwrap();
    let changed = index.index_files_parallel_skip_cached(std::slice::from_ref(&file_path));

    assert_eq!(changed.files_indexed, 1);
    assert_eq!(changed.files_failed, 0);
    assert!(index.find_symbol("com.example.Test.after").is_some());
    assert!(index.find_symbol("com.example.Test.before").is_none());
}

#[test]
fn instantiable_workspace_classes_are_filtered_inside_one_index_snapshot() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Types.java");
    std::fs::write(
        &file_path,
        r#"
            package example;
            abstract class AbstractBase {}
            interface Contract {}
            class Concrete {}
            enum Choice { ONE }
            class Excluded {}
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&file_path).unwrap();
    let excluded = HashSet::from([Arc::<str>::from("example.Excluded")]);

    let locations = index.get_workspace_instantiable_class_locations(10, &excluded);
    let fqns: HashSet<_> = locations
        .iter()
        .map(|location| location.fqn.as_ref())
        .collect();

    assert_eq!(fqns, HashSet::from(["example.Concrete", "example.Choice"]));
    assert!(locations
        .iter()
        .all(|location| location.workspace_source_revision().is_some()));
}

#[test]
fn indexed_in_memory_source_is_not_replaced_by_stale_disk_content() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Test.java");
    let disk_source = "package test; class Test { void oldName() {} }";
    let editor_source = "package test; class Test { void newName() {} }";
    std::fs::write(&file_path, disk_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&file_path, editor_source)
        .unwrap();
    let file_id = index.get_file_id(&file_path).expect("indexed file id");
    let validated = index
        .get_validated_file_contents(file_id)
        .expect("validated editor content");

    assert_eq!(validated.content.as_str(), editor_source);
    assert!(index.get_symbol("test.Test.newName").is_some());
    assert!(index.get_symbol("test.Test.oldName").is_none());
}

#[test]
fn evicted_caller_revision_is_not_replaced_by_disk_or_cached_as_indexed() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Test.kt");
    let disk_source = "package test\nclass DiskRevision";
    let editor_source = "package test\nclass EditorRevision";
    std::fs::write(&file_path, disk_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&file_path, editor_source)
        .unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();
    let editor_hash = index.get_content_hash(file_id).unwrap();

    // Kotlin revisions have no Java parse-cache source fallback. Force this
    // caller-owned buffer out of the bounded content LRU.
    for number in 0..110 {
        index
            .index_file_with_source(
                &temp.path().join(format!("evict/E{number}.kt")),
                &format!("package evict\nclass E{number}"),
            )
            .unwrap();
    }
    assert!(!index.inner.read().file_content_cache.contains(&file_id));

    assert!(index
        .get_indexed_file_contents(file_id, editor_hash)
        .is_none());
    assert_eq!(
        index.get_current_file_contents(file_id).unwrap().as_str(),
        disk_source
    );
    assert!(!index.inner.read().file_content_cache.contains(&file_id));
    assert!(index.get_validated_file_contents(file_id).is_none());

    assert_eq!(index.get_content_hash(file_id), Some(editor_hash));
    assert!(index.find_symbol("test.EditorRevision").is_some());
    assert!(index.find_symbol("test.DiskRevision").is_none());
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );
}

#[test]
fn validated_cache_miss_rejects_disk_drift_without_request_side_reindex() {
    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Test.kt");
    let old_source = "package test\nclass OldRevision";
    let new_source = "package test\nclass NewRevision";
    std::fs::write(&file_path, old_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&file_path).unwrap();
    let file_id = index.get_file_id(&file_path).expect("indexed file id");

    index.clear_file_cache();
    std::fs::write(&file_path, new_source).unwrap();
    assert!(index.get_validated_file_contents(file_id).is_none());
    assert!(index.get_symbol("test.NewRevision").is_none());
    assert!(index.get_symbol("test.OldRevision").is_some());
}

#[test]
fn validated_reindex_cannot_overwrite_a_newer_editor_revision() {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    let temp = TempDir::new().unwrap();
    let file_path = temp.path().join("Test.java");
    let old_source = "package test; class OldRevision {}";
    let disk_source = "package test; class DiskRevision {}";
    let editor_source = "package test; class EditorRevision {}";
    std::fs::write(&file_path, old_source).unwrap();

    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(&file_path, old_source)
        .unwrap();
    let file_id = index.get_file_id(&file_path).unwrap();
    index.clear_file_cache();
    std::fs::write(&file_path, disk_source).unwrap();

    let read_finished = Arc::new(Barrier::new(2));
    let allow_validation = Arc::new(Barrier::new(2));
    let validator = {
        let index = Arc::clone(&index);
        let read_finished = Arc::clone(&read_finished);
        let allow_validation = Arc::clone(&allow_validation);
        std::thread::spawn(move || {
            index.get_validated_file_contents_after_read_for_test(file_id, || {
                read_finished.wait();
                allow_validation.wait();
            })
        })
    };
    read_finished.wait();

    let (editor_started_tx, editor_started_rx) = mpsc::sync_channel(0);
    let (editor_done_tx, editor_done_rx) = mpsc::sync_channel(0);
    let editor = {
        let index = Arc::clone(&index);
        let file_path = file_path.clone();
        std::thread::spawn(move || {
            editor_started_tx.send(()).unwrap();
            index
                .index_file_with_source_and_id(&file_path, editor_source, file_id)
                .unwrap();
            editor_done_tx.send(()).unwrap();
        })
    };
    editor_started_rx.recv().unwrap();
    editor_done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the initial validation snapshot must not block editor publication");

    allow_validation.wait();
    let validated = validator.join().unwrap().unwrap();
    assert_eq!(validated.content.as_str(), editor_source);
    editor.join().unwrap();

    assert!(index.find_symbol("test.EditorRevision").is_some());
    assert!(index.find_symbol("test.DiskRevision").is_none());
    assert!(index.find_symbol("test.OldRevision").is_none());
}

#[tokio::test]
async fn test_find_by_simple_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("Foo.java");
    std::fs::write(&file1, "package com.a;\npublic class Foo {}").unwrap();

    let file2 = root.join("Foo2.java");
    std::fs::write(&file2, "package com.b;\npublic class Foo {}").unwrap();

    let file3 = root.join("Holder.java");
    std::fs::write(&file3, "package com.c;\npublic class Holder { int Foo; }").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();
    index.index_file(&file3).await.unwrap();

    let results = index.find_by_simple_name("Foo");
    assert_eq!(results.len(), 3);
    assert_eq!(
        results
            .iter()
            .map(|location| location.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["com.a.Foo", "com.b.Foo", "com.c.Holder.Foo"]
    );

    let type_results = index.find_type_declarations_by_simple_name("Foo");
    assert_eq!(type_results.len(), 2);
    assert_eq!(
        type_results
            .iter()
            .map(|location| location.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["com.a.Foo", "com.b.Foo"]
    );
    assert!(type_results
        .iter()
        .all(|location| crate::is_type_declaration_kind(location.kind)));
}

#[tokio::test]
async fn simple_name_indices_share_fqns_and_remove_them_cleanly() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Widget.java");
    std::fs::write(&path, "package com.example; public class Widget {}").unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&path).await.unwrap();
    let file_id = index.get_file_id(&path).expect("indexed file id");

    {
        let inner = index.inner.read();
        let canonical = &inner.symbols["com.example.Widget"].fqn;
        let exact = inner.simple_name_index["Widget"]
            .get("com.example.Widget")
            .expect("exact-name entry");
        let lowercase = inner.simple_name_lower_index["widget"]
            .get("com.example.Widget")
            .expect("lowercase-name entry");
        assert!(Arc::ptr_eq(canonical, exact));
        assert!(Arc::ptr_eq(canonical, lowercase));
    }

    index.remove_file(file_id);
    let inner = index.inner.read();
    assert!(!inner.simple_name_index.contains_key("Widget"));
    assert!(!inner.simple_name_lower_index.contains_key("widget"));
}

#[tokio::test]
async fn case_distinct_simple_names_remain_distinct_in_lowercase_index() {
    let temp = TempDir::new().unwrap();
    let upper = temp.path().join("Upper.java");
    let lower = temp.path().join("Lower.java");
    std::fs::write(&upper, "package com.example; class Widget {}").unwrap();
    std::fs::write(&lower, "package com.example; class widget {}").unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&upper).await.unwrap();
    index.index_file(&lower).await.unwrap();

    let mut matches = index.get_fqns_by_simple_prefix("wid", 10);
    matches.sort();
    assert_eq!(matches, vec!["com.example.Widget", "com.example.widget"],);

    let mut workspace_matches: Vec<_> = index
        .search("com.example.wid")
        .into_iter()
        .map(|location| location.fqn.to_string())
        .collect();
    workspace_matches.sort();
    assert_eq!(
        workspace_matches,
        vec!["com.example.Widget", "com.example.widget"],
        "case-folded workspace search must retain every exact FQN"
    );

    let lower_id = index.get_file_id(&lower).unwrap();
    index.remove_file(lower_id);
    assert_eq!(
        index
            .search("com.example.wid")
            .into_iter()
            .map(|location| location.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["com.example.Widget"],
        "removing one case variant must not remove the survivor's search key"
    );
}

#[tokio::test]
async fn test_index_enum_and_find_by_simple_name() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create directory structure with nested module source roots
    let src_dir = root.join("modules/library-types/src/main/java/org/example/library/types");
    std::fs::create_dir_all(&src_dir).unwrap();

    let file_path = src_dir.join("BookFormat.java");
    std::fs::write(
        &file_path,
        r#"package org.example.library.types;

import java.util.HashMap;
import java.util.Map;

public enum BookFormat {
UNKNOWN,
PAPER,
DIGITAL;

public static BookFormat fromLabel(String label) {
    return UNKNOWN;
}
}"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    let stats = index.index_file(&file_path).await.unwrap();

    println!("Indexed {} symbols", stats.symbols_added);

    // Verify enum can be found by FQN
    let by_fqn = index.find_symbol("org.example.library.types.BookFormat");
    assert!(by_fqn.is_some(), "Should find BookFormat by FQN");
    println!("Found by FQN: {:?}", by_fqn);

    // Verify enum can be found by simple name
    let by_simple = index.find_by_simple_name("BookFormat");
    assert!(
        !by_simple.is_empty(),
        "Should find BookFormat by simple name"
    );
    println!("Found by simple name: {} results", by_simple.len());

    // Verify it's the right kind
    let sym = &by_simple[0];
    assert_eq!(&*sym.name, "BookFormat");
    assert_eq!(sym.kind, lsp_types::SymbolKind::ENUM);
}

#[tokio::test]
async fn test_workspace_build_finds_enum_in_subdir() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a multi-module structure
    // Module 1: modules/library-types with the enum
    let enum_dir = root.join("modules/library-types/src/main/java/org/example/library/types");
    std::fs::create_dir_all(&enum_dir).unwrap();
    std::fs::write(
        enum_dir.join("BookFormat.java"),
        "package org.example.library.types;\npublic enum BookFormat { PAPER, DIGITAL }",
    )
    .unwrap();

    // Module 2: tools/catalog that uses the enum
    let service_dir = root.join("tools/catalog/catalog-cli/src/main/java/org/example/library/cli");
    std::fs::create_dir_all(&service_dir).unwrap();
    std::fs::write(
        service_dir.join("CatalogTool.java"),
        r#"package org.example.library.cli;
import org.example.library.types.BookFormat;
public class CatalogTool {
private BookFormat type;
}"#,
    )
    .unwrap();

    // Build the workspace index using the full scanner path
    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    println!(
        "Scanned {} files, indexed {} symbols",
        stats.files_indexed, stats.symbols_indexed
    );
    // Verify BookFormat was found by the scanner and indexed
    let by_simple = index.find_by_simple_name("BookFormat");
    assert!(
        !by_simple.is_empty(),
        "Scanner should find BookFormat.java in modules/library-types and index it"
    );

    // Also verify CatalogTool was found
    let file_tools = index.find_by_simple_name("CatalogTool");
    assert!(
        !file_tools.is_empty(),
        "Scanner should find CatalogTool.java in tools/catalog"
    );

    // The cold workspace build uses a declaration pass followed by a usage
    // pass. Fresh files do not need stale-usage cleanup, but their newly
    // extracted usages must still be recorded.
    let file_tools_id = index
        .get_file_id(&service_dir.join("CatalogTool.java"))
        .expect("CatalogTool should have a file id");
    assert!(
        index
            .find_usages("org.example.library.types.BookFormat")
            .iter()
            .any(|usage| usage.file_id == file_tools_id),
        "Two-pass workspace build should record initial usages"
    );

    println!("BookFormat locations: {:?}", by_simple);
    println!("CatalogTool locations: {:?}", file_tools);
}

#[test]
fn cold_build_preserves_an_authoritative_caller_revision() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let path = root.join("src/main/java/app/Owned.java");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "package app; class DiskRevision {}").unwrap();
    std::fs::write(
        path.parent().unwrap().join("Dependency.java"),
        "package app; class Dependency {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    let editor_source = "package app; class EditorRevision { Dependency value; }";
    index.index_file_with_source(&path, editor_source).unwrap();
    let file_id = index.get_file_id(&path).expect("caller-owned file id");
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );

    let stats = index.build().unwrap();

    assert_eq!(stats.files_indexed, 2);
    assert!(index.find_symbol("app.EditorRevision").is_some());
    assert!(index.find_symbol("app.DiskRevision").is_none());
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );
    assert_eq!(
        index
            .get_file_contents(file_id)
            .as_deref()
            .map(String::as_str),
        Some(editor_source)
    );
    assert_eq!(
        index
            .find_usages("app.Dependency")
            .iter()
            .filter(|usage| usage.file_id == file_id)
            .count(),
        1,
        "the cold usage pass must replace, not duplicate, preserved caller usages"
    );
    assert!(!index.cold_declaration_pass_active());
}

/// Regression test: enum-constant usages must be found through wildcard
/// imports.
///
/// `ImportResolver::resolve` ranks the current package above wildcard
/// imports, so `Order.OrderStatus.PENDING` inside package `com.acme.service`
/// with `import com.acme.model.*;` used to be recorded under the nonexistent
/// FQN `com.acme.service.Order.OrderStatus.PENDING`, and `find_usages` for
/// the real constant missed the file entirely. Usage extraction now prefers
/// candidates that exist in the symbol index.
#[tokio::test]
async fn test_find_usages_of_enum_constant_through_wildcard_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let model_dir = root.join("src/main/java/com/acme/model");
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(
        model_dir.join("Order.java"),
        r#"package com.acme.model;
public class Order {
    private OrderStatus status;
    public void setStatus(OrderStatus status) { this.status = status; }
    public enum OrderStatus { PENDING, SHIPPED }
}"#,
    )
    .unwrap();

    let service_dir = root.join("src/main/java/com/acme/service");
    std::fs::create_dir_all(&service_dir).unwrap();
    // The broken specimen: wildcard import, qualified enum-constant access.
    std::fs::write(
        service_dir.join("OrderService.java"),
        r#"package com.acme.service;
import com.acme.model.*;
public class OrderService {
    public void create(Order order) {
        order.setStatus(Order.OrderStatus.PENDING);
    }
}"#,
    )
    .unwrap();
    // Control: explicit import of the same type.
    std::fs::write(
        service_dir.join("ShipmentService.java"),
        r#"package com.acme.service;
import com.acme.model.Order;
public class ShipmentService {
    public void ship(Order order) {
        order.setStatus(Order.OrderStatus.SHIPPED);
    }
}"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.build().unwrap();

    let wildcard_file_id = index
        .get_file_id(&service_dir.join("OrderService.java"))
        .expect("OrderService should have a file id");
    let explicit_file_id = index
        .get_file_id(&service_dir.join("ShipmentService.java"))
        .expect("ShipmentService should have a file id");

    let pending = index.find_usages("com.acme.model.Order.OrderStatus.PENDING");
    assert!(
        pending
            .iter()
            .any(|usage| usage.file_id == wildcard_file_id),
        "enum-constant usage behind a wildcard import must be recorded \
         under the real FQN (got: {pending:?})"
    );

    let shipped = index.find_usages("com.acme.model.Order.OrderStatus.SHIPPED");
    assert!(
        shipped
            .iter()
            .any(|usage| usage.file_id == explicit_file_id),
        "explicit-import control case must keep working (got: {shipped:?})"
    );
}

#[tokio::test]
async fn test_remove_file() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Test.java");

    std::fs::write(&file_path, "package com.example;\npublic class Test {}").unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path).unwrap();

    // Verify symbol exists
    assert!(index.find_symbol("com.example.Test").is_some());

    // Remove file
    index.remove_file(file_id);

    // Verify symbol is gone
    assert!(index.find_symbol("com.example.Test").is_none());

    // Every FileId-owned structure, including transient serialization locks and
    // the source-bearing parse cache, must be released with the file.
    assert!(index.get_file_id(&file_path).is_none());
    let differently_cased = root.join("test.java");
    assert!(index.get_file_id(&differently_cased).is_none());
    assert!(!index.file_locks.contains_key(&file_id));
    assert!(!index.path_locks.contains_key(&path_lookup_key(&file_path)));

    let inner = index.inner.read();
    assert!(!inner.paths.contains_file_id(file_id));
    assert!(!inner.published_revisions.contains_key(&file_id));
    assert!(inner.file_content_cache.peek(&file_id).is_none());
    assert!(inner.parse_tree_cache.peek(&file_id).is_none());
}

#[test]
fn path_scoped_removal_rejects_an_identity_replaced_by_a_newer_revision() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Replaced.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let stale_file_id = FileId::new(41);
    let current_file_id = FileId::new(42);

    index
        .index_file_with_source_and_id(&path, "class Old {}", stale_file_id)
        .unwrap();
    index
        .index_file_with_source_and_id(&path, "class New {}", current_file_id)
        .unwrap();
    assert_eq!(index.get_file_id(&path), Some(current_file_id));

    let cleanup_ran = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        index.remove_file_revision(
            &path,
            stale_file_id,
            || true,
            || {
                cleanup_ran.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        ),
        FileRevisionRemovalOutcome::RevisionChanged
    );

    assert!(!cleanup_ran.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(index.get_file_id(&path), Some(current_file_id));
    assert!(index.find_symbol("New").is_some());
    assert!(!index.file_locks.contains_key(&stale_file_id));
    assert!(index.file_locks.contains_key(&current_file_id));
    assert!(index.path_locks.contains_key(&path_lookup_key(&path)));

    assert!(matches!(
        index.remove_file_revision(
            &path,
            current_file_id,
            || true,
            || {
                cleanup_ran.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        ),
        FileRevisionRemovalOutcome::Removed { .. }
    ));
    assert!(cleanup_ran.load(std::sync::atomic::Ordering::Relaxed));
    assert!(index.get_file_id(&path).is_none());
    assert!(index.find_symbol("New").is_none());
}

#[test]
fn path_scoped_removal_rechecks_absence_after_the_predicate() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Recreated.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let file_id = FileId::new(43);
    index
        .index_file_with_source_and_id(&path, "class Retained {}", file_id)
        .unwrap();
    let cleanup_ran = std::sync::atomic::AtomicBool::new(false);

    let outcome = index.remove_file_revision(
        &path,
        file_id,
        || {
            std::fs::write(&path, "class Recreated {}").unwrap();
            true
        },
        || cleanup_ran.store(true, std::sync::atomic::Ordering::Relaxed),
    );

    assert_eq!(outcome, FileRevisionRemovalOutcome::RevisionChanged);
    assert!(!cleanup_ran.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(index.get_file_id(&path), Some(file_id));
    assert!(index.find_symbol("Retained").is_some());
}

#[test]
fn exact_revision_removal_rejects_a_newer_publication() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Exact.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let file_id = FileId::new(44);
    index
        .index_file_with_source_and_id(&path, "class OldExact {}", file_id)
        .unwrap();
    let stale_revision = index.current_file_revision(file_id).unwrap();
    index
        .index_file_with_source_and_id(&path, "class NewExact {}", file_id)
        .unwrap();

    assert_eq!(
        index.remove_exact_file_revision(stale_revision, || true, || {}),
        FileRevisionRemovalOutcome::RevisionChanged
    );
    assert!(index.find_symbol("NewExact").is_some());
    assert!(index.find_symbol("OldExact").is_none());
}

#[test]
fn repeated_transient_files_do_not_accumulate_path_or_file_locks() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for number in 0..64 {
        let path = temp.path().join(format!("Transient{number}.java"));
        let source = format!("package transient; class Transient{number} {{}}");
        index.index_file_with_source(&path, &source).unwrap();
        let file_id = index.get_file_id(&path).expect("indexed FileId");

        index.remove_file(file_id);

        assert!(index.get_file_id(&path).is_none());
        assert!(index.path_locks.is_empty());
        assert!(index.file_locks.is_empty());
    }

    let inner = index.inner.read();
    assert!(inner.paths.is_empty());
    inner.paths.assert_consistent();
    assert_eq!(inner.parse_tree_cache.len(), 0);
}

#[cfg(unix)]
#[test]
fn removing_a_broken_symlink_path_reclaims_its_registered_lock_key() {
    let temp = TempDir::new().unwrap();
    let real = temp.path().join("real");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("Aliased.java"), "class Aliased {}").unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&alias_path).unwrap();
    let file_id = index.get_file_id(&alias_path).unwrap();
    let registered_key = index
        .inner
        .read()
        .paths
        .get(file_id)
        .unwrap()
        .lookup_key()
        .clone();
    assert!(index.path_locks.contains_key(&registered_key));

    std::fs::remove_file(&alias).unwrap();
    assert_ne!(
        registered_key,
        path_lookup_key(&alias_path),
        "the regression requires the broken alias to recompute differently"
    );

    index.remove_file(file_id);

    assert!(!index.path_locks.contains_key(&registered_key));
    assert!(index.path_locks.is_empty());
}

#[cfg(unix)]
#[test]
fn revision_removal_falls_back_to_the_exact_display_path_after_symlink_breakage() {
    let temp = TempDir::new().unwrap();
    let real = temp.path().join("real");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("Aliased.java"), "class Aliased {}").unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&alias_path).unwrap();
    let file_id = index.get_file_id(&alias_path).unwrap();
    let registered_key = index
        .inner
        .read()
        .paths
        .get(file_id)
        .unwrap()
        .lookup_key()
        .clone();

    std::fs::remove_file(&alias).unwrap();
    assert_ne!(registered_key, path_lookup_key(&alias_path));
    assert_eq!(
        index.get_file_id(&alias_path),
        None,
        "normal lookup must not treat stale display text as filesystem identity"
    );
    assert_eq!(
        index.get_file_id_for_key_or_display(&path_lookup_key(&alias_path), &alias_path),
        Some(file_id),
        "the deletion-only lookup may use an unambiguous absent display spelling"
    );

    assert!(matches!(
        index.remove_file_revision(&alias_path, file_id, || true, || {}),
        FileRevisionRemovalOutcome::Removed { .. }
    ));
    assert!(index.find_symbol("Aliased").is_none());
    assert!(!index.path_locks.contains_key(&registered_key));
}

#[test]
fn exact_display_fallback_rejects_ambiguous_stale_identities() {
    let temp = TempDir::new().unwrap();
    let display = temp.path().join("alias/A.java");
    let first_key = path_lookup_key(&temp.path().join("first/A.java"));
    let second_key = path_lookup_key(&temp.path().join("second/A.java"));
    let missing_key = path_lookup_key(&temp.path().join("missing/A.java"));
    let first_id = FileId::new(1_000_101);
    let second_id = FileId::new(1_000_102);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    {
        let mut inner = index.inner.write();
        inner.register_file_path(first_id, display.clone(), first_key.clone());
        inner.register_file_path(second_id, display.clone(), second_key.clone());
    }

    assert_eq!(index.get_file_id_for_key(&first_key), Some(first_id));
    assert_eq!(index.get_file_id_for_key(&second_key), Some(second_id));
    assert_eq!(
        index.get_file_id_for_key_or_display(&first_key, &display),
        Some(first_id),
        "an exact live key owner must win despite display ambiguity"
    );
    assert_eq!(
        index.get_file_id_for_key_or_display(&missing_key, &display),
        None,
        "display text alone must not choose nondeterministically between stale identities"
    );
    let inner = index.inner.read();
    assert_eq!(inner.file_path(first_id), Some(display.as_path()));
    assert_eq!(inner.file_path(second_id), Some(display.as_path()));
    inner.paths.assert_consistent();
}

#[test]
fn registered_path_snapshot_cannot_mix_revisions_across_a_move() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Old.java");
    let new_path = temp.path().join("New.java");
    let file_id = FileId::new(1_000_103);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    {
        let mut inner = index.inner.write();
        inner.register_file_path(file_id, old_path.clone(), path_lookup_key(&old_path));
    }
    let old_snapshot = index.get_registered_file_path(file_id).unwrap();

    {
        let mut inner = index.inner.write();
        inner.register_file_path(file_id, new_path.clone(), path_lookup_key(&new_path));
        inner.paths.assert_consistent();
    }

    assert!(!index.is_registered_file_path_current(file_id, &old_snapshot));
    let new_snapshot = index.get_registered_file_path(file_id).unwrap();
    assert_eq!(new_snapshot.display_path(), new_path);
    assert_eq!(new_snapshot.lookup_key(), &path_lookup_key(&new_path));
}

#[cfg(unix)]
#[test]
fn disk_publication_rejects_a_symlink_retargeted_after_key_capture() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("Aliased.java"), "class FirstTarget {}").unwrap();
    std::fs::write(second.join("Aliased.java"), "class SecondTarget {}").unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&alias_path).unwrap();
    let file_id = index.get_file_id(&alias_path).unwrap();
    let original_revision = index.current_file_revision(file_id).unwrap();
    let source_read_under_first_identity = crate::read_source_file(&alias_path).unwrap();
    let (established_key, path_lock) = index.establish_path_lock(&alias_path);
    assert_eq!(
        index.get_file_path_key(file_id),
        Some(established_key.clone())
    );
    assert_eq!(index.get_file_id_for_key(&established_key), Some(file_id));
    let path_guard = path_lock.lock().unwrap();
    let file_lock = index.get_file_lock(file_id);
    let file_guard = file_lock.lock().unwrap();

    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    let retargeted_key = path_lookup_key(&alias_path);
    assert_ne!(established_key, retargeted_key);
    assert_eq!(index.get_file_id_for_key(&retargeted_key), None);
    assert_eq!(
        index.get_file_id_for_key_or_display(&retargeted_key, &alias_path),
        None,
        "a live retargeted alias must never fall back to stale display text"
    );

    let result = index.index_disk_file_with_key_and_locks_held(
        &alias_path,
        &established_key,
        &source_read_under_first_identity,
        file_id,
    );
    assert!(result.is_err());
    drop(file_guard);
    drop(file_lock);
    drop(path_guard);
    drop(path_lock);

    assert_eq!(
        index.current_file_revision(file_id),
        Some(original_revision)
    );
    assert!(index.find_symbol("FirstTarget").is_some());
    assert!(index.find_symbol("SecondTarget").is_none());
    assert_eq!(index.get_file_path_key(file_id), Some(established_key));
}

#[cfg(unix)]
#[test]
fn live_symlink_retarget_publishes_a_distinct_automatic_identity() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("Aliased.java"), "class FirstTarget {}").unwrap();
    std::fs::write(second.join("Aliased.java"), "class SecondTarget {}").unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&alias_path).unwrap();
    let first_id = index.get_file_id(&alias_path).unwrap();
    let first_key = index.get_file_path_key(first_id).unwrap();

    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    let second_key = path_lookup_key(&alias_path);
    assert_ne!(first_key, second_key);
    assert_eq!(index.get_file_id(&alias_path), None);

    index
        .index_file_with_source(&alias_path, "class SecondTarget {}")
        .unwrap();
    let second_id = index.get_file_id(&alias_path).unwrap();
    assert_ne!(first_id, second_id);
    assert_eq!(index.get_file_id_for_key(&first_key), Some(first_id));
    assert_eq!(index.get_file_id_for_key(&second_key), Some(second_id));
    assert!(index.find_symbol("FirstTarget").is_some());
    assert!(index.find_symbol("SecondTarget").is_some());
}

#[cfg(unix)]
#[test]
fn same_display_different_key_collision_relocates_without_reusing_parse_state() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");
    let first_source = "class FirstTarget {}";
    let second_source = "class SecondTarget { int value; }";

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&alias_path, first_source)
        .unwrap();
    let colliding_id = index.get_file_id(&alias_path).unwrap();
    let first_key = index.get_file_path_key(colliding_id).unwrap();

    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&second, &alias).unwrap();
    let second_key = path_lookup_key(&alias_path);
    let unrelated_noop =
        hyperion_parser::TextEdit::new(rowan::TextRange::new(0.into(), 0.into()), String::new());
    index
        .index_file_with_source_and_edit(
            &alias_path,
            second_source,
            colliding_id,
            Some(&unrelated_noop),
            Some(first_source),
        )
        .unwrap();

    let relocated_id = index
        .get_file_id_for_key(&first_key)
        .expect("the first physical identity must remain indexed");
    assert_ne!(relocated_id, colliding_id);
    assert_eq!(index.get_file_id_for_key(&second_key), Some(colliding_id));
    assert!(index.find_symbol("FirstTarget").is_some());
    assert!(index.find_symbol("SecondTarget").is_some());
    let inner = index.inner.write();
    assert_eq!(
        inner
            .parse_tree_cache
            .peek(&relocated_id)
            .map(|(_, source)| source.as_str()),
        Some(first_source)
    );
    assert_eq!(
        inner
            .parse_tree_cache
            .peek(&colliding_id)
            .map(|(_, source)| source.as_str()),
        Some(second_source)
    );
}

#[cfg(unix)]
#[test]
fn current_disk_read_rejects_an_alias_retargeted_after_read() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("Aliased.java"), "class FirstTarget {}").unwrap();
    std::fs::write(second.join("Aliased.java"), "class SecondTarget {}").unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let alias_path = alias.join("Aliased.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&alias_path).unwrap();
    let file_id = index.get_file_id(&alias_path).unwrap();

    let content = index.get_current_file_contents_after_read_for_test(file_id, || {
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
    });

    assert!(content.is_none());
    assert!(index.find_symbol("FirstTarget").is_some());
    assert!(index.find_symbol("SecondTarget").is_none());
}

#[test]
fn remove_file_retries_if_the_registered_path_changes_before_lock_acquisition() {
    use std::sync::{mpsc, Arc};

    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Before.java");
    let new_path = temp.path().join("After.java");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(&old_path, "class Before {}")
        .unwrap();
    let file_id = index.get_file_id(&old_path).unwrap();
    let old_key = path_lookup_key(&old_path);
    let new_key = path_lookup_key(&new_path);

    let (selected_tx, selected_rx) = mpsc::sync_channel(0);
    let (continue_tx, continue_rx) = mpsc::sync_channel(0);
    let (done_tx, done_rx) = mpsc::sync_channel(0);
    let removing_index = Arc::clone(&index);
    let remover = std::thread::spawn(move || {
        removing_index.remove_file_after_key_selection_for_test(file_id, |selected| {
            selected_tx.send(selected.cloned()).unwrap();
            continue_rx.recv().unwrap();
        });
        done_tx.send(()).unwrap();
    });

    assert_eq!(selected_rx.recv().unwrap(), Some(old_key));
    let (locked_old_key, old_path_lock) = index.establish_path_lock(&old_path);
    let (locked_new_key, new_path_lock) = index.establish_path_lock(&new_path);
    let file_lock = index.get_file_lock(file_id);
    let old_path_guard = old_path_lock.lock().unwrap();
    let new_path_guard = new_path_lock.lock().unwrap();
    let file_guard = file_lock.lock().unwrap();
    index
        .index_moved_file_with_keys_and_locks_held(
            &old_path,
            &locked_old_key,
            &new_path,
            &locked_new_key,
            "class After {}",
            file_id,
        )
        .unwrap();
    drop(file_guard);
    drop(file_lock);
    drop(old_path_guard);
    drop(old_path_lock);

    continue_tx.send(()).unwrap();
    assert_eq!(selected_rx.recv().unwrap(), Some(new_key));
    continue_tx.send(()).unwrap();
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

    drop(new_path_guard);
    drop(new_path_lock);
    done_rx.recv().unwrap();
    remover.join().unwrap();

    assert!(index.get_file_id(&new_path).is_none());
    assert!(index.find_symbol("After").is_none());
}

#[cfg(unix)]
#[test]
fn stale_display_removal_locks_requested_and_stored_identities() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};

    let temp = TempDir::new().unwrap();
    let real = temp.path().join("real");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    let alias_path = alias.join("Aliased.java");
    let old_source = "class BeforeOpen {}";
    std::fs::write(real.join("Aliased.java"), old_source).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();

    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(&alias_path, old_source)
        .unwrap();
    let file_id = index.get_file_id(&alias_path).unwrap();
    let stored_key = index.get_file_path_key(file_id).unwrap();
    std::fs::remove_file(&alias).unwrap();
    let requested_key = path_lookup_key(&alias_path);
    assert_ne!(stored_key, requested_key);

    let requested_lock = index.get_path_lock_for_key(&requested_key);
    let requested_guard = requested_lock.lock().unwrap();
    let editor_owned = Arc::new(AtomicBool::new(false));
    let (selected_tx, selected_rx) = mpsc::channel();
    let worker_index = Arc::clone(&index);
    let worker_path = alias_path.clone();
    let worker_editor_owned = Arc::clone(&editor_owned);
    let worker = std::thread::spawn(move || {
        worker_index.remove_file_revision_after_key_selection_for_test(
            &worker_path,
            file_id,
            move |keys| selected_tx.send(keys.to_vec()).unwrap(),
            || !worker_editor_owned.load(Ordering::SeqCst),
            || {},
        )
    });

    let selected = selected_rx.recv().unwrap();
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&requested_key));
    assert!(selected.contains(&stored_key));

    let file_lock = index.get_file_lock(file_id);
    let file_guard = file_lock.lock().unwrap();
    index
        .index_file_with_key_and_locks_held(
            &alias_path,
            &requested_key,
            "class OpenedDuringDelete {}",
            file_id,
        )
        .unwrap();
    editor_owned.store(true, Ordering::SeqCst);
    drop(file_guard);
    drop(file_lock);
    drop(requested_guard);
    drop(requested_lock);

    assert_eq!(worker.join().unwrap(), FileRevisionRemovalOutcome::Retained);
    assert_eq!(index.get_file_id_for_key(&requested_key), Some(file_id));
    let relocated_id = index.get_file_id_for_key(&stored_key).unwrap();
    assert_ne!(relocated_id, file_id);
    assert!(index.find_symbol("BeforeOpen").is_some());
    assert!(index.find_symbol("OpenedDuringDelete").is_some());
}

#[test]
fn unpublished_revision_locks_can_be_reclaimed_after_external_publication_fails() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Unpublished.java");
    let file_id = FileId::new(1_000_001);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    let (path_key, path_lock) = index.establish_path_lock(&path);
    let file_lock = index.get_file_lock(file_id);
    {
        let _path_guard = path_lock.lock().unwrap();
        let _file_guard = file_lock.lock().unwrap();
    }
    drop(path_lock);
    drop(file_lock);

    index.reclaim_unpublished_revision_locks_for_key(&path_key, file_id);

    assert!(!index.path_locks.contains_key(&path_lookup_key(&path)));
    assert!(!index.file_locks.contains_key(&file_id));
}

#[test]
fn workspace_subtree_snapshot_is_ordered_and_scoped() {
    let temp = TempDir::new().unwrap();
    let subtree = temp.path().join("src/pkg");
    let first_path = subtree.join("A.java");
    let second_path = subtree.join("nested/B.java");
    let outside_path = temp.path().join("src/Other.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(&second_path, "class B {}", FileId::new(52))
        .unwrap();
    index
        .index_file_with_source_and_id(&outside_path, "class Other {}", FileId::new(53))
        .unwrap();
    index
        .index_file_with_source_and_id(&first_path, "class A {}", FileId::new(51))
        .unwrap();
    assert_eq!(
        index.workspace_files_under(&subtree),
        vec![
            (FileId::new(51), first_path),
            (FileId::new(52), second_path)
        ]
    );

    let (bounded, truncated) = index.workspace_files_under_bounded(&subtree, 1);
    assert_eq!(bounded.len(), 1);
    assert!(truncated);

    let (complete, truncated) = index.workspace_files_under_bounded(&subtree, 2);
    assert_eq!(complete.len(), 2);
    assert!(!truncated);
}

#[test]
fn moved_revision_preserves_its_identity_and_replaces_old_declarations_atomically() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Old.java");
    let new_path = temp.path().join("New.java");
    let importer_path = temp.path().join("Importer.java");
    let file_id = FileId::new(1_000_010);
    let importer_id = FileId::new(1_000_011);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(&old_path, "package old; public class Old {}", file_id)
        .unwrap();
    index
        .index_file_with_source_and_id(
            &importer_path,
            "package app; import old.Old; class Importer { Old value; }",
            importer_id,
        )
        .unwrap();

    let (old_key, old_path_lock) = index.establish_path_lock(&old_path);
    let (new_key, new_path_lock) = index.establish_path_lock(&new_path);
    let file_lock = index.get_file_lock(file_id);
    let _old_path_guard = old_path_lock.lock().unwrap();
    let _new_path_guard = new_path_lock.lock().unwrap();
    let _file_guard = file_lock.lock().unwrap();
    let stats = index
        .index_moved_file_with_keys_and_locks_held(
            &old_path,
            &old_key,
            &new_path,
            &new_key,
            "package moved; public class New {}",
            file_id,
        )
        .unwrap();

    assert_eq!(index.get_file_id(&old_path), None);
    assert_eq!(index.get_file_id(&new_path), Some(file_id));
    assert!(index.find_symbol("old.Old").is_none());
    assert!(index.find_symbol("moved.New").is_some());
    assert_eq!(stats.dependent_files, vec![importer_id]);
    assert!(stats.retired_file_ids.is_empty());
}

#[test]
fn moved_revision_retires_a_closed_destination_without_reindexing_it() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Old.java");
    let new_path = temp.path().join("Destination.java");
    let moved_id = FileId::new(1_000_020);
    let destination_id = FileId::new(1_000_021);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(&old_path, "class Old {}", moved_id)
        .unwrap();
    index
        .index_file_with_source_and_id(&new_path, "class ReplacedDestination {}", destination_id)
        .unwrap();

    let (old_key, old_path_lock) = index.establish_path_lock(&old_path);
    let (new_key, new_path_lock) = index.establish_path_lock(&new_path);
    let moved_file_lock = index.get_file_lock(moved_id);
    let destination_file_lock = index.get_file_lock(destination_id);
    let _old_path_guard = old_path_lock.lock().unwrap();
    let _new_path_guard = new_path_lock.lock().unwrap();
    let _moved_file_guard = moved_file_lock.lock().unwrap();
    let _destination_file_guard = destination_file_lock.lock().unwrap();
    let stats = index
        .index_moved_file_with_keys_and_locks_held(
            &old_path,
            &old_key,
            &new_path,
            &new_key,
            "class MovedIntoDestination {}",
            moved_id,
        )
        .unwrap();

    assert_eq!(index.get_file_id(&old_path), None);
    assert_eq!(index.get_file_id(&new_path), Some(moved_id));
    assert!(index.find_symbol("Old").is_none());
    assert!(index.find_symbol("ReplacedDestination").is_none());
    assert!(index.find_symbol("MovedIntoDestination").is_some());
    assert_eq!(stats.retired_file_ids, vec![destination_id]);
    assert!(!stats.dependent_files.contains(&destination_id));
}

#[test]
fn moved_revision_masks_removed_members_from_source_and_destination() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Local.kt");
    let new_path = temp.path().join("Destination.kt");
    let moved_id = FileId::new(1_000_025);
    let destination_id = FileId::new(1_000_026);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(
            &old_path,
            "package same\nclass Local { fun obsolete() = 1; fun use() = obsolete() }",
            moved_id,
        )
        .unwrap();
    index
        .index_file_with_source_and_id(
            &new_path,
            "package same\nclass Destination { fun stale() = 1 }",
            destination_id,
        )
        .unwrap();
    let obsolete = index
        .get_symbol("same.Local.obsolete")
        .expect("old moved method");
    assert_eq!(index.find_usages(&obsolete.fqn).len(), 1);

    let (old_key, old_path_lock) = index.establish_path_lock(&old_path);
    let (new_key, new_path_lock) = index.establish_path_lock(&new_path);
    let moved_file_lock = index.get_file_lock(moved_id);
    let destination_file_lock = index.get_file_lock(destination_id);
    let _old_path_guard = old_path_lock.lock().unwrap();
    let _new_path_guard = new_path_lock.lock().unwrap();
    let _moved_file_guard = moved_file_lock.lock().unwrap();
    let _destination_file_guard = destination_file_lock.lock().unwrap();
    index
        .index_moved_file_with_keys_and_locks_held(
            &old_path,
            &old_key,
            &new_path,
            &new_key,
            "package same\nclass Local { fun use() = obsolete() }",
            moved_id,
        )
        .unwrap();

    assert!(index.get_symbol("same.Local.obsolete").is_none());
    assert!(index.get_symbol("same.Destination.stale").is_none());
    assert!(
        index.find_usages(&obsolete.fqn).is_empty(),
        "move extraction must not resolve through either replaced revision"
    );
}

#[test]
fn stale_moved_revision_leaves_both_paths_untouched() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("Old.java");
    let new_path = temp.path().join("New.java");
    let current_id = FileId::new(1_000_030);
    let stale_id = FileId::new(1_000_031);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(&old_path, "class Current {}", current_id)
        .unwrap();

    let (old_key, old_path_lock) = index.establish_path_lock(&old_path);
    let (new_key, new_path_lock) = index.establish_path_lock(&new_path);
    let stale_file_lock = index.get_file_lock(stale_id);
    let _old_path_guard = old_path_lock.lock().unwrap();
    let _new_path_guard = new_path_lock.lock().unwrap();
    let _stale_file_guard = stale_file_lock.lock().unwrap();
    let result = index.index_moved_file_with_keys_and_locks_held(
        &old_path,
        &old_key,
        &new_path,
        &new_key,
        "class Stale {}",
        stale_id,
    );

    assert!(result.is_err());
    assert_eq!(index.get_file_id(&old_path), Some(current_id));
    assert_eq!(index.get_file_id(&new_path), None);
    assert!(index.find_symbol("Current").is_some());
    assert!(index.find_symbol("Stale").is_none());
}

#[test]
fn reindex_and_delete_remove_wildcard_import_dependency_edges() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Importer.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let with_import = "package app; import com.acme.model.*; class Importer { Widget value; }";

    index.index_file_with_source(&path, with_import).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    assert!(index
        .get_dependent_files("com.acme.model.Widget")
        .contains(&file_id));

    index
        .index_file_with_source(&path, "package app; class Importer {}")
        .unwrap();
    assert!(!index
        .get_dependent_files("com.acme.model.Widget")
        .contains(&file_id));

    index.index_file_with_source(&path, with_import).unwrap();
    assert!(index
        .get_dependent_files("com.acme.model.Widget")
        .contains(&file_id));
    index.remove_file(file_id);
    assert!(!index
        .get_dependent_files("com.acme.model.Widget")
        .contains(&file_id));
}

#[test]
fn declaration_absence_does_not_destroy_importer_edges() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("Target.java");
    let importer_path = temp.path().join("Importer.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &target_path,
            "package api; class Target { void before() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &importer_path,
            "package app; import api.Target; class Importer { Target value; }",
        )
        .unwrap();
    let importer_id = index.get_file_id(&importer_path).unwrap();
    assert!(index
        .get_dependent_files("api.Target")
        .contains(&importer_id));

    index
        .index_file_with_source(
            &target_path,
            "package api; class Target { void after() {} }",
        )
        .unwrap();
    assert!(
        index
            .get_dependent_files("api.Target")
            .contains(&importer_id),
        "a metadata replacement must preserve incoming dependency edges"
    );

    let stats = index
        .index_file_with_source(&target_path, "package api; class RenamedTarget {}")
        .unwrap();
    assert!(stats
        .affected_fqns
        .iter()
        .any(|fqn| fqn.as_ref() == "api.Target"));
    assert!(stats
        .affected_fqns
        .iter()
        .any(|fqn| fqn.as_ref() == "api.RenamedTarget"));
    assert_eq!(stats.dependent_files, vec![importer_id]);
    assert!(
        index
            .get_dependent_files("api.Target")
            .contains(&importer_id),
        "imports remain meaningful unresolved dependencies while a target is absent"
    );

    let reappeared = index
        .index_file_with_source(&target_path, "package api; class Target {}")
        .unwrap();
    assert_eq!(reappeared.dependent_files, vec![importer_id]);
}

#[tokio::test]
async fn test_search() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file = root.join("MyService.java");
    std::fs::write(&file, "package com.example;\npublic class MyService {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file).await.unwrap();

    let results = index.search("service");
    assert_eq!(results.len(), 1);
    assert_eq!(&*results[0].name, "MyService");
}

#[test]
fn broad_search_is_bounded_and_prioritizes_workspace_declarations() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let external = (0..150)
        .map(|number| {
            synthetic_type(
                &format!("external.Type{number:03}"),
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )
        })
        .collect();
    index.add_external_symbols_batch(external);

    for name in ["LocalOne", "LocalTwo"] {
        index
            .index_file_with_source(
                &temp.path().join(format!("{name}.java")),
                &format!("package workspace; class {name} {{}}"),
            )
            .unwrap();
    }

    let results = index.search("");
    assert_eq!(results.len(), 100);
    assert!(
        results[..2]
            .iter()
            .all(|location| !index.is_external_file(location.file_id)),
        "workspace results must precede external matches"
    );
    assert!(results[2..]
        .iter()
        .all(|location| index.is_external_file(location.file_id)));
}

#[test]
fn descriptorless_symbol_lookup_requires_one_callable_declaration() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Service.java");
    index
        .index_file_with_source(
            &path,
            r#"package lookup;
               class Service {
                   void pick(int value) {}
                   void pick(String value) {}
                   void only(int value) {}
               }"#,
        )
        .unwrap();

    assert!(index.get_symbol("lookup.Service.pick").is_none());
    assert_eq!(
        index
            .get_symbol("lookup.Service.only")
            .expect("unique descriptorless compatibility lookup")
            .fqn
            .as_ref(),
        "lookup.Service.only(int)",
    );
}

#[tokio::test]
async fn test_get_fqns_by_simple_prefix_case_insensitive() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file1 = root.join("SystemConfig.java");
    std::fs::write(&file1, "package com.example;\npublic class SystemConfig {}").unwrap();

    let file2 = root.join("SystemConsole.java");
    std::fs::write(
        &file2,
        "package com.example;\npublic class SystemConsole {}",
    )
    .unwrap();

    let file3 = root.join("Other.java");
    std::fs::write(&file3, "package com.example;\npublic class Other {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file1).await.unwrap();
    index.index_file(&file2).await.unwrap();
    index.index_file(&file3).await.unwrap();

    let results = index.get_fqns_by_simple_prefix("systemco", 10);
    assert!(results.contains(&"com.example.SystemConfig".into()));
    assert!(results.contains(&"com.example.SystemConsole".into()));
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_get_constructors_for_class_from_index() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("User.java");

    std::fs::write(
        &file_path,
        r#"
            package com.example;
            public class User {
                public User() {}
                public User(String name) {}
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let constructors = index.get_constructors_for_class("com.example.User");
    assert_eq!(constructors.len(), 2);
    assert!(constructors
        .iter()
        .all(|ctor| ctor.kind == SymbolKind::CONSTRUCTOR));
}

#[tokio::test]
async fn test_remove_file_cleans_constructor_index() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("User.java");

    std::fs::write(
        &file_path,
        r#"
            package com.example;
            public class User {
                public User() {}
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    let constructors = index.get_constructors_for_class("com.example.User");
    assert_eq!(constructors.len(), 1);

    let file_id = index.get_file_id(&file_path).unwrap();
    index.remove_file(file_id);

    let constructors_after = index.get_constructors_for_class("com.example.User");
    assert!(constructors_after.is_empty());
}

#[tokio::test]
async fn test_find_class_by_suffix_matches_inner_class() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("Outer.java");
    std::fs::write(
        &file_path,
        r#"
            package com.example;

            public class Outer {
                public static class Inner {}
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    let match_fqn = index
        .find_class_by_suffix("Outer.Inner")
        .expect("expected inner class to be indexed");
    assert_eq!(match_fqn, "com.example.Outer.Inner");

    // Verify we don't get a match for a missing inner type
    assert!(
        index.find_class_by_suffix("Outer.Missing").is_none(),
        "suffix lookup should return None when no class matches"
    );
}

#[tokio::test]
async fn test_find_class_by_suffix_dot_boundary() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Index both UserProfile and Profile
    let user_profile_path = root.join("UserProfile.java");
    std::fs::write(
        &user_profile_path,
        "package com.example;\npublic class UserProfile {}",
    )
    .unwrap();

    let profile_path = root.join("Profile.java");
    std::fs::write(
        &profile_path,
        "package com.example.model;\npublic class Profile {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&user_profile_path).await.unwrap();
    index.index_file(&profile_path).await.unwrap();

    // Searching for "Profile" should match com.example.model.Profile (dot boundary)
    // and NOT com.example.UserProfile (no dot boundary)
    let result = index.find_class_by_suffix("Profile");
    assert!(result.is_some(), "Should find a class matching 'Profile'");
    assert_eq!(
        result.unwrap(),
        "com.example.model.Profile",
        "Should match Profile with dot boundary, not UserProfile"
    );
}

// ==================== Reverse Dependency Tracking Tests ====================

#[tokio::test]
async fn test_reverse_deps_explicit_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java (the imported class)
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example;
            public class User {
                public String getName() { return ""; }
            }
        "#,
    )
    .unwrap();

    // Create Main.java (imports User)
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.User;
            public class Main {
                public void test() {
                    User user = new User();
                }
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    // Main.java should be tracked as depending on User
    let dependents = index.get_dependent_files("com.example.User");
    let main_file_id = index.get_file_id(&main_path).unwrap();

    assert!(
        dependents.contains(&main_file_id),
        "Main.java should be a dependent of com.example.User"
    );
}

#[tokio::test]
async fn test_reverse_deps_wildcard_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example.model;
            public class User {}
        "#,
    )
    .unwrap();

    // Create Main.java with wildcard import
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.model.*;
            public class Main {
                public void test() {
                    User user = new User();
                }
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    // Main.java should be found via wildcard import resolution
    let dependents = index.get_dependent_files("com.example.model.User");
    let main_file_id = index.get_file_id(&main_path).unwrap();

    assert!(
        dependents.contains(&main_file_id),
        "Main.java should be a dependent of com.example.model.User via wildcard import"
    );
}

#[tokio::test]
async fn static_imports_follow_descriptor_qualified_member_changes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let utils_path = root.join("Utils.java");
    let explicit_path = root.join("Explicit.java");
    let wildcard_path = root.join("Wildcard.java");

    let old_utils = r#"
        package api;
        public class Utils {
            public static String helper(String value) { return value; }
            public static int COUNT = 1;
        }
    "#;
    std::fs::write(&utils_path, old_utils).unwrap();
    std::fs::write(
        &explicit_path,
        r#"
            package app;
            import static api.Utils.helper;
            public class Explicit { String value = helper("x"); }
        "#,
    )
    .unwrap();
    std::fs::write(
        &wildcard_path,
        r#"
            package app;
            import static api.Utils.*;
            public class Wildcard { String value = helper("x") + COUNT; }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index
        .index_file_with_source(&utils_path, old_utils)
        .unwrap();
    index.index_file(&explicit_path).await.unwrap();
    index.index_file(&wildcard_path).await.unwrap();
    let utils_id = index.get_file_id(&utils_path).unwrap();
    let explicit_id = index.get_file_id(&explicit_path).unwrap();
    let wildcard_id = index.get_file_id(&wildcard_path).unwrap();

    let method_fqn = index
        .get_file_symbols(utils_id)
        .into_iter()
        .find(|fqn| fqn.starts_with("api.Utils.helper("))
        .expect("indexed helper method");
    let method_dependents = index.get_dependent_files(&method_fqn);
    assert!(method_dependents.contains(&explicit_id));
    assert!(method_dependents.contains(&wildcard_id));
    assert!(index
        .get_dependent_files("api.Utils.COUNT")
        .contains(&wildcard_id));

    let new_utils = r#"
        package api;
        public class Utils {
            public static String renamed(String value) { return value; }
            public static int COUNT = 2;
        }
    "#;
    let stats = index
        .index_file_with_source(&utils_path, new_utils)
        .unwrap();
    assert!(stats.dependent_files.contains(&explicit_id));
    assert!(stats.dependent_files.contains(&wildcard_id));
}

#[test]
fn enclosing_call_applicability_uses_assignability_and_never_fabricates_edges() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Accepted.java"),
            "package types; public class Accepted {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Rejected.java"),
            "package types; public final class Rejected {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package app; public class Base { void pick(types.Accepted value) {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Tools.java"),
            "package tools; public class Tools { public static void pick(types.Rejected value) {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package app; import types.Rejected; import static tools.Tools.pick; ",
                "class Child extends Base { void use(Rejected value) { pick(value); pick(unknown); } }",
            ),
        )
        .unwrap();

    let static_pick = index
        .find_methods_by_name("tools.Tools", "pick")
        .pop()
        .expect("static overload");
    assert_eq!(
        index.find_usages(&static_pick.fqn).len(),
        1,
        "indexed Child usages: {:#?}",
        index.get_file_resolved_method_usages(
            index.get_file_id(&temp.path().join("Child.java")).unwrap()
        ),
    );
    let enclosing_pick = index
        .find_methods_by_name("app.Base", "pick")
        .pop()
        .expect("enclosing overload");
    assert!(index.find_usages(&enclosing_pick.fqn).is_empty());
    assert!(index.find_usages("app.Child.pick(_)").is_empty());
}

#[tokio::test]
async fn test_reverse_deps_multiple_dependents() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java
    let user_path = root.join("User.java");
    std::fs::write(&user_path, "package com.example;\npublic class User {}").unwrap();

    // Create two files that import User
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        "package com.example;\nimport com.example.User;\npublic class Main {}",
    )
    .unwrap();

    let service_path = root.join("Service.java");
    std::fs::write(
        &service_path,
        "package com.example;\nimport com.example.User;\npublic class Service {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();
    index.index_file(&service_path).await.unwrap();

    let dependents = index.get_dependent_files("com.example.User");
    let main_id = index.get_file_id(&main_path).unwrap();
    let service_id = index.get_file_id(&service_path).unwrap();

    assert_eq!(dependents.len(), 2, "Should have 2 dependents");
    assert!(
        dependents.contains(&main_id),
        "Main.java should be a dependent"
    );
    assert!(
        dependents.contains(&service_id),
        "Service.java should be a dependent"
    );
}

#[tokio::test]
async fn test_get_file_symbols() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("User.java");
    std::fs::write(
        &file_path,
        r#"
            package com.example;
            public class User {
                public String getName() { return ""; }
                public void setName(String name) {}
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let file_id = index.get_file_id(&file_path).unwrap();
    let symbols = index.get_file_symbols(file_id);

    // The class should definitely be indexed
    assert!(
        symbols.contains(&"com.example.User".into()),
        "Expected com.example.User, got: {:?}",
        symbols
    );

    // get_file_symbols returns the FQNs stored in file_symbols
    // This may only include the class, not methods - that's fine for our use case
    // What matters is that we can identify which files define which types
    assert!(!symbols.is_empty(), "File should have at least one symbol");
}

#[test]
fn file_dependency_fqns_include_resolved_method_usages() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let provider = temp.path().join("Service.java");
    let client = temp.path().join("Client.java");

    index
        .index_file_with_source(
            &provider,
            "package api; public class Service { public String load() { return null; } }",
        )
        .unwrap();
    let client_stats = index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import api.Service; ",
                "class Client { String run(Service service) { return service.load(); } }"
            ),
        )
        .unwrap();
    assert!(client_stats.symbols_added > 0);

    let client_id = index.get_file_id(&client).unwrap();
    let dependencies = index
        .get_current_file_dependency_fqns(client_id)
        .unwrap()
        .fqns;
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency.starts_with("api.Service.load(")),
        "resolved method dependency missing from {dependencies:?}"
    );
}

#[test]
fn exact_usage_and_dependency_queries_include_extensionless_java_sources() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let provider = temp.path().join("Service.java");
    let client = temp.path().join("Client");

    index
        .index_file_with_source(
            &provider,
            "package api; public class Service { public void load() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import api.Service; class Client { ",
                "void run(Service service) { service.load(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let client_revision = index.current_file_revision(client_id).unwrap();
    let dependencies = index.get_current_file_dependency_fqns(client_id).unwrap();
    assert_eq!(dependencies.revision, client_revision);
    assert!(
        dependencies
            .fqns
            .iter()
            .any(|fqn| fqn.as_ref() == "api.Service"),
        "the extensionless Java import must remain a dependency: {:?}",
        dependencies.fqns
    );
    assert!(
        dependencies
            .fqns
            .iter()
            .any(|fqn| fqn.starts_with("api.Service.load(")),
        "the extensionless Java call must remain a dependency: {:?}",
        dependencies.fqns
    );

    let usages = index.find_current_usages("api.Service").unwrap();
    assert!(usages.scanned_revisions().contains(&client_revision));
    assert!(usages.files.iter().any(|file| {
        file.revision == client_revision
            && file
                .locations
                .iter()
                .any(|location| location.file_id == client_id)
    }));
}

#[test]
fn defining_file_reverse_index_tracks_duplicate_declarations_and_removal() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let first = temp.path().join("first/Service.java");
    let second = temp.path().join("second/Service.java");
    let source = "package api; class Service { String load() { return null; } }";

    index.index_file_with_source(&first, source).unwrap();
    index.index_file_with_source(&second, source).unwrap();
    let first_id = index.get_file_id(&first).unwrap();
    let second_id = index.get_file_id(&second).unwrap();

    assert_eq!(
        index.find_files_defining_symbol("api.Service.load()"),
        vec![first_id, second_id]
    );
    assert_eq!(
        index.find_files_defining_symbols(&[Arc::from("api.Service.load()")]),
        vec![first_id, second_id]
    );

    index.remove_file(first_id);
    assert_eq!(
        index.find_files_defining_symbol("api.Service.load()"),
        vec![second_id]
    );
}

#[test]
fn duplicate_workspace_owner_promotion_preserves_one_complete_source_shape() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let preferred_path = temp.path().join("a/Service.java");
    let fallback_path = temp.path().join("z/Service.java");
    let fallback = concat!(
        "package dup; class Service extends FallbackBase { ",
        "int onlyFallback; int value() { return 1; } }"
    );
    let preferred = concat!(
        "package dup; class Service extends PreferredBase { ",
        "String onlyPreferred; String value() { return \"a\"; } }"
    );

    // Publication order deliberately opposes stable normalized-path authority.
    index
        .index_file_with_source(&fallback_path, fallback)
        .unwrap();
    index
        .index_file_with_source(&preferred_path, preferred)
        .unwrap();
    let preferred_id = index.get_file_id(&preferred_path).unwrap();
    let fallback_id = index.get_file_id(&fallback_path).unwrap();

    assert_eq!(
        index
            .get_symbol("dup.Service")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("PreferredBase")
    );
    assert_eq!(
        index
            .get_symbol("dup.Service.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("java.lang.String")
    );
    assert!(index.get_symbol("dup.Service.onlyPreferred").is_some());
    assert!(index.get_symbol("dup.Service.onlyFallback").is_none());

    // Relocating the preferred source must retain its exact Arc-backed
    // candidates and path-based precedence, while the caller keeps its ID.
    let editor_path = temp.path().join("Editor.java");
    index
        .index_file_with_source_and_id(&editor_path, "class Editor {}", preferred_id)
        .unwrap();
    let relocated_preferred_id = index.get_file_id(&preferred_path).unwrap();
    assert_ne!(relocated_preferred_id, preferred_id);
    assert_eq!(index.get_file_id(&editor_path), Some(preferred_id));
    assert_eq!(
        index.find_symbol("dup.Service").unwrap().file_id,
        relocated_preferred_id
    );
    assert!(index.get_symbol("dup.Service.onlyPreferred").is_some());

    index.remove_file(relocated_preferred_id);
    assert_eq!(
        index.find_symbol("dup.Service").unwrap().file_id,
        fallback_id
    );
    assert_eq!(
        index
            .get_symbol("dup.Service")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("FallbackBase")
    );
    assert_eq!(
        index
            .get_symbol("dup.Service.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("int")
    );
    assert!(index.get_symbol("dup.Service.onlyPreferred").is_none());
    assert!(index.get_symbol("dup.Service.onlyFallback").is_some());
}

#[test]
fn surviving_workspace_duplicate_precedes_external_owner_and_members() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let dependency = PathBuf::from("/deps/duplicate.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&dependency));
    let dependency_id = index
        .inner
        .read()
        .external_file_id_for_path(&dependency)
        .expect("registered dependency");
    let mut external_value = synthetic_method("dup.Owner", "value");
    external_value.return_type = Some("external".into());
    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "dup.Owner",
                SymbolKind::CLASS,
                Some("dep.ExternalBase"),
                &[],
                crate::Modifiers::default(),
            ),
            external_value,
            synthetic_method("dup.Owner", "externalOnly"),
        ],
        dependency_id,
    );

    let preferred_path = temp.path().join("a/Owner.java");
    let fallback_path = temp.path().join("z/Owner.java");
    index
        .index_file_with_source(
            &fallback_path,
            "package dup; class Owner extends WorkspaceFallback { int fallbackOnly; int value() { return 1; } }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &preferred_path,
            "package dup; class Owner extends WorkspacePreferred { String preferredOnly; String value() { return null; } }",
        )
        .unwrap();
    let preferred_id = index.get_file_id(&preferred_path).unwrap();
    let fallback_id = index.get_file_id(&fallback_path).unwrap();

    index.remove_file(preferred_id);
    assert_eq!(index.find_symbol("dup.Owner").unwrap().file_id, fallback_id);
    assert_eq!(
        index
            .get_symbol("dup.Owner.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("int")
    );
    assert!(index.get_symbol("dup.Owner.fallbackOnly").is_some());
    assert!(index.get_symbol("dup.Owner.externalOnly()").is_none());

    index.remove_file(fallback_id);
    assert_eq!(
        index.find_symbol("dup.Owner").unwrap().file_id,
        dependency_id
    );
    assert_eq!(
        index
            .get_symbol("dup.Owner.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("external")
    );
    assert!(index.get_symbol("dup.Owner.externalOnly()").is_some());
}

#[test]
fn duplicate_workspace_cache_keeps_each_files_exact_shape_and_range() {
    let temp = TempDir::new().unwrap();
    let preferred_path = temp.path().join("a/Service.java");
    let fallback_path = temp.path().join("z/Service.java");
    std::fs::create_dir_all(preferred_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(fallback_path.parent().unwrap()).unwrap();
    let preferred = "package cachedup; class Service { String value() { return null; } }";
    let fallback = concat!(
        "package cachedup;                         ",
        "class Service { int value() { return 1; } }"
    );
    std::fs::write(&preferred_path, preferred).unwrap();
    std::fs::write(&fallback_path, fallback).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&fallback_path, fallback)
        .unwrap();
    index
        .index_file_with_source(&preferred_path, preferred)
        .unwrap();
    let cache = index.extract_cache_data();
    let preferred_value = cache[&preferred_path]
        .symbols
        .iter()
        .find(|symbol| symbol.fqn == "cachedup.Service.value()")
        .unwrap();
    let fallback_value = cache[&fallback_path]
        .symbols
        .iter()
        .find(|symbol| symbol.fqn == "cachedup.Service.value()")
        .unwrap();
    assert_eq!(
        preferred_value.return_type.as_deref(),
        Some("java.lang.String")
    );
    assert_eq!(fallback_value.return_type.as_deref(), Some("int"));
    assert_ne!(preferred_value.offset, fallback_value.offset);
    assert!(preferred_value.end_offset > preferred_value.offset);
    assert!(fallback_value.end_offset > fallback_value.offset);

    let restored = WorkspaceIndex::new(temp.path().to_path_buf());
    assert!(restored.restore_from_cache_batch(&cache).is_empty());
    let active = restored.get_symbol("cachedup.Service.value()").unwrap();
    assert_eq!(active.return_type.as_deref(), Some("java.lang.String"));
    assert_eq!(active.range.start, preferred_value.offset);
    assert_eq!(active.range.end, preferred_value.end_offset);
}

#[test]
fn cache_round_trip_preserves_contextual_supertype_provenance() {
    let temp = TempDir::new().unwrap();
    let q_path = temp.path().join("q/Base.java");
    let child_path = temp.path().join("p/Child.java");
    let r_path = temp.path().join("r/Base.java");
    std::fs::create_dir_all(q_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(child_path.parent().unwrap()).unwrap();
    std::fs::create_dir_all(r_path.parent().unwrap()).unwrap();
    let q_source = "package q; public class Base {}";
    let child_source = "package p; import q.*; import r.*; class Child extends Base {}";
    std::fs::write(&q_path, q_source).unwrap();
    std::fs::write(&child_path, child_source).unwrap();

    let indexed = WorkspaceIndex::new(temp.path().to_path_buf());
    indexed.index_file_with_source(&q_path, q_source).unwrap();
    indexed
        .index_file_with_source(&child_path, child_source)
        .unwrap();
    assert_eq!(indexed.direct_supertypes("p.Child"), ["q.Base"]);
    let cache = indexed.extract_cache_data();
    assert_eq!(
        cache[&child_path]
            .symbols
            .iter()
            .find(|symbol| symbol.fqn == "p.Child")
            .and_then(|symbol| symbol.parent_class.as_deref()),
        Some("Base"),
        "cache extraction must not freeze the currently unique q.Base binding"
    );

    let restored = WorkspaceIndex::new(temp.path().to_path_buf());
    assert!(restored.restore_from_cache_batch(&cache).is_empty());
    assert_eq!(restored.direct_supertypes("p.Child"), ["q.Base"]);
    assert_eq!(
        restored.get_inheritance_chain("p.Child"),
        ["p.Child", "q.Base"]
    );

    let r_source = "package r; public class Base {}";
    std::fs::write(&r_path, r_source).unwrap();
    restored.index_file_with_source(&r_path, r_source).unwrap();
    assert!(restored.direct_supertypes("p.Child").is_empty());
    assert_eq!(restored.get_inheritance_chain("p.Child"), ["p.Child"]);
}

#[test]
fn per_source_symbol_registry_is_pointer_compact() {
    assert!(
        std::mem::size_of::<Arc<crate::Symbol>>() < std::mem::size_of::<Arc<str>>(),
        "per-source metadata should replace, not enlarge, the old FQN list pointer"
    );
    assert_eq!(
        std::mem::size_of::<SourceSymbolCandidate>(),
        8,
        "reverse candidates must remain a compact FileId+ordinal pair"
    );
    assert!(
        std::mem::size_of::<SmallVec<[SourceSymbolCandidate; 1]>>() <= 32,
        "the inline-one reverse candidate value must stay within four words"
    );
    assert!(
        std::mem::size_of::<crate::Symbol>() <= 256,
        "Symbol growth multiplies across every declaration"
    );
    assert_eq!(
        std::mem::size_of::<crate::SymbolLocation>(),
        88,
        "SymbolLocation is high-cardinality persistent state and must not grow"
    );
    assert!(
        std::mem::size_of::<FileRevisionToken>() <= 16,
        "one exact token should remain at most two 64-bit words"
    );
    assert!(
        std::mem::size_of::<PublishedRevision>() <= 24,
        "per-file revision metadata must remain at most three words"
    );
    assert_eq!(
        std::mem::size_of::<WorkspaceRevision>(),
        8,
        "the coherent hierarchy generation must remain one word"
    );
    assert!(
        std::mem::size_of::<ExternalSourceRevisionToken>() <= 16,
        "external publication authority must remain at most two words"
    );
    assert!(
        std::mem::size_of::<ExternalSourceIdentity>() <= 24,
        "a retained external identity must remain at most one tagged fat pointer"
    );
    assert!(
        std::mem::size_of::<RegisteredExternalSource>() <= 48,
        "identity plus display spelling must remain at most two tagged fat pointers"
    );
    assert_eq!(
        std::mem::size_of::<PublishedResolvedMethodUsages>(),
        32,
        "resolved calls keep exact file/workspace provenance inline with one Arc slice and no second allocation"
    );
    assert!(
        std::mem::size_of::<CurrentUsageLocations>() <= 56,
        "ephemeral exact usage results should remain one revision word, one Vec, one Arc slice, and one optional resource-proof pointer"
    );
    eprintln!(
        "representation bytes: candidate={}, candidate_smallvec={}, symbol={}, location={}, token={}, external_token={}, published_revision={}, published_method_usages={}",
        std::mem::size_of::<SourceSymbolCandidate>(),
        std::mem::size_of::<SmallVec<[SourceSymbolCandidate; 1]>>(),
        std::mem::size_of::<crate::Symbol>(),
        std::mem::size_of::<crate::SymbolLocation>(),
        std::mem::size_of::<FileRevisionToken>(),
        std::mem::size_of::<ExternalSourceRevisionToken>(),
        std::mem::size_of::<PublishedRevision>(),
        std::mem::size_of::<PublishedResolvedMethodUsages>(),
    );
}

#[test]
fn shadowed_nested_types_and_members_follow_the_selected_outer_source() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let preferred_path = temp.path().join("a/Outer.java");
    let fallback_path = temp.path().join("z/Outer.java");
    index
        .index_file_with_source(
            &fallback_path,
            concat!(
                "package nesteddup; class Outer { ",
                "static class Extra { int fallbackOnly; int value() { return 1; } } }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(&preferred_path, "package nesteddup; class Outer {}")
        .unwrap();
    let use_path = temp.path().join("use/Use.java");
    index
        .index_file_with_source(
            &use_path,
            concat!(
                "package use; import nesteddup.Outer.Extra; ",
                "class Use extends Extra {}"
            ),
        )
        .unwrap();

    assert!(index.get_symbol("nesteddup.Outer.Extra").is_none());
    assert!(index
        .get_symbol("nesteddup.Outer.Extra.fallbackOnly")
        .is_none());
    assert!(
        index.direct_supertypes("use.Use").is_empty(),
        "a nested type belonging to a shadowed outer candidate is not an indexed binding"
    );
    assert_eq!(index.get_inheritance_chain("use.Use"), ["use.Use"]);

    let preferred_id = index.get_file_id(&preferred_path).unwrap();
    index.remove_file(preferred_id);
    assert!(index.get_symbol("nesteddup.Outer.Extra").is_some());
    assert_eq!(
        index.direct_supertypes("use.Use"),
        ["nesteddup.Outer.Extra"]
    );
    assert_eq!(
        index.get_inheritance_chain("use.Use"),
        ["use.Use", "nesteddup.Outer.Extra"],
        "candidate promotion must invalidate contextual chains that were previously missing"
    );
    assert_eq!(
        index
            .get_symbol("nesteddup.Outer.Extra.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("int")
    );
    assert!(index
        .get_symbol("nesteddup.Outer.Extra.fallbackOnly")
        .is_some());
}

#[test]
fn duplicate_nested_type_uses_one_sources_complete_recursive_shape() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let preferred_path = temp.path().join("a/Outer.java");
    let fallback_path = temp.path().join("z/Outer.java");
    index
        .index_file_with_source(
            &fallback_path,
            concat!(
                "package nesteddup; class Outer { static class Inner { ",
                "int fallbackOnly; int value() { return 1; } ",
                "static class Deep { int fallbackDeep; } } }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &preferred_path,
            concat!(
                "package nesteddup; class Outer { static class Inner { ",
                "String preferredOnly; String value() { return null; } } }"
            ),
        )
        .unwrap();

    assert_eq!(
        index
            .get_symbol("nesteddup.Outer.Inner.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("java.lang.String")
    );
    assert!(index
        .get_symbol("nesteddup.Outer.Inner.preferredOnly")
        .is_some());
    assert!(index
        .get_symbol("nesteddup.Outer.Inner.fallbackOnly")
        .is_none());
    assert!(index.get_symbol("nesteddup.Outer.Inner.Deep").is_none());
    assert!(index
        .get_symbol("nesteddup.Outer.Inner.Deep.fallbackDeep")
        .is_none());
}

#[test]
fn same_fqn_type_and_field_have_independent_typed_definitions() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("A.java");
    let source = "package collision; class A { static class B {} int B; }";
    std::fs::write(&path, source).unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();

    let same_fqn = index.inner.read().file_symbols[&file_id]
        .iter()
        .filter(|symbol| symbol.fqn.as_ref() == "collision.A.B")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(same_fqn.len(), 2, "the per-source store must be lossless");
    assert!(same_fqn
        .iter()
        .any(|symbol| crate::is_type_declaration_kind(symbol.kind)));
    assert!(same_fqn
        .iter()
        .any(|symbol| symbol.kind == SymbolKind::FIELD));
    assert!(crate::is_type_declaration_kind(
        index.get_symbol("collision.A.B").unwrap().kind
    ));
    let active_type = index.get_symbol("collision.A.B").unwrap();
    let field = index
        .find_symbol_in_file(file_id, "B", SymbolKind::FIELD, None)
        .expect("value-namespace lookup must retain the field");
    assert_eq!(field.kind, SymbolKind::FIELD);
    let type_definition = index
        .find_symbol_definition(&crate::SymbolTarget::new(
            "collision.A.B",
            crate::DeclarationNamespace::Type,
        ))
        .expect("typed nested-class definition");
    let field_definition = index
        .find_symbol_definition(&crate::SymbolTarget::new(
            "collision.A.B",
            crate::DeclarationNamespace::Value,
        ))
        .expect("typed field definition");
    assert_ne!(
        type_definition.location.range,
        field_definition.location.range
    );
    assert!(Arc::ptr_eq(
        &index
            .find_symbol_in_file(
                file_id,
                "B",
                active_type.kind,
                Some(active_type.range.start)
            )
            .expect("the declaration start belongs to the active type"),
        &active_type,
    ));
    assert!(
        index
            .find_symbol_in_file(file_id, "B", active_type.kind, Some(active_type.range.end))
            .is_none(),
        "TextRange::end is exclusive and must not select the preceding declaration"
    );

    let cached = index.extract_cache_data();
    assert_eq!(
        cached[&path]
            .symbols
            .iter()
            .filter(|symbol| symbol.fqn == "collision.A.B")
            .count(),
        2,
        "cache extraction must retain both typed declarations"
    );
}

#[test]
fn external_nested_type_is_a_nominal_definition_not_an_outer_member() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "java.lang.Thread",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        // Bytecode class declarations deliberately have no `container`:
        // members live in the class file, while the class itself is a nominal
        // declaration with an independently indexed source-level FQN.
        synthetic_type(
            "java.lang.Thread.State",
            SymbolKind::ENUM,
            Some("java.lang.Enum"),
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    let target =
        crate::SymbolTarget::new("java.lang.Thread.State", crate::DeclarationNamespace::Type);
    let definition = index
        .find_symbol_definition(&target)
        .expect("external nested type should retain a typed definition");

    assert_eq!(definition.target, target);
    assert_eq!(definition.symbol.kind, SymbolKind::ENUM);
    assert!(matches!(
        definition.source,
        DefinitionSourceAuthority::External
    ));
}

#[test]
fn constructor_and_same_named_method_are_independent_in_both_orders() {
    for (suffix, declarations) in [
        ("constructor_first", "Widget() {} void Widget() {}"),
        ("method_first", "void Widget() {} Widget() {}"),
    ] {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(format!("Widget_{suffix}.java"));
        let source = format!(
            "package collision; class Widget {{ {declarations} void use() {{ new Widget(); Widget(); }} }}"
        );
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file_with_source(&path, &source).unwrap();
        let constructor = crate::SymbolTarget::new(
            "collision.Widget.Widget()",
            crate::DeclarationNamespace::Constructor,
        );
        let method = crate::SymbolTarget::new(
            "collision.Widget.Widget()",
            crate::DeclarationNamespace::Callable,
        );

        let constructor_definition = index
            .find_symbol_definition(&constructor)
            .expect("constructor definition");
        let method_definition = index
            .find_symbol_definition(&method)
            .expect("ordinary method definition");
        assert_eq!(constructor_definition.symbol.kind, SymbolKind::CONSTRUCTOR);
        assert_eq!(method_definition.symbol.kind, SymbolKind::METHOD);
        assert_ne!(
            constructor_definition.location.range,
            method_definition.location.range
        );
        assert_eq!(
            index.get_constructors_for_class("collision.Widget").len(),
            1
        );
        assert_eq!(
            index
                .find_methods_by_name("collision.Widget", "Widget")
                .len(),
            1
        );

        let constructor_usages = index
            .find_current_usages_for_symbol_target(&constructor)
            .expect("typed constructor usages");
        let method_usages = index
            .find_current_usages_for_symbol_target(&method)
            .expect("typed method usages");
        assert!(constructor_usages
            .locations()
            .all(|usage| usage.kind == SymbolKind::CONSTRUCTOR));
        assert!(method_usages
            .locations()
            .all(|usage| matches!(usage.kind, SymbolKind::METHOD | SymbolKind::FUNCTION)));
        assert_eq!(constructor_usages.locations().count(), 1);
        assert_eq!(method_usages.locations().count(), 1);
        assert!(index
            .find_current_usages("collision.Widget.Widget()")
            .is_err());
    }
}

#[test]
fn large_external_source_candidate_lookup_does_not_scan_its_file_vector() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let source = PathBuf::from("/deps/large.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&source));
    let file_id = index
        .inner
        .read()
        .external_file_id_for_path(&source)
        .expect("registered external source");
    let symbols = (0..8_192)
        .map(|number| {
            synthetic_type(
                &format!("large.Type{number:05}"),
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )
        })
        .collect();
    index.insert_symbols_batch(symbols, file_id);

    let inner = index.inner.read();
    assert_eq!(inner.file_symbols[&file_id].len(), 8_192);
    let candidate = &inner.symbol_definition_files["large.Type08191"][0];
    assert_eq!(candidate.file_id, file_id);
    assert_eq!(
        inner.file_symbols[&candidate.file_id][candidate.ordinal as usize]
            .fqn
            .as_ref(),
        "large.Type08191"
    );
}

#[test]
fn canonical_supertype_resolution_handles_lexical_and_contextual_qualified_names() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let default_outer = temp.path().join("default/Outer.java");
    let imported_outer = temp.path().join("q/Outer.java");
    let current_outer = temp.path().join("p/Outer.java");
    let imported_child = temp.path().join("p/ImportedChild.java");
    let current_child = temp.path().join("p/CurrentChild.java");
    let lexical = temp.path().join("p/Lexical.java");
    let record_lexical = temp.path().join("p/RecordLexical.java");
    index
        .index_file_with_source(&default_outer, "class Outer { static class Base {} }")
        .unwrap();
    index
        .index_file_with_source(
            &imported_outer,
            "package q; public class Outer { public static class Base {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &current_outer,
            "package p; class Outer { static class Base {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &imported_child,
            "package p; import q.Outer; class ImportedChild extends Outer.Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &current_child,
            "package p; class CurrentChild extends Outer.Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &lexical,
            concat!(
                "package p; import q.Base; class Lexical { ",
                "static class Base {} static class Mid { static class Child extends Base {} } }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &record_lexical,
            concat!(
                "package p; record RecordLexical() { record Holder() { ",
                "static class Base {} static class Child extends Base {} } }"
            ),
        )
        .unwrap();

    assert_eq!(
        index
            .find_direct_subtypes("q.Outer.Base")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["p.ImportedChild"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("p.Outer.Base")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["p.CurrentChild"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("p.Lexical.Base")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["p.Lexical.Mid.Child"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("p.RecordLexical.Holder.Base")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["p.RecordLexical.Holder.Child"]
    );
    assert!(index.find_direct_subtypes("Outer.Base").is_empty());
}

#[test]
fn canonical_supertype_resolution_preserves_on_demand_ambiguity() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&temp.path().join("a/Base.java"), "package a; class Base {}")
        .unwrap();
    index
        .index_file_with_source(&temp.path().join("b/Base.java"), "package b; class Base {}")
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Child.java"),
            "package app; import a.*; import b.*; class Child extends Base {}",
        )
        .unwrap();
    assert!(index.find_direct_subtypes("a.Base").is_empty());
    assert!(index.find_direct_subtypes("b.Base").is_empty());

    index
        .index_file_with_source(
            &temp.path().join("java/lang/Record.java"),
            "package java.lang; class Record {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("custom/Record.java"),
            "package custom; class Record {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/AmbiguousRecord.java"),
            "package app; import custom.*; class AmbiguousRecord extends Record {}",
        )
        .unwrap();
    assert!(index.find_direct_subtypes("java.lang.Record").is_empty());
    assert!(index.find_direct_subtypes("custom.Record").is_empty());
}

#[test]
fn qualified_type_suffix_does_not_disambiguate_the_bound_head() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        (
            "a/Outer.java",
            "package a; public class Outer { public static class Inner {} }",
        ),
        (
            "b/Host.java",
            "package b; public class Host { public static class Outer {} }",
        ),
        ("c/Outer.java", "package c; public class Outer {}"),
        (
            "d/Outer.java",
            "package d; public class Outer { public static class Inner {} }",
        ),
        (
            "e/Host.java",
            "package e; public class Host { public static class Outer {} }",
        ),
        (
            "f/Host.java",
            concat!(
                "package f; public class Host { public static class Outer { ",
                "public static class Inner {} } }"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }

    let explicit_path = temp.path().join("app/ExplicitChild.java");
    index
        .index_file_with_source(
            &explicit_path,
            concat!(
                "package app; import a.Outer; import static b.Host.Outer; ",
                "class ExplicitChild extends Outer.Inner {}"
            ),
        )
        .unwrap();
    let explicit_id = index.get_file_id(&explicit_path).unwrap();
    assert_eq!(
        index.resolve_type_name(explicit_id, &[], "Outer.Inner"),
        crate::TypeNameResolution::Ambiguous(vec!["a.Outer".into(), "b.Host.Outer".into()])
    );
    assert!(index.direct_supertypes("app.ExplicitChild").is_empty());

    let wildcard_path = temp.path().join("app/WildcardChild.java");
    index
        .index_file_with_source(
            &wildcard_path,
            concat!(
                "package app; import c.*; import d.*; ",
                "class WildcardChild extends Outer.Inner {}"
            ),
        )
        .unwrap();
    let wildcard_id = index.get_file_id(&wildcard_path).unwrap();
    assert_eq!(
        index.resolve_type_name(wildcard_id, &[], "Outer.Inner"),
        crate::TypeNameResolution::Ambiguous(vec!["c.Outer".into(), "d.Outer".into()])
    );
    assert!(index.direct_supertypes("app.WildcardChild").is_empty());

    let static_wildcard_path = temp.path().join("app/StaticWildcardChild.java");
    index
        .index_file_with_source(
            &static_wildcard_path,
            concat!(
                "package app; import static e.Host.*; import static f.Host.*; ",
                "class StaticWildcardChild extends Outer.Inner {}"
            ),
        )
        .unwrap();
    let static_wildcard_id = index.get_file_id(&static_wildcard_path).unwrap();
    assert_eq!(
        index.resolve_type_name(static_wildcard_id, &[], "Outer.Inner"),
        crate::TypeNameResolution::Ambiguous(vec!["e.Host.Outer".into(), "f.Host.Outer".into(),])
    );
    assert!(index
        .direct_supertypes("app.StaticWildcardChild")
        .is_empty());
}

#[test]
fn on_demand_type_head_obscures_a_same_named_package() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Outer.java"),
            "package p; public class Outer {}",
        )
        .unwrap();
    let imported_head = temp.path().join("q/p.java");
    index
        .index_file_with_source(
            &imported_head,
            "package q; public class p { public static class Outer {} }",
        )
        .unwrap();
    let use_path = temp.path().join("app/Use.java");
    let source = "package app; import q.*; class Use extends p.Outer {}";
    index.index_file_with_source(&use_path, source).unwrap();
    let file_id = index.get_file_id(&use_path).unwrap();
    assert_eq!(
        index.resolve_type_name(file_id, &[], "p.Outer"),
        crate::TypeNameResolution::Exact("q.p.Outer".into())
    );
    assert_eq!(index.direct_supertypes("app.Use"), ["q.p.Outer"]);

    index
        .index_file_with_source(&imported_head, "package q; public class p {}")
        .unwrap();
    assert_eq!(
        index.resolve_type_name(file_id, &[], "p.Outer"),
        crate::TypeNameResolution::Missing,
        "a bound type head with a missing member must not fall back to package p"
    );
    assert!(index.direct_supertypes("app.Use").is_empty());
}

#[test]
fn caller_supplied_type_context_does_not_reuse_stale_indexed_imports() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&temp.path().join("a/Base.java"), "package a; class Base {}")
        .unwrap();
    index
        .index_file_with_source(&temp.path().join("b/Base.java"), "package b; class Base {}")
        .unwrap();
    let use_path = temp.path().join("app/Use.java");
    index
        .index_file_with_source(
            &use_path,
            "package app; import a.Base; class Use extends Base {}",
        )
        .unwrap();
    let file_id = index.get_file_id(&use_path).unwrap();
    assert_eq!(
        index.resolve_type_name(file_id, &[], "Base"),
        crate::TypeNameResolution::Exact("a.Base".into())
    );

    let mut current_imports = crate::ImportMap::new();
    current_imports.current_package = Some("app".into());
    current_imports.add_import("Base".into(), "b.Base".into());
    assert_eq!(
        index.resolve_type_name_in_context(&current_imports, &[], "Base", crate::Language::Java,),
        crate::TypeNameResolution::Exact("b.Base".into()),
        "an unpublished syntax revision must not inherit the FileId's old imports"
    );
}

#[test]
fn contextual_qualified_edges_invalidate_when_their_head_binding_changes() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Outer.java"),
            concat!(
                "package q; public class Outer { ",
                "public static class Inner {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Child.java"),
            "package p; import q.*; class Child extends Outer.Inner {}",
        )
        .unwrap();
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child", "q.Outer.Inner"]
    );
    assert_eq!(
        index
            .find_implementations("q.Outer.Inner")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        ["p.Child"]
    );

    let shadow_path = temp.path().join("p/Outer.java");
    index
        .index_file_with_source(&shadow_path, "package p; class Outer {}")
        .unwrap();
    assert!(index.direct_supertypes("p.Child").is_empty());
    assert_eq!(index.get_inheritance_chain("p.Child"), ["p.Child"]);
    assert!(index.find_implementations("q.Outer.Inner").is_empty());

    let shadow_id = index.get_file_id(&shadow_path).unwrap();
    index.remove_file(shadow_id);
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child", "q.Outer.Inner"]
    );
    assert_eq!(index.find_implementations("q.Outer.Inner").len(), 1);

    index
        .index_file_with_source(
            &temp.path().join("a/Outer.java"),
            concat!(
                "package a; public class Outer { ",
                "public static class Inner {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/QualifiedChild.java"),
            "package p; class QualifiedChild extends a.Outer.Inner {}",
        )
        .unwrap();
    assert_eq!(
        index.get_inheritance_chain("p.QualifiedChild"),
        ["p.QualifiedChild", "a.Outer.Inner"]
    );

    let bound_head_path = temp.path().join("p/a.java");
    index
        .index_file_with_source(&bound_head_path, "package p; class a {}")
        .unwrap();
    assert_eq!(
        index.get_inheritance_chain("p.QualifiedChild"),
        ["p.QualifiedChild"]
    );
    let bound_head_id = index.get_file_id(&bound_head_path).unwrap();
    index.remove_file(bound_head_id);
    assert_eq!(
        index.get_inheritance_chain("p.QualifiedChild"),
        ["p.QualifiedChild", "a.Outer.Inner"]
    );
}

#[test]
fn canonical_supertype_resolution_handles_java_lang_shadowing_and_static_member_types() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/ClassLoader.java"),
            "package java.lang; public class ClassLoader {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Loader.java"),
            "package app; class Loader extends ClassLoader {}",
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("java.lang.ClassLoader").len(), 1);
    index
        .index_file_with_source(
            &temp.path().join("app/ClassLoader.java"),
            "package app; class ClassLoader {}",
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("app.ClassLoader").len(), 1);
    assert!(index
        .find_direct_subtypes("java.lang.ClassLoader")
        .is_empty());

    index
        .index_file_with_source(
            &temp.path().join("q/Outer.java"),
            "package q; public class Outer { public static class Base {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/StaticSingle.java"),
            "package app; import static q.Outer.Base; class StaticSingle extends Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/StaticStar.java"),
            "package app; import static q.Outer.*; class StaticStar extends Base {}",
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("q.Outer.Base").len(), 2);
}

#[test]
fn canonical_supertype_resolution_excludes_own_body_and_handles_inherited_member_types() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&temp.path().join("q/Base.java"), "package q; class Base {}")
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Child.java"),
            concat!(
                "package p; import q.Base; class Child extends Base { ",
                "static class Base {} }"
            ),
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("q.Base").len(), 1);
    assert!(index.find_direct_subtypes("p.Child.Base").is_empty());

    index
        .index_file_with_source(
            &temp.path().join("p/Enclosing.java"),
            concat!(
                "package p; class Enclosing { static class Base {} ",
                "static class QualifiedChild extends Enclosing.Base {} }"
            ),
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("p.Enclosing.Base").len(), 1);

    index
        .index_file_with_source(
            &temp.path().join("p/Parent.java"),
            "package p; class Parent { static class Member {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Inherited.java"),
            concat!(
                "package p; class Inherited extends Parent { ",
                "static class Nested extends Member {} }"
            ),
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("p.Parent.Member").len(), 1);

    index
        .index_file_with_source(
            &temp.path().join("p/Contract.java"),
            "package p; interface Contract { class Member {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Ambiguous.java"),
            concat!(
                "package p; class Ambiguous extends Parent implements Contract { ",
                "static class Nested extends Member {} }"
            ),
        )
        .unwrap();
    assert!(index
        .find_direct_subtypes("p.Parent.Member")
        .iter()
        .all(|symbol| symbol.fqn.as_ref() != "p.Ambiguous.Nested"));
    assert!(index.find_direct_subtypes("p.Contract.Member").is_empty());

    index
        .index_file_with_source(
            &temp.path().join("p/RootTypes.java"),
            "package p; interface RootTypes { class DeepMember {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/MidTypes.java"),
            "package p; interface MidTypes extends RootTypes {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/TransitiveTypes.java"),
            concat!(
                "package p; class TransitiveTypes implements MidTypes { ",
                "static class Nested extends DeepMember {} }"
            ),
        )
        .unwrap();
    assert_eq!(
        index.find_direct_subtypes("p.RootTypes.DeepMember").len(),
        1
    );

    index
        .index_file_with_source(
            &temp.path().join("a/TypeGrand.java"),
            "package a; public class TypeGrand { static class Token {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/TypeParent.java"),
            "package b; class TypeParent extends a.TypeGrand {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("a/TypeChild.java"),
            concat!(
                "package a; class TypeChild extends b.TypeParent { ",
                "static class Nested extends Token {} }"
            ),
        )
        .unwrap();
    assert!(index.find_direct_subtypes("a.TypeGrand.Token").is_empty());

    index
        .index_file_with_source(
            &temp.path().join("a/HidingParent.java"),
            concat!(
                "package a; class HidingParent extends TypeGrand { ",
                "private static class Token {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("a/HidingChild.java"),
            concat!(
                "package a; class HidingChild extends HidingParent { ",
                "static class Nested extends Token {} }"
            ),
        )
        .unwrap();
    assert!(index.find_direct_subtypes("a.TypeGrand.Token").is_empty());
    assert!(index
        .find_direct_subtypes("a.HidingParent.Token")
        .is_empty());
}

#[test]
fn canonical_type_resolution_stops_after_a_bound_first_segment() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Foo.java"),
            "package q; public class Foo { public static class Missing {} }",
        )
        .unwrap();
    index
        .index_file_with_source(&temp.path().join("p/Foo.java"), "package p; class Foo {}")
        .unwrap();
    let use_path = temp.path().join("p/Use.java");
    index
        .index_file_with_source(
            &use_path,
            concat!(
                "package p; import q.*; class Use { static class Foo {} ",
                "static class Nested {} }"
            ),
        )
        .unwrap();
    let file_id = index.get_file_id(&use_path).unwrap();
    assert_eq!(
        index.resolve_type_name(file_id, &[], "Foo.Missing"),
        crate::TypeNameResolution::Missing,
        "same-package Foo binds the head even though its suffix is missing"
    );
    assert_eq!(
        index.resolve_type_name(file_id, &["p.Use".to_owned()], "Foo.Missing"),
        crate::TypeNameResolution::Missing,
        "lexical Foo binds the head and must not fall through to q.Foo.Missing"
    );
}

#[test]
fn inherited_lexical_member_type_relinks_after_removal_and_restore() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let parent_path = temp.path().join("p/Parent.java");
    let child_path = temp.path().join("p/Outer.java");
    index
        .index_file_with_source(
            &parent_path,
            "package p; class Parent { static class T {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &child_path,
            concat!(
                "package p; class Outer extends Parent { ",
                "static class Nested extends T {} }"
            ),
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("p.Parent.T").len(), 1);
    let parent_id = index.get_file_id(&parent_path).unwrap();
    index.remove_file(parent_id);
    assert!(index.find_direct_subtypes("p.Parent.T").is_empty());
    index
        .index_file_with_source(
            &parent_path,
            "package p; class Parent { static class T {} }",
        )
        .unwrap();
    assert_eq!(index.find_direct_subtypes("p.Parent.T").len(), 1);
}

#[test]
fn contextual_supertype_relinks_when_a_same_package_type_arrives_late() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base {}",
        )
        .unwrap();
    let child_path = temp.path().join("p/Child.java");
    index
        .index_file_with_source(
            &child_path,
            "package p; import q.*; class Child extends Base {}",
        )
        .unwrap();

    assert_eq!(index.direct_supertypes("p.Child"), ["q.Base"]);
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child", "q.Base"]
    );
    assert_eq!(
        index.get_symbol("p.Child").unwrap().parent_class.as_deref(),
        Some("Base"),
        "source declarations must retain the written spelling for open-world relinking"
    );

    index
        .index_file_with_source(&temp.path().join("p/Base.java"), "package p; class Base {}")
        .unwrap();

    assert_eq!(index.direct_supertypes("p.Child"), ["p.Base"]);
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child", "p.Base"],
        "late same-package shadowing must invalidate the old cached q.Base chain"
    );
    assert!(index.find_direct_subtypes("q.Base").is_empty());
    assert_eq!(index.find_direct_subtypes("p.Base").len(), 1);
}

#[test]
fn cached_java_parse_requires_the_exact_source_revision() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Parse.java");
    let source = "package p; class Parse {}";
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();

    let cached = index
        .cached_java_parse(file_id, source)
        .expect("the published Java revision should retain its parse product");
    let inner = index.inner.read();
    let (stored, stored_source) = inner.parse_tree_cache.peek(&file_id).unwrap();
    assert!(Arc::ptr_eq(&cached, stored));
    assert_eq!(stored_source.as_str(), source);
    drop(inner);

    assert!(index
        .cached_java_parse(file_id, "package p; class Parse { int newer; }")
        .is_none());
    assert!(index
        .cached_java_parse(FileId::new(file_id.as_u32() + 1), source)
        .is_none());
}

#[test]
fn exact_revision_supertypes_distinguish_duplicate_owners_and_rebinding() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        ("p/A.java", "package p; class A {}"),
        ("p/B.java", "package p; class B {}"),
        ("q/Base.java", "package q; public class Base {}"),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }
    let first_path = temp.path().join("p/First.java");
    let second_path = temp.path().join("p/Second.java");
    let first_source = "package p; class C extends A {}";
    let second_source = "package p; class C extends B {}";
    index
        .index_file_with_source(&first_path, first_source)
        .unwrap();
    index
        .index_file_with_source(&second_path, second_source)
        .unwrap();
    let first_id = index.get_file_id(&first_path).unwrap();
    let second_id = index.get_file_id(&second_path).unwrap();
    assert_eq!(
        index
            .direct_supertypes_for_revision(first_id, "p.C", first_source)
            .unwrap()
            .superclass
            .as_deref(),
        Some("p.A")
    );
    assert_eq!(
        index
            .direct_supertypes_for_revision(second_id, "p.C", second_source)
            .unwrap()
            .superclass
            .as_deref(),
        Some("p.B")
    );
    assert!(index
        .direct_supertypes_for_revision(first_id, "p.C", second_source)
        .is_none());

    let contextual_path = temp.path().join("app/Child.java");
    let contextual_source = "package app; import q.*; class Child extends Base {}";
    index
        .index_file_with_source(&contextual_path, contextual_source)
        .unwrap();
    let contextual_id = index.get_file_id(&contextual_path).unwrap();
    let contextual_hash = index.get_content_hash(contextual_id);
    let hierarchy_before = index
        .file_supertypes_for_revision(contextual_id, contextual_source)
        .unwrap();
    let before = index
        .direct_supertypes_for_revision(contextual_id, "app.Child", contextual_source)
        .unwrap();
    assert_eq!(before.superclass.as_deref(), Some("q.Base"));
    index
        .index_file_with_source(
            &temp.path().join("app/Base.java"),
            "package app; class Base {}",
        )
        .unwrap();
    let after = index
        .direct_supertypes_for_revision(contextual_id, "app.Child", contextual_source)
        .unwrap();
    let hierarchy_after = index
        .file_supertypes_for_revision(contextual_id, contextual_source)
        .unwrap();
    assert_eq!(after.superclass.as_deref(), Some("app.Base"));
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_eq!(
        hierarchy_before.file_revision,
        hierarchy_after.file_revision
    );
    assert_eq!(index.get_content_hash(contextual_id), contextual_hash);
    assert_ne!(
        hierarchy_before.workspace_revision, hierarchy_after.workspace_revision,
        "a late same-package declaration must retire lazy hierarchy snapshots"
    );
    assert!(!index.resolved_file_supertypes_are_current(&hierarchy_before));
    assert!(index.resolved_file_supertypes_are_current(&hierarchy_after));

    let ambiguous_path = temp.path().join("amb/Child.java");
    let ambiguous_source = "package amb; import q.*; import r.*; class Child extends Base {}";
    index
        .index_file_with_source(&ambiguous_path, ambiguous_source)
        .unwrap();
    let ambiguous_id = index.get_file_id(&ambiguous_path).unwrap();
    let ambiguous_hash = index.get_content_hash(ambiguous_id);
    let unique_batch = index
        .file_supertypes_for_revision(ambiguous_id, ambiguous_source)
        .unwrap();
    let unique = index
        .direct_supertypes_for_revision(ambiguous_id, "amb.Child", ambiguous_source)
        .unwrap();
    assert_eq!(unique.superclass.as_deref(), Some("q.Base"));
    index
        .index_file_with_source(
            &temp.path().join("r/Base.java"),
            "package r; public class Base {}",
        )
        .unwrap();
    let ambiguous = index
        .direct_supertypes_for_revision(ambiguous_id, "amb.Child", ambiguous_source)
        .unwrap();
    let ambiguous_batch = index
        .file_supertypes_for_revision(ambiguous_id, ambiguous_source)
        .unwrap();
    assert!(ambiguous.superclass.is_none());
    assert!(ambiguous.interfaces.is_empty());
    assert_ne!(unique.fingerprint, ambiguous.fingerprint);
    assert_eq!(unique_batch.file_revision, ambiguous_batch.file_revision);
    assert_eq!(index.get_content_hash(ambiguous_id), ambiguous_hash);
    assert_ne!(
        unique_batch.workspace_revision, ambiguous_batch.workspace_revision,
        "a late on-demand ambiguity must retire lazy hierarchy snapshots"
    );
    assert!(!index.resolved_file_supertypes_are_current(&unique_batch));
    assert!(index.resolved_file_supertypes_are_current(&ambiguous_batch));
}

#[test]
fn exact_file_supertype_batch_covers_multiple_and_nested_types() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for name in ["A", "B", "C"] {
        index
            .index_file_with_source(
                &temp.path().join(format!("q/{name}.java")),
                &format!("package q; public class {name} {{}}"),
            )
            .unwrap();
    }
    let path = temp.path().join("app/Types.java");
    let source = concat!(
        "package app; import q.*; class Outer extends A { ",
        "static class Nested extends B {} } class Sibling extends C {}"
    );
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let before = index.file_supertypes_for_revision(file_id, source).unwrap();
    assert_eq!(
        before
            .types
            .iter()
            .map(|entry| (entry.type_fqn.clone(), entry.superclass.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("app.Outer".to_owned(), Some("q.A".to_owned())),
            ("app.Outer.Nested".to_owned(), Some("q.B".to_owned())),
            ("app.Sibling".to_owned(), Some("q.C".to_owned())),
        ]
    );

    index
        .index_file_with_source(&temp.path().join("app/B.java"), "package app; class B {}")
        .unwrap();
    let after = index.file_supertypes_for_revision(file_id, source).unwrap();
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_eq!(
        after
            .types
            .iter()
            .find(|entry| entry.type_fqn == "app.Outer.Nested")
            .unwrap()
            .superclass
            .as_deref(),
        Some("app.B")
    );

    let duplicate_path = temp.path().join("bad/Duplicates.java");
    let duplicate_source = "package bad; class Same {} class Same {}";
    index
        .index_file_with_source(&duplicate_path, duplicate_source)
        .unwrap();
    let duplicate_id = index.get_file_id(&duplicate_path).unwrap();
    assert!(index
        .file_supertypes_for_revision(duplicate_id, duplicate_source)
        .is_none());
}

#[test]
fn exact_file_supertype_batch_rejects_disk_bytes_from_an_unindexed_revision() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(&temp.path().join("q/Base.java"), "package q; class Base {}")
        .unwrap();
    let path = temp.path().join("app/Child.java");
    let indexed = "package app; import q.Base; class Child extends Base {}";
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, indexed).unwrap();
    index.index_file_with_source(&path, indexed).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let indexed_hash = index.get_content_hash(file_id).unwrap();

    // Evict Child from the source-byte LRU while its exact indexed parse/hash
    // remain published.
    for number in 0..110 {
        index
            .index_file_with_source(
                &temp.path().join(format!("evict/E{number}.java")),
                &format!("package evict; class E{number} {{}}"),
            )
            .unwrap();
    }
    let changed = "package app; class Child {}";
    std::fs::write(&path, changed).unwrap();
    assert_eq!(
        index.get_current_file_contents(file_id).unwrap().as_str(),
        changed
    );
    assert_eq!(
        index
            .get_indexed_file_contents(file_id, indexed_hash)
            .unwrap()
            .content
            .as_str(),
        indexed
    );
    assert!(
        index
            .file_supertypes_for_revision(file_id, changed)
            .is_none(),
        "matching refill bytes are not proof that the changed disk revision was indexed"
    );
    assert!(index
        .file_supertypes_for_revision(file_id, indexed)
        .is_some());
    assert_eq!(
        index.get_file_contents(file_id).unwrap().as_str(),
        indexed,
        "a raw-current read must not poison the exact indexed cache"
    );
}

#[test]
fn contextual_supertype_becomes_ambiguous_when_a_wildcard_type_arrives_late() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Child.java"),
            "package p; import q.*; import r.*; class Child extends Base {}",
        )
        .unwrap();
    assert_eq!(index.direct_supertypes("p.Child"), ["q.Base"]);
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child", "q.Base"]
    );

    index
        .index_file_with_source(
            &temp.path().join("r/Base.java"),
            "package r; public class Base {}",
        )
        .unwrap();

    assert!(index.direct_supertypes("p.Child").is_empty());
    assert_eq!(
        index.get_inheritance_chain("p.Child"),
        ["p.Child"],
        "a late wildcard collision must invalidate the formerly exact cached chain"
    );
    assert!(index.find_direct_subtypes("q.Base").is_empty());
    assert!(index.find_direct_subtypes("r.Base").is_empty());
}

#[test]
fn late_wildcard_ambiguity_invalidates_resolved_method_batch_not_file_revision() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base { public void ping() {} }",
        )
        .unwrap();
    let caller_path = temp.path().join("p/Caller.java");
    let caller_source = concat!(
        "package p; import q.*; import r.*; ",
        "class Caller { void use(Base value) { value.ping(); } }"
    );
    index
        .index_file_with_source(&caller_path, caller_source)
        .unwrap();
    let caller_id = index.get_file_id(&caller_path).unwrap();
    let caller_revision = index.current_file_revision(caller_id).unwrap();
    let caller_hash = index.get_content_hash(caller_id);
    let resolved_before = index
        .get_file_resolved_method_usages_revisioned(caller_id)
        .unwrap();
    assert!(resolved_before
        .usages
        .iter()
        .any(|(_, fqn)| fqn.starts_with("q.Base.ping")));
    assert!(index.resolved_method_usages_are_current(&resolved_before));

    index
        .index_file_with_source(
            &temp.path().join("r/Base.java"),
            "package r; public class Base { public void ping() {} }",
        )
        .unwrap();

    assert_eq!(
        index.current_file_revision(caller_id),
        Some(caller_revision)
    );
    assert_eq!(index.get_content_hash(caller_id), caller_hash);
    assert!(!index.resolved_method_usages_are_current(&resolved_before));
    let recaptured = index
        .get_file_resolved_method_usages_revisioned(caller_id)
        .unwrap();
    assert_eq!(recaptured.revision, caller_revision);
    assert_eq!(
        recaptured.workspace_revision, resolved_before.workspace_revision,
        "retrieval must preserve the bindings' original context, not launder stale FQNs"
    );
    assert!(!index.resolved_method_usages_are_current(&recaptured));

    let indexed = index
        .get_indexed_revision_content(caller_revision)
        .expect("exact caller source");
    let lazily_resolved = index
        .resolve_current_method_usages(&indexed)
        .expect("coherent lazy call resolution");
    assert_eq!(
        lazily_resolved.workspace_revision,
        index.current_workspace_revision()
    );
    assert!(
        lazily_resolved.usages.is_empty(),
        "the now-ambiguous Base call must not reuse the stored q.Base binding"
    );
}

#[test]
fn hierarchy_install_closure_excludes_a_contextual_writer() {
    let temp = TempDir::new().unwrap();
    let index = std::sync::Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(&temp.path().join("q/Base.java"), "package q; class Base {}")
        .unwrap();
    let child_path = temp.path().join("app/Child.java");
    let child_source = "package app; import q.*; class Child extends Base {}";
    index
        .index_file_with_source(&child_path, child_source)
        .unwrap();
    let child_id = index.get_file_id(&child_path).unwrap();
    let resolved = index
        .file_supertypes_for_revision(child_id, child_source)
        .unwrap();

    let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let writer_index = std::sync::Arc::clone(&index);
    let writer_entered = std::sync::Arc::clone(&entered);
    let late_path = temp.path().join("app/Base.java");
    let writer = std::thread::spawn(move || {
        writer_entered.wait();
        attempted_tx.send(()).unwrap();
        writer_index
            .index_file_with_source(&late_path, "package app; class Base {}")
            .unwrap();
        finished_tx.send(()).unwrap();
    });

    let installed_revision = index
        .with_current_resolved_file_supertypes(&resolved, || {
            entered.wait();
            attempted_rx.recv().unwrap();
            assert!(
                finished_rx.try_recv().is_err(),
                "workspace writer crossed the validation/install read barrier"
            );
            resolved.workspace_revision
        })
        .expect("old batch is valid while its installation lock is held");
    writer.join().unwrap();
    assert_eq!(installed_revision, resolved.workspace_revision);
    assert!(!index.resolved_file_supertypes_are_current(&resolved));
}

#[test]
fn semantic_publication_batch_excludes_writer_and_rejects_every_old_fact() {
    let temp = TempDir::new().unwrap();
    let index = std::sync::Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let service_path = temp.path().join("api/Service.java");
    let service_source = "package api; class Service { void load(int value) {} }";
    index
        .index_file_with_source(&service_path, service_source)
        .unwrap();
    let caller_path = temp.path().join("app/Caller.java");
    let caller_source = concat!(
        "package app; import api.Service; class Caller { ",
        "void run(Service service) { service.load(1); } }"
    );
    index
        .index_file_with_source(&caller_path, caller_source)
        .unwrap();
    let service_id = index.get_file_id(&service_path).unwrap();
    let caller_id = index.get_file_id(&caller_path).unwrap();
    let service_indexed = index
        .get_indexed_revision_content(index.current_file_revision(service_id).unwrap())
        .unwrap();
    let caller_indexed = index
        .get_indexed_revision_content(index.current_file_revision(caller_id).unwrap())
        .unwrap();
    let service_hierarchy = index
        .file_supertypes_for_revision(service_id, &service_indexed.content)
        .unwrap();
    let caller_hierarchy = index
        .file_supertypes_for_revision(caller_id, &caller_indexed.content)
        .unwrap();
    let calls = index
        .resolve_current_method_usages(&caller_indexed)
        .expect("coherent call batch");
    let workspace_revision = calls.workspace_revision;
    assert_eq!(service_hierarchy.workspace_revision, workspace_revision);
    assert_eq!(caller_hierarchy.workspace_revision, workspace_revision);

    let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (attempted_tx, attempted_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let writer_index = std::sync::Arc::clone(&index);
    let writer_entered = std::sync::Arc::clone(&entered);
    let writer = std::thread::spawn(move || {
        writer_entered.wait();
        attempted_tx.send(()).unwrap();
        writer_index
            .index_file_with_source(
                &temp.path().join("other/Later.java"),
                "package other; class Later {}",
            )
            .unwrap();
        finished_tx.send(()).unwrap();
    });

    let revisions = [service_indexed.revision, caller_indexed.revision];
    let indexed = [&service_indexed, &caller_indexed];
    let hierarchies = [&service_hierarchy, &caller_hierarchy];
    let installed = index
        .with_current_semantic_publication(
            workspace_revision,
            &revisions,
            &indexed,
            &hierarchies,
            Some(&calls),
            || {
                entered.wait();
                attempted_rx.recv().unwrap();
                assert!(
                    finished_rx.try_recv().is_err(),
                    "writer crossed the all-fact validation/install barrier"
                );
                true
            },
        )
        .expect("entire old semantic batch is current while read guard is held");
    assert!(installed);
    writer.join().unwrap();
    assert!(index
        .with_current_semantic_publication(
            workspace_revision,
            &revisions,
            &indexed,
            &hierarchies,
            Some(&calls),
            || (),
        )
        .is_none());
}

#[test]
fn semantic_publication_does_not_require_exact_source_cache_residency() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("Target.java");
    let target_source = "class Target {}";
    index
        .index_file_with_source(&target_path, target_source)
        .unwrap();
    let target_id = index.get_file_id(&target_path).unwrap();
    let indexed = index
        .get_indexed_revision_content(index.current_file_revision(target_id).unwrap())
        .unwrap();

    // Both retained-source caches are bounded (100 source entries and 1,000
    // parse entries). The revisioned value above remains an exact, owned view
    // even after unrelated indexing evicts it from both implementation caches.
    for ordinal in 0..=1_000 {
        index
            .index_file_with_source(
                &temp.path().join(format!("evict/Evict{ordinal}.java")),
                &format!("class Evict{ordinal} {{}}"),
            )
            .unwrap();
    }

    assert!(
        index
            .with_current_semantic_publication(
                index.current_workspace_revision(),
                &[indexed.revision],
                &[&indexed],
                &[],
                None,
                || true,
            )
            .expect("an exact owned revision must survive cache eviction"),
        "cache capacity must not determine semantic publication correctness"
    );
}

#[test]
fn replacing_a_source_does_not_resolve_against_its_old_revision() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base {}",
        )
        .unwrap();
    let source_path = temp.path().join("p/Types.java");
    index
        .index_file_with_source(
            &source_path,
            concat!(
                "package p; import q.*; class Base {} ",
                "class Child extends Base {}"
            ),
        )
        .unwrap();
    assert_eq!(index.direct_supertypes("p.Child"), ["p.Base"]);

    index
        .index_file_with_source(
            &source_path,
            "package p; import q.*; class Child extends Base {}",
        )
        .unwrap();

    assert!(index.get_symbol("p.Base").is_none());
    assert_eq!(index.direct_supertypes("p.Child"), ["q.Base"]);
    assert_eq!(index.find_direct_subtypes("q.Base").len(), 1);
}

#[test]
fn inherited_method_lookup_exhausts_classes_and_filters_non_inherited_members() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("a/Grand.java"),
            "package a; public class Grand { public String value() { return null; } }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("a/Parent.java"),
            "package a; public class Parent extends Grand {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/Contract.java"),
            "package b; public interface Contract { default String value() { return null; } }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/Child.java"),
            concat!(
                "package b; import a.Parent; class Child extends Parent ",
                "implements Contract {}"
            ),
        )
        .unwrap();
    let selected = index
        .find_method_by_args_with_inheritance("b.Child", "value", 0)
        .unwrap();
    assert_eq!(selected.container.as_deref(), Some("a.Grand"));

    index
        .index_file_with_source(
            &temp.path().join("a/PrivateBase.java"),
            "package a; public class PrivateBase { private void hidden() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/DefaultContract.java"),
            "package b; interface DefaultContract { default void hidden() {} static void staticOnly() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/AccessChild.java"),
            concat!(
                "package b; import a.PrivateBase; class AccessChild extends PrivateBase ",
                "implements DefaultContract {}"
            ),
        )
        .unwrap();
    assert_eq!(
        index
            .find_method_by_args_with_inheritance("b.AccessChild", "hidden", 0)
            .unwrap()
            .container
            .as_deref(),
        Some("b.DefaultContract")
    );
    assert!(index
        .find_method_by_args_with_inheritance("b.AccessChild", "staticOnly", 0)
        .is_none());

    index
        .index_file_with_source(
            &temp.path().join("c/CrossPackage.java"),
            "package c; import b.DefaultContract; class CrossPackage implements DefaultContract {}",
        )
        .unwrap();
    assert_eq!(
        index
            .find_method_by_args_with_inheritance("c.CrossPackage", "hidden", 0)
            .unwrap()
            .container
            .as_deref(),
        Some("b.DefaultContract"),
        "interface methods are implicitly public even without an explicit modifier"
    );

    index
        .index_file_with_source(
            &temp.path().join("a/PackageGrand.java"),
            "package a; public class PackageGrand { void packageOnly() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("b/ForeignParent.java"),
            "package b; import a.PackageGrand; public class ForeignParent extends PackageGrand {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("a/ReturningChild.java"),
            "package a; import b.ForeignParent; class ReturningChild extends ForeignParent {}",
        )
        .unwrap();
    assert!(index
        .find_method_by_args_with_inheritance("a.ReturningChild", "packageOnly", 0)
        .is_none());
}

#[test]
fn method_arity_does_not_treat_array_parameters_as_varargs() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Methods.java"),
            concat!(
                "package p; class Methods { ",
                "void array(String[] values) {} ",
                "void variadic(String... values) {} }"
            ),
        )
        .unwrap();

    assert!(index.find_method_by_args("p.Methods", "array", 0).is_none());
    assert!(index.find_method_by_args("p.Methods", "array", 1).is_some());
    assert!(index
        .find_method_by_args("p.Methods", "variadic", 3)
        .is_some());
    assert!(index
        .find_method_by_args_with_inheritance("p.Methods", "array", 0)
        .is_none());
    assert!(index
        .find_method_by_args_with_inheritance("p.Methods", "array", 1)
        .is_some());
    assert!(index
        .find_method_by_args_with_inheritance("p.Methods", "variadic", 0)
        .is_some());
    assert!(index
        .find_method_by_args_with_inheritance("p.Methods", "variadic", 3)
        .is_some());
}

#[test]
fn effective_method_set_collapses_dispatch_slots_and_filters_non_members() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        (
            "api/Contract.java",
            concat!(
                "package api; public interface Contract { ",
                "Object convert(String value); void act(); ",
                "void privateCollision(); void staticCollision(); ",
                "private void privateHelper() {} static void interfaceFactory() {} }"
            ),
        ),
        (
            "base/Base.java",
            concat!(
                "package base; public class Base { ",
                "public Object convert(String value) { return value; } ",
                "public void overloaded(Object value) {} ",
                "private void baseSecret() {} public static void classFactory() {} ",
                "public static void staticCollision() {} }"
            ),
        ),
        (
            "impl/Child.java",
            concat!(
                "package impl; import api.Contract; import base.Base; ",
                "public class Child extends Base implements Contract { ",
                "public String convert(String value) { return value; } ",
                "public void overloaded(String value) {} public void act() {} ",
                "private void privateCollision() {} }"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }

    let convert = index.find_effective_methods_by_name("impl.Child", "convert");
    assert_eq!(
        convert.len(),
        1,
        "override slot was duplicated: {convert:?}"
    );
    assert_eq!(convert[0].container.as_deref(), Some("impl.Child"));
    assert_eq!(convert[0].return_type.as_deref(), Some("java.lang.String"));

    let act = index.find_effective_methods_by_name("impl.Child", "act");
    assert_eq!(act.len(), 1, "interface contract was duplicated: {act:?}");
    assert_eq!(act[0].container.as_deref(), Some("impl.Child"));
    for method_name in ["privateCollision", "staticCollision"] {
        let methods = index.find_effective_methods_by_name("impl.Child", method_name);
        assert!(
            methods
                .iter()
                .any(|method| method.container.as_deref() == Some("api.Contract")),
            "a private/static class declaration suppressed interface member {method_name}: {methods:?}"
        );
    }
    assert!(index
        .find_effective_methods_by_name("impl.Child", "baseSecret")
        .is_empty());
    assert!(index
        .find_effective_methods_by_name("impl.Child", "privateHelper")
        .is_empty());
    assert!(index
        .find_effective_methods_by_name("impl.Child", "interfaceFactory")
        .is_empty());
    assert_eq!(
        index
            .find_effective_methods_by_name("base.Base", "baseSecret")
            .len(),
        1,
        "a private declaration remains callable in its own lexical owner"
    );
    assert_eq!(
        index
            .find_effective_methods_by_name("api.Contract", "privateHelper")
            .len(),
        1,
        "interface-private methods remain callable inside their declaring interface"
    );
    assert_eq!(
        index
            .find_effective_methods_by_name("impl.Child", "classFactory")
            .first()
            .and_then(|method| method.container.as_deref()),
        Some("base.Base"),
        "Java class static methods are inherited even though interface static methods are not"
    );

    let all_methods = index.find_effective_methods("impl.Child");
    let counts = all_methods.iter().fold(
        std::collections::HashMap::<&str, usize>::new(),
        |mut counts, method| {
            *counts.entry(method.simple_name.as_ref()).or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("convert"), Some(&1));
    assert_eq!(counts.get("act"), Some(&1));
    assert_eq!(counts.get("classFactory"), Some(&1));
    assert_eq!(counts.get("overloaded"), Some(&2));
    assert!(!counts.contains_key("baseSecret"));
    assert!(!counts.contains_key("privateHelper"));
    assert!(!counts.contains_key("interfaceFactory"));
}

#[test]
fn effective_method_set_does_not_resurrect_package_private_members() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        (
            "a/Grand.java",
            "package a; public class Grand { void packageOnly() {} }",
        ),
        (
            "b/Middle.java",
            "package b; public class Middle extends a.Grand {}",
        ),
        (
            "a/Child.java",
            "package a; public class Child extends b.Middle {}",
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }

    assert!(index
        .find_effective_methods_by_name("a.Child", "packageOnly")
        .is_empty());
}

#[test]
fn effective_method_set_erases_multidimensional_type_variable_slots() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            "package java.lang; public class Object {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Base<T> { T[][] copy(T[][] value) { return value; } } ",
                "class Child extends Base<Object> { ",
                "Object[][] copy(Object[][] value) { return value; } }"
            ),
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Child", "copy");
    assert_eq!(
        methods.len(),
        1,
        "generic erasure produced duplicate dispatch slots: {methods:?}"
    );
    assert_eq!(methods[0].container.as_deref(), Some("p.Child"));
}

#[test]
fn effective_method_set_projects_owner_generics_before_override_election() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/String.java"),
            "package java.lang; public final class String {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Base<T> { ",
                "T direct(T value) { return value; } ",
                "java.util.List<T> nested(java.util.List<T> value) { return value; } } ",
                "class Child extends Base<String> { ",
                "@Override String direct(String value) { return value; } ",
                "@Override java.util.List<String> nested(java.util.List<String> value) { return value; } }"
            ),
        )
        .unwrap();

    for method_name in ["direct", "nested"] {
        let methods = index.find_effective_methods_by_name("p.Child", method_name);
        assert_eq!(
            methods.len(),
            1,
            "the receiver-relative override must own one source member slot: {methods:#?}"
        );
        assert_eq!(methods[0].container.as_deref(), Some("p.Child"));
    }
}

#[test]
fn projected_effective_members_share_receiver_relative_generic_contract() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Bound {} class Text {} class List<E> {} ",
                "interface Marker<E> {} ",
                "class Base<T> { T value; T echo(T input) { return input; } ",
                "<T extends Marker<T>> T shadow(T input) { return input; } } ",
                "class Mid<U> extends Base<List<U>> {} ",
                "class Child<V> extends Mid<V> {} ",
                "class Bounded<T extends Bound> { T rawValue; T raw(T input) { return input; } } ",
                "class Raw extends Bounded {} ",
                "interface Root<T> { T conflict(T input); } ",
                "interface Left extends Root<Text> {} ",
                "interface Right extends Root<Bound> {} ",
                "interface Conflict extends Left, Right {}"
            ),
        )
        .unwrap();

    let arguments = ["p.Text".to_string()];
    let echo_methods =
        index.find_projected_effective_methods_by_name("p.Child", &arguments, "echo");
    let [echo] = echo_methods.as_slice() else {
        panic!("one inherited echo method expected")
    };
    let echo = echo.projection.as_ref().expect("multi-hop projection");
    assert_eq!(echo.parameters[0].type_name, "p.List<p.Text>");
    assert_eq!(echo.return_type.as_deref(), Some("p.List<p.Text>"));

    let field = index
        .find_projected_effective_field_by_name("p.Child", &arguments, "value")
        .expect("inherited field");
    assert_eq!(field.projected_type.as_deref(), Some("p.List<p.Text>"));

    let shadow_methods =
        index.find_projected_effective_methods_by_name("p.Child", &arguments, "shadow");
    let [shadow] = shadow_methods.as_slice() else {
        panic!("one shadow method expected")
    };
    let shadow = shadow.projection.as_ref().expect("shadow projection");
    assert_eq!(shadow.parameters[0].type_name, "T");
    assert_eq!(shadow.return_type.as_deref(), Some("T"));
    assert_eq!(shadow.type_parameters[0].bounds[0].as_ref(), "Marker<T>");

    let raw_methods = index.find_projected_effective_methods_by_name("p.Raw", &[], "raw");
    let [raw] = raw_methods.as_slice() else {
        panic!("one raw method expected")
    };
    let raw = raw.projection.as_ref().expect("raw erasure projection");
    assert_eq!(raw.parameters[0].type_name, "p.Bound");
    assert_eq!(raw.return_type.as_deref(), Some("p.Bound"));
    let raw_field = index
        .find_projected_effective_field_by_name("p.Raw", &[], "rawValue")
        .expect("raw field");
    assert_eq!(raw_field.projected_type.as_deref(), Some("p.Bound"));

    let conflict_methods =
        index.find_projected_effective_methods_by_name("p.Conflict", &[], "conflict");
    let [conflict] = conflict_methods.as_slice() else {
        panic!("the shared declaration remains visible")
    };
    assert!(
        conflict.projection.is_none(),
        "conflicting paths must not expose a raw declaration spelling"
    );
}

#[test]
fn projected_field_access_applies_java_visibility_and_qualifier_rules_atomically() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        (
            "p/Base.java",
            concat!(
                "package p; public class Base { ",
                "public int open; protected int guarded; int packageOnly; ",
                "private int secret; public class Nested {} }"
            ),
        ),
        ("p/Peer.java", "package p; public class Peer {}"),
        (
            "q/Child.java",
            concat!(
                "package q; public class Child extends p.Base { ",
                "public class Nested {} } class GrandChild extends Child {}"
            ),
        ),
        ("q/Foreign.java", "package q; public class Foreign {}"),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }

    let resolve =
        |receiver: &str, member: &str, owners: &[&str], qualifier: FieldAccessQualifier| {
            index.resolve_projected_field_access(
                receiver,
                &[],
                member,
                &owners.iter().map(ToString::to_string).collect::<Vec<_>>(),
                qualifier,
            )
        };
    let is_resolved =
        |resolution| matches!(resolution, ProjectedFieldAccessResolution::Resolved(_));

    assert!(is_resolved(resolve(
        "p.Base",
        "open",
        &["q.Foreign"],
        FieldAccessQualifier::Expression,
    )));
    assert!(is_resolved(resolve(
        "p.Base",
        "packageOnly",
        &["p.Peer"],
        FieldAccessQualifier::Expression,
    )));
    assert!(matches!(
        resolve(
            "p.Base",
            "packageOnly",
            &["q.Foreign"],
            FieldAccessQualifier::Expression,
        ),
        ProjectedFieldAccessResolution::Inaccessible
    ));
    assert!(is_resolved(resolve(
        "p.Base",
        "secret",
        &["p.Base.Nested", "p.Base"],
        FieldAccessQualifier::Expression,
    )));
    assert!(matches!(
        resolve(
            "p.Base",
            "secret",
            &["p.Peer"],
            FieldAccessQualifier::Expression,
        ),
        ProjectedFieldAccessResolution::Inaccessible
    ));

    // A cross-package subclass may access a protected field through itself or
    // a subtype, but not through an expression statically typed as the base.
    assert!(is_resolved(resolve(
        "q.Child",
        "guarded",
        &["q.Child"],
        FieldAccessQualifier::Expression,
    )));
    assert!(is_resolved(resolve(
        "q.GrandChild",
        "guarded",
        &["q.Child"],
        FieldAccessQualifier::Expression,
    )));
    assert!(matches!(
        resolve(
            "p.Base",
            "guarded",
            &["q.Child"],
            FieldAccessQualifier::Expression,
        ),
        ProjectedFieldAccessResolution::Inaccessible
    ));
    assert!(is_resolved(resolve(
        "p.Base",
        "guarded",
        &["q.Child"],
        FieldAccessQualifier::Super,
    )));
    assert!(matches!(
        resolve(
            "p.Base",
            "guarded",
            &["q.Foreign"],
            FieldAccessQualifier::Super,
        ),
        ProjectedFieldAccessResolution::Inaccessible
    ));
    assert!(matches!(
        resolve(
            "p.Base",
            "absent",
            &["p.Base"],
            FieldAccessQualifier::Expression,
        ),
        ProjectedFieldAccessResolution::Missing
    ));
}

#[test]
fn concrete_arguments_cannot_heal_a_declaration_level_generic_diamond_conflict() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Text {} ",
                "interface Root<T> { T value(); T field(); } ",
                "interface Left<A> extends Root<A> {} ",
                "interface Right<B> extends Root<B> {} ",
                "interface Conflict<A, B> extends Left<A>, Right<B> {}"
            ),
        )
        .unwrap();

    let equal_arguments = ["p.Text".to_string(), "p.Text".to_string()];
    let methods =
        index.find_projected_effective_methods_by_name("p.Conflict", &equal_arguments, "value");
    let [method] = methods.as_slice() else {
        panic!("one shared declaration expected: {methods:#?}")
    };
    assert!(
        method.projection.is_none(),
        "equal use-site arguments cannot legalize conflicting declaration paths"
    );
}

#[test]
fn interface_object_fallback_retains_a_projected_signature() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            "package java.lang; public class Object { public String toString() { return null; } }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Task.java"),
            "package p; public interface Task {}",
        )
        .unwrap();

    let methods = index.find_projected_effective_methods_by_name("p.Task", &[], "toString");
    let [method] = methods.as_slice() else {
        panic!("Object fallback expected: {methods:#?}")
    };
    let signature = method
        .projection
        .as_ref()
        .expect("Object fallback has no receiver variables to project");
    assert_eq!(signature.return_type.as_deref(), Some("java.lang.String"));
}

#[test]
fn projected_kotlin_vararg_preserves_prefix_and_normalizes_semantic_type() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.kt"),
            concat!(
                "package p\n",
                "class Collector { fun spread(vararg values: String): String = values[0] }"
            ),
        )
        .unwrap();

    let methods = index.find_projected_effective_methods_by_name("p.Collector", &[], "spread");
    let [method] = methods.as_slice() else {
        panic!("one inherited Kotlin spread method expected: {methods:#?}")
    };
    let projection = method.projection.as_ref().expect("Kotlin projection");
    assert_eq!(projection.parameters[0].type_name, "String[]");
    assert!(projection.parameters[0].is_variadic());
    assert_eq!(
        projection.parameter_declarations(),
        ["vararg String values"]
    );
}

#[test]
fn effective_method_set_projects_covariant_returns_across_generic_constructors() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Text {} class Other {} class Wide<T> {} ",
                "class Narrow<T> extends Wide<T> {} class RawNarrow<T> extends Wide {} ",
                "class Base<T> { Wide<T> create() { return null; } } ",
                "class Child extends Base<Text> { ",
                "@Override Narrow<Text> create() { return null; } } ",
                "class RawChild extends Base<Text> { ",
                "@Override RawNarrow<Text> create() { return null; } } ",
                "class Invalid extends Base<Text> { ",
                "Narrow<Other> create() { return null; } }"
            ),
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Child", "create");
    assert_eq!(
        methods.len(),
        1,
        "the projected generic covariant return must preserve one override slot: {methods:#?}"
    );
    assert_eq!(methods[0].container.as_deref(), Some("p.Child"));
    let raw = index.find_effective_methods_by_name("p.RawChild", "create");
    assert_eq!(
        raw.len(),
        1,
        "javac permits the raw generic return edge with an unchecked conversion: {raw:#?}"
    );
    assert_eq!(raw[0].container.as_deref(), Some("p.RawChild"));
    let invalid = index.find_effective_methods_by_name("p.Invalid", "create");
    assert_eq!(
        invalid.len(),
        2,
        "invariant generic arguments must not be collapsed through erasure: {invalid:#?}"
    );
}

#[test]
fn effective_method_set_keeps_method_type_parameters_shadowing_owner_parameters() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Types.java"),
            concat!(
                "package java.lang; public class Object {} ",
                "public abstract class Number extends Object {} ",
                "public final class String extends Object {}"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Base<T> { <T> void accept(T value) {} } ",
                "class Child extends Base<String> { void accept(String value) {} }"
            ),
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Child", "accept");
    assert_eq!(
        methods.len(),
        2,
        "the method-level T is independent of the owner's T: {methods:#?}"
    );

    index
        .index_file_with_source(
            &temp.path().join("p/GenericMethods.java"),
            concat!(
                "package p; class GenericBase { ",
                "<U extends Number> U choose(U value) { return value; } } ",
                "class RenamedTypeParameter extends GenericBase { ",
                "@Override <V extends Number> V choose(V value) { return value; } } ",
                "class ErasedSignature extends GenericBase { ",
                "@Override Number choose(Number value) { return value; } }"
            ),
        )
        .unwrap();
    for receiver in ["p.RenamedTypeParameter", "p.ErasedSignature"] {
        let methods = index.find_effective_methods_by_name(receiver, "choose");
        assert_eq!(
            methods.len(),
            1,
            "adapted and erasure subsignatures must each override: {methods:#?}"
        );
        assert_eq!(methods[0].container.as_deref(), Some(receiver));
    }
}

#[test]
fn effective_method_set_applies_erasure_subsignatures_directionally() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            "package java.lang; public class Object {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class PlainParent { void apply(Object value) {} } ",
                "class GenericChild extends PlainParent { <T> void apply(T value) {} } ",
                "class GenericParent { <T> void accept(T value) {} } ",
                "class ErasedChild extends GenericParent { ",
                "@Override void accept(Object value) {} } ",
                "interface PlainContract { void map(Object value); } ",
                "interface GenericContract extends PlainContract { <T> void map(T value); } ",
                "interface GenericBaseContract { <T> void fold(T value); } ",
                "interface ErasedContract extends GenericBaseContract { void fold(Object value); } ",
                "class GenericImplementation implements PlainContract { ",
                "public <T> void map(T value) {} } ",
                "class ErasedImplementation implements GenericBaseContract { ",
                "@Override public void fold(Object value) {} }"
            ),
        )
        .unwrap();

    for (receiver, method_name) in [
        ("p.GenericChild", "apply"),
        ("p.GenericContract", "map"),
        ("p.GenericImplementation", "map"),
    ] {
        let methods = index.find_effective_methods_by_name(receiver, method_name);
        assert_eq!(
            methods.len(),
            2,
            "a generic child is not a subsignature of its erased parent: {methods:#?}"
        );
    }
    for (receiver, method_name) in [
        ("p.ErasedChild", "accept"),
        ("p.ErasedContract", "fold"),
        ("p.ErasedImplementation", "fold"),
    ] {
        let methods = index.find_effective_methods_by_name(receiver, method_name);
        assert_eq!(
            methods.len(),
            1,
            "a child matching the parent's erasure is a directional subsignature: {methods:#?}"
        );
        assert_eq!(methods[0].container.as_deref(), Some(receiver));
    }
}

#[test]
fn effective_method_set_does_not_treat_private_or_static_interface_methods_as_overrides() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; interface Parent { default void run() {} } ",
                "interface PrivateChild extends Parent { private void run() {} } ",
                "interface StaticChild extends Parent { static void run() {} }"
            ),
        )
        .unwrap();

    for receiver in ["p.PrivateChild", "p.StaticChild"] {
        let methods = index.find_effective_methods_by_name(receiver, "run");
        assert_eq!(
            methods.len(),
            2,
            "a non-virtual declaration cannot supersede an inherited default: {methods:#?}"
        );
    }
}

#[test]
fn effective_method_set_fails_closed_on_conflicting_return_projection() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Text {} class Other {} ",
                "interface Root<T> { T value(); } ",
                "interface Left<X> extends Root<X> { @Override X value(); } ",
                "interface TextPath extends Left<Text> {} ",
                "interface OtherPath extends Left<Other> {} ",
                "interface Conflict extends TextPath, OtherPath {}"
            ),
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Conflict", "value");
    assert_eq!(
        methods.len(),
        2,
        "conflicting owner projections must not masquerade as absent return types: {methods:#?}"
    );
}

#[test]
fn effective_method_set_projects_symbolic_receiver_parameters_and_raw_edges() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Base<T> { T copy(T value) { return value; } } ",
                "class GenericChild<S extends CharSequence> extends Base<S> { ",
                "@Override S copy(S value) { return value; } } ",
                "class RawChild extends Base { ",
                "@Override Object copy(Object value) { return value; } }"
            ),
        )
        .unwrap();

    for receiver in ["p.GenericChild", "p.RawChild"] {
        let methods = index.find_effective_methods_by_name(receiver, "copy");
        assert_eq!(
            methods.len(),
            1,
            "{receiver} must expose one override slot: {methods:#?}"
        );
        assert_eq!(methods[0].container.as_deref(), Some(receiver));
    }
}

#[test]
fn effective_method_set_elects_projected_interface_contracts_and_returns() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class Wide {} class Narrow extends Wide {} class Text {} ",
                "interface Generic<T, R> { R value(); void accept(T value); } ",
                "interface Fixed { Wide value(); void accept(Text value); } ",
                "interface Combined extends Generic<Text, Narrow>, Fixed {}"
            ),
        )
        .unwrap();

    let accept = index.find_effective_methods_by_name("p.Combined", "accept");
    assert_eq!(
        accept.len(),
        1,
        "override-equivalent projected abstracts are one receiver method: {accept:#?}"
    );
    let value = index.find_effective_methods_by_name("p.Combined", "value");
    assert_eq!(
        value.len(),
        1,
        "the projected covariant return must elect one declaration: {value:#?}"
    );
    assert_eq!(value[0].container.as_deref(), Some("p.Generic"));
}

#[test]
fn effective_method_set_preserves_projected_abstract_default_conflict() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/String.java"),
            "package java.lang; public final class String {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; interface Generic<T> { default void accept(T value) {} } ",
                "interface Fixed { void accept(String value); } ",
                "abstract class Conflict implements Generic<String>, Fixed {}"
            ),
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Conflict", "accept");
    assert_eq!(
        methods.len(),
        2,
        "unrelated default/abstract declarations remain a source conflict: {methods:#?}"
    );
}

#[test]
fn effective_method_set_preserves_unrelated_abstract_default_conflicts() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; interface DefaultSide { default void run() {} } ",
                "interface AbstractSide { void run(); } ",
                "abstract class Conflict implements DefaultSide, AbstractSide {} ",
                "class Resolved implements DefaultSide, AbstractSide { public void run() {} }"
            ),
        )
        .unwrap();

    let conflict = index.find_effective_methods_by_name("p.Conflict", "run");
    assert_eq!(
        conflict.len(),
        2,
        "unrelated abstract/default contracts require an explicit implementation"
    );
    assert!(index
        .find_method_by_args_with_inheritance("p.Conflict", "run", 0)
        .is_none());
    let resolved = index.find_effective_methods_by_name("p.Resolved", "run");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].container.as_deref(), Some("p.Resolved"));
}

#[test]
fn effective_method_set_is_coherent_across_workspace_and_dependency_owners() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dependency.Base",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        crate::Symbol {
            return_type: Some("java.lang.Object".into()),
            ..synthetic_public_method("dependency.Base", "value")
        },
    ]);
    index
        .index_file_with_source(
            &temp.path().join("app/Child.java"),
            concat!(
                "package app; public class Child extends dependency.Base { ",
                "public String value() { return null; } }"
            ),
        )
        .unwrap();

    for _ in 0..4 {
        let methods = index.find_effective_methods_by_name("app.Child", "value");
        assert_eq!(methods.len(), 1, "authority owners were mixed: {methods:?}");
        assert_eq!(methods[0].container.as_deref(), Some("app.Child"));
    }
}

#[test]
fn effective_method_set_exposes_public_object_methods_on_interface_receivers() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            concat!(
                "package java.lang; public class Object { ",
                "public String toString() { return null; } ",
                "private void hidden() {} public static void factory() {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Contract.java"),
            "package p; public interface Contract { void run(); }",
        )
        .unwrap();

    let methods = index.find_effective_methods_by_name("p.Contract", "toString");
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].container.as_deref(), Some("java.lang.Object"));
    assert!(index
        .find_effective_methods_by_name("p.Contract", "hidden")
        .is_empty());
    assert!(index
        .find_effective_methods_by_name("p.Contract", "factory")
        .is_empty());
}

#[test]
fn revision_usage_inheritance_exhausts_superclasses_before_interfaces() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("p/Types.java");
    index
        .index_file_with_source(
            &path,
            concat!(
                "package p; class Grand { public void target() {} } ",
                "class Parent extends Grand {} ",
                "interface Contract { default void target() {} } ",
                "class Child extends Parent implements Contract { ",
                "void use() { target(); } }"
            ),
        )
        .unwrap();

    assert_eq!(index.find_usages("p.Grand.target()").len(), 1);
    assert!(
        index.find_usages("p.Contract.target()").is_empty(),
        "a direct interface default must not precede a grandparent class declaration"
    );
}

#[test]
fn revision_usage_inheritance_refuses_ambiguous_wildcard_parent() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base { public void target() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("r/Base.java"),
            "package r; public class Base { public void target() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Child.java"),
            concat!(
                "package app; import q.*; import r.*; ",
                "class Child extends Base { void use() { target(); } }"
            ),
        )
        .unwrap();

    assert!(index.find_usages("q.Base.target()").is_empty());
    assert!(index.find_usages("r.Base.target()").is_empty());
}

#[test]
fn usage_resolution_refuses_java_lang_wildcard_ambiguity() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("java/lang/String.java"),
            concat!(
                "package java.lang; public class String { ",
                "public void target() {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("q/String.java"),
            "package q; public class String { public void target() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Use.java"),
            concat!(
                "package app; import q.*; class Use { ",
                "void run(String value) { value.target(); } }"
            ),
        )
        .unwrap();

    assert!(index.find_usages("java.lang.String.target()").is_empty());
    assert!(index.find_usages("q.String.target()").is_empty());
}

#[test]
fn dependency_fqns_record_the_inherited_method_declaration_owner() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let contract = temp.path().join("Contract.java");
    let implementation = temp.path().join("Implementation.java");
    let client = temp.path().join("Client.java");

    index
        .index_file_with_source(
            &contract,
            concat!(
                "package api; public interface Contract { ",
                "default String load() { return null; } }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &implementation,
            concat!(
                "package impl; import api.Contract; ",
                "public class Implementation implements Contract {}"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Implementation; class Client { ",
                "int run(Implementation implementation) { ",
                "return implementation.load().length(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let dependencies = index
        .get_current_file_dependency_fqns(client_id)
        .unwrap()
        .fqns;
    assert!(
        dependencies
            .iter()
            .any(|dependency| dependency.starts_with("api.Contract.load(")),
        "inherited call did not retain its declaration owner: {dependencies:?}"
    );
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.starts_with("api.Contract.load(")),
        "forward method binding lost the inherited declaration: {method_usages:?}"
    );
    assert!(
        dependencies
            .iter()
            .any(|fqn| fqn.as_ref() == "java.lang.String.length()"),
        "chained call did not advance through the inherited return type: {dependencies:?}"
    );
    assert!(
        dependencies
            .iter()
            .all(|fqn| fqn.as_ref() != "impl.Implementation.length()"),
        "outer call was incorrectly attributed to the root receiver: {dependencies:?}"
    );
    let legacy_dependencies = index.get_file_dependency_fqns(client_id);
    assert!(
        legacy_dependencies
            .windows(2)
            .all(|pair| pair[0] != pair[1]),
        "legacy dependency snapshots must remain sorted and deduplicated: {legacy_dependencies:?}"
    );
    assert!(
        legacy_dependencies
            .iter()
            .any(|fqn| fqn.starts_with("api.Contract.load(")),
        "legacy dependency snapshots must retain indexed usage dependencies: {legacy_dependencies:?}"
    );

    index
        .index_file_with_source(&client, "package app; class Client {}")
        .unwrap();
    assert!(index.get_file_resolved_method_usages(client_id).is_empty());
}

#[test]
fn inherited_chain_return_types_use_the_declaring_files_imports() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let result = temp.path().join("Result.java");
    let contract = temp.path().join("Contract.java");
    let implementation = temp.path().join("Implementation.java");
    let client = temp.path().join("Client.java");

    index
        .index_file_with_source(
            &result,
            "package model; public class Result { public int size() { return 0; } }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &contract,
            concat!(
                "package api; import model.Result; public interface Contract { ",
                "Result load(); }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &implementation,
            concat!(
                "package impl; import api.Contract; ",
                "public abstract class Implementation implements Contract {}"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Implementation; class Client { ",
                "int run(Implementation implementation) { ",
                "return implementation.load().size(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.Contract.load()"),
        "inherited declaration owner was lost: {method_usages:?}"
    );
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "model.Result.size()"),
        "declaration import context was not used for the return type: {method_usages:?}"
    );
    assert!(method_usages
        .iter()
        .all(|(_, fqn)| fqn.as_ref() != "impl.Implementation.size()"));
}

#[test]
fn ambiguous_declaring_file_return_type_does_not_advance_usage_chain() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (path, source) in [
        (
            "alpha/Result.java",
            "package alpha; public class Result { public void alphaOnly() {} }",
        ),
        (
            "beta/Result.java",
            "package beta; public class Result { public void betaOnly() {} }",
        ),
        (
            "api/Factory.java",
            concat!(
                "package api; import alpha.*; import beta.*; ",
                "public class Factory { public Result make() { return null; } }"
            ),
        ),
        (
            "use/Client.java",
            concat!(
                "package use; import api.Factory; class Client { ",
                "void run(Factory factory) { factory.make().alphaOnly(); } }"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(path), source)
            .unwrap();
    }

    let client_id = index
        .get_file_id(&temp.path().join("use/Client.java"))
        .unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.Factory.make()"),
        "the unambiguous first call should still be recorded: {method_usages:?}"
    );
    assert!(
        method_usages.iter().all(|(_, fqn)| {
            !matches!(
                fqn.as_ref(),
                "alpha.Result.alphaOnly()" | "beta.Result.betaOnly()"
            )
        }),
        "an ambiguous return spelling must not select a chain owner: {method_usages:?}"
    );
}

#[test]
fn inherited_method_resolution_prefers_the_class_hierarchy_over_interfaces() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "Contract.java",
            concat!(
                "package api; public interface Contract { ",
                "default String load() { return \"interface\"; } }"
            ),
        ),
        (
            "GrandBase.java",
            concat!(
                "package base; public class GrandBase { ",
                "public String load() { return \"class\"; } }"
            ),
        ),
        (
            "Base.java",
            "package base; public class Base extends GrandBase {}",
        ),
        (
            "Child.java",
            concat!(
                "package impl; import api.Contract; import base.Base; ",
                "public class Child extends Base implements Contract {}"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Child; class Client { ",
                "String run(Child child) { return child.load(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert_eq!(
        method_usages.len(),
        1,
        "unexpected calls: {method_usages:?}"
    );
    assert_eq!(method_usages[0].1.as_ref(), "base.GrandBase.load()");
}

#[test]
fn unrelated_inherited_interface_methods_are_not_selected_by_declaration_order() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "First.java",
            "package api; public interface First { String load(); }",
        ),
        (
            "Second.java",
            "package api; public interface Second { String load(); }",
        ),
        (
            "Implementation.java",
            concat!(
                "package impl; import api.First; import api.Second; ",
                "public abstract class Implementation implements First, Second {}"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Implementation; class Client { ",
                "String run(Implementation implementation) { return implementation.load(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    assert!(
        index.get_file_resolved_method_usages(client_id).is_empty(),
        "an unrelated interface must not win merely by implements order"
    );
}

#[test]
fn subinterface_method_shadows_its_parent_declaration() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "Parent.java",
            "package api; public interface Parent { String load(); }",
        ),
        (
            "Child.java",
            concat!(
                "package api; public interface Child extends Parent { ",
                "String load(); }"
            ),
        ),
        (
            "Implementation.java",
            concat!(
                "package impl; import api.Child; ",
                "public abstract class Implementation implements Child {}"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Implementation; class Client { ",
                "String run(Implementation implementation) { return implementation.load(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert_eq!(
        method_usages.len(),
        1,
        "unexpected calls: {method_usages:?}"
    );
    assert_eq!(method_usages[0].1.as_ref(), "api.Child.load()");
}

#[test]
fn inherited_overload_chains_use_the_selected_declaration_and_return_type() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "Numeric.java",
            "package model; public class Numeric { public int size() { return 0; } }",
        ),
        (
            "Textual.java",
            "package model; public class Textual { public int size() { return 0; } }",
        ),
        (
            "Base.java",
            concat!(
                "package base; import model.Numeric; public class Base { ",
                "public Numeric load(int value) { return null; } }"
            ),
        ),
        (
            "Child.java",
            concat!(
                "package impl; import base.Base; import model.Textual; ",
                "public class Child extends Base { ",
                "public Textual load(String value) { return null; } }"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Child; class Client { ",
                "int run(Child child) { return child.load(1).size(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "base.Base.load(int)"),
        "the inherited overload was not selected: {method_usages:?}"
    );
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "model.Numeric.size()"),
        "the chain did not use the selected overload's return type: {method_usages:?}"
    );
    assert!(method_usages.iter().all(|(_, fqn)| {
        !matches!(
            fqn.as_ref(),
            "impl.Child.load(java.lang.String)" | "model.Textual.size()" | "impl.Child.size()"
        )
    }));
}

#[test]
fn declared_variable_types_select_same_arity_overloads() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let service = temp.path().join("Service.java");
    let client = temp.path().join("Client.java");

    index
        .index_file_with_source(
            &service,
            concat!(
                "package api; public class Service { ",
                "public void consume(String value) {} ",
                "public void consume(Integer value) {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import api.Service; class Client { ",
                "void run(Service service, String text, Integer number) { ",
                "service.consume(text); service.consume(number); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(method_usages
        .iter()
        .any(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.String)"));
    assert!(method_usages
        .iter()
        .any(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.Integer)"));
}

#[test]
fn argument_variable_types_respect_lexical_scope_and_declaration_order() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let service = temp.path().join("Service.java");
    let client = temp.path().join("Client.java");

    index
        .index_file_with_source(
            &service,
            concat!(
                "package api; public class Service { ",
                "public void consume(String value) {} ",
                "public void consume(Integer value) {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import api.Service; class Client { ",
                "void run(Service service, String parameter) { ",
                "service.consume(parameter); service.consume(later); ",
                "Integer later = 1; service.consume(later); ",
                "{ Integer scoped = 1; service.consume(scoped); } ",
                "{ String scoped = \"text\"; service.consume(scoped); } } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert_eq!(
        method_usages
            .iter()
            .filter(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.String)")
            .count(),
        2,
        "parameter and String block local should resolve: {method_usages:?}"
    );
    assert_eq!(
        method_usages
            .iter()
            .filter(|(_, fqn)| fqn.as_ref() == "api.Service.consume(java.lang.Integer)")
            .count(),
        2,
        "only the post-declaration and Integer block calls should resolve: {method_usages:?}"
    );
    assert_eq!(
        method_usages.len(),
        4,
        "the use before `later` is declared must remain unresolved: {method_usages:?}"
    );
}

#[test]
fn unknown_same_arity_overloads_do_not_fabricate_a_chained_receiver() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "Numeric.java",
            "package model; public class Numeric { public int size() { return 0; } }",
        ),
        (
            "Textual.java",
            "package model; public class Textual { public int size() { return 0; } }",
        ),
        (
            "Base.java",
            concat!(
                "package base; import model.Numeric; import model.Textual; ",
                "public class Base { ",
                "public Numeric load(int value) { return null; } ",
                "public Textual load(String value) { return null; } }"
            ),
        ),
        (
            "Child.java",
            "package impl; import base.Base; public class Child extends Base {}",
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Child; class Client { ",
                "int run(Child child, Object value) { ",
                "return child.load(value).size(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages.is_empty(),
        "an unknown argument selected an overload or fabricated the outer owner: {method_usages:?}"
    );
    let dependencies = index
        .get_current_file_dependency_fqns(client_id)
        .unwrap()
        .fqns;
    assert!(
        dependencies
            .iter()
            .all(|fqn| fqn.as_ref() != "impl.Child.size()"),
        "failed chain resolution fell back to the root receiver: {dependencies:?}"
    );
}

#[test]
fn unrelated_interface_overloads_are_selected_by_descriptor_not_order() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, source) in [
        (
            "Numeric.java",
            "package model; public class Numeric { public int size() { return 0; } }",
        ),
        (
            "Textual.java",
            "package model; public class Textual { public int size() { return 0; } }",
        ),
        (
            "NumericLoader.java",
            concat!(
                "package api; import model.Numeric; public interface NumericLoader { ",
                "Numeric load(int value); }"
            ),
        ),
        (
            "TextLoader.java",
            concat!(
                "package api; import model.Textual; public interface TextLoader { ",
                "Textual load(String value); }"
            ),
        ),
        (
            "Implementation.java",
            concat!(
                "package impl; import api.NumericLoader; import api.TextLoader; ",
                "public abstract class Implementation implements NumericLoader, TextLoader {}"
            ),
        ),
    ] {
        index
            .index_file_with_source(&temp.path().join(name), source)
            .unwrap();
    }
    let client = temp.path().join("Client.java");
    index
        .index_file_with_source(
            &client,
            concat!(
                "package app; import impl.Implementation; class Client { ",
                "int run(Implementation implementation) { ",
                "return implementation.load(1).size(); } }"
            ),
        )
        .unwrap();

    let client_id = index.get_file_id(&client).unwrap();
    let method_usages = index.get_file_resolved_method_usages(client_id);
    assert!(
        method_usages
            .iter()
            .any(|(_, fqn)| fqn.as_ref() == "api.NumericLoader.load(int)"),
        "descriptor did not select the matching interface overload: {method_usages:?}"
    );
    assert!(method_usages
        .iter()
        .any(|(_, fqn)| fqn.as_ref() == "model.Numeric.size()"));
    assert!(method_usages
        .iter()
        .all(|(_, fqn)| fqn.as_ref() != "api.TextLoader.load(java.lang.String)"));
}

#[tokio::test]
async fn test_reverse_deps_cleaned_on_remove() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java
    let user_path = root.join("User.java");
    std::fs::write(&user_path, "package com.example;\npublic class User {}").unwrap();

    // Create Main.java that imports User
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        "package com.example;\nimport com.example.User;\npublic class Main {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // Verify Main.java is in dependents
    let dependents_before = index.get_dependent_files("com.example.User");
    assert!(dependents_before.contains(&main_id));

    // Remove Main.java
    index.remove_file(main_id);

    // Verify Main.java is no longer in dependents
    let dependents_after = index.get_dependent_files("com.example.User");
    assert!(
        !dependents_after.contains(&main_id),
        "Removed file should not be in dependents"
    );
}

#[tokio::test]
async fn test_reverse_deps_updated_on_reindex() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java and Product.java
    let user_path = root.join("User.java");
    std::fs::write(&user_path, "package com.example;\npublic class User {}").unwrap();

    let product_path = root.join("Product.java");
    std::fs::write(
        &product_path,
        "package com.example;\npublic class Product {}",
    )
    .unwrap();

    // Create Main.java that initially imports User
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        "package com.example;\nimport com.example.User;\npublic class Main {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&user_path).await.unwrap();
    index.index_file(&product_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // Verify Main depends on User, not Product
    assert!(index
        .get_dependent_files("com.example.User")
        .contains(&main_id));
    assert!(!index
        .get_dependent_files("com.example.Product")
        .contains(&main_id));

    // Now modify Main.java to import Product instead of User
    let new_main_content =
        "package com.example;\nimport com.example.Product;\npublic class Main {}";

    // Re-index with new content
    index
        .index_file_with_source_and_id(&main_path, new_main_content, main_id)
        .unwrap();

    // Verify Main now depends on Product, not User
    assert!(
        !index
            .get_dependent_files("com.example.User")
            .contains(&main_id),
        "After re-index, Main should no longer depend on User"
    );
    assert!(
        index
            .get_dependent_files("com.example.Product")
            .contains(&main_id),
        "After re-index, Main should depend on Product"
    );
}

#[tokio::test]
async fn test_symbol_usages_do_not_accumulate_on_reindex() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let user_path = root.join("User.java");
    std::fs::write(&user_path, "package com.example;\npublic class User {}").unwrap();

    let main_path = root.join("Main.java");
    let main_source = r#"
package com.example;
import com.example.User;
public class Main {
    void run() {
        User u = new User();
    }
}
"#;
    std::fs::write(&main_path, main_source).unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();
    let initial_usage_count = index
        .find_usages("com.example.User")
        .into_iter()
        .filter(|loc| loc.file_id == main_id)
        .count();
    assert!(
        initial_usage_count > 0,
        "Expected Main.java to contribute at least one usage"
    );

    // Re-index identical content. Usage count must stay stable (no accumulation).
    index
        .index_file_with_source_and_id(&main_path, main_source, main_id)
        .unwrap();
    let after_reindex_count = index
        .find_usages("com.example.User")
        .into_iter()
        .filter(|loc| loc.file_id == main_id)
        .count();
    assert_eq!(
        after_reindex_count, initial_usage_count,
        "Re-indexing the same file should replace old usages, not append duplicates"
    );

    // Remove all User references from Main and re-index; old usages must be removed.
    let main_without_user = r#"
package com.example;
public class Main {
    void run() {}
}
"#;
    index
        .index_file_with_source_and_id(&main_path, main_without_user, main_id)
        .unwrap();
    let after_removal_count = index
        .find_usages("com.example.User")
        .into_iter()
        .filter(|loc| loc.file_id == main_id)
        .count();
    assert_eq!(
        after_removal_count, 0,
        "Re-indexing after reference removal should clear stale usages for the file"
    );
}

#[tokio::test]
async fn test_get_package_class_names() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create various classes in java.lang package
    let string_path = root.join("String.java");
    std::fs::write(&string_path, "package java.lang;\npublic class String {}").unwrap();

    let integer_path = root.join("Integer.java");
    std::fs::write(&integer_path, "package java.lang;\npublic class Integer {}").unwrap();

    let runnable_path = root.join("Runnable.java");
    std::fs::write(
        &runnable_path,
        "package java.lang;\npublic interface Runnable {}",
    )
    .unwrap();

    // Create a class in different package
    let other_path = root.join("Other.java");
    std::fs::write(&other_path, "package com.example;\npublic class Other {}").unwrap();

    // Create an inner class (should be excluded)
    let outer_path = root.join("Outer.java");
    std::fs::write(
        &outer_path,
        "package java.lang;\npublic class Outer { public static class Inner {} }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&string_path).await.unwrap();
    index.index_file(&integer_path).await.unwrap();
    index.index_file(&runnable_path).await.unwrap();
    index.index_file(&other_path).await.unwrap();
    index.index_file(&outer_path).await.unwrap();

    // Query java.lang classes
    let java_lang_classes = index.get_package_class_names("java.lang");

    // Should include String, Integer, Runnable, Outer (but not Inner)
    assert!(java_lang_classes.contains(&"String".into()));
    assert!(java_lang_classes.contains(&"Integer".into()));
    assert!(java_lang_classes.contains(&"Runnable".into()));
    assert!(java_lang_classes.contains(&"Outer".into()));

    // Should NOT include Inner (inner class) or Other (different package)
    assert!(!java_lang_classes.contains(&"Inner".into()));
    assert!(!java_lang_classes.contains(&"Other".into()));

    // Query com.example package
    let com_example_classes = index.get_package_class_names("com.example");
    assert_eq!(com_example_classes.len(), 1);
    assert!(com_example_classes.contains(&"Other".into()));

    // Empty package should return empty vec
    let empty_package = index.get_package_class_names("nonexistent.package");
    assert!(empty_package.is_empty());

    // Results should be sorted (deterministic)
    assert_eq!(java_lang_classes, {
        let mut sorted = java_lang_classes.clone();
        sorted.sort();
        sorted
    });
}

/// The `java.lang` memo is revision-keyed, not one-shot.
///
/// It is queried before JDK indexing lands, when the answer is legitimately
/// empty. A `OnceLock`-style cache would pin that emptiness for the session
/// and make every `java.lang` type permanently unresolvable — worse than the
/// per-resolution recomputation it replaced.
#[tokio::test]
async fn java_lang_memo_observes_types_indexed_after_the_first_query() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root.clone());

    assert!(
        index.java_lang_class_names().is_empty(),
        "no JDK indexed yet, so the set must be empty"
    );

    let string_path = root.join("String.java");
    std::fs::write(&string_path, "package java.lang;\npublic class String {}").unwrap();
    index.index_file(&string_path).await.unwrap();

    let names = index.java_lang_class_names();
    assert!(
        names.contains("String"),
        "the memo must observe types published after the first query, got {names:?}"
    );

    // A second query at an unchanged revision must return the same memo.
    assert!(index.java_lang_class_names().contains("String"));
}

/// `package_has_class_names` is the short-circuiting emptiness answer used by
/// the semantic walker's per-import gate. It must agree with
/// `get_package_class_names(..).is_empty()` on every input, including packages
/// that hold only inner classes or non-type symbols.
#[tokio::test]
async fn package_has_class_names_matches_get_package_class_names_emptiness() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let types_path = root.join("Types.java");
    std::fs::write(
        &types_path,
        "package com.example;\npublic class Outer { public static class Inner {} }\npublic interface Contract {}",
    )
    .unwrap();

    // A package whose only member is an inner class: the prefix map holds an
    // entry, but no top-level type qualifies.
    let inner_only_path = root.join("Holder.java");
    std::fs::write(
        &inner_only_path,
        "package com.example.inner;\nclass Holder { static class Nested {} }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&types_path).await.unwrap();
    index.index_file(&inner_only_path).await.unwrap();

    for package in [
        "com.example",
        "com.example.inner",
        "nonexistent.package",
        "com",
        "",
    ] {
        assert_eq!(
            index.package_has_class_names(package),
            !index.get_package_class_names(package).is_empty(),
            "emptiness disagreement for package {package:?}"
        );
    }

    assert!(index.package_has_class_names("com.example"));
    assert!(!index.package_has_class_names("nonexistent.package"));
}

#[test]
fn test_external_symbol_tracking_add_external() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

    // Create test symbols
    let class_symbol = Symbol {
        fqn: "java.util.ArrayList".into(),
        simple_name: "ArrayList".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };

    let method_symbol = Symbol {
        fqn: "java.util.ArrayList.add".into(),
        simple_name: "add".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::METHOD,
        modifiers: Modifiers::default(),
        container: Some("java.util.ArrayList".into()),
        return_type: Some("boolean".into()),
        parameters: vec!["E".into()],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };

    // Add as external symbols via add_external_symbol()
    index.add_external_symbol(class_symbol);
    index.add_external_symbol(method_symbol);

    // Get external symbols
    let external = index.get_external_symbols();

    println!("Total symbols: {}", index.symbol_count());
    println!("External symbols retrieved: {}", external.len());

    // Both symbols should be retrieved
    assert_eq!(external.len(), 2, "Should retrieve both external symbols");

    let fqns: Vec<_> = external.iter().map(|s| s.fqn.as_ref()).collect();
    assert!(fqns.contains(&"java.util.ArrayList"));
    assert!(fqns.contains(&"java.util.ArrayList.add"));
    let inner = index.inner.read();
    let jdk_id = inner
        .external_virtual_file_id("jdk")
        .expect("registered JDK source");
    let source_symbols = &inner.file_symbols[&jdk_id];
    assert_eq!(source_symbols.len(), 2);
    for source_symbol in source_symbols {
        assert!(Arc::ptr_eq(
            source_symbol,
            inner.symbols.get(source_symbol.fqn.as_ref()).unwrap()
        ));
    }
}

#[test]
fn test_external_symbols_populate_package_class_index() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

    index.add_external_symbol(Symbol {
        fqn: "java.lang.String".into(),
        simple_name: "String".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: Some("java.lang.Object".into()),
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });
    index.add_external_symbol(Symbol {
        fqn: "java.lang.Integer".into(),
        simple_name: "Integer".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: Some("java.lang.Object".into()),
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });
    index.add_external_symbol(Symbol {
        fqn: "java.lang.Runnable".into(),
        simple_name: "Runnable".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::INTERFACE,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });

    let java_lang_classes = index.get_package_class_names("java.lang");
    assert!(
        java_lang_classes.contains(&"String".to_string()),
        "package_to_classes should include external class String"
    );
    assert!(
        java_lang_classes.contains(&"Integer".to_string()),
        "package_to_classes should include external class Integer"
    );
    assert!(
        java_lang_classes.contains(&"Runnable".to_string()),
        "package_to_classes should include external interface Runnable"
    );
}

#[test]
fn test_external_symbols_populate_inheritance_indices() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

    // Interface + base + impl class hierarchy
    index.add_external_symbol(Symbol {
        fqn: "pkg.Api".into(),
        simple_name: "Api".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::INTERFACE,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });
    index.add_external_symbol(Symbol {
        fqn: "pkg.Base".into(),
        simple_name: "Base".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: Some("java.lang.Object".into()),
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });
    index.add_external_symbol(Symbol {
        fqn: "pkg.Impl".into(),
        simple_name: "Impl".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: Some("pkg.Base".into()),
        interfaces: vec!["pkg.Api".into()],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });

    // Methods for override lookup
    index.add_external_symbol(Symbol {
        fqn: "pkg.Base.run".into(),
        simple_name: "run".into(),
        range: TextRange::new(1, 2),
        kind: SymbolKind::METHOD,
        modifiers: Modifiers::default(),
        container: Some("pkg.Base".into()),
        return_type: Some("void".into()),
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });
    index.add_external_symbol(Symbol {
        fqn: "pkg.Impl.run".into(),
        simple_name: "run".into(),
        range: TextRange::new(3, 4),
        kind: SymbolKind::METHOD,
        modifiers: Modifiers::default(),
        container: Some("pkg.Impl".into()),
        return_type: Some("void".into()),
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    });

    let implementors = index.get_interface_implementations("pkg.Api");
    assert!(
        implementors.iter().any(|fqn| fqn == "pkg.Impl"),
        "interface_implementations should include external implementing class"
    );

    let inner = index.inner.read();
    let subclasses = inner.direct_subtypes("pkg.Base");
    assert!(
        subclasses.iter().any(|fqn| fqn.as_ref() == "pkg.Impl"),
        "inheritance graph should include external subclass"
    );
}

fn synthetic_type(
    fqn: &str,
    kind: lsp_types::SymbolKind,
    parent_class: Option<&str>,
    interfaces: &[&str],
    modifiers: crate::Modifiers,
) -> crate::Symbol {
    crate::Symbol {
        fqn: fqn.into(),
        simple_name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
        range: crate::TextRange::new(0, 0),
        kind,
        modifiers,
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: parent_class.map(Into::into),
        interfaces: interfaces.iter().copied().map(Into::into).collect(),
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

fn synthetic_method(container: &str, name: &str) -> crate::Symbol {
    crate::Symbol {
        fqn: format!("{container}.{name}()").into(),
        simple_name: name.into(),
        range: crate::TextRange::new(0, 0),
        kind: lsp_types::SymbolKind::METHOD,
        modifiers: crate::Modifiers::default(),
        container: Some(container.into()),
        return_type: Some("void".into()),
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

fn synthetic_public_method(container: &str, name: &str) -> crate::Symbol {
    crate::Symbol {
        modifiers: crate::Modifiers {
            is_public: true,
            ..crate::Modifiers::default()
        },
        ..synthetic_method(container, name)
    }
}

fn synthetic_public_typed_method(
    container: &str,
    name: &str,
    parameter_fqn: &str,
    parameter_display: &str,
    return_type: &str,
) -> crate::Symbol {
    crate::Symbol {
        fqn: format!("{container}.{name}({parameter_fqn})").into(),
        parameters: vec![format!("{parameter_display} arg0").into()],
        return_type: Some(return_type.into()),
        ..synthetic_public_method(container, name)
    }
}

#[test]
fn callable_family_crosses_workspace_and_external_authorities() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "model.Input",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        synthetic_type(
            "model.Result",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        synthetic_type(
            "dep.Contract",
            lsp_types::SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        crate::Symbol {
            modifiers: crate::Modifiers {
                is_public: true,
                is_abstract: true,
                ..crate::Modifiers::default()
            },
            ..synthetic_public_typed_method(
                "dep.Contract",
                "map",
                "model.Input",
                "Input",
                "model.Result",
            )
        },
    ]);
    let base_path = temp.path().join("Base.java");
    index
        .index_file_with_source(
            &base_path,
            concat!(
                "package app; public class Base implements dep.Contract { ",
                "public model.Result map(model.Input input) { return null; } }"
            ),
        )
        .unwrap();
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.ExternalChild",
            lsp_types::SymbolKind::CLASS,
            Some("app.Base"),
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        synthetic_public_typed_method(
            "dep.ExternalChild",
            "map",
            "model.Input",
            "Input",
            "model.Result",
        ),
    ]);

    let roots = [
        index
            .find_methods_by_name("dep.Contract", "map")
            .into_iter()
            .next()
            .unwrap(),
        index
            .find_methods_by_name("app.Base", "map")
            .into_iter()
            .next()
            .unwrap(),
        index
            .find_methods_by_name("dep.ExternalChild", "map")
            .into_iter()
            .next()
            .unwrap(),
    ];
    let revision = index.current_workspace_revision();
    let mut expected = None;
    for root in roots {
        let family = index
            .find_current_callable_override_family(
                &crate::SymbolTarget::for_symbol(&root),
                revision,
            )
            .unwrap();
        let targets = family
            .iter()
            .map(|definition| definition.target.fqn.to_string())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(targets.len(), 3);
        assert_eq!(
            family
                .iter()
                .filter(|definition| matches!(
                    definition.source,
                    DefinitionSourceAuthority::External
                ))
                .count(),
            2
        );
        assert_eq!(
            family
                .iter()
                .filter(|definition| matches!(
                    definition.source,
                    DefinitionSourceAuthority::Workspace(_)
                ))
                .count(),
            1
        );
        if let Some(expected) = expected.as_ref() {
            assert_eq!(&targets, expected);
        } else {
            expected = Some(targets);
        }
    }
}

#[test]
fn refactoring_callable_hierarchy_requires_implicit_java_root() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Local.java");
    index
        .index_file_with_source(
            &path,
            "package app; class Local { public String toString() { return \"x\"; } }",
        )
        .unwrap();
    let method = index
        .find_methods_by_name("app.Local", "toString")
        .into_iter()
        .next()
        .unwrap();
    let target = crate::SymbolTarget::for_symbol(&method);
    let revision = index.current_workspace_revision();

    assert!(!index
        .current_callable_owner_hierarchy_is_complete(&target, revision)
        .unwrap());
    assert!(matches!(
        index.find_current_callable_override_family_for_refactoring_cancellable(
            &target,
            revision,
            || false
        ),
        Err(CurrentUsageQueryError::Unavailable(_))
    ));
}

#[test]
fn refactoring_callable_hierarchy_rejects_missing_intermediate_ancestor() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![synthetic_type(
        "java.lang.Object",
        lsp_types::SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    )]);
    let path = temp.path().join("Hierarchy.java");
    index
        .index_file_with_source(
            &path,
            concat!(
                "package app; class Base extends missing.Parent {} ",
                "class Child extends Base { public void run() {} }"
            ),
        )
        .unwrap();
    let method = index
        .find_methods_by_name("app.Child", "run")
        .into_iter()
        .next()
        .unwrap();
    let result = index.find_current_callable_override_family_for_refactoring_cancellable(
        &crate::SymbolTarget::for_symbol(&method),
        index.current_workspace_revision(),
        || false,
    );
    assert!(matches!(
        result,
        Err(CurrentUsageQueryError::Unavailable(_))
    ));
}

#[test]
fn refactoring_callable_conflicts_cover_ancestors_and_non_overriding_descendants() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![synthetic_type(
        "java.lang.Object",
        lsp_types::SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    )]);
    let path = temp.path().join("Hierarchy.java");
    index
        .index_file_with_source(
            &path,
            concat!(
                "package app; ",
                "class Parent { public void ancestorName() {} } ",
                "class Base extends Parent { public void oldName() {} } ",
                "class Child extends Base {} ",
                "class Grandchild extends Child { public void descendantName() {} }"
            ),
        )
        .unwrap();
    let method = index
        .find_methods_by_name("app.Base", "oldName")
        .into_iter()
        .next()
        .unwrap();
    let revision = index.current_workspace_revision();
    let family = index
        .find_current_callable_override_family_for_refactoring_cancellable(
            &crate::SymbolTarget::for_symbol(&method),
            revision,
            || false,
        )
        .unwrap();
    let targets = family
        .iter()
        .map(|definition| definition.target.clone())
        .collect::<Vec<_>>();

    for conflicting_name in ["ancestorName", "descendantName"] {
        assert!(index
            .current_callable_family_conflicts_with_name_for_refactoring(
                &targets,
                conflicting_name,
                revision,
                || false,
            )
            .unwrap());
    }
    assert!(!index
        .current_callable_family_conflicts_with_name_for_refactoring(
            &targets,
            "freshName",
            revision,
            || false,
        )
        .unwrap());
}

#[test]
fn dependency_wildcard_signature_does_not_match_concrete_workspace_parameter() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.WildcardContract",
            lsp_types::SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers {
                is_public: true,
                ..crate::Modifiers::default()
            },
        ),
        crate::Symbol {
            modifiers: crate::Modifiers {
                is_public: true,
                is_abstract: true,
                ..crate::Modifiers::default()
            },
            ..synthetic_public_typed_method(
                "dep.WildcardContract",
                "accept",
                "java.util.List",
                "java.util.List<? extends java.lang.Number>",
                "void",
            )
        },
    ]);
    let child_path = temp.path().join("WildcardChild.java");
    index
        .index_file_with_source(
            &child_path,
            concat!(
                "package app; public class WildcardChild implements dep.WildcardContract { ",
                "public void accept(java.util.List<java.lang.Number> values) {} }"
            ),
        )
        .unwrap();
    let matching_path = temp.path().join("MatchingWildcardChild.java");
    index
        .index_file_with_source(
            &matching_path,
            concat!(
                "package app; public class MatchingWildcardChild implements dep.WildcardContract { ",
                "public void accept(java.util.List<? extends java.lang.Number> values) {} }"
            ),
        )
        .unwrap();
    let source_contract_path = temp.path().join("SourceWildcardContract.java");
    index
        .index_file_with_source(
            &source_contract_path,
            concat!(
                "package app; public interface SourceWildcardContract { ",
                "void check(java.util.List<? super java.lang.Number> values); }"
            ),
        )
        .unwrap();
    let source_child_path = temp.path().join("SourceConcreteChild.java");
    index
        .index_file_with_source(
            &source_child_path,
            concat!(
                "package app; public class SourceConcreteChild implements SourceWildcardContract { ",
                "public void check(java.util.List<java.lang.Number> values) {} }"
            ),
        )
        .unwrap();

    let revision = index.current_workspace_revision();
    for owner in ["dep.WildcardContract", "app.MatchingWildcardChild"] {
        let method = index
            .find_methods_by_name(owner, "accept")
            .into_iter()
            .next()
            .unwrap();
        let family = index
            .find_current_callable_override_family(
                &crate::SymbolTarget::for_symbol(&method),
                revision,
            )
            .unwrap();
        assert_eq!(
            family.len(),
            2,
            "equal dependency/source wildcard signatures must join for {owner}"
        );
    }
    for (owner, method_name) in [
        ("app.WildcardChild", "accept"),
        ("app.SourceWildcardContract", "check"),
        ("app.SourceConcreteChild", "check"),
    ] {
        let method = index
            .find_methods_by_name(owner, method_name)
            .into_iter()
            .next()
            .unwrap();
        let family = index
            .find_current_callable_override_family(
                &crate::SymbolTarget::for_symbol(&method),
                revision,
            )
            .unwrap();
        assert_eq!(
            family.len(),
            1,
            "wildcard variance must not be collapsed for {owner}"
        );
    }
}

#[test]
fn transitive_inheritance_walk_terminates_on_cycles_without_returning_the_root() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "graph.A",
            lsp_types::SymbolKind::CLASS,
            Some("graph.B"),
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "graph.B",
            lsp_types::SymbolKind::CLASS,
            Some("graph.A"),
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    assert_eq!(
        index
            .find_implementations("graph.A")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["graph.B"]
    );
}

#[test]
fn dependency_batch_publishes_inheritance_and_invalidates_partial_chains() {
    let temp = TempDir::new().unwrap();
    let child_path = temp.path().join("WorkspaceChild.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &child_path,
            r#"package app;
               import dep.AbstractDependency;
               class WorkspaceChild extends AbstractDependency {}"#,
        )
        .unwrap();

    assert_eq!(
        index.get_inheritance_chain("app.WorkspaceChild"),
        vec!["app.WorkspaceChild"],
        "an explicit binding is not a hierarchy edge until its target is indexed"
    );

    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.Contract",
            lsp_types::SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dep.AbstractDependency",
            lsp_types::SymbolKind::CLASS,
            None,
            &["dep.Contract"],
            crate::Modifiers {
                is_abstract: true,
                ..crate::Modifiers::default()
            },
        ),
        synthetic_type(
            "dep.DependencyChild",
            lsp_types::SymbolKind::CLASS,
            Some("app.WorkspaceChild"),
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    assert_eq!(
        index.get_inheritance_chain("app.WorkspaceChild"),
        vec![
            "app.WorkspaceChild",
            "dep.AbstractDependency",
            "dep.Contract"
        ],
        "publishing a dependency must invalidate an earlier partial chain"
    );
    assert_eq!(
        index
            .find_direct_subtypes("dep.AbstractDependency")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["app.WorkspaceChild"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("app.WorkspaceChild")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["dep.DependencyChild"],
        "dependency types may extend workspace types"
    );
    assert_eq!(
        index
            .find_implementations("dep.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.WorkspaceChild", "dep.DependencyChild"],
        "concrete descendants must cross dependency/workspace boundaries in both directions"
    );
}

#[test]
fn jar_batch_uses_the_same_inheritance_path_as_other_external_symbols() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let external_file_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("test-dependency.jar");

    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "dep.JarContract",
                lsp_types::SymbolKind::INTERFACE,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_type(
                "dep.JarImplementation",
                lsp_types::SymbolKind::CLASS,
                None,
                &["dep.JarContract"],
                crate::Modifiers::default(),
            ),
        ],
        external_file_id,
    );

    assert_eq!(
        index.get_interface_implementations("dep.JarContract"),
        vec!["dep.JarImplementation"]
    );
}

#[test]
fn jar_replacement_is_one_reader_visible_revision() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let external_file_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("atomic-replacement.jar");
    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "dep.AtomicOwner",
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_method("dep.AtomicOwner", "oldOnly"),
        ],
        external_file_id,
    );

    let (removed_tx, removed_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let replacement_index = Arc::clone(&index);
    let replacement = std::thread::spawn(move || {
        replacement_index.insert_symbols_batch_before_replacement_for_test(
            vec![
                synthetic_type(
                    "dep.AtomicOwner",
                    lsp_types::SymbolKind::CLASS,
                    None,
                    &[],
                    crate::Modifiers::default(),
                ),
                synthetic_method("dep.AtomicOwner", "newOnly"),
            ],
            external_file_id,
            || {
                removed_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            },
        );
    });
    removed_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("replacement should reach its swap boundary");

    let (reader_tx, reader_rx) = std::sync::mpsc::channel();
    let reader_index = Arc::clone(&index);
    let reader = std::thread::spawn(move || {
        reader_tx
            .send((
                reader_index.has_class("dep.AtomicOwner"),
                reader_index.get_symbol("dep.AtomicOwner.oldOnly").is_some(),
                reader_index.get_symbol("dep.AtomicOwner.newOnly").is_some(),
            ))
            .unwrap();
    });
    assert!(matches!(
        reader_rx.recv_timeout(std::time::Duration::from_millis(25)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    resume_tx.send(()).unwrap();
    replacement.join().unwrap();
    assert_eq!(
        reader_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("reader should resume after the complete replacement"),
        (true, false, true)
    );
    reader.join().unwrap();
}

#[test]
fn workspace_shadow_replaces_external_edges_and_survives_dependency_unload() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let external_file_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("shadowed.jar");

    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.OldBase",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dep.NewBase",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    index.insert_symbols_batch(
        vec![synthetic_type(
            "shared.Shadow",
            lsp_types::SymbolKind::CLASS,
            Some("dep.OldBase"),
            &[],
            crate::Modifiers::default(),
        )],
        external_file_id,
    );
    assert_eq!(
        index
            .find_direct_subtypes("dep.OldBase")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["shared.Shadow"]
    );

    let source_path = temp.path().join("Shadow.java");
    index
        .index_file_with_source(
            &source_path,
            "package shared; class Shadow extends dep.NewBase {}",
        )
        .unwrap();

    assert!(index.find_direct_subtypes("dep.OldBase").is_empty());
    assert_eq!(
        index
            .find_direct_subtypes("dep.NewBase")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["shared.Shadow"]
    );

    index.remove_file(external_file_id);
    let shadow = index
        .get_symbol("shared.Shadow")
        .expect("workspace declaration must remain indexed");
    assert_eq!(shadow.parent_class.as_deref(), Some("dep.NewBase"));
}

#[test]
fn removing_workspace_shadow_restores_external_declaration_loaded_first() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let external_file_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("dependency.jar");
    index.add_external_symbol(synthetic_type(
        "dep.ExternalBase",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index.insert_symbols_batch(
        vec![synthetic_type(
            "shared.Shadow",
            lsp_types::SymbolKind::CLASS,
            Some("dep.ExternalBase"),
            &[],
            crate::Modifiers::default(),
        )],
        external_file_id,
    );

    let source_path = temp.path().join("Shadow.java");
    index
        .index_file_with_source(
            &source_path,
            "package shared; class Shadow extends dep.WorkspaceBase {}",
        )
        .unwrap();
    let workspace_file_id = index.get_file_id(&source_path).unwrap();
    index.remove_file(workspace_file_id);

    let restored = index
        .get_symbol("shared.Shadow")
        .expect("the still-loaded dependency declaration must be restored");
    assert_eq!(restored.parent_class.as_deref(), Some("dep.ExternalBase"));
    assert_eq!(
        index
            .find_direct_subtypes("dep.ExternalBase")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["shared.Shadow"]
    );
}

#[test]
fn removing_workspace_shadow_restores_external_declaration_loaded_later() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "dep.ExternalBase",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    let source_path = temp.path().join("Shadow.java");
    index
        .index_file_with_source(
            &source_path,
            "package shared; class Shadow extends dep.WorkspaceBase {}",
        )
        .unwrap();

    let external_file_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("dependency.jar");
    index.insert_symbols_batch(
        vec![synthetic_type(
            "shared.Shadow",
            lsp_types::SymbolKind::CLASS,
            Some("dep.ExternalBase"),
            &[],
            crate::Modifiers::default(),
        )],
        external_file_id,
    );

    let workspace_file_id = index.get_file_id(&source_path).unwrap();
    index.remove_file(workspace_file_id);
    let restored = index
        .get_symbol("shared.Shadow")
        .expect("a dependency loaded behind workspace source must remain available");
    assert_eq!(restored.parent_class.as_deref(), Some("dep.ExternalBase"));
}

#[test]
fn external_precedence_follows_registration_not_extraction_completion() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let (preferred_id, fallback_id) = {
        let mut inner = index.inner.write();
        (
            inner.allocate_external_virtual_file_id("01-preferred.jar"),
            inner.allocate_external_virtual_file_id("02-fallback.jar"),
        )
    };

    index.insert_symbols_batch(
        vec![synthetic_type(
            "duplicate.Type",
            lsp_types::SymbolKind::CLASS,
            Some("dep.FallbackBase"),
            &[],
            crate::Modifiers::default(),
        )],
        fallback_id,
    );
    index.insert_symbols_batch(
        vec![synthetic_type(
            "duplicate.Type",
            lsp_types::SymbolKind::CLASS,
            Some("dep.PreferredBase"),
            &[],
            crate::Modifiers::default(),
        )],
        preferred_id,
    );

    assert_eq!(
        index
            .get_symbol("duplicate.Type")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.PreferredBase"),
        "parallel extraction completion must not change classpath precedence"
    );

    index.remove_file(preferred_id);
    assert_eq!(
        index
            .get_symbol("duplicate.Type")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.FallbackBase"),
        "removing the preferred dependency must reveal the next declaration"
    );
}

#[test]
fn external_precedence_follows_refreshed_classpath_order() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let preferred = PathBuf::from("/deps/preferred.jar");
    let fallback = PathBuf::from("/deps/fallback.jar");
    index.replace_external_sources("maven:/workspace", &[preferred.clone(), fallback.clone()]);
    let (preferred_id, fallback_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&preferred).unwrap(),
            inner.external_file_id_for_path(&fallback).unwrap(),
        )
    };

    let declaration = |parent: &str| {
        vec![synthetic_type(
            "duplicate.Reordered",
            lsp_types::SymbolKind::CLASS,
            Some(parent),
            &[],
            crate::Modifiers::default(),
        )]
    };
    index.insert_symbols_batch(declaration("dep.FallbackBase"), fallback_id);
    index.insert_symbols_batch(declaration("dep.PreferredBase"), preferred_id);
    assert_eq!(
        index
            .get_symbol("duplicate.Reordered")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.PreferredBase")
    );

    index.replace_external_sources("maven:/workspace", &[fallback, preferred]);
    index.insert_symbols_batch(declaration("dep.PreferredBase"), preferred_id);
    index.insert_symbols_batch(declaration("dep.FallbackBase"), fallback_id);

    assert_eq!(
        index
            .get_symbol("duplicate.Reordered")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.FallbackBase"),
        "stable FileIds must not freeze authority to the initial classpath order"
    );
}

#[test]
fn external_precedence_reorders_loaded_owner_without_reinsertion() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let first = PathBuf::from("/deps/first-loaded.jar");
    let second = PathBuf::from("/deps/second-loaded.jar");
    index.replace_external_sources("maven:/workspace", &[first.clone(), second.clone()]);
    let (first_id, second_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&first).unwrap(),
            inner.external_file_id_for_path(&second).unwrap(),
        )
    };
    let declarations = |parent: &str, return_type: &str| {
        let mut value = synthetic_method("duplicate.Loaded", "value");
        value.return_type = Some(return_type.into());
        vec![
            synthetic_type(
                "duplicate.Loaded",
                SymbolKind::CLASS,
                Some(parent),
                &[],
                crate::Modifiers::default(),
            ),
            value,
        ]
    };
    index.insert_symbols_batch(declarations("dep.First", "first"), first_id);
    index.insert_symbols_batch(declarations("dep.Second", "second"), second_id);
    assert_eq!(
        index
            .get_symbol("duplicate.Loaded.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("first")
    );

    index.replace_external_sources("maven:/workspace", &[second, first]);

    assert_eq!(
        index
            .get_symbol("duplicate.Loaded")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.Second")
    );
    assert_eq!(
        index
            .get_symbol("duplicate.Loaded.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("second")
    );
}

#[test]
fn external_exact_supertypes_survive_canonical_resolution() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let source = PathBuf::from("/deps/hierarchy.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&source));
    let file_id = index
        .inner
        .read()
        .external_file_id_for_path(&source)
        .expect("registered external source");
    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "external.Grand",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_type(
                "external.Base",
                SymbolKind::CLASS,
                Some("external.Grand"),
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_type(
                "external.Child",
                SymbolKind::CLASS,
                Some("external.Base"),
                &[],
                crate::Modifiers::default(),
            ),
        ],
        file_id,
    );
    assert_eq!(
        index.direct_supertypes("external.Child"),
        vec!["external.Base"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("external.Base")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        vec!["external.Child"]
    );
    assert_eq!(
        index.get_inheritance_chain("external.Child"),
        vec!["external.Child", "external.Base", "external.Grand"]
    );
}

#[test]
fn java_source_implicit_supertypes_match_bytecode_hierarchy() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "java.lang.Object",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.lang.Enum",
            SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.lang.Record",
            SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.lang.annotation.Annotation",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);
    index
        .index_file_with_source(
            &temp.path().join("app/Types.java"),
            concat!(
                "package app; class Plain {} interface Contract {} ",
                "enum Choice { ONE } record Snapshot(int value) {} ",
                "@interface Marker {}"
            ),
        )
        .unwrap();

    assert_eq!(index.direct_supertypes("app.Plain"), ["java.lang.Object"]);
    assert!(index.direct_supertypes("app.Contract").is_empty());
    assert_eq!(index.direct_supertypes("app.Choice"), ["java.lang.Enum"]);
    assert_eq!(
        index.direct_supertypes("app.Snapshot"),
        ["java.lang.Record"]
    );
    assert_eq!(
        index.direct_supertypes("app.Marker"),
        ["java.lang.annotation.Annotation"]
    );
    assert_eq!(
        index.get_inheritance_chain("app.Choice"),
        ["app.Choice", "java.lang.Enum", "java.lang.Object"]
    );
    assert_eq!(index.find_direct_subtypes("java.lang.Object").len(), 3);
    assert_eq!(index.find_direct_subtypes("java.lang.Enum").len(), 1);
    assert_eq!(index.find_direct_subtypes("java.lang.Record").len(), 1);
    assert_eq!(
        index
            .find_direct_subtypes("java.lang.annotation.Annotation")
            .len(),
        1
    );

    index
        .index_file_with_source(
            &temp.path().join("java/lang/Object.java"),
            "package java.lang; public class Object {}",
        )
        .unwrap();
    assert!(
        index.direct_supertypes("java.lang.Object").is_empty(),
        "java.lang.Object must not acquire an implicit self-edge"
    );
}

#[test]
fn implicit_java_object_edge_is_exact_even_when_java_is_a_type_name() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.lang.Object",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index
        .index_file_with_source(
            &temp.path().join("p/Types.java"),
            concat!(
                "package p; class java { static class lang { ",
                "static class Object {} } } class Plain {}"
            ),
        )
        .unwrap();

    assert_eq!(index.direct_supertypes("p.Plain"), ["java.lang.Object"]);
    assert!(index
        .find_direct_subtypes("p.java.lang.Object")
        .iter()
        .all(|symbol| symbol.fqn.as_ref() != "p.Plain"));
}

#[test]
fn external_group_priority_precedes_allocation_order() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let dependency = PathBuf::from("/deps/shadow.jar");
    let jdk = PathBuf::from("/jdk/java.base.jmod");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&dependency));
    index.replace_external_sources_with_priority("jdk-classpath", 0, std::slice::from_ref(&jdk));
    let (dependency_id, jdk_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&dependency).unwrap(),
            inner.external_file_id_for_path(&jdk).unwrap(),
        )
    };
    assert!(dependency_id.as_u32() > jdk_id.as_u32());

    index.insert_symbols_batch(
        vec![synthetic_type(
            "java.lang.String",
            lsp_types::SymbolKind::CLASS,
            Some("dep.FakeStringBase"),
            &[],
            crate::Modifiers::default(),
        )],
        dependency_id,
    );
    index.insert_symbols_batch(
        vec![synthetic_type(
            "java.lang.String",
            lsp_types::SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        )],
        jdk_id,
    );

    assert_eq!(
        index
            .get_symbol("java.lang.String")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("java.lang.Object")
    );
}

#[test]
fn scoped_external_lookup_keeps_jdk_archive_but_excludes_unlisted_dependency() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let dependency = PathBuf::from("/deps/unlisted.jar");
    let jdk = PathBuf::from("/jdk/java.base.jmod");
    index.replace_external_sources("gradle-classpath", std::slice::from_ref(&dependency));
    index.replace_external_sources_with_priority("jdk-classpath", 0, std::slice::from_ref(&jdk));
    let (dependency_id, jdk_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&dependency).unwrap(),
            inner.external_file_id_for_path(&jdk).unwrap(),
        )
    };
    index.insert_symbols_batch(
        vec![synthetic_type(
            "java.lang.String",
            lsp_types::SymbolKind::CLASS,
            Some("dep.FakeBase"),
            &[],
            crate::Modifiers::default(),
        )],
        dependency_id,
    );
    index.insert_symbols_batch(
        vec![synthetic_type(
            "java.lang.String",
            lsp_types::SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        )],
        jdk_id,
    );

    let scoped = index.with_source_scope(SourceScope::new(Vec::<PathBuf>::new()));
    let selected = scoped
        .find_symbol("java.lang.String")
        .expect("JDK archive remains visible to a Gradle requester");
    assert_eq!(selected.file_id, jdk_id);
    assert_eq!(
        scoped
            .get_symbol("java.lang.String")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("java.lang.Object")
    );
}

#[test]
fn scoped_has_class_hides_dependencies_outside_the_source_set_classpath() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let main_jar = PathBuf::from("/deps/guava.jar");
    let test_jar = PathBuf::from("/deps/junit.jar");
    index.replace_external_sources("gradle-classpath", &[main_jar.clone(), test_jar.clone()]);
    let (main_id, test_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&main_jar).unwrap(),
            inner.external_file_id_for_path(&test_jar).unwrap(),
        )
    };
    for (fqn, id) in [
        ("com.google.common.ImmutableList", main_id),
        ("org.junit.Test", test_id),
    ] {
        index.insert_symbols_batch(
            vec![synthetic_type(
                fqn,
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )],
            id,
        );
    }

    let main = index.with_source_scope(SourceScope::with_external_archives(
        Vec::<PathBuf>::new(),
        vec![main_jar.clone()],
    ));
    let test = index.with_source_scope(SourceScope::with_external_archives(
        Vec::<PathBuf>::new(),
        vec![main_jar, test_jar],
    ));

    assert!(
        index.has_class("org.junit.Test"),
        "unscoped sees every archive"
    );
    assert!(main.has_class("com.google.common.ImmutableList"));
    assert!(
        !main.has_class("org.junit.Test"),
        "main must not see test-only JARs"
    );
    assert!(test.has_class("org.junit.Test"));
    assert!(test.has_class("com.google.common.ImmutableList"));
}

#[test]
fn published_scope_survives_pending_refresh_and_retires_with_owner() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let source_root = temp.path().join("src/main/java");
    let file = source_root.join("p/Use.java");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "package p; class Use {}").unwrap();
    index
        .index_file_with_source(&file, "package p; class Use {}")
        .unwrap();
    let file_id = index.get_file_id(&file).unwrap();

    let first = index.begin_external_source_refresh("build:gradle");
    let _ = index
        .replace_external_sources_for_refresh(&first, &[])
        .unwrap();
    assert!(index.publish_source_scopes_for_refresh(
        &first,
        vec![SourceScopeInput {
            requester_roots: vec![source_root.clone()],
            visible_source_roots: vec![source_root],
            visible_external_archives: Vec::new(),
        }],
    ));
    let last_good = index.scoped_view_for_file(file_id).unwrap();
    assert!(last_good.source_scope_is_current());

    let pending = index.begin_external_source_refresh("build:gradle");
    assert!(last_good.source_scope_is_current());
    let foreign = WorkspaceIndex::new(temp.path().to_path_buf());
    let foreign_ticket = foreign.begin_external_source_refresh("build:gradle");
    let _ = foreign
        .replace_external_sources_for_refresh(&foreign_ticket, &[])
        .unwrap();
    assert!(!index.retire_source_scopes_for_refresh(&foreign_ticket));
    assert!(index.scoped_view_for_file(file_id).is_some());
    let unrelated = index.begin_external_source_refresh("build:maven");
    let _ = index
        .replace_external_sources_for_refresh(&unrelated, &[])
        .unwrap();
    assert!(index.retire_source_scopes_for_refresh(&unrelated));
    assert!(index.scoped_view_for_file(file_id).is_some());

    let _ = index
        .replace_external_sources_for_refresh(&pending, &[])
        .unwrap();
    assert!(index.retire_source_scopes_for_refresh(&pending));
    assert!(!last_good.source_scope_is_current());
    assert!(index.scoped_view_for_file(file_id).is_none());
}

#[test]
fn external_fallback_promotion_uses_refreshed_precedence() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let first = PathBuf::from("/deps/first.jar");
    let second = PathBuf::from("/deps/second.jar");
    let third = PathBuf::from("/deps/third.jar");
    index.replace_external_sources(
        "maven:/workspace",
        &[first.clone(), second.clone(), third.clone()],
    );
    let (first_id, second_id, third_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&first).unwrap(),
            inner.external_file_id_for_path(&second).unwrap(),
            inner.external_file_id_for_path(&third).unwrap(),
        )
    };
    let declaration = |parent: &str| {
        vec![synthetic_type(
            "duplicate.Promotion",
            lsp_types::SymbolKind::CLASS,
            Some(parent),
            &[],
            crate::Modifiers::default(),
        )]
    };
    index.insert_symbols_batch(declaration("dep.Third"), third_id);
    index.insert_symbols_batch(declaration("dep.Second"), second_id);
    index.insert_symbols_batch(declaration("dep.First"), first_id);

    index.replace_external_sources("maven:/workspace", &[third, second, first]);
    index.remove_file(first_id);

    assert_eq!(
        index
            .get_symbol("duplicate.Promotion")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.Third"),
        "fallback promotion must use current classpath rank, not old FileId order"
    );
}

#[test]
fn external_reorder_republishes_owner_shape_and_clears_warm_hierarchy() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let first = PathBuf::from("/deps/first-warm.jar");
    let second = PathBuf::from("/deps/second-warm.jar");
    index.replace_external_sources("maven:/workspace", &[first.clone(), second.clone()]);
    let (first_id, second_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&first).unwrap(),
            inner.external_file_id_for_path(&second).unwrap(),
        )
    };
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.First",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dep.Second",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);
    let declarations = |parent: &str, return_type: &str| {
        let mut method = synthetic_method("duplicate.Warm", "value");
        method.return_type = Some(return_type.into());
        vec![
            synthetic_type(
                "duplicate.Warm",
                SymbolKind::CLASS,
                Some(parent),
                &[],
                crate::Modifiers::default(),
            ),
            method,
        ]
    };
    index.insert_symbols_batch(declarations("dep.First", "first"), first_id);
    index.insert_symbols_batch(declarations("dep.Second", "second"), second_id);

    assert_eq!(
        index.get_inheritance_chain("duplicate.Warm"),
        ["duplicate.Warm", "dep.First"]
    );
    assert_eq!(
        index
            .find_method_by_args_with_inheritance("duplicate.Warm", "value", 0)
            .unwrap()
            .return_type
            .as_deref(),
        Some("first")
    );
    assert_eq!(
        index
            .find_method_locations_by_name("duplicate.Warm", "value")
            .first()
            .map(|location| location.file_id),
        Some(first_id),
        "method metadata and locations must come from the elected owner source"
    );

    index.replace_external_sources("maven:/workspace", &[second, first]);
    assert_eq!(
        index.get_inheritance_chain("duplicate.Warm"),
        ["duplicate.Warm", "dep.Second"],
        "a warmed chain must not survive a precedence-only classpath refresh"
    );
    assert_eq!(
        index
            .find_method_by_args_with_inheritance("duplicate.Warm", "value", 0)
            .unwrap()
            .return_type
            .as_deref(),
        Some("second")
    );
    assert_eq!(
        index
            .find_method_locations_by_name("duplicate.Warm", "value")
            .first()
            .map(|location| location.file_id),
        Some(second_id),
        "classpath reordering must move method locations with their owner"
    );
}

#[test]
fn ownerless_external_members_follow_refreshed_precedence_after_workspace_shadow_removal() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let first = PathBuf::from("/deps/first-legacy.jar");
    let second = PathBuf::from("/deps/second-legacy.jar");
    index.replace_external_sources("maven:/workspace", &[first.clone(), second.clone()]);
    let (first_id, second_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&first).unwrap(),
            inner.external_file_id_for_path(&second).unwrap(),
        )
    };
    let legacy_method = |return_type: &str| {
        let mut method = synthetic_method("legacy.Owner", "value");
        method.return_type = Some(return_type.into());
        vec![method]
    };
    index.insert_symbols_batch(legacy_method("first"), first_id);
    index.insert_symbols_batch(legacy_method("second"), second_id);

    let workspace_path = temp.path().join("Owner.java");
    index
        .index_file_with_source(&workspace_path, "package legacy; class Owner {}")
        .unwrap();
    let workspace_id = index.get_file_id(&workspace_path).unwrap();
    assert!(index.get_symbol("legacy.Owner.value()").is_none());

    // Build-tool refreshes retain stable FileIds while changing classpath
    // authority. Removing the workspace shadow must consult the new rank.
    index.replace_external_sources("maven:/workspace", &[second, first]);
    index.remove_file(workspace_id);

    assert_eq!(
        index
            .get_symbol("legacy.Owner.value()")
            .unwrap()
            .return_type
            .as_deref(),
        Some("second")
    );
}

#[test]
fn duplicate_external_types_do_not_merge_members_across_classpath_entries() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let (preferred_id, fallback_id) = {
        let mut inner = index.inner.write();
        (
            inner.allocate_external_virtual_file_id("01-preferred.jar"),
            inner.allocate_external_virtual_file_id("02-fallback.jar"),
        )
    };

    index.insert_symbols_batch(
        vec![
            synthetic_method("duplicate.Service", "fallbackOnly"),
            synthetic_type(
                "duplicate.Service",
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
        ],
        fallback_id,
    );
    index.insert_symbols_batch(
        vec![
            synthetic_method("duplicate.Service", "preferredOnly"),
            synthetic_type(
                "duplicate.Service",
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
        ],
        preferred_id,
    );

    assert_eq!(
        index
            .get_class_members("duplicate.Service")
            .into_iter()
            .map(|symbol| symbol.simple_name.to_string())
            .collect::<Vec<_>>(),
        vec!["preferredOnly"],
        "a lower classpath copy must not contribute phantom members"
    );

    index.remove_file(preferred_id);
    assert_eq!(
        index
            .get_class_members("duplicate.Service")
            .into_iter()
            .map(|symbol| symbol.simple_name.to_string())
            .collect::<Vec<_>>(),
        vec!["fallbackOnly"],
        "removing the preferred class must restore the fallback as one coherent type"
    );
}

#[test]
fn workspace_type_shadow_hides_and_restores_external_only_members() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let external_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("dependency.jar");
    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "shared.Service",
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_method("shared.Service", "externalOnly"),
        ],
        external_id,
    );

    let workspace_path = temp.path().join("Service.java");
    index
        .index_file_with_source(&workspace_path, "package shared; class Service {}")
        .unwrap();
    assert!(
        index.get_class_members("shared.Service").is_empty(),
        "workspace class replacement must hide members from the shadowed dependency class"
    );

    index
        .index_file_with_source(
            &workspace_path,
            "package shared; class Service { void workspaceOnly() {} }",
        )
        .unwrap();
    assert_eq!(
        index
            .get_class_members("shared.Service")
            .into_iter()
            .map(|symbol| symbol.simple_name.to_string())
            .collect::<Vec<_>>(),
        vec!["workspaceOnly"],
        "reindexing the workspace owner must not reveal external-only members"
    );

    let workspace_id = index.get_file_id(&workspace_path).unwrap();
    index.remove_file(workspace_id);
    assert_eq!(
        index
            .get_class_members("shared.Service")
            .into_iter()
            .map(|symbol| symbol.simple_name.to_string())
            .collect::<Vec<_>>(),
        vec!["externalOnly"]
    );
}

#[test]
fn reindexing_external_source_replaces_removed_and_changed_declarations() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dep.OldBase",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dep.NewBase",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);
    let source_id = index
        .inner
        .write()
        .allocate_external_virtual_file_id("dependency.jar");
    index.insert_symbols_batch(
        vec![
            synthetic_type(
                "dep.Changed",
                lsp_types::SymbolKind::CLASS,
                Some("dep.OldBase"),
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_type(
                "dep.Removed",
                lsp_types::SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
        ],
        source_id,
    );

    index.insert_symbols_batch(
        vec![synthetic_type(
            "dep.Changed",
            lsp_types::SymbolKind::CLASS,
            Some("dep.NewBase"),
            &[],
            crate::Modifiers::default(),
        )],
        source_id,
    );

    assert_eq!(
        index
            .get_symbol("dep.Changed")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.NewBase")
    );
    assert!(index.get_symbol("dep.Removed").is_none());
    assert!(index.find_direct_subtypes("dep.OldBase").is_empty());
    assert_eq!(
        index
            .find_direct_subtypes("dep.NewBase")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["dep.Changed"]
    );
}

#[test]
fn replacing_external_source_group_removes_released_symbols() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let first = PathBuf::from("/deps/first.jar");
    let retained = PathBuf::from("/deps/retained.jar");
    index.replace_external_sources("maven:/workspace", &[first.clone(), retained.clone()]);

    let (first_id, retained_id) = {
        let inner = index.inner.read();
        (
            inner.external_file_id_for_path(&first).unwrap(),
            inner.external_file_id_for_path(&retained).unwrap(),
        )
    };
    index.insert_symbols_batch(
        vec![synthetic_type(
            "dep.Removed",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        first_id,
    );
    index.insert_symbols_batch(
        vec![synthetic_type(
            "dep.Retained",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        retained_id,
    );

    assert_eq!(
        index.replace_external_sources("maven:/workspace", &[retained]),
        vec![first_id]
    );
    assert!(index.find_symbol("dep.Removed").is_none());
    assert!(index.find_symbol("dep.Retained").is_some());
    assert!(!index.is_external_file(first_id));
    assert!(index.is_external_file(retained_id));
}

#[test]
fn shared_external_source_survives_until_its_last_owner_releases_it() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let shared = PathBuf::from("/deps/shared.jar");
    let _ = index
        .replace_external_sources_revisioned("gradle:/workspace", std::slice::from_ref(&shared));
    let maven_refresh = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&shared));
    let shared_revision = maven_refresh
        .revision_for(&shared)
        .expect("latest shared owner refresh must reserve the source");

    let shared_id = index
        .inner
        .read()
        .external_file_id_for_path(&shared)
        .expect("registered shared source");
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "dep.Shared",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        shared_revision,
    ));

    assert!(index
        .replace_external_sources("gradle:/workspace", &[])
        .is_empty());
    assert!(index.find_symbol("dep.Shared").is_some());
    assert!(
        index
            .inner
            .read()
            .external_source_revision_is_current(shared_revision),
        "releasing one owner must not invalidate another owner's current shared-source extraction"
    );

    assert_eq!(
        index.replace_external_sources("maven:/workspace", &[]),
        vec![shared_id]
    );
    assert!(index.find_symbol("dep.Shared").is_none());
}

#[cfg(unix)]
#[test]
fn non_utf8_external_archive_paths_do_not_collapse_to_one_source() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = TempDir::new().unwrap();
    let first = temp.path().join(OsString::from_vec(vec![
        b'l', b'i', b'b', b'-', 0x80, b'.', b'j', b'a', b'r',
    ]));
    let second = temp.path().join(OsString::from_vec(vec![
        b'l', b'i', b'b', b'-', 0x81, b'.', b'j', b'a', b'r',
    ]));
    assert_eq!(
        first.to_string_lossy(),
        second.to_string_lossy(),
        "the regression requires distinct paths with the same lossy display string"
    );

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let refresh = index
        .replace_external_sources_revisioned("maven:/workspace", &[first.clone(), second.clone()]);
    let first_revision = refresh
        .revision_for(&first)
        .expect("first archive revision");
    let second_revision = refresh
        .revision_for(&second)
        .expect("second archive revision");
    assert_ne!(first_revision.file_id(), second_revision.file_id());

    let first_identity = index
        .get_external_source_identity(first_revision.file_id())
        .expect("first lossless identity");
    let second_identity = index
        .get_external_source_identity(second_revision.file_id())
        .expect("second lossless identity");
    assert_ne!(first_identity, second_identity);
    assert_eq!(
        first_identity.archive_path(),
        Some(path_lookup_key(&first).as_path())
    );
    assert_eq!(
        second_identity.archive_path(),
        Some(path_lookup_key(&second).as_path())
    );

    {
        let inner = index.inner.read();
        assert_eq!(
            inner.external_file_id_to_source[&first_revision.file_id()].display_path(),
            Some(first.clone())
        );
        assert_eq!(
            inner.external_file_id_to_source[&second_revision.file_id()].display_path(),
            Some(second.clone())
        );
    }

    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "first.Dependency",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        first_revision,
    ));
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "second.Dependency",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        second_revision,
    ));
    assert!(index.find_symbol("first.Dependency").is_some());
    assert!(index.find_symbol("second.Dependency").is_some());
}

#[test]
fn legacy_registration_does_not_cancel_an_unrelated_archive_publication() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let first = PathBuf::from("/deps/first.jar");
    let second = PathBuf::from("/deps/second.jar");

    let first_refresh = index.register_external_sources(std::slice::from_ref(&first));
    let first_revision = first_refresh
        .revision_for(&first)
        .expect("first registration must reserve its archive");
    let second_refresh = index.register_external_sources(std::slice::from_ref(&second));

    assert_eq!(second_refresh.revision_for(&first), None);
    assert!(second_refresh.revision_for(&second).is_some());
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "first.StillCurrent",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        first_revision,
    ));
    assert!(index.find_symbol("first.StillCurrent").is_some());
}

#[test]
fn legacy_jdk_virtual_identity_is_isolated_from_an_absolute_archive_path() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("jdk");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.lang.Object",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    let refresh = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&archive));
    let archive_revision = refresh.revision_for(&archive).unwrap();
    let inner = index.inner.read();
    let virtual_id = inner.external_virtual_file_id("jdk").unwrap();
    assert_ne!(virtual_id, archive_revision.file_id());
    drop(inner);

    let explicit_relative_archive = ExternalSourceIdentity::for_archive_path(Path::new("jdk"));
    let explicit_virtual = ExternalSourceIdentity::for_virtual("jdk");
    assert_ne!(explicit_relative_archive, explicit_virtual);
    assert!(explicit_relative_archive.archive_path().is_some());
    assert_eq!(explicit_virtual.virtual_name(), Some("jdk"));

    assert_eq!(
        index
            .get_external_source_identity(virtual_id)
            .and_then(|identity| identity.virtual_name().map(str::to_owned)),
        Some("jdk".to_owned())
    );
    assert_eq!(
        index
            .get_external_source_identity(archive_revision.file_id())
            .and_then(|identity| identity.archive_path().map(Path::to_path_buf)),
        Some(path_lookup_key(&archive).as_path().to_path_buf())
    );
}

#[test]
fn external_publication_gate_linearizes_successor_ticket_creation() {
    let index = std::sync::Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/gated-refresh")));
    let first = index.begin_external_source_refresh("build:gradle");
    let publication = index
        .begin_external_source_publication(&first)
        .expect("first refresh enters its publication section");
    let (ticket_tx, ticket_rx) = std::sync::mpsc::sync_channel(1);
    let successor_index = std::sync::Arc::clone(&index);
    let successor = std::thread::spawn(move || {
        ticket_tx
            .send(successor_index.begin_external_source_refresh("build:gradle"))
            .unwrap();
    });

    assert!(
        ticket_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "a successor cannot be minted inside the prior cross-registry commit"
    );
    assert!(first.is_latest());

    drop(publication);
    let second = ticket_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("successor proceeds after publication");
    successor.join().unwrap();
    assert!(!first.is_latest());
    assert!(second.is_latest());
}

#[cfg(unix)]
#[test]
fn external_alias_display_follows_the_effective_classpath_owner() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("library.jar");
    let alias = temp.path().join("library-alias.jar");
    std::fs::write(&archive, b"archive").unwrap();
    symlink(&archive, &alias).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let fallback = index.replace_external_sources_with_priority_revisioned(
        "fallback:/workspace",
        100,
        std::slice::from_ref(&archive),
    );
    let preferred = index.replace_external_sources_with_priority_revisioned(
        "preferred:/workspace",
        10,
        std::slice::from_ref(&alias),
    );
    let fallback_revision = fallback.revision_for(&archive).unwrap();
    let preferred_revision = preferred.revision_for(&alias).unwrap();
    assert_eq!(fallback_revision.file_id(), preferred_revision.file_id());
    assert_eq!(
        index.get_external_source_name(preferred_revision.file_id()),
        Some(alias.to_string_lossy().into_owned()),
        "the effective owner's spelling should be used for navigation"
    );

    assert!(index
        .replace_external_sources_with_priority_revisioned("preferred:/workspace", 10, &[])
        .released_file_ids()
        .is_empty());
    assert_eq!(
        index.get_external_source_name(fallback_revision.file_id()),
        Some(archive.to_string_lossy().into_owned()),
        "releasing an alias owner should restore the surviving owner's spelling"
    );
}

#[cfg(unix)]
#[test]
fn external_refresh_revision_lookup_fails_closed_after_symlink_retarget() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first.jar");
    let second = temp.path().join("second.jar");
    let alias = temp.path().join("current.jar");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();
    symlink(&first, &alias).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let refresh =
        index.replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&alias));
    let first_revision = refresh
        .revision_for(&alias)
        .expect("the original alias target was reserved");
    assert_eq!(refresh.revision_for(&first), Some(first_revision));

    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    assert_eq!(
        refresh.revision_for(&alias),
        None,
        "a retargeted path must not inherit publication authority for the old archive"
    );

    let replacement =
        index.replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&alias));
    let second_revision = replacement
        .revision_for(&alias)
        .expect("the new alias target was reserved");
    assert_ne!(first_revision.file_id(), second_revision.file_id());
    assert_eq!(replacement.released_file_ids(), &[first_revision.file_id()]);
}

#[cfg(unix)]
#[test]
fn captured_external_archive_retains_transaction_identity_after_retarget() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first.jar");
    let second = temp.path().join("second.jar");
    let alias = temp.path().join("current.jar");
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();
    symlink(&first, &alias).unwrap();

    let archive = ExternalSourceArchive::capture(&alias);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let ticket = index.begin_external_source_refresh("maven:/workspace");
    let refresh = index
        .replace_external_archives_for_refresh(&ticket, std::slice::from_ref(&archive))
        .unwrap();
    let revision = refresh
        .revision_for_identity(archive.identity())
        .expect("captured identity was reserved");

    std::fs::remove_file(&alias).unwrap();
    symlink(&second, &alias).unwrap();
    assert_eq!(
        refresh.revision_for_identity(archive.identity()),
        Some(revision),
        "the transaction does not re-resolve its captured identity"
    );
    assert_eq!(refresh.revision_for(&alias), None);
    assert!(index.index_jar_file_for_revision(&alias, revision).is_err());
}

#[test]
fn external_cache_snapshot_is_scoped_to_one_exact_refresh() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let jdk_archive = PathBuf::from("/jdk/lib/modules.jmod");
    let dependency_archive = PathBuf::from("/deps/application.jar");

    let jdk_ticket = index.begin_external_source_refresh("jdk-classpath");
    let jdk_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &jdk_ticket,
            0,
            std::slice::from_ref(&jdk_archive),
        )
        .expect("JDK refresh");
    let dependency_refresh =
        index.replace_external_sources_revisioned("gradle:/workspace", &[dependency_archive]);

    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "java.lang.ProvenJdkType",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        jdk_refresh
            .revision_for(&jdk_archive)
            .expect("JDK revision"),
    ));
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "application.DependencyType",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        dependency_refresh
            .revision_for(Path::new("/deps/application.jar"))
            .expect("dependency revision"),
    ));
    index
        .index_file_with_source(
            Path::new("/tmp/test/ProvenJdkType.java"),
            "package java.lang; public class ProvenJdkType {}",
        )
        .expect("workspace shadow publication");

    let cached = index
        .get_external_symbols_for_refresh(&jdk_refresh)
        .expect("current JDK refresh must be snapshotable");
    assert!(cached
        .iter()
        .any(|symbol| symbol.fqn.as_ref() == "java.lang.ProvenJdkType"));
    assert!(!cached
        .iter()
        .any(|symbol| symbol.fqn.as_ref() == "application.DependencyType"));

    let newer = index.begin_external_source_refresh("jdk-classpath");
    let _replacement_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &newer,
            0,
            std::slice::from_ref(&jdk_archive),
        )
        .expect("replacement JDK refresh");
    assert!(index
        .get_external_symbols_for_refresh(&jdk_refresh)
        .is_none());
}

#[test]
fn external_cache_snapshot_preserves_each_archive_owner() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let base_archive = PathBuf::from("/jdk/jmods/java.base.jmod");
    let sql_archive = PathBuf::from("/jdk/jmods/java.sql.jmod");
    let ticket = index.begin_external_source_refresh("jdk-classpath");
    let refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &ticket,
            0,
            &[base_archive.clone(), sql_archive.clone()],
        )
        .expect("JDK refresh");
    let base_revision = refresh.revision_for(&base_archive).expect("base revision");
    let sql_revision = refresh.revision_for(&sql_archive).expect("sql revision");
    assert!(index.add_external_index_data_responsive_for_revision(
        vec![synthetic_type(
            "split.Duplicate",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        vec![crate::TypeGenericSupertypes {
            type_fqn: "split.Duplicate".to_string(),
            edges: vec![crate::GenericSupertype {
                written_target: "split.Parent".to_string(),
                arguments: vec!["T".to_string()],
            }],
        }],
        base_revision,
    ));
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "split.Duplicate",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        sql_revision,
    ));

    let by_source: HashMap<_, _> = index
        .get_external_symbols_by_source_for_refresh(&refresh)
        .expect("current JDK refresh must be snapshotable")
        .into_iter()
        .collect();

    assert_eq!(by_source.len(), 2);
    assert_eq!(
        by_source[&ExternalSourceIdentity::for_archive_path(&base_archive)][0]
            .fqn
            .as_ref(),
        "split.Duplicate"
    );
    assert_eq!(
        by_source[&ExternalSourceIdentity::for_archive_path(&sql_archive)][0]
            .fqn
            .as_ref(),
        "split.Duplicate"
    );

    let index_data_by_source: HashMap<_, _> = index
        .get_external_index_data_by_source_for_refresh(&refresh)
        .expect("current JDK refresh index data must be snapshotable")
        .into_iter()
        .map(|(identity, symbols, generic_supertypes)| (identity, (symbols, generic_supertypes)))
        .collect();
    assert_eq!(
        index_data_by_source[&ExternalSourceIdentity::for_archive_path(&base_archive)].1,
        vec![crate::TypeGenericSupertypes {
            type_fqn: "split.Duplicate".to_string(),
            edges: vec![crate::GenericSupertype {
                written_target: "split.Parent".to_string(),
                arguments: vec!["T".to_string()],
            }],
        }]
    );
}

#[test]
fn stale_external_publication_cannot_resurrect_a_released_source() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let stale = PathBuf::from("/deps/stale.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&stale));
    let stale_id = index
        .inner
        .read()
        .external_file_id_for_path(&stale)
        .expect("registered stale source");

    index.replace_external_sources("maven:/workspace", &[]);
    index.insert_symbols_batch(
        vec![synthetic_type(
            "dep.Resurrected",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        stale_id,
    );

    assert!(index.find_symbol("dep.Resurrected").is_none());
    assert!(!index.is_external_file(stale_id));
}

#[test]
fn older_same_path_external_refresh_cannot_replace_newer_publication() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let retained = PathBuf::from("/deps/retained.jar");
    let first_refresh = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&retained));
    let first_revision = first_refresh
        .revision_for(&retained)
        .expect("first refresh must reserve retained archive");
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "dep.Baseline",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        first_revision,
    ));

    let old_refresh = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&retained));
    let old_revision = old_refresh
        .revision_for(&retained)
        .expect("older replacement must reserve retained archive");

    // Model extraction N completing but pausing immediately before
    // publication. N+1 retains the path (and stable FileId), publishes, then N
    // resumes. The old task must not reacquire authority by path or FileId.
    let extraction_ready = Arc::new(std::sync::Barrier::new(2));
    let resume_old = Arc::new(std::sync::Barrier::new(2));
    let old_index = Arc::clone(&index);
    let old_ready = Arc::clone(&extraction_ready);
    let old_resume = Arc::clone(&resume_old);
    let old_publication = std::thread::spawn(move || {
        old_ready.wait();
        old_resume.wait();
        old_index.insert_symbols_batch_for_revision_for_test(
            vec![synthetic_type(
                "dep.OldExtraction",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )],
            old_revision,
        )
    });
    extraction_ready.wait();

    let workspace_before = index.current_workspace_revision();
    let newer_refresh = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&retained));
    let newer_revision = newer_refresh
        .revision_for(&retained)
        .expect("second refresh must reserve retained archive");
    assert_eq!(
        old_revision.file_id(),
        newer_revision.file_id(),
        "retaining one path must preserve its external FileId"
    );
    assert_ne!(old_revision, newer_revision);
    assert_ne!(
        index.current_workspace_revision(),
        workspace_before,
        "an effective classpath refresh must advance contextual workspace state"
    );
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "dep.NewExtraction",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        newer_revision,
    ));

    resume_old.wait();
    assert!(
        !old_publication.join().unwrap(),
        "the older extraction must be rejected after N+1 reserves authority"
    );
    assert!(index.find_symbol("dep.Baseline").is_none());
    assert!(index.find_symbol("dep.OldExtraction").is_none());
    assert!(index.find_symbol("dep.NewExtraction").is_some());
}

#[test]
fn superseding_chunked_first_publication_retracts_every_partial_candidate() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let fallback = PathBuf::from("/deps/fallback.jar");
    let retained = PathBuf::from("/deps/partial.jar");

    let fallback_refresh = index.replace_external_sources_with_priority_revisioned(
        "fallback:/workspace",
        200,
        std::slice::from_ref(&fallback),
    );
    let fallback_revision = fallback_refresh
        .revision_for(&fallback)
        .expect("fallback refresh must reserve its archive");
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "dep.Partial000",
            SymbolKind::CLASS,
            Some("dep.FallbackBase"),
            &[],
            crate::Modifiers::default(),
        )],
        fallback_revision,
    ));

    let first_refresh = index.replace_external_sources_with_priority_revisioned(
        "primary:/workspace",
        100,
        std::slice::from_ref(&retained),
    );
    let first_revision = first_refresh
        .revision_for(&retained)
        .expect("first publication must reserve the retained archive");
    let partial_symbols = (0..150)
        .map(|number| {
            synthetic_type(
                &format!("dep.Partial{number:03}"),
                SymbolKind::CLASS,
                Some("dep.PartialBase"),
                &[],
                crate::Modifiers::default(),
            )
        })
        .collect();
    let first_chunk_visible = Arc::new(std::sync::Barrier::new(2));
    let resume_old = Arc::new(std::sync::Barrier::new(2));
    let old_index = Arc::clone(&index);
    let old_visible = Arc::clone(&first_chunk_visible);
    let old_resume = Arc::clone(&resume_old);
    let old_publication = std::thread::spawn(move || {
        old_index.insert_symbols_batch_for_revision_after_first_chunk_for_test(
            partial_symbols,
            first_revision,
            || {
                old_visible.wait();
                old_resume.wait();
            },
        )
    });

    first_chunk_visible.wait();
    {
        let inner = index.inner.read();
        assert_eq!(
            inner
                .external_source_publications_in_progress
                .get(&first_revision.file_id()),
            Some(&first_revision.generation())
        );
        assert_eq!(
            inner.file_symbols[&first_revision.file_id()].len(),
            100,
            "unpublished chunks must not enter the typed candidate registry"
        );
    }
    assert_eq!(
        index
            .get_symbol("dep.Partial000")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.PartialBase"),
        "the first owner-complete chunk is visible before cancellation"
    );
    assert!(
        index
            .find_symbol_definition(&crate::SymbolTarget::new(
                "dep.Partial149",
                crate::DeclarationNamespace::Type,
            ))
            .is_none(),
        "typed queries must not see candidates from an unpublished chunk"
    );

    let failed_replacement = index.replace_external_sources_with_priority_revisioned(
        "primary:/workspace",
        100,
        std::slice::from_ref(&retained),
    );
    let failed_revision = failed_replacement
        .revision_for(&retained)
        .expect("the newer failed extraction must still reserve authority");
    assert_ne!(first_revision, failed_revision);
    // Simulate extraction failure: reserve N+1 but publish no symbols.
    {
        let inner = index.inner.read();
        assert!(!inner
            .external_source_publications_in_progress
            .contains_key(&first_revision.file_id()));
        assert!(!inner.file_symbols.contains_key(&first_revision.file_id()));
        assert_eq!(
            inner.symbol_locations["dep.Partial000"].file_id,
            fallback_revision.file_id(),
            "fallback election must be part of the same cleanup transaction"
        );
    }

    resume_old.wait();
    assert!(
        !old_publication.join().unwrap(),
        "the superseded publisher must fail its generation check"
    );
    assert_eq!(
        index
            .get_symbol("dep.Partial000")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("dep.FallbackBase")
    );
    for number in 1..150 {
        assert!(
            index
                .get_symbol(&format!("dep.Partial{number:03}"))
                .is_none(),
            "partial declaration {number} survived failed replacement"
        );
    }
}

#[test]
fn completed_external_publication_remains_last_good_until_replacement_commits() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let retained = PathBuf::from("/deps/last-good.jar");
    let initial = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&retained));
    let initial_revision = initial.revision_for(&retained).unwrap();
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![
            synthetic_type(
                "dep.LastGood",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_method("dep.LastGood", "oldOnly"),
        ],
        initial_revision,
    ));
    let initial_location = index.find_symbol("dep.LastGood").unwrap();
    assert_eq!(
        index.external_source_revision(&initial_location),
        Some(initial_revision)
    );

    let replacement = index
        .replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&retained));
    let replacement_revision = replacement.revision_for(&retained).unwrap();
    assert!(index.get_symbol("dep.LastGood").is_some());
    assert!(index.get_symbol("dep.LastGood.oldOnly").is_some());
    let last_good_location = index.find_symbol("dep.LastGood").unwrap();
    assert_eq!(
        index.external_source_revision(&last_good_location),
        Some(initial_revision),
        "reserving a replacement must not relabel last-good declarations"
    );
    assert!(
        index
            .get_external_symbols_by_source_for_refresh(&replacement)
            .is_none(),
        "a cache snapshot must not authenticate last-good symbols as an unpublished replacement"
    );
    assert!(!index
        .inner
        .read()
        .external_source_publications_in_progress
        .contains_key(&replacement_revision.file_id()));

    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![
            synthetic_type(
                "dep.LastGood",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_method("dep.LastGood", "newOnly"),
        ],
        replacement_revision,
    ));
    assert!(index.get_symbol("dep.LastGood.oldOnly").is_none());
    assert!(index.get_symbol("dep.LastGood.newOnly").is_some());
    let replacement_location = index.find_symbol("dep.LastGood").unwrap();
    assert_eq!(
        index.external_source_revision(&replacement_location),
        Some(replacement_revision),
        "the visible declaration generation advances with the atomic symbol swap"
    );
    assert!(index
        .get_external_symbols_by_source_for_refresh(&replacement)
        .is_some());
}

#[test]
fn empty_external_archive_is_a_completed_published_generation() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let empty = PathBuf::from("/deps/empty.jar");
    let refresh =
        index.replace_external_sources_revisioned("maven:/workspace", std::slice::from_ref(&empty));
    let revision = refresh.revision_for(&empty).unwrap();

    assert!(index.insert_symbols_batch_for_revision_for_test(Vec::new(), revision));

    let partitions = index
        .get_external_symbols_by_source_for_refresh(&refresh)
        .expect("an empty archive is published, not failed");
    assert_eq!(partitions.len(), 1);
    assert!(partitions[0].1.is_empty());
}

#[test]
fn superseded_owner_refresh_cannot_reserve_archive_generations_late() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let retained = PathBuf::from("/deps/owner-generation.jar");
    let old_ticket = index.begin_external_source_refresh("maven:/workspace");
    let resolution_ready = Arc::new(std::sync::Barrier::new(2));
    let resume_old = Arc::new(std::sync::Barrier::new(2));
    let old_index = Arc::clone(&index);
    let old_path = retained.clone();
    let old_ready = Arc::clone(&resolution_ready);
    let old_resume = Arc::clone(&resume_old);
    let old_refresh = std::thread::spawn(move || {
        old_ready.wait();
        old_resume.wait();
        old_index.replace_external_sources_for_refresh(&old_ticket, std::slice::from_ref(&old_path))
    });
    resolution_ready.wait();

    let new_ticket = index.begin_external_source_refresh("maven:/workspace");
    let new_refresh = index
        .replace_external_sources_for_refresh(&new_ticket, std::slice::from_ref(&retained))
        .expect("newest owner refresh must retain publication authority");
    let new_revision = new_refresh
        .revision_for(&retained)
        .expect("newest owner refresh must reserve the archive");
    assert!(index.insert_symbols_batch_for_revision_for_test(
        vec![synthetic_type(
            "dep.OwnerGenerationWinner",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        new_revision,
    ));

    resume_old.wait();
    assert!(
        old_refresh.join().unwrap().is_none(),
        "an older resolver must not mint a newer source generation after cancellation"
    );
    assert!(index.find_symbol("dep.OwnerGenerationWinner").is_some());
    assert!(
        index
            .inner
            .read()
            .external_source_revision_is_current(new_revision),
        "rejecting the owner refresh must leave the newer archive token current"
    );
}

#[test]
fn external_refresh_ticket_tracks_exact_index_commit_authority() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/authority-a"));
    let other_index = WorkspaceIndex::new(PathBuf::from("/tmp/authority-b"));
    let owner = "maven:/workspace";

    let first = index.begin_external_source_refresh(owner);
    let impostor = other_index.begin_external_source_refresh(owner);
    assert!(first.is_latest());
    assert!(!first.is_committed());
    assert!(!first.has_same_authority(&impostor));
    assert!(index
        .replace_external_sources_for_refresh(&impostor, &[])
        .is_none());

    let _ = index
        .replace_external_sources_for_refresh(&first, &[])
        .expect("the exact index ticket must commit");
    assert!(first.is_committed());

    let second = index.begin_external_source_refresh(owner);
    assert!(!first.is_latest());
    assert!(
        first.is_committed(),
        "the previous classpath stays visible until replacement"
    );
    assert!(second.is_latest());
    assert!(!second.is_committed());

    let _ = index
        .replace_external_sources_for_refresh(&second, &[])
        .expect("the latest exact ticket must commit");
    assert!(!first.is_committed());
    assert!(second.is_committed());
    assert!(std::mem::size_of::<ExternalSourceRefreshTicket>() <= 16);
    assert!(
        std::mem::size_of::<(FileId, std::num::NonZeroU64)>() <= 16,
        "the per-source in-progress generation marker must remain two machine words"
    );
}

#[test]
fn revisioned_external_publication_rejects_a_source_released_during_extraction() {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let released = PathBuf::from("/deps/released.jar");
    let refresh = index
        .replace_external_sources_revisioned("gradle:/workspace", std::slice::from_ref(&released));
    let revision = refresh
        .revision_for(&released)
        .expect("refresh must reserve archive before extraction");

    let extraction_ready = Arc::new(std::sync::Barrier::new(2));
    let resume_publication = Arc::new(std::sync::Barrier::new(2));
    let stale_index = Arc::clone(&index);
    let stale_ready = Arc::clone(&extraction_ready);
    let stale_resume = Arc::clone(&resume_publication);
    let stale_publication = std::thread::spawn(move || {
        stale_ready.wait();
        stale_resume.wait();
        stale_index.insert_symbols_batch_for_revision_for_test(
            vec![synthetic_type(
                "dep.ReleasedExtraction",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )],
            revision,
        )
    });
    extraction_ready.wait();
    let removal = index.replace_external_sources_revisioned("gradle:/workspace", &[]);
    assert_eq!(removal.released_file_ids(), &[revision.file_id()]);
    resume_publication.wait();
    assert!(!stale_publication.join().unwrap());
    assert!(index.find_symbol("dep.ReleasedExtraction").is_none());
}

#[test]
fn workspace_hierarchy_covers_interfaces_abstract_classes_enums_and_records() {
    let temp = TempDir::new().unwrap();
    let source_path = temp.path().join("Hierarchy.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &source_path,
            r#"package model;
               interface Root { void run(); }
               interface Mid extends Root {}
               abstract class AbstractWorker implements Mid {
                   public abstract void run();
               }
               class ConcreteWorker extends AbstractWorker {
                   public void run() {}
               }
               class DirectWorker implements Root {
                   public void run() {}
               }
               enum Mode implements Root {
                   ONE;
                   public void run() {}
               }
               record Snapshot() implements Root {
                   public void run() {}
               }"#,
        )
        .unwrap();

    let direct: Vec<_> = index
        .find_direct_subtypes("model.Root")
        .into_iter()
        .map(|symbol| symbol.fqn.to_string())
        .collect();
    assert_eq!(
        direct,
        vec![
            "model.DirectWorker",
            "model.Mid",
            "model.Mode",
            "model.Snapshot"
        ]
    );
    assert_eq!(
        index
            .find_implementations("model.Root")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec![
            "model.ConcreteWorker",
            "model.DirectWorker",
            "model.Mode",
            "model.Snapshot"
        ],
        "transitive implementation search must see through abstract classes and subinterfaces"
    );
    assert_eq!(
        index.get_inheritance_chain("model.ConcreteWorker"),
        vec![
            "model.ConcreteWorker",
            "model.AbstractWorker",
            "model.Mid",
            "model.Root"
        ]
    );

    let implementation_methods: HashSet<_> = index
        .find_interface_method_locations("model.Root", "run()")
        .into_iter()
        .map(|location| location.fqn.to_string())
        .collect();
    for expected in [
        "model.AbstractWorker.run()",
        "model.ConcreteWorker.run()",
        "model.DirectWorker.run()",
        "model.Mode.run()",
        "model.Snapshot.run()",
    ] {
        assert!(
            implementation_methods.contains(expected),
            "missing transitive interface implementation method {expected}"
        );
    }
}

#[test]
fn kotlin_interface_and_object_edges_use_resolved_dependency_names() {
    let temp = TempDir::new().unwrap();
    let source_path = temp.path().join("Hierarchy.kt");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index.add_external_symbol(synthetic_type(
        "dependency.Contract",
        lsp_types::SymbolKind::INTERFACE,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index
        .index_file_with_source(
            &source_path,
            r#"package app
               import dependency.Contract as ImportedContract
               interface LocalContract : ImportedContract
               sealed class Base : LocalContract
               object Singleton : Base()"#,
        )
        .unwrap();

    let local_contract = index.get_symbol("app.LocalContract").unwrap();
    assert!(local_contract.parent_class.is_none());
    assert_eq!(
        local_contract
            .interfaces
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["ImportedContract"],
        "Kotlin interface supertypes retain raw spelling and interface role"
    );
    assert_eq!(
        index.direct_supertypes("app.LocalContract"),
        ["dependency.Contract"]
    );
    assert_eq!(
        index
            .find_direct_subtypes("dependency.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.LocalContract"]
    );
    assert_eq!(
        index.get_inheritance_chain("app.Singleton"),
        vec![
            "app.Singleton",
            "app.Base",
            "app.LocalContract",
            "dependency.Contract"
        ]
    );
    assert_eq!(
        index
            .find_implementations("dependency.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.Singleton"],
        "the implicitly abstract sealed base is not a concrete implementation"
    );
}

/// A Kotlin library typealias (`kotlin.Comparator`, which the
/// stdlib's `kotlin.TypeAliasesKt` expands to `java.util.Comparator`) resolves
/// to its expanded class, and leaves the index with the facade declaring it.
#[test]
fn kotlin_library_typealias_resolves_to_its_expanded_class() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let stdlib = PathBuf::from("/deps/kotlin-stdlib.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&stdlib));
    let stdlib_id = index
        .inner
        .read()
        .external_file_id_for_path(&stdlib)
        .expect("registered stdlib");
    let mut facade = synthetic_type(
        "kotlin.TypeAliasesKt",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    );
    facade.kotlin = Some(Box::new(crate::KotlinSymbolMetadata {
        type_aliases: vec![crate::KotlinTypeAlias {
            fqn: "kotlin.Comparator".into(),
            expanded_class: "java.util.Comparator".into(),
            type_parameters: Vec::new(),
            expanded_type_arguments: Vec::new(),
        }],
        ..crate::KotlinSymbolMetadata::default()
    }));
    index.insert_symbols_batch(vec![facade], stdlib_id);
    index.add_external_symbol(synthetic_type(
        "java.util.Comparator",
        SymbolKind::INTERFACE,
        None,
        &[],
        crate::Modifiers::default(),
    ));

    let use_path = temp.path().join("app/Sort.kt");
    index
        .index_file_with_source(
            &use_path,
            "package app\n\nclass ByLength : Comparator<String> {\n    \
             override fun compare(a: String, b: String) = a.length - b.length\n}\n",
        )
        .unwrap();
    let file_id = index.get_file_id(&use_path).unwrap();
    let comparator = crate::TypeNameResolution::Exact("java.util.Comparator".into());

    // `kotlin.*` is a default import, and `java.util.*` is not.
    assert_eq!(
        index.resolve_type_name(file_id, &[], "Comparator"),
        comparator
    );
    assert_eq!(
        index.resolve_type_name(file_id, &[], "kotlin.Comparator"),
        comparator
    );
    let mut imports = crate::ImportMap::new();
    imports.current_package = Some("app".into());
    imports.add_import("Comparator".into(), "kotlin.Comparator".into());
    assert_eq!(
        index.resolve_type_name_in_context(&imports, &[], "Comparator", crate::Language::Kotlin),
        comparator,
        "an explicit import of the alias"
    );
    assert!(
        index
            .direct_supertypes("app.ByLength")
            .contains(&"java.util.Comparator".to_owned()),
        "the supertype written through the alias"
    );
    assert!(
        !index.has_class("kotlin.Comparator"),
        "the alias is not a class of its own"
    );
    assert_eq!(
        index.resolve_type_name_in_context(&imports, &[], "Comparator", crate::Language::Java),
        crate::TypeNameResolution::Missing,
        "Java does not see Kotlin typealiases"
    );

    // Releasing the library takes the alias with its facade.
    index.replace_external_sources("maven:/workspace", &[]);
    assert_eq!(
        index.resolve_type_name(file_id, &[], "Comparator"),
        crate::TypeNameResolution::Missing
    );
    assert!(index.inner.read().kotlin_type_alias_owners.is_empty());
}

/// A top-level typealias in the workspace's own Kotlin source
/// resolves from another file to the class it expands to, the same way a
/// library alias does, and leaves the index with its file.
#[test]
fn kotlin_source_typealias_resolves_to_its_expanded_class() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for fqn in ["java.util.ArrayList", "java.util.LinkedList"] {
        index.add_external_symbol(synthetic_type(
            fqn,
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }

    let alias_path = temp.path().join("app/Aliases.kt");
    index
        .index_file_with_source(
            &alias_path,
            "package app\n\nimport java.util.LinkedList\n\n\
             typealias Names = java.util.ArrayList<String>\n\
             typealias Queue = LinkedList<String>\n",
        )
        .unwrap();
    let use_path = temp.path().join("app/Roster.kt");
    index
        .index_file_with_source(&use_path, "package app\n\nclass Roster : Names()\n")
        .unwrap();
    let client_path = temp.path().join("other/Client.kt");
    index
        .index_file_with_source(
            &client_path,
            "package other\n\nimport app.Queue\n\nclass Client\n",
        )
        .unwrap();
    let use_id = index.get_file_id(&use_path).unwrap();
    let client_id = index.get_file_id(&client_path).unwrap();
    let names = crate::TypeNameResolution::Exact("java.util.ArrayList".into());

    // Same package, and qualified.
    assert_eq!(index.resolve_type_name(use_id, &[], "Names"), names);
    assert_eq!(index.resolve_type_name(use_id, &[], "app.Names"), names);
    // An imported alias whose expansion its own file imported.
    assert_eq!(
        index.resolve_type_name(client_id, &[], "Queue"),
        crate::TypeNameResolution::Exact("java.util.LinkedList".into())
    );
    assert!(
        index
            .direct_supertypes("app.Roster")
            .contains(&"java.util.ArrayList".to_owned()),
        "the supertype written through the alias"
    );
    assert!(
        index.has_class("app.AliasesKt"),
        "an alias-only file has the facade that carries its aliases"
    );
    assert!(
        !index.has_class("app.Names"),
        "the alias is not a class of its own"
    );

    // Editing the alias away, then removing the file, takes it with them.
    index
        .index_file_with_source(
            &alias_path,
            "package app\n\ntypealias Names = java.util.ArrayList<String>\n",
        )
        .unwrap();
    assert_eq!(
        index.resolve_type_name(client_id, &[], "Queue"),
        crate::TypeNameResolution::Missing
    );
    assert_eq!(index.resolve_type_name(use_id, &[], "Names"), names);
    index.remove_file(index.get_file_id(&alias_path).unwrap());
    assert_eq!(
        index.resolve_type_name(use_id, &[], "Names"),
        crate::TypeNameResolution::Missing
    );
    assert!(index.inner.read().kotlin_type_alias_owners.is_empty());
}

/// A Kotlin class whose supertype is written through a source
/// typealias is a subtype of the class the alias expands to, whichever file
/// is indexed first, and the subtype keeps the spelling it wrote. Adding or
/// removing the alias in its own file updates the subtype search.
#[test]
fn kotlin_supertype_written_through_a_source_typealias_is_a_subtype_of_its_expansion() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.util.ArrayList",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    let subtypes = |index: &WorkspaceIndex| {
        let direct = index
            .find_direct_subtypes("java.util.ArrayList")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>();
        let implementations = index
            .find_implementations("java.util.ArrayList")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>();
        assert_eq!(direct, implementations);
        assert!(index
            .inner
            .read()
            .direct_subtype_resolution_is_complete("java.util.ArrayList"));
        direct
    };

    // The subtypes are indexed before the alias they name.
    index
        .index_file_with_source(
            &temp.path().join("app/Roster.kt"),
            "package app\n\nclass Roster : Names()\n",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("other/Squad.kt"),
            "package other\n\nimport app.Names\n\nclass Squad : Names()\n",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Plain.kt"),
            "package app\n\nimport java.util.ArrayList\n\nclass Plain : ArrayList<String>()\n",
        )
        .unwrap();
    assert_eq!(subtypes(&index), ["app.Plain"]);

    let alias_path = temp.path().join("app/Aliases.kt");
    let alias_source = "package app\n\ntypealias Names = java.util.ArrayList<String>\n";
    index
        .index_file_with_source(&alias_path, alias_source)
        .unwrap();
    assert_eq!(subtypes(&index), ["app.Plain", "app.Roster", "other.Squad"]);
    let roster = index.get_symbol("app.Roster").unwrap();
    assert!(
        roster
            .parent_class
            .iter()
            .chain(roster.interfaces.iter())
            .any(|written| written.as_ref() == "Names"),
        "the subtype keeps the supertype spelling it wrote"
    );

    index.remove_file(index.get_file_id(&alias_path).unwrap());
    assert_eq!(subtypes(&index), ["app.Plain"]);
    index
        .index_file_with_source(&alias_path, alias_source)
        .unwrap();
    assert_eq!(subtypes(&index), ["app.Plain", "app.Roster", "other.Squad"]);
}

/// A cached inheritance chain that runs through a typealias name
/// is cleared when another file adds, edits away, or re-adds that alias, for
/// the class written against the alias and for its own subclasses.
#[test]
fn kotlin_inheritance_chain_follows_a_typealias_changed_in_another_file() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.util.ArrayList",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index
        .index_file_with_source(
            &temp.path().join("app/Roster.kt"),
            "package app\n\nopen class Roster : Names()\n",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("other/Squad.kt"),
            "package other\n\nimport app.Roster\n\nclass Squad : Roster()\n",
        )
        .unwrap();
    // Each read caches the chains before the next change.
    let chains = |index: &WorkspaceIndex| {
        [
            index.get_inheritance_chain("app.Roster"),
            index.get_inheritance_chain("other.Squad"),
        ]
    };
    let without_alias = [vec!["app.Roster"], vec!["other.Squad", "app.Roster"]];
    let with_alias = [
        vec!["app.Roster", "java.util.ArrayList"],
        vec!["other.Squad", "app.Roster", "java.util.ArrayList"],
    ];
    assert_eq!(chains(&index), without_alias);

    let alias_path = temp.path().join("app/Aliases.kt");
    let alias_source = "package app\n\ntypealias Names = java.util.ArrayList<String>\n";
    index
        .index_file_with_source(&alias_path, alias_source)
        .unwrap();
    assert_eq!(chains(&index), with_alias, "the alias is added");

    index
        .index_file_with_source(&alias_path, "package app\n\nfun names() = 0\n")
        .unwrap();
    assert_eq!(chains(&index), without_alias, "the alias is edited away");

    index
        .index_file_with_source(&alias_path, alias_source)
        .unwrap();
    assert_eq!(chains(&index), with_alias, "the alias is added back");

    index.remove_file(index.get_file_id(&alias_path).unwrap());
    assert_eq!(chains(&index), without_alias, "the alias file is removed");
}

/// A Kotlin supertype bound by an explicit import, or written
/// qualified, is a subtype of the class it names, with no typealias involved.
#[test]
fn kotlin_supertype_written_through_an_import_is_a_subtype_of_the_imported_class() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.util.ArrayList",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index
        .index_file_with_source(
            &temp.path().join("app/Imported.kt"),
            "package app\n\nimport java.util.ArrayList\n\nclass Imported : ArrayList<String>()\n",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/Qualified.kt"),
            "package app\n\nclass Qualified : java.util.ArrayList<String>()\n",
        )
        .unwrap();

    for subtypes in [
        index.find_direct_subtypes("java.util.ArrayList"),
        index.find_implementations("java.util.ArrayList"),
    ] {
        assert_eq!(
            subtypes
                .into_iter()
                .map(|symbol| symbol.fqn.to_string())
                .collect::<Vec<_>>(),
            ["app.Imported", "app.Qualified"]
        );
    }
}

/// Usage extraction resolves an unpublished revision of a Kotlin
/// file against itself, so that revision's facade answers for the typealiases
/// it declares, and the ones only its published revision declared are hidden.
#[test]
fn kotlin_source_typealias_follows_an_unpublished_revision() {
    use crate::import_resolver::{ImportResolver, RevisionSymbolOverlay};

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for fqn in ["java.util.ArrayList", "java.util.LinkedList"] {
        index.add_external_symbol(synthetic_type(
            fqn,
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }
    index
        .index_file_with_source(
            &temp.path().join("app/Aliases.kt"),
            "package app\n\ntypealias Names = java.util.ArrayList<String>\n",
        )
        .unwrap();
    let mut imports = crate::ImportMap::new();
    imports.current_package = Some("app".into());
    let names = crate::TypeNameResolution::Exact("java.util.ArrayList".into());
    assert_eq!(
        ImportResolver::for_language(&imports, &index, crate::Language::Kotlin)
            .resolve_type_identity("Names"),
        names
    );

    // The next revision of the file replaces `Names` with `Queue`.
    let mut facade = (*index.get_symbol("app.AliasesKt").unwrap()).clone();
    facade.kotlin.as_mut().unwrap().type_aliases = vec![crate::KotlinTypeAlias {
        fqn: "app.Queue".into(),
        expanded_class: "java.util.LinkedList".into(),
        type_parameters: Vec::new(),
        expanded_type_arguments: vec!["String".into()],
    }];
    let overlay = RevisionSymbolOverlay::new(
        &[facade],
        HashSet::from([Arc::<str>::from("app.AliasesKt")]),
    );
    let revision =
        ImportResolver::for_revision(&imports, &index, &overlay, crate::Language::Kotlin);
    assert_eq!(
        revision.resolve_type_identity("Queue"),
        crate::TypeNameResolution::Exact("java.util.LinkedList".into())
    );
    assert_eq!(
        revision.resolve_type_identity("Names"),
        crate::TypeNameResolution::Missing
    );
    assert_eq!(
        ImportResolver::for_language(&imports, &index, crate::Language::Kotlin)
            .resolve_type_identity("Names"),
        names,
        "the published view keeps the published revision"
    );
}

/// A source typealias that expands to a type parameter, or that is
/// private to its file, is not resolved from another file.
#[test]
fn kotlin_source_typealias_to_a_type_parameter_or_private_is_not_resolved() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(synthetic_type(
        "java.util.ArrayList",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    ));
    index
        .index_file_with_source(
            &temp.path().join("app/Aliases.kt"),
            "package app\n\ntypealias Id<T> = T\n\
             private typealias Hidden = java.util.ArrayList<String>\n\
             typealias Names = java.util.ArrayList<String>\n",
        )
        .unwrap();
    let use_path = temp.path().join("app/Use.kt");
    index
        .index_file_with_source(&use_path, "package app\n\nclass Use\n")
        .unwrap();
    let use_id = index.get_file_id(&use_path).unwrap();

    for alias in ["Id", "app.Id", "Hidden", "app.Hidden"] {
        assert_eq!(
            index.resolve_type_name(use_id, &[], alias),
            crate::TypeNameResolution::Missing,
            "{alias}"
        );
    }
    // The public class alias beside them is recorded, and only it.
    assert_eq!(
        index.resolve_type_name(use_id, &[], "Names"),
        crate::TypeNameResolution::Exact("java.util.ArrayList".into())
    );
    let inner = index.inner.read();
    let mut recorded = inner
        .kotlin_type_alias_owners
        .keys()
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>();
    recorded.sort_unstable();
    assert_eq!(recorded, ["app.Names"]);
}

/// A typealias to a generic class with concrete arguments keeps
/// them, from source and from a library, wherever the alias is named from:
/// the same package, an import, or its qualified name. The name still
/// resolves to the expanded class.
#[test]
fn kotlin_typealias_keeps_its_expansion_type_arguments() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for (fqn, kind) in [
        ("java.util.ArrayList", SymbolKind::CLASS),
        ("java.util.Map", SymbolKind::INTERFACE),
    ] {
        index.add_external_symbol(synthetic_type(
            fqn,
            kind,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }
    let library = PathBuf::from("/deps/tags.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&library));
    let library_id = index
        .inner
        .read()
        .external_file_id_for_path(&library)
        .expect("registered library");
    let mut facade = synthetic_type(
        "lib.TypeAliasesKt",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    );
    facade.kotlin = Some(Box::new(crate::KotlinSymbolMetadata {
        type_aliases: vec![crate::KotlinTypeAlias {
            fqn: "lib.Tags".into(),
            expanded_class: "java.util.ArrayList".into(),
            type_parameters: Vec::new(),
            expanded_type_arguments: vec!["kotlin.String".into()],
        }],
        ..crate::KotlinSymbolMetadata::default()
    }));
    index.insert_symbols_batch(vec![facade], library_id);

    index
        .index_file_with_source(
            &temp.path().join("app/Aliases.kt"),
            "package app\n\n\
             typealias Names = java.util.ArrayList<String>\n\
             typealias Pairs = kotlin.collections.Map<String, Int>\n",
        )
        .unwrap();
    let use_path = temp.path().join("app/Roster.kt");
    index
        .index_file_with_source(&use_path, "package app\n\nval roster: Names = Names()\n")
        .unwrap();
    let client_path = temp.path().join("other/Client.kt");
    index
        .index_file_with_source(
            &client_path,
            "package other\n\nimport app.Names\nimport lib.Tags\n\nclass Client\n",
        )
        .unwrap();
    let use_id = index.get_file_id(&use_path).unwrap();
    let client_id = index.get_file_id(&client_path).unwrap();

    use crate::KotlinTypeAliasArguments::{Expanded, NotAnAlias, Unmatched};
    let names = Expanded(vec!["String".to_owned()]);
    assert_eq!(
        index.resolve_type_name(use_id, &[], "Names"),
        crate::TypeNameResolution::Exact("java.util.ArrayList".into())
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(use_id, &[], "Names", &[]),
        names
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(use_id, &[], "app.Names", &[]),
        names
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(client_id, &[], "Names", &[]),
        names,
        "an imported alias"
    );
    assert_eq!(
        index.resolve_type_name(use_id, &[], "Pairs"),
        crate::TypeNameResolution::Exact("java.util.Map".into())
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(use_id, &[], "Pairs", &[]),
        Expanded(vec!["String".to_owned(), "Int".to_owned()]),
        "both arguments of a built-in collection"
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(client_id, &[], "Tags", &[]),
        Expanded(vec!["kotlin.String".to_owned()]),
        "a library alias"
    );
    // `Names` declares no type parameter, so a use cannot give it one.
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(use_id, &[], "Names", &["Int".to_owned()]),
        Unmatched
    );
    // A name that is not an alias, and one that binds nothing, are not aliases.
    for written in ["java.util.ArrayList", "Missing"] {
        assert_eq!(
            index.resolve_kotlin_type_alias_arguments(use_id, &[], written, &[]),
            NotAnAlias,
            "{written}"
        );
    }
}

/// An alias whose expansion has no type arguments of its own, one
/// to a raw generic class and one to a class that takes none, resolves as
/// before and gives no arguments.
#[test]
fn kotlin_typealias_without_expansion_arguments_gives_none() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for fqn in ["java.util.ArrayList", "java.lang.StringBuilder"] {
        index.add_external_symbol(synthetic_type(
            fqn,
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }
    index
        .index_file_with_source(
            &temp.path().join("app/Aliases.kt"),
            "package app\n\n\
             typealias Raw = java.util.ArrayList\n\
             typealias Text = java.lang.StringBuilder\n",
        )
        .unwrap();
    let use_path = temp.path().join("app/Use.kt");
    index
        .index_file_with_source(&use_path, "package app\n\nclass Use\n")
        .unwrap();
    let use_id = index.get_file_id(&use_path).unwrap();

    for (alias, expanded) in [
        ("Raw", "java.util.ArrayList"),
        ("Text", "java.lang.StringBuilder"),
    ] {
        assert_eq!(
            index.resolve_type_name(use_id, &[], alias),
            crate::TypeNameResolution::Exact(expanded.into()),
            "{alias}"
        );
        assert_eq!(
            index.resolve_kotlin_type_alias_arguments(use_id, &[], alias, &[]),
            crate::KotlinTypeAliasArguments::Expanded(Vec::new()),
            "{alias}"
        );
    }
}

/// An index with `java.util.ArrayList` and `java.util.HashMap`, the source
/// aliases `aliases` in package `app`, and the library alias
/// `typealias Keyed<V> = java.util.HashMap<kotlin.String, V>` in package `lib`;
/// with the id of a file in `app` that imports `lib.Keyed`.
fn index_with_generic_typealiases(temp: &TempDir, aliases: &str) -> (WorkspaceIndex, FileId) {
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for fqn in ["java.util.ArrayList", "java.util.HashMap"] {
        index.add_external_symbol(synthetic_type(
            fqn,
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }
    let library = PathBuf::from("/deps/keyed.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&library));
    let library_id = index
        .inner
        .read()
        .external_file_id_for_path(&library)
        .expect("registered library");
    let mut facade = synthetic_type(
        "lib.TypeAliasesKt",
        SymbolKind::CLASS,
        None,
        &[],
        crate::Modifiers::default(),
    );
    facade.kotlin = Some(Box::new(crate::KotlinSymbolMetadata {
        type_aliases: vec![crate::KotlinTypeAlias {
            fqn: "lib.Keyed".into(),
            expanded_class: "java.util.HashMap".into(),
            type_parameters: vec!["V".into()],
            expanded_type_arguments: vec!["kotlin.String".into(), "V".into()],
        }],
        ..crate::KotlinSymbolMetadata::default()
    }));
    index.insert_symbols_batch(vec![facade], library_id);
    index
        .index_file_with_source(
            &temp.path().join("app/Aliases.kt"),
            &format!("package app\n\n{aliases}"),
        )
        .unwrap();
    let use_path = temp.path().join("app/Use.kt");
    index
        .index_file_with_source(&use_path, "package app\n\nimport lib.Keyed\n\nclass Use\n")
        .unwrap();
    let use_id = index.get_file_id(&use_path).unwrap();
    (index, use_id)
}

/// A generic alias binds each of its type parameters to the use's
/// argument for it, wherever the expansion names it: `StringMap<Int>` under
/// `typealias StringMap<V> = java.util.HashMap<String, V>` is a HashMap of
/// String to Int, and `Inverse<Int, String>` swaps its arguments back.
#[test]
fn kotlin_generic_typealias_binds_its_type_parameters_to_the_use_arguments() {
    let temp = TempDir::new().unwrap();
    let (index, use_id) = index_with_generic_typealiases(
        &temp,
        "typealias Box<T> = java.util.ArrayList<T>\n\
         typealias StringMap<V> = java.util.HashMap<String, V>\n\
         typealias Inverse<K, V> = java.util.HashMap<V, K>\n\
         typealias Grouped<T> = java.util.HashMap<String, java.util.List<T>?>\n\
         typealias Covariant<T> = java.util.ArrayList<out T>\n",
    );
    let arguments = |written: &[&str]| {
        written
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect::<Vec<_>>()
    };
    let at_use = |alias: &str, written: &[&str]| {
        index.resolve_kotlin_type_alias_arguments(use_id, &[], alias, &arguments(written))
    };
    let expanded =
        |spellings: &[&str]| crate::KotlinTypeAliasArguments::Expanded(arguments(spellings));

    assert_eq!(
        index.resolve_type_name(use_id, &[], "StringMap"),
        crate::TypeNameResolution::Exact("java.util.HashMap".into())
    );
    assert_eq!(at_use("StringMap", &["Int"]), expanded(&["String", "Int"]));
    assert_eq!(
        at_use("app.StringMap", &["Int"]),
        expanded(&["String", "Int"])
    );
    assert_eq!(at_use("Box", &["String"]), expanded(&["String"]));
    assert_eq!(
        at_use("Inverse", &["Int", "String"]),
        expanded(&["String", "Int"]),
        "the parameters in another order than the class's"
    );
    assert_eq!(
        at_use("Grouped", &["Int"]),
        expanded(&["String", "java.util.List<Int>?"]),
        "a parameter nested in an argument"
    );
    assert_eq!(
        at_use("Grouped", &["*"]),
        expanded(&["String", "java.util.List<*>?"]),
        "a projection where a type argument stands"
    );
    assert_eq!(at_use("Box", &["out Number"]), expanded(&["out Number"]));
    assert_eq!(at_use("Covariant", &["Int"]), expanded(&["out Int"]));
    assert_eq!(at_use("Covariant", &["*"]), expanded(&["*"]));
    assert_eq!(
        at_use("Keyed", &["Int"]),
        expanded(&["kotlin.String", "Int"]),
        "a library alias"
    );
}

/// A use of a generic alias whose arguments do not match the
/// alias's type parameters gives no type, rather than arguments matched by
/// position: one argument too many or too few, a raw use, an empty argument,
/// or a projection where the expansion cannot take one.
#[test]
fn kotlin_generic_typealias_with_unmatched_arguments_gives_no_type() {
    let temp = TempDir::new().unwrap();
    let (index, use_id) = index_with_generic_typealiases(
        &temp,
        "typealias StringMap<V> = java.util.HashMap<String, V>\n\
         typealias Inverse<K, V> = java.util.HashMap<V, K>\n\
         typealias Optional<T> = java.util.ArrayList<T?>\n\
         typealias Covariant<T> = java.util.ArrayList<out T>\n",
    );
    let at_use = |alias: &str, written: &[&str]| {
        let written = written
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect::<Vec<_>>();
        index.resolve_kotlin_type_alias_arguments(use_id, &[], alias, &written)
    };
    for (alias, written) in [
        ("StringMap", &["Int", "String"][..]),
        ("StringMap", &[][..]),
        ("StringMap", &[""][..]),
        ("Inverse", &["Int"][..]),
        ("Keyed", &["Int", "Int"][..]),
        ("Optional", &["*"][..]),
        ("Covariant", &["in Int"][..]),
    ] {
        assert_eq!(
            at_use(alias, written),
            crate::KotlinTypeAliasArguments::Unmatched,
            "{alias}{written:?}"
        );
    }
    // The name still resolves to the class; only its arguments are refused.
    assert_eq!(
        index.resolve_type_name(use_id, &[], "StringMap"),
        crate::TypeNameResolution::Exact("java.util.HashMap".into())
    );
    assert_eq!(
        at_use("java.util.HashMap", &["Int"]),
        crate::KotlinTypeAliasArguments::NotAnAlias
    );
}

/// Facades that agree about an alias's class but not about its
/// type arguments give the class with none, rather than one set picked by
/// order.
#[test]
fn kotlin_typealias_facades_that_disagree_about_arguments_give_none() {
    let facade = |fqn: &str, arguments: &[&str]| {
        let mut facade = synthetic_type(
            fqn,
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        );
        facade.kotlin = Some(Box::new(crate::KotlinSymbolMetadata {
            type_aliases: vec![crate::KotlinTypeAlias {
                fqn: "app.Names".into(),
                expanded_class: "java.util.ArrayList".into(),
                type_parameters: Vec::new(),
                expanded_type_arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
            }],
            ..crate::KotlinSymbolMetadata::default()
        }));
        Arc::new(facade)
    };
    let agreeing = crate::index::kotlin_type_alias_expansion_from(
        "app.Names",
        [
            facade("app.AKt", &["String"]),
            facade("app.BKt", &["String"]),
        ],
    )
    .expect("one class");
    assert_eq!(agreeing.type_arguments, ["String".to_owned()]);
    let disagreeing = crate::index::kotlin_type_alias_expansion_from(
        "app.Names",
        [facade("app.AKt", &["String"]), facade("app.BKt", &["Int"])],
    )
    .expect("one class");
    assert_eq!(disagreeing.class, "java.util.ArrayList");
    assert!(disagreeing.type_arguments.is_empty());
}

#[test]
fn kotlin_alias_reverse_keys_are_rebound_and_removed_symmetrically() {
    let temp = TempDir::new().unwrap();
    let source_path = temp.path().join("Alias.kt");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for fqn in ["first.Contract", "second.Contract"] {
        index.add_external_symbol(synthetic_type(
            fqn,
            lsp_types::SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ));
    }

    index
        .index_file_with_source(
            &source_path,
            "package app\nimport first.Contract as Alias\ninterface Child : Alias",
        )
        .unwrap();
    assert_eq!(
        index
            .find_direct_subtypes("first.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        ["app.Child"]
    );
    assert!(index.find_direct_subtypes("second.Contract").is_empty());

    index
        .index_file_with_source(
            &source_path,
            "package app\nimport second.Contract as Alias\ninterface Child : Alias",
        )
        .unwrap();
    assert!(index.find_direct_subtypes("first.Contract").is_empty());
    assert_eq!(
        index
            .find_direct_subtypes("second.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        ["app.Child"]
    );

    let child_file = index.get_file_id(&source_path).expect("indexed child");
    index.remove_file(child_file);
    assert!(index.find_direct_subtypes("first.Contract").is_empty());
    assert!(index.find_direct_subtypes("second.Contract").is_empty());
}

#[test]
fn kotlin_class_delegate_expression_is_structured_for_usage_extraction() {
    let temp = TempDir::new().unwrap();
    let source_path = temp.path().join("Delegate.kt");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &source_path,
            concat!(
                "package app\n",
                "interface Contract\n",
                "fun build(): Contract = TODO()\n",
                "class Delegating : Contract by build() { fun body() {} }",
            ),
        )
        .unwrap();

    assert!(index.get_symbol("app.Delegating.body").is_some());
    assert_eq!(
        index.find_usages("app.build").len(),
        1,
        "the class-header delegate call must remain a CALL_EXPR"
    );
}

#[test]
fn kotlin_wildcard_supertype_resolves_when_dependency_arrives_later() {
    let temp = TempDir::new().unwrap();
    let source_path = temp.path().join("Hierarchy.kt");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &source_path,
            r#"package app
               import dependency.*
               interface LocalContract : Contract
               object Singleton : LocalContract"#,
        )
        .unwrap();
    assert!(index.find_direct_subtypes("dependency.Contract").is_empty());

    index.add_external_symbol(synthetic_type(
        "dependency.Contract",
        lsp_types::SymbolKind::INTERFACE,
        None,
        &[],
        crate::Modifiers::default(),
    ));

    assert_eq!(
        index
            .find_direct_subtypes("dependency.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.LocalContract"]
    );
    assert_eq!(
        index
            .find_implementations("dependency.Contract")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.Singleton"]
    );
}

#[test]
fn kotlin_supertype_provenance_distinguishes_qualified_and_imported_names() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let imported_path = temp.path().join("app/ImportedChild.kt");
    index
        .index_file_with_source(
            &imported_path,
            "package app\nimport q.Base\nclass ImportedChild : Base()",
        )
        .unwrap();
    assert_eq!(
        index
            .get_symbol("app.ImportedChild")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("Base")
    );
    assert!(index.direct_supertypes("app.ImportedChild").is_empty());

    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base {}",
        )
        .unwrap();
    assert_eq!(index.direct_supertypes("app.ImportedChild"), ["q.Base"]);

    index
        .index_file_with_source(
            &temp.path().join("p/Base.java"),
            "package p; public class Base {}",
        )
        .unwrap();
    let qualified_path = temp.path().join("p/QualifiedChild.kt");
    index
        .index_file_with_source(
            &qualified_path,
            "package p\nimport q.Base\nclass QualifiedChild : p.Base()",
        )
        .unwrap();
    assert_eq!(
        index
            .get_symbol("p.QualifiedChild")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("p.Base")
    );
    assert_eq!(
        index.direct_supertypes("p.QualifiedChild"),
        ["p.Base"],
        "a genuinely qualified same-prefix spelling must not be rebound through q.Base"
    );
}

#[test]
fn groovy_and_scala_preserve_explicitly_qualified_supertype_spelling() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Base.java"),
            "package p; public class Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/GroovyChild.groovy"),
            "package p\nimport q.Base\nclass GroovyChild extends p.Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/ScalaChild.scala"),
            "package p\nimport q.Base\nclass ScalaChild extends p.Base",
        )
        .unwrap();

    assert_eq!(index.direct_supertypes("p.GroovyChild"), ["p.Base"]);
    assert_eq!(index.direct_supertypes("p.ScalaChild"), ["p.Base"]);
}

#[test]
fn jvm_language_default_imports_and_kotlin_aliases_relink_late() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("app/KotlinChild.kt"),
            "package app\nclass KotlinChild : ArrayList<String>()",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/KotlinListChild.kt"),
            "package app\nclass KotlinListChild : List<String>",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/KotlinEntryChild.kt"),
            concat!(
                "package app\n",
                "class KotlinEntryChild : MutableMap.MutableEntry<String, String>"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/GroovyChild.groovy"),
            "package app\nclass GroovyChild extends ArrayList {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/ScalaChild.scala"),
            "package app\nclass ScalaChild extends Product",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/KotlinMathChild.kt"),
            "package app\nclass KotlinMathChild : MathType",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/GroovyBig.groovy"),
            concat!(
                "package app\n",
                "class GroovyBig extends BigInteger {}\n",
                "class GroovyDecimal extends BigDecimal {}\n",
                "class GroovyInstant extends Instant {}\n",
                "class GroovyMathContext extends MathContext {}"
            ),
        )
        .unwrap();
    for child in [
        "app.KotlinChild",
        "app.KotlinListChild",
        "app.KotlinEntryChild",
        "app.GroovyChild",
        "app.ScalaChild",
        "app.KotlinMathChild",
        "app.GroovyBig",
        "app.GroovyDecimal",
        "app.GroovyInstant",
        "app.GroovyMathContext",
    ] {
        assert!(index.direct_supertypes(child).is_empty());
    }
    assert_eq!(
        index
            .get_symbol("app.KotlinEntryChild")
            .unwrap()
            .interfaces
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["MutableMap.MutableEntry"]
    );

    index.add_external_symbols_batch(vec![
        synthetic_type(
            "java.util.ArrayList",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.util.List",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.util.Map",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.util.Map.Entry",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "scala.Product",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "kotlin.math.MathType",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.math.BigInteger",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.math.BigDecimal",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.math.MathContext",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.time.Instant",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    assert_eq!(
        index.direct_supertypes("app.KotlinChild"),
        ["java.util.ArrayList"]
    );
    assert_eq!(
        index.direct_supertypes("app.KotlinListChild"),
        ["java.util.List"]
    );
    assert_eq!(
        index.direct_supertypes("app.KotlinEntryChild"),
        ["java.util.Map.Entry"]
    );
    assert_eq!(
        index.direct_supertypes("app.GroovyChild"),
        ["java.util.ArrayList"]
    );
    assert_eq!(index.direct_supertypes("app.ScalaChild"), ["scala.Product"]);
    assert_eq!(
        index.direct_supertypes("app.KotlinMathChild"),
        ["kotlin.math.MathType"]
    );
    assert_eq!(
        index.direct_supertypes("app.GroovyBig"),
        ["java.math.BigInteger"]
    );
    assert_eq!(
        index.direct_supertypes("app.GroovyDecimal"),
        ["java.math.BigDecimal"]
    );
    assert_eq!(
        index.direct_supertypes("app.GroovyInstant"),
        ["java.time.Instant"]
    );
    assert!(
        index.direct_supertypes("app.GroovyMathContext").is_empty(),
        "Groovy imports BigInteger/BigDecimal singly, not java.math.*"
    );
    assert_eq!(index.find_direct_subtypes("java.util.ArrayList").len(), 2);
    assert_eq!(
        index
            .find_direct_subtypes("java.util.Map.Entry")
            .iter()
            .map(|symbol| symbol.fqn.as_ref())
            .collect::<Vec<_>>(),
        ["app.KotlinEntryChild"]
    );
}

#[test]
fn jvm_source_classes_publish_exact_language_roots() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "java.lang.Object",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.lang.Enum",
            SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "java.lang.annotation.Annotation",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dependency.Base",
            SymbolKind::CLASS,
            Some("java.lang.Object"),
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dependency.Contract",
            SymbolKind::INTERFACE,
            None,
            &[],
            crate::Modifiers::default(),
        ),
    ]);

    let kotlin_path = temp.path().join("app/KotlinRoots.kt");
    let kotlin_source = concat!(
        "package app\n",
        "import dependency.Base\nimport dependency.Contract\n",
        "class Plain\n",
        "class InterfacesOnly : Contract\n",
        "class Delegating(private val delegate: Contract) : Contract by delegate { fun body() {} }\n",
        "class Secondary : Base, Contract { constructor() : super() {} }\n",
        "object Singleton\n",
        "enum class Choice { ONE }\n",
        "annotation class Marker"
    );
    index
        .index_file_with_source(&kotlin_path, kotlin_source)
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/GroovyPlain.groovy"),
            "package app\nclass GroovyPlain {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("app/ScalaPlain.scala"),
            "package app\nclass ScalaPlain",
        )
        .unwrap();

    assert_eq!(index.direct_supertypes("app.Plain"), ["java.lang.Object"]);
    assert_eq!(
        index.direct_supertypes("app.InterfacesOnly"),
        ["dependency.Contract", "java.lang.Object"]
    );
    assert_eq!(
        index.direct_supertypes("app.Secondary"),
        ["dependency.Base", "dependency.Contract"]
    );
    assert_eq!(
        index.direct_supertypes("app.Delegating"),
        ["dependency.Contract", "java.lang.Object"]
    );
    assert!(index.get_symbol("app.Delegating.body").is_some());
    assert_eq!(
        index.direct_supertypes("app.Singleton"),
        ["java.lang.Object"]
    );
    assert_eq!(
        index.get_parent_class("app.Singleton").as_deref(),
        Some("java.lang.Object"),
        "an implicit Kotlin root must retain its synthesized class role"
    );
    assert_eq!(
        index.get_parent_class("app.Secondary").as_deref(),
        Some("dependency.Base"),
        "a bare Kotlin superclass must be distinguished from following interfaces"
    );
    assert_eq!(
        index.get_parent_class("app.InterfacesOnly").as_deref(),
        Some("java.lang.Object"),
        "an interfaces-only Kotlin class still has Object as its class parent"
    );
    assert_eq!(
        index.get_symbol("app.Choice").unwrap().kind,
        SymbolKind::ENUM
    );
    assert_eq!(index.direct_supertypes("app.Choice"), ["java.lang.Enum"]);
    assert_eq!(
        index.get_symbol("app.Marker").unwrap().kind,
        SymbolKind::INTERFACE
    );
    assert_eq!(
        index.direct_supertypes("app.Marker"),
        ["java.lang.annotation.Annotation"]
    );
    assert_eq!(
        index.direct_supertypes("app.GroovyPlain"),
        ["java.lang.Object"]
    );
    assert_eq!(
        index.direct_supertypes("app.ScalaPlain"),
        ["java.lang.Object"]
    );
    assert!(!index
        .find_direct_subtypes("java.lang.Object")
        .iter()
        .any(|symbol| symbol.fqn.as_ref() == "app.Secondary"));

    let kotlin_id = index.get_file_id(&kotlin_path).unwrap();
    let batch = index
        .file_supertypes_for_revision(kotlin_id, kotlin_source)
        .unwrap();
    let by_type = batch
        .types
        .into_iter()
        .map(|entry| (entry.type_fqn, (entry.superclass, entry.interfaces)))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_type["app.Plain"],
        (Some("java.lang.Object".to_owned()), Vec::new())
    );
    assert_eq!(
        by_type["app.InterfacesOnly"],
        (
            Some("java.lang.Object".to_owned()),
            vec!["dependency.Contract".to_owned()]
        )
    );
    assert_eq!(
        by_type["app.Secondary"],
        (
            Some("dependency.Base".to_owned()),
            vec!["dependency.Contract".to_owned()]
        )
    );
    assert_eq!(
        by_type["app.Delegating"],
        (
            Some("java.lang.Object".to_owned()),
            vec!["dependency.Contract".to_owned()]
        )
    );
    assert_eq!(
        by_type["app.Choice"],
        (Some("java.lang.Enum".to_owned()), Vec::new())
    );
    assert_eq!(
        by_type["app.Marker"],
        (None, vec!["java.lang.annotation.Annotation".to_owned()])
    );
}

#[test]
fn kotlin_bare_supertype_role_comes_from_the_resolved_declaration_kind() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("p/Base.java"),
            "package p; public class Base { public void member() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Contract.java"),
            "package p; public interface Contract { default void member() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/OtherContract.java"),
            "package p; public interface OtherContract { default void member() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("q/Base.java"),
            "package q; public class Base { public void chosen() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Secondary.kt"),
            concat!(
                "package p\n",
                "class Secondary : Base, Contract { constructor() : super() {} }"
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("p/Outer.kt"),
            concat!(
                "package p\nimport q.Base\n",
                "class Outer { open class Base { open fun chosen() {} } ",
                "class Child : Base() { fun call() { chosen() } } }"
            ),
        )
        .unwrap();

    let secondary = index.get_symbol("p.Secondary").unwrap();
    assert!(secondary.parent_class.is_none());
    assert_eq!(
        secondary
            .interfaces
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["Base", "Contract"],
        "bare Kotlin entries retain unresolved role in the source representation"
    );
    assert_eq!(
        index.direct_supertypes("p.Secondary"),
        ["p.Base", "p.Contract"]
    );
    assert_eq!(index.direct_supertypes("p.Outer.Child"), ["p.Outer.Base"]);
    assert_eq!(index.find_usages("p.Outer.Base.chosen").len(), 1);
    assert!(index.find_usages("q.Base.chosen").is_empty());
    let chain = index.get_inheritance_chain("p.Secondary");
    let base_position = chain.iter().position(|fqn| fqn == "p.Base").unwrap();
    let contract_position = chain.iter().position(|fqn| fqn == "p.Contract").unwrap();
    assert!(
        base_position < contract_position,
        "resolved class targets must precede interfaces: {chain:?}"
    );
    assert_eq!(
        index
            .find_method_by_args_with_inheritance("p.Secondary", "member", 0)
            .unwrap()
            .container
            .as_deref(),
        Some("p.Base")
    );

    index
        .index_file_with_source(
            &temp.path().join("p/InterfacesOnly.kt"),
            "package p\nclass InterfacesOnly : Contract, OtherContract",
        )
        .unwrap();
    assert!(index
        .find_method_by_args_with_inheritance("p.InterfacesOnly", "member", 0)
        .is_none());
}

#[test]
fn sealed_subclasses_are_order_independent_and_follow_parent_modifiers() {
    let temp = TempDir::new().unwrap();
    let child_path = temp.path().join("Child.java");
    let parent_path = temp.path().join("Parent.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &child_path,
            "package sealedtypes; final class Child extends Parent {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &parent_path,
            "package sealedtypes; sealed class Parent permits Child {}",
        )
        .unwrap();
    assert_eq!(
        index.get_sealed_subclasses("sealedtypes.Parent"),
        vec!["sealedtypes.Child"]
    );

    index
        .index_file_with_source(&parent_path, "package sealedtypes; class Parent {}")
        .unwrap();
    assert!(index.get_sealed_subclasses("sealedtypes.Parent").is_empty());

    index
        .index_file_with_source(
            &parent_path,
            "package sealedtypes; sealed class Parent permits Child {}",
        )
        .unwrap();
    assert_eq!(
        index.get_sealed_subclasses("sealedtypes.Parent"),
        vec!["sealedtypes.Child"]
    );

    let child_file_id = index.get_file_id(&child_path).unwrap();
    index.remove_file(child_file_id);
    assert!(index.get_sealed_subclasses("sealedtypes.Parent").is_empty());
}

#[test]
fn reindexing_a_subtype_replaces_all_old_inheritance_edges() {
    let temp = TempDir::new().unwrap();
    let child_path = temp.path().join("Child.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (name, kind) in [
        ("OldBase", "class"),
        ("NewBase", "class"),
        ("OldContract", "interface"),
        ("NewContract", "interface"),
    ] {
        index
            .index_file_with_source(
                &temp.path().join(format!("{name}.java")),
                &format!("package graph; {kind} {name} {{}}"),
            )
            .unwrap();
    }

    index
        .index_file_with_source(
            &child_path,
            "package graph; class Child extends OldBase implements OldContract {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &child_path,
            "package graph; class Child extends NewBase implements NewContract {}",
        )
        .unwrap();

    for old_supertype in ["graph.OldBase", "graph.OldContract"] {
        assert!(
            index.find_direct_subtypes(old_supertype).is_empty(),
            "stale edge remained under {old_supertype}"
        );
    }
    for new_supertype in ["graph.NewBase", "graph.NewContract"] {
        assert_eq!(
            index
                .find_direct_subtypes(new_supertype)
                .iter()
                .map(|symbol| symbol.fqn.as_ref())
                .collect::<Vec<_>>(),
            vec!["graph.Child"]
        );
    }
}

// ========================================================================
// JDK Shadowing Tests - Workspace symbols should always win over JDK
// ========================================================================

#[test]
fn test_workspace_symbols_shadow_external_jdk() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root.clone());

    // 1. Add JDK external symbol first (simulating JDK cache load)
    let jdk_symbol = Symbol {
        fqn: "java.io.PrintStream".into(),
        simple_name: "PrintStream".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };
    index.add_external_symbol(jdk_symbol);

    // 2. Add workspace symbol (simulating workspace file indexing)
    let workspace_file = root.join("java/io/PrintStream.java");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(
        &workspace_file,
        "package java.io;\npublic class PrintStream {}",
    )
    .unwrap();

    index
        .index_file_with_source(
            &workspace_file,
            "package java.io;\npublic class PrintStream {}",
        )
        .unwrap();

    let workspace_file_id = index.get_file_id(&workspace_file).unwrap();

    // 3. CRITICAL TEST: Workspace symbol should win
    let found = index.find_symbol("java.io.PrintStream").unwrap();

    assert_eq!(
        found.file_id, workspace_file_id,
        "Workspace symbol should shadow JDK external symbol, \
         but got FileId({}) instead of workspace FileId({})",
        found.file_id.0, workspace_file_id.0
    );
}

#[test]
fn test_jdk_symbols_available_when_no_workspace_conflict() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root);

    // Add JDK symbol that doesn't conflict with workspace
    let jdk_symbol = Symbol {
        fqn: "java.util.ArrayList".into(),
        simple_name: "ArrayList".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };
    index.add_external_symbol(jdk_symbol);

    // Should be able to find JDK symbol
    let found = index.find_symbol("java.util.ArrayList");
    assert!(
        found.is_some(),
        "Should find JDK symbol when no workspace conflict"
    );

    // Note: We can't assert FileId(u32::MAX) because add_external_symbol
    // assigns a new FileId. We just verify it's findable.
}

#[test]
fn test_workspace_symbol_added_before_jdk_still_wins() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root.clone());

    // 1. Add workspace symbol FIRST
    let workspace_file = root.join("java/lang/String.java");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(
        &workspace_file,
        "package java.lang;\npublic class String {}",
    )
    .unwrap();

    index
        .index_file_with_source(
            &workspace_file,
            "package java.lang;\npublic class String {}",
        )
        .unwrap();

    let workspace_file_id = index.get_file_id(&workspace_file).unwrap();

    // 2. Add JDK external symbol (simulating JDK cache load AFTER workspace)
    let jdk_symbol = Symbol {
        fqn: "java.lang.String".into(),
        simple_name: "String".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };
    index.add_external_symbol(jdk_symbol);

    // 3. Workspace should STILL win (order-independent)
    let found = index.find_symbol("java.lang.String").unwrap();
    assert_eq!(
        found.file_id, workspace_file_id,
        "Workspace symbol should win regardless of insertion order"
    );
}

#[test]
fn test_multiple_jdk_symbols_batch() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root.clone());

    // Workspace mock
    let workspace_file = root.join("java/io/PrintStream.java");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(
        &workspace_file,
        "package java.io;\npublic class PrintStream {}",
    )
    .unwrap();
    index
        .index_file_with_source(
            &workspace_file,
            "package java.io;\npublic class PrintStream {}",
        )
        .unwrap();

    let workspace_file_id = index.get_file_id(&workspace_file).unwrap();

    // Batch load JDK symbols (like cache loading)
    let jdk_symbols = vec![
        Symbol {
            fqn: "java.io.PrintStream".into(),
            simple_name: "PrintStream".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces: vec![],
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        },
        Symbol {
            fqn: "java.util.ArrayList".into(),
            simple_name: "ArrayList".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces: vec![],
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        },
    ];
    index.add_external_symbols_batch(jdk_symbols);

    // Workspace PrintStream should win
    let printstream = index.find_symbol("java.io.PrintStream").unwrap();
    assert_eq!(
        printstream.file_id, workspace_file_id,
        "Workspace PrintStream should win over JDK in batch load"
    );

    // ArrayList should be from external (no conflict)
    let arraylist = index.find_symbol("java.util.ArrayList");
    assert!(
        arraylist.is_some(),
        "ArrayList should be available from JDK batch"
    );
}

#[test]
fn responsive_external_restore_publishes_complete_chunks_to_readers() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;
    use std::cell::RefCell;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let symbol = |name: &str| Symbol {
        fqn: format!("java.lang.{name}").into(),
        simple_name: name.into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers::default(),
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };
    let observed_counts = RefCell::new(Vec::new());

    index.add_external_symbols_in_chunks_for_test(
        vec![symbol("First"), symbol("Second"), symbol("Third")],
        1,
        || observed_counts.borrow_mut().push(index.symbol_count()),
    );

    assert_eq!(
        observed_counts.into_inner(),
        vec![1, 2],
        "readers should observe each committed chunk before restoration completes"
    );
    assert!(index.has_class("java.lang.First"));
    assert!(index.has_class("java.lang.Second"));
    assert!(index.has_class("java.lang.Third"));
}

#[test]
fn responsive_external_restore_never_splits_an_owner_across_chunks() {
    use lsp_types::SymbolKind;
    use std::cell::RefCell;

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let symbols = vec![
        synthetic_type(
            "dependency.First",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_type(
            "dependency.Second",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_method("dependency.First", "method"),
        synthetic_method("dependency.Second", "method"),
    ];
    let observations = RefCell::new(Vec::new());

    index.add_external_symbols_in_chunks_for_test(symbols, 2, || {
        observations.borrow_mut().push((
            index.has_class("dependency.First"),
            index.get_symbol("dependency.First.method").is_some(),
            index.has_class("dependency.Second"),
            index.get_symbol("dependency.Second.method").is_some(),
        ));
    });

    assert_eq!(
        observations.into_inner(),
        vec![(true, true, false, false)],
        "the first owner must be complete before readers run, even when input symbols are interleaved"
    );
    assert!(index.has_class("dependency.Second"));
    assert!(index.get_symbol("dependency.Second.method").is_some());
}

#[test]
fn test_workspace_method_shadows_jdk_method() {
    use crate::{Modifiers, Symbol, TextRange};
    use lsp_types::SymbolKind;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let index = WorkspaceIndex::new(root.clone());

    // Add JDK method
    let jdk_method = Symbol {
        fqn: "java.io.PrintStream.println".into(),
        simple_name: "println".into(),
        range: TextRange::new(0, 0),
        kind: SymbolKind::METHOD,
        modifiers: Modifiers::default(),
        container: Some("java.io.PrintStream".into()),
        return_type: Some("void".into()),
        parameters: vec!["Object".into()],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    };
    index.add_external_symbol(jdk_method);

    // Add workspace class with method
    let workspace_file = root.join("java/io/PrintStream.java");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(
        &workspace_file,
        "package java.io;\npublic class PrintStream { public void println(Object x) {} }",
    )
    .unwrap();

    index
        .index_file_with_source(
            &workspace_file,
            "package java.io;\npublic class PrintStream { public void println(Object x) {} }",
        )
        .unwrap();

    let workspace_file_id = index.get_file_id(&workspace_file).unwrap();

    // Workspace method should win
    let found = index.find_symbol("java.io.PrintStream.println").unwrap();
    assert_eq!(
        found.file_id, workspace_file_id,
        "Workspace method should shadow JDK method"
    );

    index.remove_file(workspace_file_id);
    let restored = index
        .find_symbol("java.io.PrintStream.println")
        .expect("legacy descriptor-less JDK method must be restored");
    assert!(index.is_external_file(restored.file_id));
}

/// Test that method resolution cache is properly invalidated on file changes
///
/// This test verifies:
/// 1. Cache is populated when method resolution is performed
/// 2. Cache is cleared when file is re-indexed
/// 3. New method return type is seen after cache invalidation
///
/// Without proper cache invalidation, stale method return types persist and cause
/// incorrect type inference in chained method calls and completion suggestions.
#[tokio::test]
async fn test_method_resolution_cache_invalidation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Phase 1: Create initial file with method returning String
    let file_path = root.join("Builder.java");
    std::fs::write(
        &file_path,
        r#"
package com.example;

public class Builder {
    public String getName() {
        return "initial";
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    // Phase 2: Perform method resolution to populate cache
    // Simulate type resolution querying for Builder.getName() return type
    let initial_return_type =
        index.get_or_cache_method_resolution("com.example.Builder", "getName", || {
            // Simulate expensive method resolution lookup
            index
                .get_symbol("com.example.Builder.getName")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });

    assert_eq!(
        initial_return_type,
        Some("java.lang.String".into()),
        "Initial method should return String"
    );

    // Phase 3: Modify file to change return type to Integer
    std::fs::write(
        &file_path,
        r#"
package com.example;

public class Builder {
    public Integer getName() {
        return 42;
    }
}
"#,
    )
    .unwrap();

    // Phase 4: Re-index the file (should trigger cache invalidation)
    index.index_file(&file_path).await.unwrap();

    // Phase 5: Query method resolution again - should see new return type
    // If cache wasn't cleared, this would still return String (stale)
    let updated_return_type =
        index.get_or_cache_method_resolution("com.example.Builder", "getName", || {
            index
                .get_symbol("com.example.Builder.getName")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });

    assert_eq!(
        updated_return_type,
        Some("java.lang.Integer".into()),
        "Cache should be invalidated - method now returns Integer, not String"
    );

    // Phase 6: Verify cache is actually being used (performance check)
    // Query same method again - should hit cache, not re-execute lookup
    let mut lookup_executed = false;
    let cached_result =
        index.get_or_cache_method_resolution("com.example.Builder", "getName", || {
            lookup_executed = true;
            Some("THIS_SHOULD_NOT_BE_RETURNED".into())
        });

    assert_eq!(
        cached_result,
        Some("java.lang.Integer".into()),
        "Should return cached value"
    );
    assert!(
        !lookup_executed,
        "Lookup function should not be called when cache hit"
    );
}

#[test]
fn removing_the_last_declaration_invalidates_resolution_caches() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Builder.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(
            &path,
            "package com.example; class Builder { String value() { return \"x\"; } }",
        )
        .unwrap();
    assert_eq!(
        index.get_or_cache_method_resolution("com.example.Builder", "value", || {
            Some("java.lang.String".to_string())
        }),
        Some("java.lang.String".to_string())
    );

    // No newly inserted FQN can drive invalidation in this revision. The old
    // declarations therefore have to be carried into the affected set.
    index
        .index_file_with_source(&path, "package com.example;")
        .unwrap();

    assert_eq!(
        index.get_or_cache_method_resolution("com.example.Builder", "value", || None),
        None,
        "removing every declaration must not leave a positive cache entry"
    );
}

#[test]
fn failed_reindex_preserves_the_complete_last_good_revision() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Service.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let old_source = "package app; import com.old.api.*; class Service { OldType value; }";

    index.index_file_with_source(&path, old_source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let old_hash = index.get_content_hash(file_id).unwrap();
    assert!(index
        .get_dependent_files("com.old.api.OldType")
        .contains(&file_id));

    let result = index.index_file_with_source_before_publish_for_test(
        &path,
        "package app; import com.new.api.*; class Replacement { NewType value; }",
        || Err(anyhow::anyhow!("injected pre-publication failure")),
    );
    assert!(result.is_err());

    assert_eq!(index.get_file_id(&path), Some(file_id));
    assert!(index.find_symbol("app.Service").is_some());
    assert!(index.find_symbol("app.Replacement").is_none());
    assert_eq!(index.get_content_hash(file_id), Some(old_hash));
    assert_eq!(
        index
            .get_file_contents(file_id)
            .as_deref()
            .map(String::as_str),
        Some(old_source)
    );
    assert!(index
        .get_dependent_files("com.old.api.OldType")
        .contains(&file_id));
    assert!(!index
        .get_dependent_files("com.new.api.NewType")
        .contains(&file_id));

    let inner = index.inner.read();
    let (_, cached_source) = inner
        .parse_tree_cache
        .peek(&file_id)
        .expect("last good parse product must survive");
    assert_eq!(cached_source.as_str(), old_source);
}

#[test]
fn readers_observe_one_complete_revision_at_the_publish_boundary() {
    use std::sync::{mpsc, Arc};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Version.java");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let old_source = "package app; class Before {}";
    let new_source = "package app; class After {}";
    index.index_file_with_source(&path, old_source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();

    let (ready_tx, ready_rx) = mpsc::sync_channel(0);
    let (continue_tx, continue_rx) = mpsc::sync_channel(0);
    let background_index = Arc::clone(&index);
    let background_path = path.clone();
    let handle = std::thread::spawn(move || {
        background_index.index_file_with_source_before_publish_for_test(
            &background_path,
            new_source,
            || {
                ready_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                Ok(())
            },
        )
    });

    ready_rx.recv().unwrap();
    assert!(index.find_symbol("app.Before").is_some());
    assert!(index.find_symbol("app.After").is_none());
    assert_eq!(
        index
            .get_file_contents(file_id)
            .as_deref()
            .map(String::as_str),
        Some(old_source)
    );

    continue_tx.send(()).unwrap();
    handle.join().unwrap().unwrap();
    assert!(index.find_symbol("app.Before").is_none());
    assert!(index.find_symbol("app.After").is_some());
    assert_eq!(
        index
            .get_file_contents(file_id)
            .as_deref()
            .map(String::as_str),
        Some(new_source)
    );
}

#[test]
fn failed_first_index_leaves_no_ghost_identity_or_locks() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("NeverPublished.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    let result = index.index_file_with_source_before_publish_for_test(
        &path,
        "package app; class NeverPublished {}",
        || Err(anyhow::anyhow!("injected pre-publication failure")),
    );
    assert!(result.is_err());
    assert!(index.get_file_id(&path).is_none());
    assert!(index.find_symbol("app.NeverPublished").is_none());
    assert!(index.path_locks.is_empty());
    assert!(index.file_locks.is_empty());
    let inner = index.inner.read();
    assert!(inner.paths.is_empty());
    inner.paths.assert_consistent();
    assert!(inner.published_revisions.is_empty());
    assert!(inner.file_content_cache.is_empty());
    assert_eq!(inner.parse_tree_cache.len(), 0);
}

#[test]
fn lookup_started_before_publication_cannot_refill_a_stale_cache_entry() {
    use std::sync::{mpsc, Arc};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Service.java");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(
            &path,
            "package app; class Service { String value() { return \"old\"; } }",
        )
        .unwrap();

    let (lookup_started_tx, lookup_started_rx) = mpsc::sync_channel(0);
    let (finish_lookup_tx, finish_lookup_rx) = mpsc::sync_channel(0);
    let old_index = Arc::clone(&index);
    let stale_lookup = std::thread::spawn(move || {
        old_index.get_or_cache_method_resolution("app.Service", "value", || {
            lookup_started_tx.send(()).unwrap();
            finish_lookup_rx.recv().unwrap();
            Some("java.lang.String".to_string())
        })
    });

    lookup_started_rx.recv().unwrap();
    index
        .index_file_with_source(
            &path,
            "package app; class Service { Integer value() { return 1; } }",
        )
        .unwrap();
    finish_lookup_tx.send(()).unwrap();

    // The in-flight request began on the old revision and may finish with its
    // coherent old answer. It must not write that answer into the new cache.
    assert_eq!(
        stale_lookup.join().unwrap(),
        Some("java.lang.String".to_string())
    );
    let mut recomputed = false;
    assert_eq!(
        index.get_or_cache_method_resolution("app.Service", "value", || {
            recomputed = true;
            Some("java.lang.Integer".to_string())
        }),
        Some("java.lang.Integer".to_string())
    );
    assert!(
        recomputed,
        "new generation must recompute instead of hitting stale data"
    );
}

/// Test cache invalidation for chained method calls (fluent API pattern)
///
/// Verifies that cache invalidation works correctly for builder-pattern methods
/// that return 'this' or other class instances. Critical for accurate completion
/// in fluent API call chains.
#[tokio::test]
async fn test_method_cache_invalidation_fluent_api() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let builder_file = root.join("RequestBuilder.java");
    std::fs::write(
        &builder_file,
        r#"
package com.example;

public class RequestBuilder {
    public RequestBuilder setUrl(String url) {
        return this;
    }

    public String build() {
        return "request";
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&builder_file).await.unwrap();

    // Cache the method resolution for setUrl
    let initial =
        index.get_or_cache_method_resolution("com.example.RequestBuilder", "setUrl", || {
            index
                .get_symbol("com.example.RequestBuilder.setUrl")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });

    assert_eq!(
        initial,
        Some("com.example.RequestBuilder".into()),
        "setUrl should return RequestBuilder"
    );

    // Change setUrl to return void (breaking the fluent API)
    std::fs::write(
        &builder_file,
        r#"
package com.example;

public class RequestBuilder {
    public void setUrl(String url) {
        // Now returns void
    }

    public String build() {
        return "request";
    }
}
"#,
    )
    .unwrap();

    // Re-index should clear cache
    index.index_file(&builder_file).await.unwrap();

    // Should now see void return type (not cached RequestBuilder)
    let updated =
        index.get_or_cache_method_resolution("com.example.RequestBuilder", "setUrl", || {
            index
                .get_symbol("com.example.RequestBuilder.setUrl")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });

    assert_eq!(
        updated,
        Some("void".into()),
        "setUrl should now return void"
    );
}

/// Test that cache is also cleared on file removal
#[tokio::test]
async fn test_method_cache_cleared_on_file_removal() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("Service.java");
    std::fs::write(
        &file_path,
        r#"
package com.example;

public class Service {
    public String getData() {
        return "data";
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    // Populate cache
    let initial = index.get_or_cache_method_resolution("com.example.Service", "getData", || {
        index
            .get_symbol("com.example.Service.getData")
            .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
    });

    assert_eq!(initial, Some("java.lang.String".into()));

    // Remove the file
    let file_id = index.get_file_id(&file_path).unwrap();
    index.remove_file(file_id);

    // Cache should be cleared - if we add a different class with same method,
    // it should see the new definition, not cached old one
    std::fs::write(
        &file_path,
        r#"
package com.example;

public class Service {
    public Integer getData() {
        return 123;
    }
}
"#,
    )
    .unwrap();

    index.index_file(&file_path).await.unwrap();

    let after_removal =
        index.get_or_cache_method_resolution("com.example.Service", "getData", || {
            index
                .get_symbol("com.example.Service.getData")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });

    assert_eq!(
        after_removal,
        Some("java.lang.Integer".into()),
        "Should see new return type after file removal and re-indexing"
    );
}

#[tokio::test]
async fn test_inheritance_chain_caching() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create inheritance hierarchy: Child -> Parent -> Grandparent
    let grandparent_path = root.join("Grandparent.java");
    std::fs::write(
        &grandparent_path,
        "package com.example; public class Grandparent {}",
    )
    .unwrap();

    let parent_path = root.join("Parent.java");
    std::fs::write(
        &parent_path,
        "package com.example; public class Parent extends Grandparent {}",
    )
    .unwrap();

    let child_path = root.join("Child.java");
    std::fs::write(
        &child_path,
        "package com.example; public class Child extends Parent {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&grandparent_path).await.unwrap();
    index.index_file(&parent_path).await.unwrap();
    index.index_file(&child_path).await.unwrap();

    // Get inheritance chain - should cache it
    let chain1 = index.get_inheritance_chain("com.example.Child");
    assert_eq!(
        chain1,
        vec![
            "com.example.Child",
            "com.example.Parent",
            "com.example.Grandparent"
        ]
    );

    // Get it again - should use cache
    let chain2 = index.get_inheritance_chain("com.example.Child");
    assert_eq!(chain1, chain2);

    // Clear cache and verify it's cleared
    index.clear_inheritance_cache();

    // Get chain again - should recompute but produce same result
    let chain3 = index.get_inheritance_chain("com.example.Child");
    assert_eq!(chain1, chain3);
}

#[tokio::test]
async fn test_inheritance_chain_cache_invalidation_on_file_change() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create initial hierarchy
    let parent_path = root.join("Parent.java");
    std::fs::write(&parent_path, "package com.example; public class Parent {}").unwrap();

    let child_path = root.join("Child.java");
    std::fs::write(
        &child_path,
        "package com.example; public class Child extends Parent {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&parent_path).await.unwrap();
    index.index_file(&child_path).await.unwrap();

    // Get initial chain
    let chain_before = index.get_inheritance_chain("com.example.Child");
    assert_eq!(
        chain_before,
        vec!["com.example.Child", "com.example.Parent"]
    );

    // Modify child to extend from Object instead
    std::fs::write(&child_path, "package com.example; public class Child {}").unwrap();

    // Re-index - should clear cache
    index.index_file(&child_path).await.unwrap();

    // Get chain again - should reflect the change
    let chain_after = index.get_inheritance_chain("com.example.Child");
    assert_eq!(
        chain_after,
        vec!["com.example.Child"],
        "Cache should be invalidated on file change"
    );
}

#[tokio::test]
async fn test_method_resolution_uses_cached_inheritance() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create parent with method
    let parent_path = root.join("Parent.java");
    std::fs::write(
        &parent_path,
        "package com.example; public class Parent { public String getName() { return \"\"; } }",
    )
    .unwrap();

    // Create a child override. Inherited lookup must expose the receiver's
    // effective declaration, not concatenate both hierarchy entries.
    let child_path = root.join("Child.java");
    std::fs::write(
        &child_path,
        "package com.example; public class Child extends Parent { @Override public String getName() { return \"child\"; } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&parent_path).await.unwrap();
    index.index_file(&child_path).await.unwrap();

    // Find method through inheritance - should cache the chain
    let methods = index.find_methods_by_name_with_inheritance("com.example.Child", "getName");
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].simple_name.as_ref(), "getName");
    assert_eq!(methods[0].container.as_deref(), Some("com.example.Child"));

    // Call again - should use cached inheritance chain
    let methods2 = index.find_methods_by_name_with_inheritance("com.example.Child", "getName");
    assert_eq!(methods2.len(), 1);
}

#[tokio::test]
async fn test_get_class_members_with_inheritance_includes_superclass_members() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let parent_path = root.join("Parent.java");
    std::fs::write(
        &parent_path,
        r#"package com.example;

public class Parent {
    protected String parentField = "parent";
}"#,
    )
    .unwrap();

    let child_path = root.join("Child.java");
    std::fs::write(
        &child_path,
        r#"package com.example;

public class Child extends Parent {
    private String childField = "child";
}"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&parent_path).await.unwrap();
    index.index_file(&child_path).await.unwrap();

    let members = index.get_class_members_with_inheritance("com.example.Child");
    let names: Vec<&str> = members
        .iter()
        .map(|symbol| symbol.simple_name.as_ref())
        .collect();

    assert!(
        names.contains(&"childField"),
        "Expected child field inherited in lookup set"
    );
    assert!(
        names.contains(&"parentField"),
        "Expected parent field inherited in lookup set"
    );

    let child_pos = names.iter().position(|name| *name == "childField");
    let parent_pos = names.iter().position(|name| *name == "parentField");

    if let (Some(child_pos), Some(parent_pos)) = (child_pos, parent_pos) {
        assert!(
            child_pos < parent_pos,
            "Expected child members to be prioritized before parent members"
        );
    }
}

#[tokio::test]
async fn test_get_class_members_with_inheritance_includes_interface_members() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let source_path = root.join("Main.java");
    std::fs::write(
        &source_path,
        r#"package com.example;

public interface BaseContract {
    String BASE_LABEL = "base";
}

public interface DerivedContract extends BaseContract {
    String DERIVED_LABEL = "derived";
}

public class Impl implements DerivedContract {
    private String ownLabel = "impl";
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&source_path).await.unwrap();

    let members = index.get_class_members_with_inheritance("com.example.Impl");
    let names: Vec<&str> = members
        .iter()
        .map(|symbol| symbol.simple_name.as_ref())
        .collect();

    assert!(
        names.contains(&"ownLabel"),
        "Expected class field in inheritance/member lookup"
    );
    assert!(
        names.contains(&"DERIVED_LABEL"),
        "Expected implemented interface field in inheritance/member lookup"
    );
    assert!(
        names.contains(&"BASE_LABEL"),
        "Expected transitive interface field in inheritance/member lookup"
    );
}

// ==================== Nested Class Reverse Dependency Tests ====================

#[tokio::test]
async fn test_reverse_deps_tracks_nested_classes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User.java with nested Builder class
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example;
            public class User {
                public static class Builder {
                    public Builder setName(String name) { return this; }
                    public User build() { return new User(); }
                }
            }
        "#,
    )
    .unwrap();

    // Create Main.java that imports User (but references User.Builder)
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.User;
            public class Main {
                public void test() {
                    User.Builder builder = new User.Builder();
                    builder.setName("test");
                }
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // Main.java imports User, so it should be a dependent of User
    let user_dependents = index.get_dependent_files("com.example.User");
    assert!(
        user_dependents.contains(&main_id),
        "Main.java should be a dependent of com.example.User"
    );

    // CRITICAL: Main.java also uses User.Builder, so it should be a dependent of User.Builder
    // When Builder changes, Main.java needs to be invalidated
    let builder_dependents = index.get_dependent_files("com.example.User.Builder");
    assert!(
        builder_dependents.contains(&main_id),
        "Main.java should be a dependent of com.example.User.Builder (nested class tracking)"
    );
}

#[tokio::test]
async fn test_reverse_deps_nested_class_transitive() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User with multiple nested classes
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example;
            public class User {
                public static class Builder {}
                public static class Address {}
                public static class Profile {}
            }
        "#,
    )
    .unwrap();

    // Create Main that imports User
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.User;
            public class Main {}
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // All nested classes should have Main as a dependent
    for nested_class in &["Builder", "Address", "Profile"] {
        let fqn = format!("com.example.User.{}", nested_class);
        let dependents = index.get_dependent_files(&fqn);
        assert!(
            dependents.contains(&main_id),
            "Main.java should be a dependent of {} (imported outer class)",
            fqn
        );
    }
}

#[tokio::test]
async fn test_reverse_deps_nested_class_wildcard_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User with nested Builder
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example.model;
            public class User {
                public static class Builder {}
            }
        "#,
    )
    .unwrap();

    // Create Main with wildcard import
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.model.*;
            public class Main {
                public void test() {
                    User.Builder builder = new User.Builder();
                }
            }
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // Main imports com.example.model.*, so it's a dependent of User via wildcard
    let user_dependents = index.get_dependent_files("com.example.model.User");
    assert!(user_dependents.contains(&main_id));

    // Main should also be dependent of User.Builder (via wildcard import of outer class)
    let builder_dependents = index.get_dependent_files("com.example.model.User.Builder");
    assert!(
        builder_dependents.contains(&main_id),
        "Main.java should be a dependent of nested class via wildcard import of package"
    );
}

#[tokio::test]
async fn test_reverse_deps_nested_class_cleanup_on_reindex() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create User with nested Builder
    let user_path = root.join("User.java");
    std::fs::write(
        &user_path,
        r#"
            package com.example;
            public class User {
                public static class Builder {}
            }
        "#,
    )
    .unwrap();

    // Create Product (initially not imported)
    let product_path = root.join("Product.java");
    std::fs::write(
        &product_path,
        r#"
            package com.example;
            public class Product {
                public static class Builder {}
            }
        "#,
    )
    .unwrap();

    // Create Main that initially imports User
    let main_path = root.join("Main.java");
    std::fs::write(
        &main_path,
        r#"
            package com.example;
            import com.example.User;
            public class Main {}
        "#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_path).await.unwrap();
    index.index_file(&product_path).await.unwrap();
    index.index_file(&main_path).await.unwrap();

    let main_id = index.get_file_id(&main_path).unwrap();

    // Verify Main is dependent on User.Builder
    let user_builder_deps_before = index.get_dependent_files("com.example.User.Builder");
    assert!(
        user_builder_deps_before.contains(&main_id),
        "Main should be dependent on User.Builder initially"
    );

    // Main should NOT be dependent on Product.Builder (not imported)
    let product_builder_deps_before = index.get_dependent_files("com.example.Product.Builder");
    assert!(
        !product_builder_deps_before.contains(&main_id),
        "Main should NOT be dependent on Product.Builder initially"
    );

    // Now change Main to import Product instead of User
    let new_main_content = r#"
            package com.example;
            import com.example.Product;
            public class Main {}
        "#;

    index
        .index_file_with_source_and_id(&main_path, new_main_content, main_id)
        .unwrap();

    // Verify Main is NO LONGER dependent on User.Builder
    let user_builder_deps_after = index.get_dependent_files("com.example.User.Builder");
    assert!(
        !user_builder_deps_after.contains(&main_id),
        "Main should NOT be dependent on User.Builder after import change (nested class cleanup)"
    );

    // Verify Main IS NOW dependent on Product.Builder
    let product_builder_deps_after = index.get_dependent_files("com.example.Product.Builder");
    assert!(
        product_builder_deps_after.contains(&main_id),
        "Main should be dependent on Product.Builder after import change"
    );
}

/// Test that cache invalidation is granular - only affected entries are cleared
///
/// Verifies:
/// 1. Unrelated cache entries are preserved when a file changes
/// 2. Only entries for changed FQNs are invalidated
/// 3. Cache effectiveness is maintained across file changes
///
/// Before: Change User.java → clear ALL 2000 cache entries
/// After: Change User.java → invalidate only User-related entries (~10-50)
#[tokio::test]
async fn test_granular_cache_invalidation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create two independent classes
    let user_file = root.join("User.java");
    std::fs::write(
        &user_file,
        r#"
package com.example;

public class User {
    public String getName() {
        return "Alice";
    }
}
"#,
    )
    .unwrap();

    let product_file = root.join("Product.java");
    std::fs::write(
        &product_file,
        r#"
package com.example;

public class Product {
    public Integer getPrice() {
        return 100;
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&user_file).await.unwrap();
    index.index_file(&product_file).await.unwrap();

    // Populate cache for both User.getName and Product.getPrice
    let user_name_type =
        index.get_or_cache_method_resolution("com.example.User", "getName", || {
            index
                .get_symbol("com.example.User.getName")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });
    assert_eq!(
        user_name_type,
        Some("java.lang.String".into()),
        "User.getName() returns String"
    );

    let product_price_type =
        index.get_or_cache_method_resolution("com.example.Product", "getPrice", || {
            index
                .get_symbol("com.example.Product.getPrice")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });
    assert_eq!(
        product_price_type,
        Some("java.lang.Integer".into()),
        "Product.getPrice() returns Integer"
    );

    // Modify User.java ONLY
    std::fs::write(
        &user_file,
        r#"
package com.example;

public class User {
    public Integer getName() {
        return 42;
    }
}
"#,
    )
    .unwrap();

    index.index_file(&user_file).await.unwrap();

    // Verify User.getName cache entry was invalidated (sees new return type)
    let updated_user_type =
        index.get_or_cache_method_resolution("com.example.User", "getName", || {
            index
                .get_symbol("com.example.User.getName")
                .and_then(|s| s.return_type.as_ref().map(|t| t.to_string()))
        });
    assert_eq!(
        updated_user_type,
        Some("java.lang.Integer".into()),
        "User.getName() cache was invalidated - now returns Integer"
    );

    // CRITICAL: Verify Product.getPrice cache entry was PRESERVED (not cleared)
    // If granular invalidation works, the lookup function should NOT execute
    let mut product_lookup_executed = false;
    let cached_product_type =
        index.get_or_cache_method_resolution("com.example.Product", "getPrice", || {
            product_lookup_executed = true;
            Some("STALE_VALUE".into())
        });

    assert_eq!(
        cached_product_type,
        Some("java.lang.Integer".into()),
        "Product.getPrice() cache should be preserved"
    );
    assert!(
        !product_lookup_executed,
        "Product.getPrice() cache should NOT be cleared when User.java changes"
    );
}

/// Test that package index provides O(1) wildcard import lookup
#[tokio::test]
async fn test_package_index_wildcard_lookup() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    let user_file = root.join("com/example/User.java");
    std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    std::fs::write(
        &user_file,
        r#"
package com.example;

public class User {
    public String name;
}
"#,
    )
    .unwrap();

    let main_file = root.join("com/app/Main.java");
    std::fs::create_dir_all(main_file.parent().unwrap()).unwrap();
    std::fs::write(
        &main_file,
        r#"
package com.app;

import com.example.*;

public class Main {
    public void test() {
        User user = new User();
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.to_path_buf());
    index.index_file(&user_file).await.unwrap();
    index.index_file(&main_file).await.unwrap();

    // get_dependent_files should find Main.java via wildcard import
    let dependents = index.get_dependent_files("com.example.User");
    assert_eq!(
        dependents.len(),
        1,
        "Should find 1 dependent file via wildcard import"
    );

    // Verify it's using package index by checking the file is actually Main.java
    let main_id = index.get_file_id(&main_file).unwrap();
    assert!(
        dependents.contains(&main_id),
        "Dependents should include Main.java (via com.example.* wildcard)"
    );
}

/// Test package index cleanup when wildcard imports are removed
#[tokio::test]
async fn test_package_index_cleanup_on_reimport() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    let user_file = root.join("com/example/User.java");
    std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    std::fs::write(
        &user_file,
        r#"
package com.example;

public class User {}
"#,
    )
    .unwrap();

    let main_file = root.join("com/app/Main.java");
    std::fs::create_dir_all(main_file.parent().unwrap()).unwrap();
    std::fs::write(
        &main_file,
        r#"
package com.app;

import com.example.*;

public class Main {}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.to_path_buf());
    index.index_file(&user_file).await.unwrap();
    index.index_file(&main_file).await.unwrap();

    // Verify wildcard import is tracked
    let dependents = index.get_dependent_files("com.example.User");
    assert_eq!(dependents.len(), 1);

    // Remove wildcard import from Main.java
    std::fs::write(
        &main_file,
        r#"
package com.app;

import com.example.User;

public class Main {}
"#,
    )
    .unwrap();

    index.index_file(&main_file).await.unwrap();

    // Should still find Main.java via explicit import
    let dependents_after = index.get_dependent_files("com.example.User");
    assert_eq!(
        dependents_after.len(),
        1,
        "Should still find Main.java via explicit import"
    );

    // Verify package index was cleaned up (no longer in wildcard index)
    // This is implicitly tested - if cleanup didn't work, we might get duplicates
    let main_id = index.get_file_id(&main_file).unwrap();
    assert!(dependents_after.contains(&main_id));
}

/// H3 FIX: Test that parse tree cache respects size limits
/// Bug scenario: Large files cause unbounded memory growth with entry-based LRU
#[tokio::test]
async fn test_h3_parse_tree_cache_size_limit() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root.clone());

    // Create 10 large files (10KB each = 100KB total source)
    // With tree overhead (~2x source size), this would be ~300KB total
    // Since parse tree cache limit is 50MB, all should fit
    for i in 0..10 {
        let file = root.join(format!("LargeFile{}.java", i));
        let mut content = format!("package com.example;\n\npublic class LargeFile{} {{\n", i);

        // Add many methods to make file large (~10KB)
        for method_num in 0..100 {
            content.push_str(&format!(
                "    public void method{}() {{ System.out.println(\"Method {}\"); }}\n",
                method_num, method_num
            ));
        }
        content.push_str("}\n");

        std::fs::write(&file, &content).unwrap();
        index.index_file(&file).await.unwrap();
    }

    // All 10 files should be cached (well under 50MB limit)
    // We can't directly inspect cache size, but we can verify no panics/errors
    // In practice, 100KB source + 200KB tree overhead = ~300KB << 50MB
    // Test passes if no OOM or errors occur
}

#[test]
fn test_h3_estimate_entry_size_is_proportional_to_source() {
    let source = "package com.example;\n\npublic class Example {\n    void run() {\n        if (true) {\n            System.out.println(\"x\");\n        }\n    }\n}\n";

    let tokens: Vec<_> = hyperion_parser::Lexer::new(source).collect();
    let (tree, _errors) = hyperion_parser::Parser::parse(tokens);

    let tree_arc = Arc::new(tree);
    let source_arc = Arc::new(source.to_string());
    let estimated = ParseTreeCache::estimate_entry_size(&tree_arc, &source_arc);

    // Heuristic: entry size should be ~5x source length (source + 4x tree overhead)
    assert_eq!(
        estimated,
        source.len() * 5,
        "estimate_entry_size should be 5x source length"
    );
}

/// H3 FIX: Test that parse tree cache evicts the least-recent entry when its
/// byte limit is exceeded. Keep the cache deliberately tiny so the test
/// exercises the production eviction branch without expensive file indexing.
#[test]
fn test_h3_parse_tree_cache_eviction() {
    fn parse(source: &str) -> Arc<rowan::GreenNode> {
        let tokens = hyperion_parser::Lexer::new(source).collect();
        let (tree, errors) = hyperion_parser::Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        Arc::new(tree)
    }

    let first_source = Arc::new("class First {}".to_string());
    let second_source = Arc::new("class Second {}".to_string());
    let first_size = first_source.len() * 5;
    let second_size = second_source.len() * 5;
    let mut cache = ParseTreeCache::new(first_size.max(second_size));
    let first_id = FileId::new(1);
    let second_id = FileId::new(2);

    cache.put(first_id, parse(&first_source), Arc::clone(&first_source));
    cache.put(second_id, parse(&second_source), Arc::clone(&second_source));

    assert_eq!(cache.len(), 1);
    assert_eq!(cache.total_bytes(), second_size);
    assert!(cache.peek(&first_id).is_none());
    assert_eq!(
        cache.peek(&second_id).map(|(_, source)| source.as_str()),
        Some(second_source.as_str())
    );
}

/// H5 FIX: Test that method resolution cache is invalidated when parent class changes
/// Bug scenario: Parent adds method → Child.method() cache entry becomes stale
#[tokio::test]
async fn test_h5_method_cache_invalidated_on_parent_change() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let parent_file = root.join("Parent.java");
    let child_file = root.join("Child.java");

    // Initial state: Parent has no foo() method
    std::fs::write(
        &parent_file,
        r#"
package com.example;
public class Parent {
    public void bar() {}
}
"#,
    )
    .unwrap();

    std::fs::write(
        &child_file,
        r#"
package com.example;
public class Child extends Parent {
    public void test() {
        foo();  // This will not resolve
    }
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&parent_file).await.unwrap();
    index.index_file(&child_file).await.unwrap();

    // Simulate cache lookup: Child.foo() → None (not found)
    let cache_key = ("com.example.Child".to_string(), "foo".to_string());
    {
        let mut cache = index.method_resolution_cache.lock();
        cache.put(cache_key.clone(), None);
    }

    // Verify cache has the entry
    {
        let cache = index.method_resolution_cache.lock();
        assert_eq!(
            cache.peek(&cache_key),
            Some(&None),
            "Cache should have None for Child.foo()"
        );
    }

    // Parent class changes: add foo() method
    std::fs::write(
        &parent_file,
        r#"
package com.example;
public class Parent {
    public void bar() {}
    public void foo() {}  // NEW METHOD
}
"#,
    )
    .unwrap();

    // Re-index parent file
    index.index_file(&parent_file).await.unwrap();

    // H5 FIX: Cache entry for Child.foo() should be invalidated
    // because Parent changed (subclass caches depend on parent)
    {
        let cache = index.method_resolution_cache.lock();
        assert_eq!(
            cache.peek(&cache_key),
            None,
            "Cache entry for Child.foo() should be invalidated when Parent changes"
        );
    }
}

/// H5 FIX: Test deep inheritance hierarchy invalidation (10 levels)
/// Verifies that all descendants are invalidated when ancestor changes
#[tokio::test]
async fn test_h5_deep_inheritance_cache_invalidation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create 10-level inheritance chain: Level0 → Level1 → ... → Level9
    for i in 0..10 {
        let file = root.join(format!("Level{}.java", i));
        let parent_extends = if i == 0 {
            String::new()
        } else {
            format!(" extends Level{}", i - 1)
        };

        std::fs::write(
            &file,
            format!(
                r#"
package com.example;
public class Level{}{} {{
    public void method{}() {{}}
}}
"#,
                i, parent_extends, i
            ),
        )
        .unwrap();
    }

    let index = WorkspaceIndex::new(root.clone());

    // Index all files
    for i in 0..10 {
        let file = root.join(format!("Level{}.java", i));
        index.index_file(&file).await.unwrap();
    }

    // Populate cache with entries for all levels
    let mut cache_keys = Vec::new();
    {
        let mut cache = index.method_resolution_cache.lock();
        for i in 0..10 {
            let key = (format!("com.example.Level{}", i), "testMethod".to_string());
            cache.put(key.clone(), None);
            cache_keys.push(key);
        }
    }

    // Verify all cache entries exist
    {
        let cache = index.method_resolution_cache.lock();
        for key in &cache_keys {
            assert!(
                cache.peek(key).is_some(),
                "Cache should have entry for {:?}",
                key
            );
        }
    }

    // Change Level0 (root of hierarchy)
    let level0_file = root.join("Level0.java");
    std::fs::write(
        &level0_file,
        r#"
package com.example;
public class Level0 {
    public void method0() {}
    public void newRootMethod() {}  // NEW METHOD
}
"#,
    )
    .unwrap();

    // Re-index Level0
    index.index_file(&level0_file).await.unwrap();

    // H5 FIX: ALL descendants (Level1...Level9) should have cache invalidated
    {
        let cache = index.method_resolution_cache.lock();
        for (i, key) in cache_keys.iter().enumerate() {
            assert_eq!(
                cache.peek(key),
                None,
                "Cache entry for Level{} should be invalidated when Level0 changes",
                i
            );
        }
    }
}

/// H5 FIX: Test that sibling classes are NOT invalidated (only descendants)
#[tokio::test]
async fn test_h5_sibling_cache_not_invalidated() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let parent_file = root.join("Parent.java");
    let child1_file = root.join("Child1.java");
    let child2_file = root.join("Child2.java");

    std::fs::write(
        &parent_file,
        r#"
package com.example;
public class Parent {
    public void parentMethod() {}
}
"#,
    )
    .unwrap();

    std::fs::write(
        &child1_file,
        r#"
package com.example;
public class Child1 extends Parent {
    public void child1Method() {}
}
"#,
    )
    .unwrap();

    std::fs::write(
        &child2_file,
        r#"
package com.example;
public class Child2 extends Parent {
    public void child2Method() {}
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&parent_file).await.unwrap();
    index.index_file(&child1_file).await.unwrap();
    index.index_file(&child2_file).await.unwrap();

    // Add cache entries for both children
    let child1_key = ("com.example.Child1".to_string(), "test".to_string());
    let child2_key = ("com.example.Child2".to_string(), "test".to_string());

    {
        let mut cache = index.method_resolution_cache.lock();
        cache.put(child1_key.clone(), None);
        cache.put(child2_key.clone(), None);
    }

    // Change Child1 (NOT Parent)
    std::fs::write(
        &child1_file,
        r#"
package com.example;
public class Child1 extends Parent {
    public void child1Method() {}
    public void newMethod() {}  // NEW METHOD
}
"#,
    )
    .unwrap();

    // Re-index Child1
    index.index_file(&child1_file).await.unwrap();

    // Child1 cache should be invalidated, but Child2 should NOT be
    {
        let cache = index.method_resolution_cache.lock();
        assert_eq!(
            cache.peek(&child1_key),
            None,
            "Child1 cache should be invalidated (it changed)"
        );
        assert_eq!(
            cache.peek(&child2_key),
            Some(&None),
            "Child2 cache should NOT be invalidated (sibling, not descendant)"
        );
    }
}

// M23: FileId collision coverage includes platform-specific path semantics.
// On Windows, differently-cased paths may refer to the same file; on Unix,
// they remain distinct. The tests below exercise collision handling without
// assuming one platform's path behavior.
// ==================== FileId Collision Tests ====================
// These tests verify that FileId collisions are handled correctly.
// When an external allocator (like FileIdRegistry) assigns a FileId that's
// already in use by WorkspaceIndex, the collision must be resolved without
// losing symbols from the displaced file.

#[test]
fn automatic_file_ids_skip_caller_owned_ids() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let first_path = root.join("First.java");
    let caller_owned_path = root.join("CallerOwned.java");
    let automatic_path = root.join("Automatic.java");
    let index = WorkspaceIndex::new(root);

    index
        .index_file_with_source(&first_path, "class First {}")
        .unwrap();
    assert_eq!(index.get_file_id(&first_path), Some(FileId::new(1)));

    // The workspace allocator is about to issue FileId(2), while an editor's
    // registry has independently assigned that identity to another file.
    index
        .index_file_with_source_and_id(
            &caller_owned_path,
            "class CallerOwned { int retained; }",
            FileId::new(2),
        )
        .unwrap();

    index
        .index_file_with_source(&automatic_path, "class Automatic {}")
        .unwrap();

    assert_eq!(index.get_file_id(&caller_owned_path), Some(FileId::new(2)));
    assert_eq!(index.get_file_id(&automatic_path), Some(FileId::new(3)));
    assert!(index.find_symbol("CallerOwned").is_some());
    assert!(index.find_symbol("CallerOwned.retained").is_some());
    assert!(index.find_symbol("Automatic").is_some());
    assert!(index
        .get_file_contents(FileId::new(2))
        .is_some_and(|source| source.contains("class CallerOwned")));
}

#[test]
fn external_file_ids_skip_caller_owned_workspace_ids() {
    let temp = TempDir::new().unwrap();
    let workspace_path = temp.path().join("Workspace.java");
    let workspace_id = FileId::new(u32::MAX);
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source_and_id(
            &workspace_path,
            "class Workspace { int retained; }",
            workspace_id,
        )
        .unwrap();

    let dependency = PathBuf::from("/deps/library.jar");
    index.replace_external_sources("maven:/workspace", std::slice::from_ref(&dependency));
    let dependency_id = index
        .inner
        .read()
        .external_file_id_for_path(&dependency)
        .expect("registered dependency");
    assert_ne!(dependency_id, workspace_id);

    index.insert_symbols_batch(
        vec![synthetic_type(
            "dependency.Library",
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        dependency_id,
    );

    assert_eq!(index.get_file_id(&workspace_path), Some(workspace_id));
    assert!(index.find_symbol("Workspace").is_some());
    assert!(index.find_symbol("Workspace.retained").is_some());
    assert!(index.find_symbol("dependency.Library").is_some());
    assert!(!index.is_external_file(workspace_id));
}

#[test]
fn delayed_content_read_cannot_resurrect_a_removed_file() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Removed.java");
    let source = "class Removed { int stale; }";
    std::fs::write(&path, source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    index.clear_file_cache();

    let read_finished = Arc::new(std::sync::Barrier::new(2));
    let allow_cache_fill = Arc::new(std::sync::Barrier::new(2));
    let reader = {
        let index = index.clone();
        let read_finished = Arc::clone(&read_finished);
        let allow_cache_fill = Arc::clone(&allow_cache_fill);
        std::thread::spawn(move || {
            index.get_file_contents_after_read_for_test(file_id, || {
                read_finished.wait();
                allow_cache_fill.wait();
            })
        })
    };

    read_finished.wait();
    index.remove_file(file_id);
    allow_cache_fill.wait();

    assert!(reader.join().unwrap().is_none());
    assert!(index.get_file_contents(file_id).is_none());
    assert!(!index.inner.read().file_content_cache.contains(&file_id));
}

#[test]
fn exact_content_cache_refill_is_safe_without_a_semantic_write_transaction() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Restored.java");
    let source = "class Restored {}";
    std::fs::write(&path, source).unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.require_exclusive_writes();
    let revision = {
        let _writer = index.begin_exclusive_writes();
        index.index_file_with_source(&path, source).unwrap();
        let file_id = index.get_file_id(&path).unwrap();
        index.clear_file_cache();
        index.write_inner().parse_tree_cache.remove(&file_id);
        index.current_file_revision(file_id).unwrap()
    };
    let workspace_revision = index.inner.read().workspace_revision;
    let content = index.get_indexed_revision_content(revision).unwrap();
    assert_eq!(content.content.as_str(), source);
    assert_eq!(content.revision, revision);
    assert_eq!(index.inner.read().workspace_revision, workspace_revision);
    assert_eq!(
        index
            .get_indexed_revision_content(revision)
            .unwrap()
            .content
            .as_str(),
        source
    );
    assert_eq!(
        index.current_file_revision(revision.file_id()),
        Some(revision)
    );
    assert!(index.find_symbol("Restored").is_some());

    // Cache fill still cannot publish changed disk bytes as that revision.
    {
        let _writer = index.begin_exclusive_writes();
        index.clear_file_cache();
        index
            .write_inner()
            .parse_tree_cache
            .remove(&revision.file_id());
    }
    std::fs::write(&path, "class ChangedOnDisk {}").unwrap();
    assert!(index.get_indexed_revision_content(revision).is_none());
    assert!(index.find_symbol("ChangedOnDisk").is_none());
    assert_eq!(
        index.current_file_revision(revision.file_id()),
        Some(revision)
    );
}

#[test]
fn delayed_exact_disk_refill_cannot_cross_a_newer_publication() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Revision.kt");
    let disk_source = "package app\nclass DiskRevision";
    let editor_source = "package app\nclass EditorRevision";
    std::fs::write(&path, disk_source).unwrap();

    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index.index_file_sync_for_test(&path).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let disk_hash = index.get_content_hash(file_id).unwrap();
    index.clear_file_cache();

    let read_finished = Arc::new(std::sync::Barrier::new(2));
    let allow_cache_fill = Arc::new(std::sync::Barrier::new(2));
    let reader = {
        let index = Arc::clone(&index);
        let read_finished = Arc::clone(&read_finished);
        let allow_cache_fill = Arc::clone(&allow_cache_fill);
        std::thread::spawn(move || {
            index.get_indexed_file_contents_after_read_for_test(file_id, disk_hash, || {
                read_finished.wait();
                allow_cache_fill.wait();
            })
        })
    };

    read_finished.wait();
    index
        .index_file_with_source_and_id(&path, editor_source, file_id)
        .unwrap();
    allow_cache_fill.wait();

    assert!(reader.join().unwrap().is_none());
    assert_eq!(
        index.get_file_contents(file_id).unwrap().as_str(),
        editor_source
    );
    assert_eq!(
        index.get_current_file_contents(file_id).unwrap().as_str(),
        disk_source
    );
}

#[test]
fn revisioned_ranges_reject_same_text_republication() {
    use std::sync::{Arc, Barrier};

    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Same.java");
    let source = "package app; class Same { void call() {} void use() { call(); } }";
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();

    let definition_r1 = index
        .find_symbol_revisioned("app.Same")
        .expect("revisioned definition");
    let method_ranges_r1 = index
        .get_file_resolved_method_usages_revisioned(file_id)
        .expect("revisioned method ranges");
    assert_eq!(definition_r1.revision, method_ranges_r1.revision);
    assert_eq!(
        index
            .get_indexed_revision_content(definition_r1.revision)
            .unwrap()
            .content
            .as_str(),
        source
    );

    let publish = Arc::new(Barrier::new(2));
    let writer = {
        let index = Arc::clone(&index);
        let path = path.clone();
        let publish = Arc::clone(&publish);
        std::thread::spawn(move || {
            publish.wait();
            index.index_file_with_source(&path, source).unwrap();
        })
    };
    publish.wait();
    writer.join().unwrap();

    let definition_r2 = index
        .find_symbol_revisioned("app.Same")
        .expect("republished definition");
    assert_ne!(definition_r1.revision, definition_r2.revision);
    assert_eq!(
        index.get_content_hash(file_id),
        Some(super::compute_content_hash(source)),
        "identical bytes deliberately retain their compatibility hash"
    );
    assert!(
        index
            .get_indexed_revision_content(definition_r1.revision)
            .is_none(),
        "R1 ranges must never be paired with R2 content, even for equal bytes"
    );
    assert_eq!(
        index
            .get_indexed_revision_content(definition_r2.revision)
            .unwrap()
            .file_path,
        path
    );
}

#[test]
fn revisioned_usage_and_search_results_group_tokens_per_file() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target = temp.path().join("Target.java");
    let first = temp.path().join("First.java");
    let second = temp.path().join("Second.java");
    index
        .index_file_with_source(&target, "package app; public class Target {}")
        .unwrap();
    index
        .index_file_with_source(
            &first,
            "package app; class First { Target one; Target two; }",
        )
        .unwrap();
    index
        .index_file_with_source(&second, "package app; class Second { Target value; }")
        .unwrap();

    let usages = index.find_current_usages("app.Target").unwrap();
    assert_eq!(usages.files.len(), 2);
    assert_eq!(
        usages
            .files
            .iter()
            .map(|file| file.locations.len())
            .sum::<usize>(),
        3
    );
    for file in &usages.files {
        assert!(file
            .locations
            .iter()
            .all(|location| location.file_id == file.revision.file_id()));
        assert!(index.get_indexed_revision_content(file.revision).is_some());
    }
    assert!(index.current_usage_locations_are_current(&usages));

    let search = index.search_revisioned("Target");
    assert_eq!(search.files.len(), 1);
    assert_eq!(search.files[0].locations.len(), 1);
    assert_eq!(
        search.files[0].revision,
        index
            .current_file_revision(search.files[0].locations[0].file_id)
            .unwrap()
    );
}

#[test]
fn current_usages_rebind_same_package_and_reject_wildcard_ambiguity() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let a_target = temp.path().join("a/Target.java");
    let p_target = temp.path().join("p/Target.java");
    let r_target = temp.path().join("r/Target.java");
    let use_path = temp.path().join("p/Use.java");
    for path in [&a_target, &p_target, &r_target, &use_path] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    index
        .index_file_with_source(&a_target, "package a; public class Target {}")
        .unwrap();
    index
        .index_file_with_source(
            &use_path,
            "package p; import a.*; class Use { Target value; }",
        )
        .unwrap();
    assert_eq!(
        index
            .find_current_usages("a.Target")
            .unwrap()
            .locations()
            .count(),
        1
    );

    index
        .index_file_with_source(&p_target, "package p; public class Target {}")
        .unwrap();
    assert!(index
        .find_current_usages("a.Target")
        .unwrap()
        .files
        .is_empty());
    assert_eq!(
        index
            .find_current_usages("p.Target")
            .unwrap()
            .locations()
            .count(),
        1
    );

    index.remove_file(index.get_file_id(&p_target).unwrap());
    index
        .index_file_with_source(
            &use_path,
            "package p; import a.*; import r.*; class Use { Target value; }",
        )
        .unwrap();
    index
        .index_file_with_source(&r_target, "package r; public class Target {}")
        .unwrap();
    assert!(index
        .find_current_usages("a.Target")
        .unwrap()
        .files
        .is_empty());
    assert!(index
        .find_current_usages("r.Target")
        .unwrap()
        .files
        .is_empty());
}

#[test]
fn current_usage_query_retries_after_concurrent_rebind_and_same_text_publication() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let a_target = temp.path().join("a/Target.java");
    let p_target = temp.path().join("p/Target.java");
    let use_path = temp.path().join("p/Use.java");
    std::fs::create_dir_all(a_target.parent().unwrap()).unwrap();
    std::fs::create_dir_all(p_target.parent().unwrap()).unwrap();
    let use_source = "package p; import a.*; class Use { Target value; }";
    index
        .index_file_with_source(&a_target, "package a; public class Target {}")
        .unwrap();
    index.index_file_with_source(&use_path, use_source).unwrap();
    let use_r1 = index
        .current_file_revision(index.get_file_id(&use_path).unwrap())
        .unwrap();

    let rebound = index
        .find_current_usages_with_first_attempt_hook_for_test("a.Target", || {
            index
                .index_file_with_source(&p_target, "package p; public class Target {}")
                .unwrap();
        })
        .unwrap();
    assert!(
        rebound.files.is_empty(),
        "retry must observe the same-package rebind"
    );

    index.remove_file(index.get_file_id(&p_target).unwrap());
    let same_text = index
        .find_current_usages_with_first_attempt_hook_for_test("a.Target", || {
            index.index_file_with_source(&use_path, use_source).unwrap();
        })
        .unwrap();
    let use_r2 = same_text
        .files
        .iter()
        .find(|file| file.revision.file_id() == use_r1.file_id())
        .expect("retried use-file result")
        .revision;
    assert_ne!(use_r1, use_r2);
    assert!(index.get_indexed_revision_content(use_r1).is_none());
    assert!(index.current_usage_locations_are_current(&same_text));
}

#[test]
fn cache_restore_re_resolves_cross_language_usages_from_current_bytes() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("p/Target.java");
    let java_use = temp.path().join("p/JavaUse.java");
    let kotlin_use = temp.path().join("p/KotlinUse.kt");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "package p; public class Target {}").unwrap();
    std::fs::write(&java_use, "package p; class JavaUse { Target value; }").unwrap();
    std::fs::write(&kotlin_use, "package p\nclass KotlinUse(val value: Target)").unwrap();

    let initial = WorkspaceIndex::new(temp.path().to_path_buf());
    initial.index_file_sync_for_test(&target).unwrap();
    initial.index_file_sync_for_test(&java_use).unwrap();
    initial.index_file_sync_for_test(&kotlin_use).unwrap();
    let cache = initial.extract_cache_data();

    let restored = WorkspaceIndex::new(temp.path().to_path_buf());
    assert!(restored.restore_from_cache_batch(&cache).is_empty());
    let usages = restored.find_current_usages("p.Target").unwrap();
    let paths = usages
        .files
        .iter()
        .map(|file| restored.get_file_path(file.revision.file_id()).unwrap())
        .collect::<HashSet<_>>();
    assert!(paths.contains(&java_use));
    assert!(paths.contains(&kotlin_use));
    assert!(restored.current_usage_locations_are_current(&usages));
}

#[test]
fn source_revision_origin_tracks_disk_caller_relocation_and_removal() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Owned.java");
    let disk_source = "class DiskOwned {}";
    let caller_source = "class CallerOwned {}";
    std::fs::write(&path, disk_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&path).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let path_key = index.get_file_path_key(file_id).unwrap();
    let disk_token = index.current_file_revision(file_id).unwrap();
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    assert!(index.adopt_caller_owned_revision_with_key_and_locks_held(
        &path,
        &path_key,
        disk_source,
        file_id
    ));
    let caller_token = index.current_file_revision(file_id).unwrap();
    assert_ne!(disk_token, caller_token);
    assert!(index.get_indexed_revision_content(disk_token).is_none());
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );
    assert!(index.adopt_disk_revision_with_key_and_locks_held(
        &path,
        &path_key,
        disk_source,
        file_id
    ));
    let readopted_disk_token = index.current_file_revision(file_id).unwrap();
    assert_ne!(caller_token, readopted_disk_token);
    assert!(index.get_indexed_revision_content(caller_token).is_none());
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    assert!(!index.adopt_caller_owned_revision_with_key_and_locks_held(
        &path,
        &path_key,
        "class DifferentRevision {}",
        file_id
    ));
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    index
        .index_file_with_source_and_id(&path, caller_source, file_id)
        .unwrap();
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );

    assert!(index.adopt_disk_revision_with_key_and_locks_held(
        &path,
        &path_key,
        caller_source,
        file_id
    ));
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );
    index
        .index_file_with_source_and_id(&path, caller_source, file_id)
        .unwrap();

    let collision_path = temp.path().join("Collision.java");
    index
        .index_file_with_source_and_id(&collision_path, "class Collision {}", file_id)
        .unwrap();
    let relocated_id = index.get_file_id(&path).unwrap();
    assert_ne!(relocated_id, file_id);
    {
        let inner = index.inner.read();
        assert_eq!(
            inner.published_revisions[&relocated_id].origin,
            SourceRevisionOrigin::EditorOrCaller
        );
        assert_eq!(
            inner.published_revisions[&file_id].origin,
            SourceRevisionOrigin::EditorOrCaller
        );
    }

    index.remove_file(relocated_id);
    index.remove_file(file_id);
    assert!(index.inner.read().published_revisions.is_empty());
}

#[test]
fn origin_adoption_refuses_hash_only_proof_after_exact_bytes_are_evicted() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Owned.kt");
    let source = "class Owned";
    std::fs::write(&path, source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&path).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let path_key = index.get_file_path_key(file_id).unwrap();
    let disk_revision = index.current_file_revision(file_id).unwrap();
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    // Kotlin has no Java parse-tree source fallback. Once the bounded content
    // cache is empty, the hash alone cannot prove byte equality for authority
    // adoption (a collision must fail closed).
    index.clear_file_cache();
    assert!(!index
        .adopt_caller_owned_revision_with_key_and_locks_held(&path, &path_key, source, file_id));
    assert_eq!(index.current_file_revision(file_id), Some(disk_revision));
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );
}

#[test]
fn moved_revision_preserves_explicit_disk_or_caller_origin() {
    let temp = TempDir::new().unwrap();
    let disk_path = temp.path().join("Disk.java");
    let moved_disk_path = temp.path().join("MovedDisk.java");
    let moved_caller_path = temp.path().join("MovedCaller.java");
    let source = "class MovedRevision {}";
    std::fs::write(&disk_path, source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&disk_path).unwrap();
    let file_id = index.get_file_id(&disk_path).unwrap();
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    std::fs::rename(&disk_path, &moved_disk_path).unwrap();
    {
        let old_key = index.get_file_path_key(file_id).unwrap();
        let old_path_lock = index.get_path_lock_for_key(&old_key);
        let (new_key, new_path_lock) = index.establish_path_lock(&moved_disk_path);
        let file_lock = index.get_file_lock(file_id);
        let _old_path_guard = old_path_lock.lock().unwrap();
        let _new_path_guard = new_path_lock.lock().unwrap();
        let _file_guard = file_lock.lock().unwrap();
        index
            .index_moved_disk_file_with_keys_and_locks_held(
                &disk_path,
                &old_key,
                &moved_disk_path,
                &new_key,
                source,
                file_id,
            )
            .unwrap();
    }
    assert_eq!(index.get_file_id(&moved_disk_path), Some(file_id));
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::Disk)
    );

    {
        let old_key = index.get_file_path_key(file_id).unwrap();
        let old_path_lock = index.get_path_lock_for_key(&old_key);
        let (new_key, new_path_lock) = index.establish_path_lock(&moved_caller_path);
        let file_lock = index.get_file_lock(file_id);
        let _old_path_guard = old_path_lock.lock().unwrap();
        let _new_path_guard = new_path_lock.lock().unwrap();
        let _file_guard = file_lock.lock().unwrap();
        index
            .index_moved_file_with_keys_and_locks_held(
                &moved_disk_path,
                &old_key,
                &moved_caller_path,
                &new_key,
                source,
                file_id,
            )
            .unwrap();
    }
    assert_eq!(index.get_file_id(&moved_caller_path), Some(file_id));
    assert_eq!(
        index.get_revision_origin(file_id),
        Some(SourceRevisionOrigin::EditorOrCaller)
    );
}

#[test]
fn automatic_reindex_reselects_an_id_relocated_while_it_waits() {
    use std::sync::{mpsc, Arc};

    let temp = TempDir::new().unwrap();
    let automatic_path = temp.path().join("Automatic.java");
    let editor_path = temp.path().join("Editor.java");
    let automatic_before = "package app; class AutomaticBefore {}";
    let automatic_after = "package app; class AutomaticAfter {}";
    let editor_source = "package app; class EditorRevision {}";

    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    index
        .index_file_with_source(&automatic_path, automatic_before)
        .unwrap();
    let caller_owned_id = index.get_file_id(&automatic_path).unwrap();

    let (selected_tx, selected_rx) = mpsc::sync_channel(0);
    let (continue_tx, continue_rx) = mpsc::sync_channel(0);
    let background_index = Arc::clone(&index);
    let background_path = automatic_path.clone();
    let background = std::thread::spawn(move || {
        background_index.index_file_with_source_after_id_selection_for_test(
            &background_path,
            automatic_after,
            |selected| {
                selected_tx.send(selected).unwrap();
                continue_rx.recv().unwrap();
            },
        )
    });

    assert_eq!(selected_rx.recv().unwrap(), caller_owned_id);
    index
        .index_file_with_source_and_id(&editor_path, editor_source, caller_owned_id)
        .unwrap();
    let relocated_automatic_id = index.get_file_id(&automatic_path).unwrap();
    assert_ne!(relocated_automatic_id, caller_owned_id);
    continue_tx.send(()).unwrap();
    background.join().unwrap().unwrap();

    assert_eq!(index.get_file_id(&editor_path), Some(caller_owned_id));
    assert_eq!(
        index.get_file_id(&automatic_path),
        Some(relocated_automatic_id)
    );
    assert!(index.find_symbol("app.EditorRevision").is_some());
    assert!(index.find_symbol("app.AutomaticAfter").is_some());
    assert!(index.find_symbol("app.AutomaticBefore").is_none());
}

#[test]
fn collision_relocates_legacy_usages_without_forward_counts() {
    let temp = TempDir::new().unwrap();
    let declaration_path = temp.path().join("Target.java");
    let source_path = temp.path().join("Source.java");
    let incoming_path = temp.path().join("Incoming.java");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    index
        .index_file_with_source(&declaration_path, "package app; class Target {}")
        .unwrap();
    index
        .index_file_with_source(&source_path, "package app; class Source { Target value; }")
        .unwrap();
    let displaced_id = index.get_file_id(&source_path).unwrap();
    assert!(index
        .find_usages("app.Target")
        .iter()
        .any(|usage| usage.file_id == displaced_id));

    index.inner.write().file_usage_counts.remove(&displaced_id);
    index
        .index_file_with_source_and_id(
            &incoming_path,
            "package app; class Incoming {}",
            displaced_id,
        )
        .unwrap();

    let relocated_id = index.get_file_id(&source_path).unwrap();
    assert_ne!(relocated_id, displaced_id);
    let usages = index.find_usages("app.Target");
    assert!(usages.iter().any(|usage| usage.file_id == relocated_id));
    assert!(usages.iter().all(|usage| usage.file_id != displaced_id));
}

/// Tests that imports are accessible regardless of which FileId is used to query.
///
/// When a file is re-indexed with a different FileId, its imports should be
/// updated to be accessible under the new FileId.
#[tokio::test]
async fn test_imports_accessible_after_reindex_with_different_fileid() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let file_path = root.join("Test.java");
    std::fs::write(
        &file_path,
        r#"
package com.example;

import java.util.List;

public class Test {
    List<String> items;
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());

    // Index with internal FileId allocation
    index.index_file(&file_path).await.unwrap();
    let internal_file_id = index.get_file_id(&file_path).unwrap();

    // Re-index with a specific external FileId
    let external_file_id = FileId::new(999);
    index
        .index_file_with_source_and_id(
            &file_path,
            r#"
package com.example;

import java.util.List;

public class Test {
    List<String> items;
}
"#,
            external_file_id,
        )
        .unwrap();

    // Imports should be accessible with the new FileId
    let imports = index.get_imports(external_file_id);
    assert!(
        imports.is_some(),
        "Imports should be stored under new FileId"
    );
    assert_eq!(
        imports.as_ref().unwrap().current_package,
        Some("com.example".into()),
        "current_package should be set"
    );
    assert_eq!(index.get_file_id(&file_path), Some(external_file_id));
    assert!(index.get_file_path(internal_file_id).is_none());
    assert!(!index.file_locks.contains_key(&internal_file_id));
    let inner = index.inner.read();
    assert!(inner.parse_tree_cache.peek(&internal_file_id).is_none());
}

/// A supertype imported via a wildcard (`import com.lib.*;`) used to
/// be qualified with the current package at extraction time, dead-ending the
/// inheritance chain. The chain walk must fall back to wildcard-package
/// candidates so inherited members remain visible.
#[tokio::test]
async fn test_wildcard_import_supertype_links_inheritance_chain() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let base_path = root.join("BaseRepo.java");
    std::fs::write(
        &base_path,
        "package com.lib;\npublic class BaseRepo {\n    public void save() {}\n}",
    )
    .unwrap();

    let repo_path = root.join("Repo.java");
    std::fs::write(
        &repo_path,
        "package com.app;\nimport com.lib.*;\npublic class Repo extends BaseRepo {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&base_path).await.unwrap();
    index.index_file(&repo_path).await.unwrap();

    let chain = index.get_inheritance_chain("com.app.Repo");
    assert!(
        chain.iter().any(|fqn| fqn == "com.lib.BaseRepo"),
        "inheritance chain should link to wildcard-imported parent, got: {:?}",
        chain
    );

    let methods = index.find_methods_by_name_with_inheritance("com.app.Repo", "save");
    assert!(
        methods
            .iter()
            .any(|m| m.container.as_deref() == Some("com.lib.BaseRepo")),
        "inherited method save() from com.lib.BaseRepo should be found, got: {:?}",
        methods.iter().map(|m| m.fqn.as_ref()).collect::<Vec<_>>()
    );

    assert_eq!(
        index
            .find_direct_subtypes("com.lib.BaseRepo")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["com.app.Repo"],
        "reverse hierarchy queries must use the same wildcard resolution as forward traversal"
    );

    index.get_or_cache_method_resolution("com.app.Repo", "save", || Some("stale".to_string()));
    index.invalidate_caches_for_fqns(&[Arc::from("com.lib.BaseRepo")]);
    assert_eq!(
        index.get_or_cache_method_resolution("com.app.Repo", "save", || Some("fresh".to_string())),
        Some("fresh".to_string()),
        "invalidating a wildcard-imported parent must invalidate its descendants"
    );
}

#[tokio::test]
async fn wildcard_reverse_edge_resolves_when_dependency_arrives_later() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let child = root.join("Child.java");
    let base = root.join("Base.java");
    std::fs::write(
        &child,
        "package app; import dependency.*; class Child extends Base {}",
    )
    .unwrap();
    std::fs::write(&base, "package dependency; public class Base {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&child).await.unwrap();
    assert!(index.find_direct_subtypes("dependency.Base").is_empty());

    index.index_file(&base).await.unwrap();
    assert_eq!(
        index
            .find_implementations("dependency.Base")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.Child"]
    );

    index.get_or_cache_method_resolution("app.Child", "run", || Some("stale".to_string()));
    let base_id = index.get_file_id(&base).unwrap();
    index.remove_file(base_id);
    assert!(index.find_direct_subtypes("dependency.Base").is_empty());
    assert_eq!(
        index.get_or_cache_method_resolution("app.Child", "run", || Some("fresh".to_string())),
        Some("fresh".to_string()),
        "removing a wildcard target must invalidate its former descendants"
    );

    index.index_file(&base).await.unwrap();
    assert_eq!(
        index
            .find_direct_subtypes("dependency.Base")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.Child"],
        "the unresolved edge must relink if the dependency is restored"
    );
}

/// A type in the current package shadows wildcard imports (JLS §6.4.1), so the
/// current-package parent must win when both exist.
#[tokio::test]
async fn test_current_package_supertype_shadows_wildcard_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let lib_base = root.join("LibBase.java");
    std::fs::write(&lib_base, "package com.lib;\npublic class Base {}").unwrap();

    let app_base = root.join("AppBase.java");
    std::fs::write(&app_base, "package com.app;\npublic class Base {}").unwrap();

    let child = root.join("Child.java");
    std::fs::write(
        &child,
        "package com.app;\nimport com.lib.*;\npublic class Child extends Base {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&lib_base).await.unwrap();
    index.index_file(&app_base).await.unwrap();
    index.index_file(&child).await.unwrap();

    let chain = index.get_inheritance_chain("com.app.Child");
    assert!(
        chain.iter().any(|fqn| fqn == "com.app.Base"),
        "current-package parent should be in chain, got: {:?}",
        chain
    );
    assert!(
        !chain.iter().any(|fqn| fqn == "com.lib.Base"),
        "wildcard candidate must not override current-package parent, got: {:?}",
        chain
    );
    assert_eq!(
        index
            .find_direct_subtypes("com.app.Base")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["com.app.Child"]
    );
    assert!(index.find_direct_subtypes("com.lib.Base").is_empty());
}

#[tokio::test]
async fn path_identity_follows_the_real_workspace_volume() {
    use std::fs::OpenOptions;
    use std::io::Write;

    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a file with specific casing
    let original_path = root.join("TestFile.java");
    std::fs::write(
        &original_path,
        "package com.example;\npublic class TestFile {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&original_path).await.unwrap();

    let original_id = index.get_file_id(&original_path).unwrap();

    // Try different casing variants
    let lowercase_path = root.join("testfile.java");
    let uppercase_path = root.join("TESTFILE.JAVA");
    let mixed_path = root.join("tEsTfIlE.jAvA");

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lowercase_path)
    {
        Ok(mut lowercase_file) => {
            lowercase_file
                .write_all(b"package com.example; public class LowercaseFile {}")
                .unwrap();
            drop(lowercase_file);
            index.index_file(&lowercase_path).await.unwrap();

            assert_ne!(
                original_id,
                index.get_file_id(&lowercase_path).unwrap(),
                "a volume that creates both spellings must preserve both identities"
            );
            assert_eq!(index.get_file_id(&uppercase_path), None);
            assert_eq!(index.get_file_id(&mixed_path), None);
        }
        Err(error) => {
            assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
            assert_eq!(index.get_file_id(&lowercase_path), Some(original_id));
            assert_eq!(index.get_file_id(&uppercase_path), Some(original_id));
            assert_eq!(index.get_file_id(&mixed_path), Some(original_id));
        }
    }
}

#[test]
fn case_only_move_rebinds_display_path_when_lookup_policy_is_insensitive() {
    let temp = TempDir::new().unwrap();
    let old_path = temp.path().join("CaseOnly.java");
    let new_path = temp.path().join("caseonly.java");
    let stale_destination = temp.path().join("CASEONLY.java");
    if path_lookup_key(&old_path) != path_lookup_key(&new_path) {
        return;
    }
    let source = "class CaseOnly {}";
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_with_source(&old_path, source).unwrap();
    let file_id = index.get_file_id(&old_path).unwrap();
    let old_revision = index.current_file_revision(file_id).unwrap();

    {
        // Both spellings share one normalized path lock; acquiring it twice
        // would deadlock on case-insensitive platforms.
        let old_key = index.get_file_path_key(file_id).unwrap();
        let (new_key, path_lock) = index.establish_path_lock(&new_path);
        assert_eq!(old_key, new_key);
        let path_guard = path_lock.lock().unwrap();
        let file_lock = index.get_file_lock(file_id);
        let file_guard = file_lock.lock().unwrap();
        index
            .index_moved_file_with_keys_and_locks_held(
                &old_path, &old_key, &new_path, &new_key, source, file_id,
            )
            .unwrap();
        drop(file_guard);
        drop(file_lock);
        drop(path_guard);
        drop(path_lock);
    }

    let new_revision = index.current_file_revision(file_id).unwrap();
    assert_ne!(old_revision, new_revision);
    assert!(index.get_indexed_revision_content(old_revision).is_none());
    assert_eq!(
        index
            .get_indexed_revision_content(new_revision)
            .unwrap()
            .file_path,
        new_path
    );

    let old_key = index.get_file_path_key(file_id).unwrap();
    let old_path_lock = index.get_path_lock_for_key(&old_key);
    let old_path_guard = old_path_lock.lock().unwrap();
    let (destination_key, destination_lock) = index.establish_path_lock(&stale_destination);
    let destination_guard = destination_lock.lock().unwrap();
    let file_lock = index.get_file_lock(file_id);
    let file_guard = file_lock.lock().unwrap();
    let stale = index.index_moved_file_with_keys_and_locks_held(
        &old_path,
        &old_key,
        &stale_destination,
        &destination_key,
        source,
        file_id,
    );
    assert!(
        stale.is_err(),
        "old display casing must not name the new revision"
    );
    drop(file_guard);
    drop(file_lock);
    drop(destination_guard);
    drop(destination_lock);
    drop(old_path_guard);
    drop(old_path_lock);
    assert_eq!(index.get_file_path(file_id), Some(new_path));
    assert_eq!(index.current_file_revision(file_id), Some(new_revision));
}

#[test]
fn cache_export_preserves_the_authoritative_display_path() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("MixedCaseSource.java");
    std::fs::write(&path, "package com.example; class MixedCaseSource {}").unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file_sync_for_test(&path).unwrap();

    let cache = index.extract_cache_data();
    assert!(
        cache.contains_key(&path),
        "normalized lookup keys must never replace filesystem/display paths in persisted data"
    );
}

#[cfg(unix)]
#[test]
fn workspace_containment_uses_resolved_root_and_file_identities() {
    let temp = TempDir::new().unwrap();
    let real_root = temp.path().join("real-root");
    let root_alias = temp.path().join("root-alias");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&real_root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&real_root, &root_alias).unwrap();
    std::os::unix::fs::symlink(&outside, real_root.join("escape")).unwrap();

    let inside_path = real_root.join("Inside.java");
    let escaped_path = root_alias.join("escape/Outside.java");
    let index = WorkspaceIndex::new(root_alias);
    index
        .index_file_with_source(&inside_path, "class Inside {}")
        .unwrap();
    index
        .index_file_with_source(&escaped_path, "class Outside {}")
        .unwrap();

    let inside_id = index.get_file_id(&inside_path).unwrap();
    let escaped_id = index.get_file_id(&escaped_path).unwrap();
    assert!(index.is_workspace_file(inside_id));
    assert!(
        !index.is_workspace_file(escaped_id),
        "a lexical child that resolves outside the workspace must fail containment"
    );
}

#[test]
fn test_cache_restore_populates_structural_indices() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    // Create files with inheritance, interfaces, nested classes, and sealed classes
    let base_path = root.join("com/example/Base.java");
    let child_path = root.join("com/example/Child.java");
    let iface_path = root.join("com/example/Printable.java");

    std::fs::create_dir_all(root.join("com/example")).unwrap();

    let base_source = r#"
        package com.example;
        public class Base {
            public class Inner {}
        }
    "#;
    let child_source = r#"
        package app;
        import com.example.*;
        public class Child extends Base implements Printable {
        }
    "#;
    let iface_source = r#"
        package com.example;
        public interface Printable {
        }
    "#;

    std::fs::write(&base_path, base_source).unwrap();
    std::fs::write(&child_path, child_source).unwrap();
    std::fs::write(&iface_path, iface_source).unwrap();

    // Phase 1: Fresh indexing
    let fresh_index = WorkspaceIndex::new(root.clone());
    fresh_index
        .index_file_with_source(&base_path, base_source)
        .unwrap();
    fresh_index
        .index_file_with_source(&child_path, child_source)
        .unwrap();
    fresh_index
        .index_file_with_source(&iface_path, iface_source)
        .unwrap();

    // Capture structural indices from fresh index
    let fresh_inner = fresh_index.inner.read();
    let fresh_nested = fresh_inner.nested_class_index.clone();
    let fresh_package_to_classes = fresh_inner.package_to_classes.clone();
    let fresh_inheritance = fresh_inner.inheritance.clone();
    drop(fresh_inner);

    // Phase 2: Extract cache, then restore into a new empty index
    let cache_data = fresh_index.extract_cache_data();
    assert!(!cache_data.is_empty(), "Cache data should not be empty");

    let restored_index = WorkspaceIndex::new(root);
    assert!(restored_index
        .restore_from_cache_batch(&cache_data)
        .is_empty());

    // Phase 3: Compare structural indices
    let restored_inner = restored_index.inner.read();

    // nested_class_index: Base → [Base.Inner]
    assert_eq!(
        restored_inner.nested_class_index, fresh_nested,
        "nested_class_index mismatch after cache restore"
    );

    // package_to_classes: com.example → [Base, Printable], app → [Child]
    assert_eq!(
        restored_inner.package_to_classes, fresh_package_to_classes,
        "package_to_classes mismatch after cache restore"
    );

    // inheritance: Base → [Child], Printable → [Child]
    assert_eq!(
        restored_inner.inheritance, fresh_inheritance,
        "inheritance graph mismatch after cache restore"
    );
    drop(restored_inner);

    assert_eq!(
        restored_index
            .find_implementations("com.example.Printable")
            .into_iter()
            .map(|symbol| symbol.fqn.to_string())
            .collect::<Vec<_>>(),
        vec!["app.Child"],
        "warm restore must rebuild wildcard-supertype candidates"
    );
}

#[test]
fn test_cache_restore_re_resolves_symbol_usages_without_blessing_cached_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let user_path = root.join("com/example/User.java");
    let main_path = root.join("com/example/Main.java");
    std::fs::create_dir_all(root.join("com/example")).unwrap();

    let user_source = r#"
        package com.example;
        public class User {
            public String name() { return "user"; }
        }
    "#;
    let main_source = r#"
        package com.example;
        import com.example.User;
        public class Main {
            User first = new User();
            User second = new User();
            String name = first.name();
        }
    "#;

    std::fs::write(&user_path, user_source).unwrap();
    std::fs::write(&main_path, main_source).unwrap();

    let fresh_index = WorkspaceIndex::new(root.clone());
    fresh_index
        .index_file_with_source(&user_path, user_source)
        .unwrap();
    fresh_index
        .index_file_with_source(&main_path, main_source)
        .unwrap();

    let normalize = |index: &WorkspaceIndex, usages: Vec<SymbolLocation>| {
        let mut normalized: Vec<(std::path::PathBuf, u32, u32, String)> = usages
            .into_iter()
            .map(|usage| {
                (
                    index
                        .get_file_path(usage.file_id)
                        .expect("usage should resolve back to a file path"),
                    usage.range.start,
                    usage.range.end,
                    usage.fqn.to_string(),
                )
            })
            .collect();
        normalized.sort();
        normalized
    };

    let fresh_usages = normalize(
        &fresh_index,
        fresh_index
            .find_current_usages("com.example.User")
            .unwrap()
            .locations()
            .cloned()
            .collect(),
    );
    assert!(
        !fresh_usages.is_empty(),
        "Expected at least one usage for com.example.User before cache extraction"
    );
    let fresh_main_id = fresh_index.get_file_id(&main_path).expect("fresh main id");
    let fresh_method_usages = fresh_index.get_file_resolved_method_usages(fresh_main_id);
    assert!(fresh_method_usages
        .iter()
        .any(|(_, fqn)| fqn.as_ref() == "com.example.User.name()"));

    let cache_data = fresh_index.extract_cache_data();
    let restored_index = WorkspaceIndex::new(root);
    assert!(restored_index
        .restore_from_cache_batch(&cache_data)
        .is_empty());

    assert!(restored_index.find_usages("com.example.User").is_empty());
    let restored_current = restored_index
        .find_current_usages("com.example.User")
        .unwrap();
    let restored_usages = normalize(
        &restored_index,
        restored_current.locations().cloned().collect(),
    );
    assert_eq!(
        restored_usages, fresh_usages,
        "current bytes should reproduce the same usage set after cache restore"
    );
    assert!(restored_index.current_usage_locations_are_current(&restored_current));
    let restored_main_id = restored_index
        .get_file_id(&main_path)
        .expect("restored main id");
    assert!(restored_index
        .get_file_resolved_method_usages_revisioned(restored_main_id)
        .is_none());
    let restored_indexed = restored_index
        .get_indexed_revision_content(
            restored_index
                .current_file_revision(restored_main_id)
                .unwrap(),
        )
        .unwrap();
    let restored_method_usages = restored_index
        .resolve_current_method_usages(&restored_indexed)
        .expect("resolve current calls from restored bytes");
    assert_eq!(restored_method_usages.usages, fresh_method_usages);

    assert!(restored_index
        .restore_from_cache_batch(&cache_data)
        .is_empty());
    assert!(restored_index.find_usages("com.example.User").is_empty());
    assert_eq!(
        restored_index
            .find_current_usages("com.example.User")
            .unwrap()
            .locations()
            .count(),
        fresh_usages.len(),
        "restoring twice must not accumulate derived usages"
    );
}

#[test]
fn cache_roundtrip_preserves_complete_import_context() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Imports.java");
    let source = r#"
        package com.example;
        import java.util.List;
        import java.time.*;
        import static java.util.Collections.emptyList;
        import static first.Tools.pick;
        import static second.Tools.pick;
        import static java.util.Objects.*;
        class Imports { List<String> values = emptyList(); }
    "#;
    std::fs::write(&path, source).unwrap();

    let fresh = WorkspaceIndex::new(dir.path().to_path_buf());
    fresh.index_file_with_source(&path, source).unwrap();
    let fresh_file_id = fresh.get_file_id(&path).expect("fresh file id");
    let expected = fresh.get_imports(fresh_file_id).expect("fresh imports");
    assert_eq!(expected.current_package.as_deref(), Some("com.example"));
    assert_eq!(expected.wildcards, vec!["java.time"]);
    assert_eq!(
        expected.static_imports_named("emptyList").next(),
        Some("java.util.Collections.emptyList")
    );
    assert_eq!(
        expected.static_imports_named("pick").collect::<Vec<_>>(),
        ["first.Tools.pick", "second.Tools.pick"]
    );
    assert_eq!(expected.static_wildcards, vec!["java.util.Objects"]);

    let cache = crate::IndexCache::new(dir.path());
    cache
        .save(fresh.extract_cache_data(), HashMap::new())
        .expect("save cache");
    let persisted = cache.load().expect("load cache");
    let restored = WorkspaceIndex::new(dir.path().to_path_buf());
    assert!(restored
        .restore_from_cache_batch(persisted.entries())
        .is_empty());
    let restored_file_id = restored.get_file_id(&path).expect("restored file id");
    assert_eq!(restored.get_imports(restored_file_id), Some(expected));
    cache.invalidate().expect("remove test cache");
}

#[test]
fn batch_restore_does_not_resurrect_a_deleted_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Deleted.java");
    let source = "package com.example; class Deleted {}";
    std::fs::write(&path, source).unwrap();

    let fresh = WorkspaceIndex::new(dir.path().to_path_buf());
    fresh.index_file_with_source(&path, source).unwrap();
    let cache_data = fresh.extract_cache_data();
    std::fs::remove_file(&path).unwrap();

    let restored = WorkspaceIndex::new(dir.path().to_path_buf());
    assert_eq!(
        restored.restore_from_cache_batch(&cache_data),
        vec![path.clone()]
    );
    assert!(restored.get_file_id(&path).is_none());
    assert!(restored.get_symbol("com.example.Deleted").is_none());
}

#[cfg(unix)]
#[test]
fn batch_restore_rejects_a_symlink_retargeted_after_validation() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let alias = temp.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("Cached.java"), "class CachedTarget {}").unwrap();
    std::fs::write(second.join("Cached.java"), "class RetargetedTarget {}").unwrap();
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    let alias_path = alias.join("Cached.java");

    let fresh = WorkspaceIndex::new(temp.path().to_path_buf());
    fresh.index_file_sync_for_test(&alias_path).unwrap();
    let cache = fresh.extract_cache_data();
    let restored = WorkspaceIndex::new(temp.path().to_path_buf());
    let (rejected, revisions) =
        restored.restore_from_cache_batch_after_validation_for_test(&cache, || {
            std::fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&second, &alias).unwrap();
        });

    assert_eq!(rejected, vec![alias_path.clone()]);
    assert!(revisions.is_empty());
    assert!(restored.get_file_id(&alias_path).is_none());
    assert!(restored.find_symbol("CachedTarget").is_none());
    assert!(restored.find_symbol("RetargetedTarget").is_none());
}

#[cfg(unix)]
#[test]
fn batch_restore_rejects_duplicate_display_paths_for_one_identity() {
    let temp = TempDir::new().unwrap();
    let real = temp.path().join("real");
    let first_alias = temp.path().join("first-alias");
    let second_alias = temp.path().join("second-alias");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("Cached.java"), "class CachedTarget {}").unwrap();
    std::os::unix::fs::symlink(&real, &first_alias).unwrap();
    std::os::unix::fs::symlink(&real, &second_alias).unwrap();
    let first_path = first_alias.join("Cached.java");
    let second_path = second_alias.join("Cached.java");

    let fresh = WorkspaceIndex::new(temp.path().to_path_buf());
    fresh.index_file_sync_for_test(&first_path).unwrap();
    let mut cache = fresh.extract_cache_data();
    let cached_file = cache.get(&first_path).unwrap().clone();
    cache.insert(second_path.clone(), cached_file);

    let restored = WorkspaceIndex::new(temp.path().to_path_buf());
    let mut expected_rejected = vec![first_path, second_path];
    expected_rejected.sort_unstable();
    assert_eq!(restored.restore_from_cache_batch(&cache), expected_rejected);
    assert!(restored.find_symbol("CachedTarget").is_none());
    assert!(restored.all_file_ids().is_empty());
}

#[test]
fn batch_restore_rejects_changed_content_even_when_mtime_matches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Revision.java");
    let cached_source = "package com.example; class CachedRevision {}";
    std::fs::write(&path, cached_source).unwrap();

    let source_index = WorkspaceIndex::new(dir.path().to_path_buf());
    source_index
        .index_file_with_source(&path, cached_source)
        .unwrap();
    let cache_data = source_index.extract_cache_data();
    let cached_mtime = cache_data[&path].mtime;

    std::fs::write(
        &path,
        "package com.example; class ChangedBehindSameTimestamp {}",
    )
    .unwrap();
    std::fs::File::open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(cached_mtime))
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        cached_mtime,
        "fixture must bypass the mtime prefilter"
    );

    let restored = WorkspaceIndex::new(dir.path().to_path_buf());
    assert_eq!(restored.restore_from_cache_batch(&cache_data), vec![path]);
    assert!(restored.get_symbol("com.example.CachedRevision").is_none());
}

#[test]
fn cache_restore_never_replaces_a_preexisting_in_memory_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Child.java");
    let old_source = "package app; class Child extends OldParent {}";
    let new_source = "package app; class Child extends NewParent {}";
    std::fs::write(&path, old_source).unwrap();

    let restored = WorkspaceIndex::new(dir.path().to_path_buf());
    restored.index_file_with_source(&path, old_source).unwrap();
    assert_eq!(
        restored.get_inheritance_chain("app.Child"),
        vec!["app.Child"]
    );

    std::fs::write(&path, new_source).unwrap();
    let source_index = WorkspaceIndex::new(dir.path().to_path_buf());
    source_index
        .index_file_with_source(&path, new_source)
        .unwrap();
    let cache_data = source_index.extract_cache_data();

    assert!(restored.restore_from_cache_batch(&cache_data).is_empty());
    assert_eq!(
        restored.get_inheritance_chain("app.Child"),
        vec!["app.Child"],
        "cache seeding must not replace an editor or watcher publication"
    );
    assert_eq!(
        restored
            .get_symbol("app.Child")
            .unwrap()
            .parent_class
            .as_deref(),
        Some("OldParent")
    );
    assert!(restored.get_symbol("app.OldParent").is_none());
    assert_eq!(
        restored
            .get_file_contents(restored.get_file_id(&path).unwrap())
            .as_deref()
            .map(String::as_str),
        Some(old_source),
        "the exact pre-existing source revision must remain published"
    );
    assert!(
        restored
            .inner
            .read()
            .parse_tree_cache
            .peek(&restored.get_file_id(&path).unwrap())
            .is_some(),
        "skipping a cache entry must preserve the matching in-memory parse product"
    );
}

// ============================================================================
// Kotlin: object-literal expressions and parameter types
// ============================================================================
//
// Two gaps in the Kotlin source indexing path
// (hyperion_kotlin::SymbolExtractor, wired up via `extract_kotlin_symbols` in
// hyperion-workspace/src/index/core.rs):
//   (a) object-literal expressions (`object : SomeInterface { ... }` used as
//       a value) contributed nothing to the index — their members were
//       invisible to workspace/symbol, goto-def, and find-refs.
//   (b) indexed Kotlin functions carried no parameter types.
// These tests pin the fixed behavior through the same public WorkspaceIndex
// API surface exercised by the rest of this file.

#[tokio::test]
async fn test_kotlin_object_literal_assigned_to_property_is_indexed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Widget.kt");

    let source = r#"
package com.example

interface Listener {
    fun onClick(): Unit
}

class Widget {
    val listener = object : Listener {
        override fun onClick(): Unit {}
    }
}
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    // The anonymous object literal itself is indexed, parented to the
    // property it's assigned to.
    let anon_locations = index.find_by_simple_name("<anonymous>");
    assert_eq!(
        anon_locations.len(),
        1,
        "Expected exactly one anonymous-object entry, found {anon_locations:?}"
    );
    let anon = &anon_locations[0];
    assert_eq!(
        anon.container_name.as_deref(),
        Some("com.example.Widget.listener"),
        "Anonymous object should be parented to the property it's assigned to"
    );

    // Its member function is findable via workspace symbols (both by exact
    // simple-name lookup and by the fuzzy `search` used for workspace/symbol)
    // and is correctly parented to the anonymous object, not to Widget.
    // (There are two `onClick` entries in this file: the interface's own
    // abstract declaration, and the anonymous object's override — we only
    // care that the override is present and correctly parented.)
    let on_click_by_name = index.find_by_simple_name("onClick");
    assert!(
        on_click_by_name
            .iter()
            .any(|loc| loc.container_name.as_deref() == Some(&*anon.fqn)),
        "Expected an onClick entry parented to the anonymous object, found {on_click_by_name:?}"
    );

    let on_click_by_search = index.search("onClick");
    assert!(
        !on_click_by_search.is_empty(),
        "onClick should be discoverable via workspace/symbol fuzzy search"
    );
}

#[tokio::test]
async fn test_kotlin_object_literal_returned_from_function_is_indexed() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Factory.kt");

    let source = r#"
package com.example

interface Listener {
    fun onClick(): Unit
}

#[test]
fn generic_owner_election_keeps_anonymous_object_trees_source_coherent() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![synthetic_type(
        "dependency.Listener",
        SymbolKind::INTERFACE,
        None,
        &[],
        crate::Modifiers::default(),
    )]);
    let preferred_path = temp.path().join("a/Factory.kt");
    let fallback_path = temp.path().join("z/Factory.kt");
    let preferred_source = concat!(
        "package app\nimport dependency.Listener\n",
        "fun make() = object : Listener { fun preferred() {} }"
    );
    let fallback_source = concat!(
        "package app\nimport dependency.Listener\n",
        "fun make() = object : Listener { fun fallback() {} }"
    );
    index
        .index_file_with_source(&fallback_path, fallback_source)
        .unwrap();
    index
        .index_file_with_source(&preferred_path, preferred_source)
        .unwrap();

    let anonymous = index.find_by_simple_name("<anonymous>");
    assert_eq!(anonymous.len(), 1);
    let anonymous_fqn = anonymous[0].fqn.to_string();
    assert_eq!(
        index.direct_supertypes(&anonymous_fqn),
        ["dependency.Listener"]
    );
    assert!(index
        .get_symbol(&format!("{anonymous_fqn}.preferred"))
        .is_some());
    assert!(index
        .get_symbol(&format!("{anonymous_fqn}.fallback"))
        .is_none());

    let preferred_id = index.get_file_id(&preferred_path).unwrap();
    let batch = index
        .file_supertypes_for_revision(preferred_id, preferred_source)
        .unwrap();
    assert!(batch.types.iter().any(|entry| {
        entry.type_fqn == anonymous_fqn
            && entry.superclass.is_none()
            && entry.interfaces == ["dependency.Listener"]
    }));

    index.remove_file(preferred_id);
    assert!(index
        .get_symbol(&format!("{anonymous_fqn}.preferred"))
        .is_none());
    assert!(index
        .get_symbol(&format!("{anonymous_fqn}.fallback"))
        .is_some());
    assert_eq!(
        index.direct_supertypes(&anonymous_fqn),
        ["dependency.Listener"]
    );

    let member_path = temp.path().join("MemberFactory.kt");
    index
        .index_file_with_source(
            &member_path,
            concat!(
                "package other\nimport dependency.Listener\n",
                "class Factory { fun make() = object : Listener { fun member() {} } }"
            ),
        )
        .unwrap();
    let member_anonymous = index
        .find_by_simple_name("<anonymous>")
        .into_iter()
        .find(|location| location.fqn.starts_with("other.Factory.make."))
        .unwrap();
    assert_eq!(
        index.direct_supertypes(&member_anonymous.fqn),
        ["dependency.Listener"]
    );
    assert!(index
        .get_symbol(&format!("{}.member", member_anonymous.fqn))
        .is_some());
}

fun makeListener(): Listener {
    return object : Listener {
        override fun onClick(): Unit {}
    }
}
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let make_listener = index
        .find_symbol("com.example.makeListener")
        .expect("makeListener function should be indexed");

    let anon_locations = index.find_by_simple_name("<anonymous>");
    assert_eq!(anon_locations.len(), 1);
    let anon = &anon_locations[0];
    assert_eq!(
        anon.container_name.as_deref(),
        Some(&*make_listener.fqn),
        "Anonymous object returned from a function should be parented to that function"
    );

    let on_click = index.find_by_simple_name("onClick");
    assert!(
        on_click
            .iter()
            .any(|loc| loc.container_name.as_deref() == Some(&*anon.fqn)),
        "Expected an onClick entry parented to the anonymous object, found {on_click:?}"
    );
}

#[tokio::test]
async fn test_kotlin_function_parameters_are_indexed_with_types() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Greeter.kt");

    let source = r#"
package com.example

fun greet(name: String, times: Int): String {
    return name
}
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let greet = index
        .get_symbol("com.example.greet")
        .expect("greet function should be indexed");

    let params: Vec<&str> = greet.parameters.iter().map(|p| p.as_ref()).collect();
    assert_eq!(
        params,
        vec!["String name", "Int times"],
        "Kotlin function parameters should carry their declared types, \
         matching the \"TypeName paramName\" shape used for Java entries"
    );
}

#[tokio::test]
async fn test_kotlin_class_method_parameters_are_indexed_with_types() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Repository.kt");

    let source = r#"
package com.example

class Repository {
    fun findById(id: Int, includeDeleted: Boolean): String {
        return ""
    }
}
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let find_by_id = index
        .get_symbol("com.example.Repository.findById")
        .expect("findById method should be indexed");

    let params: Vec<&str> = find_by_id.parameters.iter().map(|p| p.as_ref()).collect();
    assert_eq!(params, vec!["Int id", "Boolean includeDeleted"]);
}

#[tokio::test]
async fn test_kotlin_constructor_parameters_are_indexed_with_types() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("User.kt");

    let source = r#"
package com.example

data class User(val name: String, val age: Int)
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let user = index
        .get_symbol("com.example.User")
        .expect("User class should be indexed");

    let params: Vec<&str> = user.parameters.iter().map(|p| p.as_ref()).collect();
    assert_eq!(
        params,
        vec!["String name", "Int age"],
        "A Kotlin class's primary constructor parameter types should be \
         represented in the index, the same way Java entries carry them"
    );
}

#[tokio::test]
async fn test_kotlin_type_parameters_reach_workspace_index_with_bounds_and_positions() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("ComparableBox.kt");

    let source = r#"
package com.example

class ComparableBox<Key : Comparable<Key>, Value>
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let class = index
        .get_symbol("com.example.ComparableBox")
        .expect("Kotlin generic class should be indexed");

    assert_eq!(class.type_parameters.len(), 2);
    assert_eq!(class.type_parameters[0].name.as_ref(), "Key");
    assert_eq!(class.type_parameters[0].position, 0);
    assert_eq!(class.type_parameters[0].bounds.len(), 1);
    assert!(class.type_parameters[0].bounds[0].contains("Comparable"));
    assert_eq!(class.type_parameters[1].name.as_ref(), "Value");
    assert_eq!(class.type_parameters[1].position, 1);
    assert!(class.type_parameters[1].bounds.is_empty());
}

#[tokio::test]
async fn test_kotlin_declaration_modifiers_survive_workspace_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let file_path = root.join("Service.kt");

    let source = r#"
package com.example

internal open class Service {
    suspend fun fetch(): String = "value"
    inline fun transform(): String = "value"
    operator fun plus(other: Service): Service = this
    infix fun merge(other: Service): Service = this
}

object Registry
"#;
    std::fs::write(&file_path, source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&file_path).await.unwrap();

    let service = index
        .get_symbol("com.example.Service")
        .expect("Kotlin class should be indexed");
    assert!(service.modifiers.is_internal);
    assert!(service.modifiers.is_open);

    let fetch = index
        .get_symbol("com.example.Service.fetch")
        .expect("suspend function should be indexed");
    assert!(fetch.modifiers.is_suspend);
    let transform = index
        .get_symbol("com.example.Service.transform")
        .expect("inline function should be indexed");
    assert!(transform.modifiers.is_inline);
    let plus = index
        .get_symbol("com.example.Service.plus")
        .expect("operator function should be indexed");
    assert!(plus.modifiers.is_operator);
    let merge = index
        .get_symbol("com.example.Service.merge")
        .expect("infix function should be indexed");
    assert!(merge.modifiers.is_infix);

    let instance = index
        .get_symbol("com.example.Registry.INSTANCE")
        .expect("Kotlin object instance field should be indexed");
    assert!(instance.modifiers.is_static);
}

#[tokio::test]
async fn test_shared_import_map_preserves_kotlin_and_groovy_import_forms() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let kotlin_path = root.join("KotlinImports.kt");
    let groovy_path = root.join("GroovyImports.groovy");

    let kotlin_source = r#"
package sample.kotlin

import java.time.Instant as Timestamp
import java.util.*

class KotlinImports
"#;
    let groovy_source = r#"
package sample.groovy

import java.util.List
import static java.lang.Math.PI
import static java.util.Collections.*

class GroovyImports {}
"#;
    std::fs::write(&kotlin_path, kotlin_source).unwrap();
    std::fs::write(&groovy_path, groovy_source).unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&kotlin_path).await.unwrap();
    index.index_file(&groovy_path).await.unwrap();

    let kotlin_file = index
        .get_file_id(&kotlin_path)
        .expect("Kotlin file should be indexed");
    let kotlin_imports = index
        .get_imports(kotlin_file)
        .expect("Kotlin imports should be retained");
    assert_eq!(
        kotlin_imports.current_package.as_deref(),
        Some("sample.kotlin")
    );
    assert_eq!(
        kotlin_imports.explicit.get("Timestamp").map(String::as_str),
        Some("java.time.Instant")
    );
    assert_eq!(kotlin_imports.wildcards, ["java.util"]);

    let groovy_file = index
        .get_file_id(&groovy_path)
        .expect("Groovy file should be indexed");
    let groovy_imports = index
        .get_imports(groovy_file)
        .expect("Groovy imports should be retained");
    assert_eq!(
        groovy_imports.current_package.as_deref(),
        Some("sample.groovy")
    );
    assert_eq!(
        groovy_imports.explicit.get("List").map(String::as_str),
        Some("java.util.List")
    );
    assert_eq!(
        groovy_imports.static_imports_named("PI").next(),
        Some("java.lang.Math.PI")
    );
    assert_eq!(groovy_imports.static_wildcards, ["java.util.Collections"]);
}

#[tokio::test]
async fn groovy_inheritance_resolves_across_java_and_groovy_types() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let base_path = root.join("Base.java");
    let api_path = root.join("Api.java");
    let groovy_path = root.join("Hierarchy.groovy");

    std::fs::write(
        &base_path,
        "package dependency; public abstract class Base<T> { public void inherited() {} }",
    )
    .unwrap();
    std::fs::write(
        &api_path,
        "package dependency; public interface Api { void run(); }",
    )
    .unwrap();
    std::fs::write(
        &groovy_path,
        r#"
package app
import dependency.Base
import dependency.Api

abstract class Child<T> extends Base<T> implements Api {}
interface Combined extends Api {}
enum Mode implements Api { ON }
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&base_path).await.unwrap();
    index.index_file(&api_path).await.unwrap();
    index.index_file(&groovy_path).await.unwrap();

    let child = index.get_symbol("app.Child").expect("Groovy child");
    assert_eq!(child.parent_class.as_deref(), Some("Base"));
    assert_eq!(
        child
            .interfaces
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["Api"]
    );
    assert_eq!(
        index.direct_supertypes("app.Child"),
        ["dependency.Api", "dependency.Base"]
    );
    let chain = index.get_inheritance_chain("app.Child");
    assert!(chain.iter().any(|parent| parent == "dependency.Base"));
    assert!(chain.iter().any(|parent| parent == "dependency.Api"));
    assert!(index
        .find_methods_by_name_with_inheritance("app.Child", "inherited")
        .iter()
        .any(|method| method.container.as_deref() == Some("dependency.Base")));

    let implementors = index.get_interface_implementations("dependency.Api");
    assert_eq!(implementors, ["app.Child", "app.Combined", "app.Mode"]);
}

#[tokio::test]
async fn scala_inheritance_resolves_aliases_traits_enums_and_wildcards() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    let base_path = root.join("Base.java");
    let api_path = root.join("Api.java");
    let mixin_path = root.join("Mixin.java");
    let scala_path = root.join("Hierarchy.scala");

    std::fs::write(
        &base_path,
        "package dependency; public abstract class Base<T> { public void inherited() {} }",
    )
    .unwrap();
    std::fs::write(
        &api_path,
        "package dependency; public interface Api { void run(); }",
    )
    .unwrap();
    std::fs::write(&mixin_path, "package mixins; public interface Mixin {}").unwrap();
    std::fs::write(
        &scala_path,
        r#"
package app
import dependency.{Base => Parent, Api}
import mixins._

sealed abstract class Child[A](value: A) extends Parent[A] with Api with Mixin
trait Combined extends Api
enum Mode extends Api:
  case On
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&base_path).await.unwrap();
    index.index_file(&api_path).await.unwrap();
    index.index_file(&mixin_path).await.unwrap();
    index.index_file(&scala_path).await.unwrap();

    let child = index.get_symbol("app.Child").expect("Scala child");
    assert!(child.modifiers.is_sealed);
    assert!(child.modifiers.is_abstract);
    assert_eq!(child.parent_class.as_deref(), Some("Parent"));
    assert_eq!(
        child
            .interfaces
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["Api", "Mixin"]
    );
    assert_eq!(
        index.direct_supertypes("app.Child"),
        ["dependency.Api", "dependency.Base", "mixins.Mixin"]
    );

    let chain = index.get_inheritance_chain("app.Child");
    assert!(chain.iter().any(|parent| parent == "dependency.Base"));
    assert!(chain.iter().any(|parent| parent == "dependency.Api"));
    assert!(chain.iter().any(|parent| parent == "mixins.Mixin"));
    assert!(index
        .find_methods_by_name_with_inheritance("app.Child", "inherited")
        .iter()
        .any(|method| method.container.as_deref() == Some("dependency.Base")));

    assert_eq!(
        index.get_interface_implementations("dependency.Api"),
        ["app.Child", "app.Combined", "app.Mode"]
    );
}

#[tokio::test]
async fn groovy_type_and_method_usages_are_indexed_with_groovy_syntax() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("Greeter.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &target_path,
        "package dependency; public class Greeter { public String greet() { return \"hi\"; } }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.Greeter\nclass Use { String call(Greeter greeter) { greeter.greet() } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    assert_eq!(index.find_usages("dependency.Greeter").len(), 1);
    let greet = index
        .get_symbol("dependency.Greeter.greet")
        .expect("Java method");
    assert_eq!(index.find_usages(&greet.fqn).len(), 1);
}

#[tokio::test]
async fn groovy_safe_navigation_usages_are_indexed() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("Greeter.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &target_path,
        "package dependency; public class Greeter { public String name; public String greet() { return \"hi\"; } }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.Greeter\nclass Use { String call(Greeter greeter) { greeter?.greet() + greeter?.name } }",
    )
    .unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let greet = index
        .get_symbol("dependency.Greeter.greet")
        .expect("Java method");
    assert_eq!(index.find_usages(&greet.fqn).len(), 1);
    assert_eq!(index.find_usages("dependency.Greeter.name").len(), 1);
}

#[tokio::test]
async fn groovy_usage_extraction_handles_dynamic_bindings_inheritance_and_lexical_noise() {
    let temp = TempDir::new().unwrap();
    let base_path = temp.path().join("Base.java");
    let target_path = temp.path().join("Greeter.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &base_path,
        "package dependency; public class Base { public String baseName; public String inherited() { return \"base\"; } }",
    )
    .unwrap();
    std::fs::write(
        &target_path,
        r#"package dependency;
public class Greeter extends Base {
    public String name;
    public String greet() { return "hi"; }
    public void overloaded() {}
    public void overloaded(String value) {}
}"#,
    )
    .unwrap();
    let groovy_source = r#"package app
import dependency.Greeter

class Use {
    Greeter field

    String direct(Greeter value) { value.greet() }
    String safe(Greeter value) { value?.inherited() }
    String property(Greeter value) { value?.name + value?.baseName }
    String inferred() { def local = new Greeter(); local.greet() }
    String viaField() { field.greet() }
    void ambiguous(Greeter value) { value.overloaded() }
    void lexicalNoise(Greeter value) {
        def text = "value.greet() and value.name"
        // value.greet()
        /* value.name */
    }
}"#;
    std::fs::write(&use_path, groovy_source).unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&base_path).await.unwrap();
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let type_usages = index.find_usages("dependency.Greeter");
    assert_eq!(
        type_usages.len(),
        7,
        "recorded type ranges: {:?}",
        type_usages
            .iter()
            .map(|usage| &groovy_source[usage.range.start as usize..usage.range.end as usize])
            .collect::<Vec<_>>()
    );
    let greet = index
        .get_symbol("dependency.Greeter.greet")
        .expect("greet method");
    assert_eq!(
        index.find_usages(&greet.fqn).len(),
        3,
        "strings and comments must not fabricate member usages"
    );
    let inherited = index
        .get_symbol("dependency.Base.inherited")
        .expect("inherited method");
    assert_eq!(index.find_usages(&inherited.fqn).len(), 1);
    assert_eq!(index.find_usages("dependency.Greeter.name").len(), 1);
    assert_eq!(index.find_usages("dependency.Base.baseName").len(), 1);

    let overloaded = index
        .find_methods_by_name("dependency.Greeter", "overloaded")
        .into_iter()
        .map(|method| index.find_usages(&method.fqn).len())
        .sum::<usize>();
    assert_eq!(overloaded, 0, "ambiguous overloads must not be guessed");
}

#[tokio::test]
async fn groovy_untyped_local_does_not_fall_back_to_same_named_type() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("value.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &target_path,
        "package dependency; public class value { public void dangerous() {} }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.value\nclass Use { void call() { def value = unknown; value.dangerous() } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let dangerous = index
        .get_symbol("dependency.value.dangerous")
        .expect("same-named type method");
    assert!(index.find_usages(&dangerous.fqn).is_empty());
}

#[tokio::test]
async fn groovy_bare_static_call_never_elects_an_overload_by_index_order() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("Tools.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &target_path,
        "package dependency; public class Tools { public static void pick(int x) {} public static void pick(String x) {} public static void only(int x) {} }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport static dependency.Tools.pick\nimport static dependency.Tools.only\nclass Use { void call() { pick(1); only(1) } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    assert!(index
        .find_methods_by_name("dependency.Tools", "pick")
        .iter()
        .all(|method| index.find_usages(&method.fqn).is_empty()));
    let only = index
        .find_methods_by_name("dependency.Tools", "only")
        .pop()
        .expect("unique static method");
    assert_eq!(index.find_usages(&only.fqn).len(), 1);
}

#[tokio::test]
async fn groovy_recovery_keeps_parameter_types_scoped_to_their_methods() {
    let temp = TempDir::new().unwrap();
    let alpha_path = temp.path().join("Alpha.java");
    let beta_path = temp.path().join("Beta.java");
    let use_path = temp.path().join("Use.groovy");
    std::fs::write(
        &alpha_path,
        "package dependency; public class Alpha { public void ping() {} }",
    )
    .unwrap();
    std::fs::write(
        &beta_path,
        "package dependency; public class Beta { public void ping() {} }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.Alpha\nimport dependency.Beta\nclass Use { String alpha(Alpha value) { value?.ping() } String beta(Beta value) { value?.ping() } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&alpha_path).await.unwrap();
    index.index_file(&beta_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let alpha_ping = index
        .get_symbol("dependency.Alpha.ping")
        .expect("Alpha.ping");
    let beta_ping = index.get_symbol("dependency.Beta.ping").expect("Beta.ping");
    assert_eq!(index.find_usages(&alpha_ping.fqn).len(), 1);
    assert_eq!(index.find_usages(&beta_ping.fqn).len(), 1);
}

#[tokio::test]
async fn scala_type_and_method_usages_are_indexed_with_scala_syntax() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("Greeter.java");
    let use_path = temp.path().join("Use.scala");
    std::fs::write(
        &target_path,
        "package dependency; public class Greeter { public String greet() { return \"hi\"; } }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.Greeter\nobject Use { def call(greeter: Greeter): String = greeter.greet() }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    assert_eq!(index.find_usages("dependency.Greeter").len(), 1);
    let greet = index
        .get_symbol("dependency.Greeter.greet")
        .expect("Java method");
    assert_eq!(index.find_usages(&greet.fqn).len(), 1);
}

#[tokio::test]
async fn scala_usage_extraction_handles_package_blocks_aliases_members_and_overloads() {
    let temp = TempDir::new().unwrap();
    let base_path = temp.path().join("Base.java");
    let target_path = temp.path().join("Greeter.java");
    let use_path = temp.path().join("Use.scala");
    std::fs::write(
        &base_path,
        "package dependency; public class Base { public String baseName; public String inherited() { return \"base\"; } }",
    )
    .unwrap();
    std::fs::write(
        &target_path,
        r#"package dependency;
public class Greeter extends Base {
    public String name;
    public String greet() { return "hi"; }
    public void overloaded() {}
    public void overloaded(String value) {}
}"#,
    )
    .unwrap();
    let scala_source = r#"package app {
  import dependency.{Greeter => Hello}
  import dependency._

  object Use {
    def greet(value: Hello): String = value.greet()
    def inherit(value: Hello): String = value.inherited()
    def read(value: Hello): String = value.name
    def readBase(value: Hello): String = value.baseName
    def generic(values: List[Hello]): Unit = ()
    def make(): Hello = new Hello()
    def wildcard(value: Greeter): Unit = ()
    def ambiguous(value: Hello): Unit = value.overloaded()
  }
}"#;
    std::fs::write(&use_path, scala_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&base_path).await.unwrap();
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let type_usages = index.find_usages("dependency.Greeter");
    assert_eq!(type_usages.len(), 9);
    assert!(type_usages.iter().all(|usage| {
        matches!(
            &scala_source[usage.range.start as usize..usage.range.end as usize],
            "Hello" | "Greeter"
        )
    }));

    let greet = index
        .get_symbol("dependency.Greeter.greet")
        .expect("greet method");
    assert_eq!(index.find_usages(&greet.fqn).len(), 1);
    let inherited = index
        .get_symbol("dependency.Base.inherited")
        .expect("inherited method");
    assert_eq!(index.find_usages(&inherited.fqn).len(), 1);
    assert_eq!(index.find_usages("dependency.Greeter.name").len(), 1);
    assert_eq!(index.find_usages("dependency.Base.baseName").len(), 1);

    let overloaded = index
        .find_methods_by_name("dependency.Greeter", "overloaded")
        .into_iter()
        .map(|method| index.find_usages(&method.fqn).len())
        .sum::<usize>();
    assert_eq!(overloaded, 0, "an unresolved overload must not be guessed");
}

#[tokio::test]
async fn scala_local_receiver_bindings_are_scoped_and_ordered() {
    let temp = TempDir::new().unwrap();
    let alpha_path = temp.path().join("Alpha.java");
    let beta_path = temp.path().join("Beta.java");
    let use_path = temp.path().join("Use.scala");
    std::fs::write(
        &alpha_path,
        "package dependency; public class Alpha { public static void ping() {} }",
    )
    .unwrap();
    std::fs::write(
        &beta_path,
        "package dependency; public class Beta { public void ping() {} }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        r#"package app
import dependency.{Alpha, Beta}
object Use {
  def first(): Unit = { val service = new Alpha(); service.ping() }
  def second(): Unit = { val service = new Beta(); service.ping() }
  def shadowed(value: Alpha): Unit = { { val value = dynamic; value.ping() } }
  def ordered(): Unit = { Alpha.ping(); val Alpha = dynamic; Alpha.ping() }
}"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&alpha_path).await.unwrap();
    index.index_file(&beta_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let alpha_ping = index
        .get_symbol("dependency.Alpha.ping")
        .expect("Alpha.ping");
    let beta_ping = index.get_symbol("dependency.Beta.ping").expect("Beta.ping");
    assert_eq!(
        index.find_usages(&alpha_ping.fqn).len(),
        2,
        "the inferred Alpha local and the reference before shadowing should resolve"
    );
    assert_eq!(
        index.find_usages(&beta_ping.fqn).len(),
        1,
        "the sibling method must retain its own receiver binding"
    );
}

#[tokio::test]
async fn scala_imported_top_level_function_call_is_indexed() {
    let temp = TempDir::new().unwrap();
    let helper_path = temp.path().join("Helper.scala");
    let use_path = temp.path().join("Use.scala");
    std::fs::write(
        &helper_path,
        "package functions\n\ndef helper(): String = \"ok\"",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport functions.helper\nobject Use { def call(): String = helper() }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&helper_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let helper = index
        .get_symbol("functions.helper")
        .expect("top-level Scala function");
    assert_eq!(helper.kind, SymbolKind::FUNCTION);
    assert_eq!(index.find_usages(&helper.fqn).len(), 1);
}

#[tokio::test]
async fn scala_untyped_local_does_not_fall_back_to_same_named_type() {
    let temp = TempDir::new().unwrap();
    let target_path = temp.path().join("value.java");
    let use_path = temp.path().join("Use.scala");
    std::fs::write(
        &target_path,
        "package dependency; public class value { public void dangerous() {} }",
    )
    .unwrap();
    std::fs::write(
        &use_path,
        "package app\nimport dependency.value\nobject Use { def call(): Unit = { val value = unknown; value.dangerous() } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&target_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let dangerous = index
        .get_symbol("dependency.value.dangerous")
        .expect("same-named type method");
    assert!(index.find_usages(&dangerous.fqn).is_empty());
}

// ============================================================================
// Kotlin-native usage extraction
// ============================================================================
//
// Previously, the workspace index's usage extractor
// (`crate::usage_extractor::UsageExtractor`) was called unconditionally for
// every file, regardless of language: it parses `source` with the *Java*
// lexer/parser (`hyperion_parser`). For Kotlin files this means Kotlin source
// was fed through a Java grammar, so Kotlin usages were never indexed (no
// method calls, no type references, no property accesses) - callers only
// found Kotlin references via a separate, request-time semantic completion
// pass. `crate::kotlin_usage_extractor::KotlinUsageExtractor`
// (wired up in `index/core.rs` for `Language::Kotlin`) fixes this at the
// index level by walking the real Kotlin syntax tree and feeding the same
// `symbol_usages`/`file_usage_counts` tables Java usages flow through, so
// find-references works straight off the index.
//
// These fixtures cover: type references (supertypes, parameter types),
// function calls (bare/constructor, member, safe-call chains, companion
// object members reached through the class name), extension function calls,
// and property access - both Kotlin-uses-Kotlin and Kotlin-uses-Java.

/// Shared two-file Kotlin fixture used by several tests below: `Animal.kt`
/// declares the symbols under test, `Dog.kt` references them.
async fn index_kotlin_usage_fixture(root: &std::path::Path) -> WorkspaceIndex {
    let animal_path = root.join("Animal.kt");
    std::fs::write(
        &animal_path,
        r#"
package com.example

interface Speaker {
    fun speak(): String
}

open class Animal(val name: String) : Speaker {
    override fun speak(): String = name

    companion object {
        fun create(name: String): Animal = Animal(name)
    }
}

class Config {
    var enabled: Boolean = true
}

class Marker(val id: Int)
class User(val name: String)

fun topLevelHelper(): String = "helper"

fun String.shout(): String = this.uppercase() + "!"
"#,
    )
    .unwrap();

    let dog_path = root.join("Dog.kt");
    std::fs::write(
        &dog_path,
        r#"
package com.example

class Dog(name: String) : Animal(name), Speaker {
    override fun speak(): String {
        val helper = topLevelHelper()
        return helper
    }
}

fun describe(a: Animal, maybe: Animal?, c: Config, user: User, word: String): String {
    val direct = a.speak()
    val safe = maybe?.speak()
    val viaCompanion = Animal.create("Rex")
    val shouted = word.shout()
    val flag = c.enabled
    val userName = user.name
    return direct + (safe ?: "") + shouted + flag.toString() + userName
}

fun makeMarker() {
    Marker(1)
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.to_path_buf());
    // Index the declaration-heavy file first so Dog.kt also exercises normal
    // cross-file resolution. Same-file references in Animal.kt resolve through
    // the unpublished-revision overlay rather than depending on this order.
    index.index_file(&animal_path).await.unwrap();
    index.index_file(&dog_path).await.unwrap();
    index
}

#[tokio::test]
async fn test_kotlin_cross_file_string_templates_are_indexed_as_property_usages() {
    let temp = TempDir::new().unwrap();
    let constants_path = temp.path().join("Constants.kt");
    let use_path = temp.path().join("Use.kt");
    std::fs::write(
        &constants_path,
        "package com.example\n\nval appName: String = \"Hyperion\"\n",
    )
    .unwrap();
    let use_source = r#"package com.example

fun simple(): String = "Welcome to $appName"
fun braced(): String = "Running ${appName} now"
fun shadowed(): String {
    val appName = "Local"
    return "Using $appName"
}
fun escaped(): String = "Not a reference: \$appName"
"#;
    std::fs::write(&use_path, use_source).unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&constants_path).await.unwrap();
    index.index_file(&use_path).await.unwrap();

    let usages = index.find_usages("com.example.appName");
    assert_eq!(usages.len(), 2, "only the two global template uses count");
    assert_eq!(
        usages
            .iter()
            .map(|usage| &use_source[usage.range.start as usize..usage.range.end as usize])
            .collect::<Vec<_>>(),
        vec!["appName", "appName"]
    );
    assert!(usages.iter().all(|usage| {
        index
            .get_file_path(usage.file_id)
            .is_some_and(|path| path == use_path)
    }));
}

#[tokio::test]
async fn test_kotlin_supertype_type_references_are_indexed_as_usages() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // Dog's `: Animal(name), Speaker` supertype clause, plus `describe`'s
    // `a: Animal` / `maybe: Animal?` parameter types, should all land as
    // CLASS usages of Animal. Animal.kt's same-file constructor and return
    // references are indexed through the unpublished-revision overlay too.
    let animal_usages = index.find_usages("com.example.Animal");
    assert!(
        animal_usages.len() >= 3,
        "Expected at least 3 usages of Animal (supertype + 2 typed params), got: {:?}",
        animal_usages
    );
    assert!(
        animal_usages.iter().all(|u| u.kind == SymbolKind::CLASS),
        "All Animal type-reference usages should carry the CLASS kind, got: {:?}",
        animal_usages
    );

    // Dog's `: ..., Speaker` supertype (a cross-file reference to the
    // interface declared in Animal.kt) should be indexed as an INTERFACE
    // usage - the emitted kind comes from the resolved symbol itself, so a
    // supertype that happens to be an interface is reported correctly.
    let speaker_usages = index.find_usages("com.example.Speaker");
    assert_eq!(
        speaker_usages.len(),
        2,
        "Expected Animal's same-file and Dog's cross-file Speaker supertypes, got: {:?}",
        speaker_usages
    );
    assert_eq!(speaker_usages[0].kind, SymbolKind::INTERFACE);
}

#[tokio::test]
async fn test_kotlin_bare_constructor_call_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `Marker` is referenced nowhere except the bare constructor call
    // `Marker()` inside `makeMarker()`, so this isolates the "no `new`
    // keyword" constructor-call resolution from type-reference extraction.
    let marker_usages = index.find_usages("com.example.Marker");
    assert_eq!(
        marker_usages.len(),
        1,
        "Expected exactly one usage of Marker from the bare `Marker()` call, got: {:?}",
        marker_usages
    );
    assert_eq!(marker_usages[0].kind, SymbolKind::CLASS);
}

#[tokio::test]
async fn test_kotlin_bare_top_level_function_call_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `topLevelHelper()` is called unqualified from inside `Dog.speak()`,
    // a method body - this pins that unqualified calls prefer the enclosing
    // class first, then fall back to a top-level function in the package.
    let usages = index.find_usages("com.example.topLevelHelper");
    assert_eq!(
        usages.len(),
        1,
        "Expected exactly one usage of topLevelHelper, got: {:?}",
        usages
    );
    assert_eq!(usages[0].kind, SymbolKind::FUNCTION);
}

#[tokio::test]
async fn kotlin_imported_top_level_function_uses_exact_binding() {
    let temp = TempDir::new().unwrap();
    let left = temp.path().join("left/Functions.kt");
    let right = temp.path().join("right/Functions.kt");
    let caller = temp.path().join("app/Use.kt");
    for path in [&left, &right, &caller] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    std::fs::write(&left, "package left\nfun greet(wrong: Int) {}\n").unwrap();
    std::fs::write(&right, "package right\nfun greet(name: String) {}\n").unwrap();
    std::fs::write(
        &caller,
        "package app\nimport right.greet\nfun use() { greet(\"ok\") }\n",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&left).await.unwrap();
    index.index_file(&right).await.unwrap();
    index.index_file(&caller).await.unwrap();

    let caller_id = index.get_file_id(&caller).unwrap();
    assert!(index
        .find_usages("right.greet")
        .iter()
        .any(|usage| usage.file_id == caller_id));
    assert!(index
        .find_usages("left.greet")
        .iter()
        .all(|usage| usage.file_id != caller_id));
}

#[tokio::test]
async fn kotlin_wildcard_function_and_template_property_ambiguity_fail_closed() {
    let temp = TempDir::new().unwrap();
    let left = temp.path().join("left/Bindings.kt");
    let right = temp.path().join("right/Bindings.kt");
    let caller = temp.path().join("app/Use.kt");
    for path in [&left, &right, &caller] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    std::fs::write(
        &left,
        "package left\nfun greet(value: String) {}\nval label = \"left\"\n",
    )
    .unwrap();
    std::fs::write(
        &right,
        "package right\nfun greet(value: String) {}\nval label = \"right\"\n",
    )
    .unwrap();
    std::fs::write(
        &caller,
        "package app\nimport left.*\nimport right.*\nfun use() { greet(\"x\"); println(\"$label\") }\n",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&left).await.unwrap();
    index.index_file(&right).await.unwrap();
    index.index_file(&caller).await.unwrap();
    let caller_id = index.get_file_id(&caller).unwrap();

    for fqn in ["left.greet", "right.greet", "left.label", "right.label"] {
        assert!(
            index
                .find_usages(fqn)
                .iter()
                .all(|usage| usage.file_id != caller_id),
            "ambiguous wildcard binding must not be attributed to {fqn}"
        );
    }
}

#[tokio::test]
async fn test_kotlin_instance_method_call_and_safe_call_chain_are_indexed_as_usages() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `a.speak()` (plain qualified call) and `maybe?.speak()` (safe-call
    // chain) should both resolve to the same indexed method.
    let usages = index.find_usages("com.example.Animal.speak");
    assert_eq!(
        usages.len(),
        2,
        "Expected both a.speak() and maybe?.speak() to be indexed as usages of \
         Animal.speak, got: {:?}",
        usages
    );
    assert!(usages.iter().all(|u| u.kind == SymbolKind::FUNCTION));
}

#[tokio::test]
async fn test_kotlin_companion_member_call_via_class_name_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `Animal.create("Rex")` calls a companion-object function through the
    // enclosing class's name; the indexed symbol actually lives at
    // `Animal.Companion.create`, so this pins the companion fallback.
    let usages = index.find_usages("com.example.Animal.Companion.create");
    assert_eq!(
        usages.len(),
        1,
        "Expected Animal.create(\"Rex\") to resolve through the Companion fallback, got: {:?}",
        usages
    );
    assert_eq!(usages[0].kind, SymbolKind::FUNCTION);
}

#[tokio::test]
async fn test_kotlin_property_access_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `c.enabled` is a qualified property read (no call parens at all).
    let usages = index.find_usages("com.example.Config.enabled");
    assert_eq!(
        usages.len(),
        1,
        "Expected c.enabled to be indexed as a property usage, got: {:?}",
        usages
    );
    assert_eq!(usages[0].kind, SymbolKind::PROPERTY);
}

#[tokio::test]
async fn kotlin_implicit_this_property_usage_respects_local_and_owner_shadowing() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Counters.kt");
    let source = r#"package com.example
class Counter {
    var count = 0
    fun increment() {
        this.count++
        count++
    }
    fun local() {
        val count = 4
        println(count)
    }
}
class OtherCounter {
    var count = 0
}
"#;
    std::fs::write(&path, source).unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&path).await.unwrap();

    let usages = index.find_usages("com.example.Counter.count");
    assert_eq!(usages.len(), 2, "qualified and implicit-this reads only");
    assert_eq!(
        usages
            .iter()
            .map(|usage| &source[usage.range.start as usize..usage.range.end as usize])
            .collect::<Vec<_>>(),
        ["count", "count"]
    );
    assert!(usages
        .iter()
        .all(|usage| usage.kind == SymbolKind::PROPERTY));
}

#[tokio::test]
async fn test_kotlin_constructor_property_access_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `User.name` originates from `val name` in User's primary constructor.
    // The extractor must materialize that property before the existing
    // qualified-access resolver can record `user.name` here.
    let usages = index.find_usages("com.example.User.name");
    assert_eq!(
        usages.len(),
        1,
        "Expected user.name to resolve to the constructor property, got: {:?}",
        usages
    );
    assert_eq!(usages[0].kind, SymbolKind::PROPERTY);
}

#[tokio::test]
async fn test_kotlin_extension_function_call_is_indexed_as_usage() {
    let temp = TempDir::new().unwrap();
    let index = index_kotlin_usage_fixture(temp.path()).await;

    // `word.shout()` calls a same-package extension function on `String` -
    // a type that is never itself indexed as a workspace symbol, so this
    // pins the receiver-type-name fallback used for extension resolution.
    let usages = index.find_usages("com.example.shout");
    assert_eq!(
        usages.len(),
        1,
        "Expected word.shout() to be indexed as a usage of the String extension \
         function, got: {:?}",
        usages
    );
    assert_eq!(usages[0].kind, SymbolKind::FUNCTION);
}

#[tokio::test]
async fn test_kotlin_uses_java_method_call_and_type_reference_are_indexed_as_usages() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let java_path = root.join("Greeter.java");
    std::fs::write(
        &java_path,
        r#"
package com.example;

public class Greeter {
    public String greet(String name) {
        return "Hello " + name;
    }
}
"#,
    )
    .unwrap();

    let kotlin_path = root.join("UseGreeter.kt");
    std::fs::write(
        &kotlin_path,
        r#"
package com.example

fun useGreeter(g: Greeter): String {
    return g.greet("World")
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&java_path).await.unwrap();
    index.index_file(&kotlin_path).await.unwrap();

    // Type reference: the `g: Greeter` parameter type.
    let type_usages = index.find_usages("com.example.Greeter");
    assert_eq!(
        type_usages.len(),
        1,
        "Expected the Greeter parameter type to be indexed as a usage, got: {:?}",
        type_usages
    );
    assert_eq!(type_usages[0].kind, SymbolKind::CLASS);

    // Method call: `g.greet("World")` against a Java-indexed method whose
    // FQN carries a parameter descriptor (`greet(String)`), unlike Kotlin's
    // own paramless FQN convention. This must still resolve through the
    // index's existing bidirectional FQN matching (see `get_symbol`/
    // `find_usages` in index/query.rs).
    let greet_symbol = index
        .get_symbol("com.example.Greeter.greet")
        .expect("Greeter.greet should be indexed from the Java file");
    let method_usages = index.find_usages(&greet_symbol.fqn);
    assert_eq!(
        method_usages.len(),
        1,
        "Expected g.greet(\"World\") to be indexed as a usage of the Java method, got: {:?}",
        method_usages
    );
    assert_eq!(method_usages[0].kind, SymbolKind::METHOD);
}

#[tokio::test]
async fn test_find_references_through_index_finds_kotlin_usage_without_semantic_pass() {
    // This exercises the plain `QueryEngine::find_references` path - the same
    // one used by find-references/rename before any request-time semantic
    // completion pass runs. Before this campaign, a Kotlin call site would
    // never show up here at all, because Kotlin files never fed
    // `symbol_usages`; rename compensated by re-deriving usages semantically
    // at request time. This test pins that the Kotlin
    // usage is now found straight off the index.
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let java_path = root.join("Greeter.java");
    std::fs::write(
        &java_path,
        r#"
package com.example;

public class Greeter {
    public String greet(String name) {
        return "Hello " + name;
    }
}
"#,
    )
    .unwrap();

    let kotlin_path = root.join("UseGreeter.kt");
    std::fs::write(
        &kotlin_path,
        r#"
package com.example

fun useGreeter(g: Greeter): String {
    return g.greet("World")
}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&java_path).await.unwrap();
    index.index_file(&kotlin_path).await.unwrap();

    let greet_symbol = index
        .get_symbol("com.example.Greeter.greet")
        .expect("Greeter.greet should be indexed from the Java file");
    let greet_fqn = greet_symbol.fqn.to_string();

    // The plain index-backed find-references path: no semantic completion
    // pass, no re-parsing at request time beyond what indexing already did.
    let query_engine = crate::QueryEngine::new(index.clone());
    let references = query_engine
        .find_current_references(&greet_fqn)
        .expect("find_references should succeed");

    assert!(
        references.locations().any(|loc| index
            .get_file_path(loc.file_id)
            .is_some_and(|p| p == kotlin_path)),
        "Expected the Kotlin call site g.greet(\"World\") to be found via the \
         plain index find_references path, without a semantic completion pass. \
         Got: {:?}",
        references
            .locations()
            .map(|l| (l.fqn.clone(), l.file_id))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn has_class_requires_an_exact_class_like_symbol() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Service.java");
    std::fs::write(
        &path,
        "package com.example; class Service { void run(int value) {} }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.index_file(&path).await.unwrap();

    assert!(index.has_class("com.example.Service"));
    assert!(
        index.get_symbol("com.example.Service.run").is_some(),
        "legacy method lookup without a signature should still work"
    );
    assert!(
        !index.has_class("com.example.Service.run"),
        "a method-prefix match must not be mistaken for a class"
    );
    assert!(!index.has_class("com.example.Missing"));

    let kotlin_path = temp.path().join("Config.kt");
    std::fs::write(&kotlin_path, "package com.example\nobject Config").unwrap();
    index.index_file(&kotlin_path).await.unwrap();
    assert!(
        index.has_class("com.example.Config"),
        "Kotlin and Scala objects are class-like lookup targets"
    );
}

#[tokio::test]
async fn reindex_and_removal_replace_source_usage_contributions() {
    let temp = TempDir::new().unwrap();
    let a_path = temp.path().join("A.java");
    let b_path = temp.path().join("B.java");
    let use_a_path = temp.path().join("UseA.java");
    let use_b_path = temp.path().join("UseB.java");
    std::fs::write(&a_path, "package model; public class A {}").unwrap();
    std::fs::write(&b_path, "package model; public class B {}").unwrap();
    std::fs::write(
        &use_a_path,
        "package app; import model.A; class UseA { A value; }",
    )
    .unwrap();
    std::fs::write(
        &use_b_path,
        "package app; import model.B; class UseB { B first; B second; }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for path in [&a_path, &b_path, &use_a_path, &use_b_path] {
        index.index_file(path).await.unwrap();
    }

    for _ in 0..5 {
        index
            .index_file_with_source(
                &use_a_path,
                "package app; import model.A; class UseA { A value; }",
            )
            .unwrap();
    }
    assert_eq!(index.get_usage_scores(&["model.A", "model.B"]), [0.5, 1.0]);
    assert!(
        index.extract_usage_stats().is_empty(),
        "source-derived counts are revision state, not persisted interaction events"
    );

    index
        .index_file_with_source(&use_a_path, "package app; class UseA {}")
        .unwrap();
    assert_eq!(index.get_usage_score("model.A"), 0.0);
    assert_eq!(index.get_usage_score("model.B"), 1.0);

    let use_b_id = index.get_file_id(&use_b_path).expect("UseB file id");
    index.remove_file(use_b_id);
    assert_eq!(index.get_usage_score("model.B"), 0.0);
}

#[tokio::test]
async fn unpublished_revision_resolves_same_file_members_in_every_language() {
    let cases = [
        (
            "JavaLocal.java",
            "package same; class JavaLocal { void target() {} void use() { target(); } }",
            "same.JavaLocal.target",
        ),
        (
            "KotlinLocal.kt",
            "package same\nclass KotlinLocal { fun target() = 1; fun use() = target() }",
            "same.KotlinLocal.target",
        ),
        (
            "GroovyLocal.groovy",
            "package same\nclass GroovyLocal { int target() { 1 }; int use() { target() } }",
            "same.GroovyLocal.target",
        ),
        (
            "ScalaLocal.scala",
            "package same\nclass ScalaLocal { def target(): Int = 1; def use(): Int = target() }",
            "same.ScalaLocal.target",
        ),
    ];

    for (file_name, source, target_fqn) in cases {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(file_name);
        std::fs::write(&path, source).unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&path).await.unwrap();

        let target = index
            .get_symbol(target_fqn)
            .unwrap_or_else(|| panic!("{target_fqn} should be indexed from {file_name}"));
        assert_eq!(
            index.find_usages(&target.fqn).len(),
            1,
            "same-file call should resolve in {file_name}"
        );
    }
}

#[tokio::test]
async fn unpublished_revision_resolves_same_file_inheritance() {
    let cases = [
        (
            "Local.groovy",
            "package same\nclass Base { int inherited() { 1 } }\nclass Child extends Base { int use() { inherited() } }",
            "same.Base.inherited",
        ),
        (
            "Local.scala",
            "package same\nclass Base { def inherited(): Int = 1 }\nclass Child extends Base { def use(): Int = inherited() }",
            "same.Base.inherited",
        ),
    ];

    for (file_name, source, target_fqn) in cases {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(file_name);
        std::fs::write(&path, source).unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&path).await.unwrap();

        let target = index
            .get_symbol(target_fqn)
            .unwrap_or_else(|| panic!("{target_fqn} should be indexed from {file_name}"));
        assert_eq!(
            index.find_usages(&target.fqn).len(),
            1,
            "inherited same-file call should resolve to its declaration in {file_name}"
        );
    }
}

#[test]
fn unpublished_revision_masks_members_removed_from_previous_revision() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("Local.kt");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &path,
            "package same\nclass Local { fun obsolete() = 1; fun use() = obsolete() }",
        )
        .unwrap();
    let obsolete = index
        .get_symbol("same.Local.obsolete")
        .expect("old method should be published");
    assert_eq!(index.find_usages(&obsolete.fqn).len(), 1);

    index
        .index_file_with_source(
            &path,
            "package same\nclass Local { fun use() = obsolete() }",
        )
        .unwrap();

    assert!(index.get_symbol("same.Local.obsolete").is_none());
    assert!(
        index.find_usages(&obsolete.fqn).is_empty(),
        "the new revision must not resolve through its removed predecessor"
    );
}

#[tokio::test]
async fn inherited_usages_resolve_to_abstract_class_and_interface_declarations() {
    let temp = TempDir::new().unwrap();
    let base_path = temp.path().join("Base.java");
    let contract_path = temp.path().join("Contract.java");
    let java_child_path = temp.path().join("JavaChild.java");
    let kotlin_path = temp.path().join("KotlinTypes.kt");
    std::fs::write(
        &base_path,
        "package workspace; public abstract class Base { public int count; public abstract void inherited(); }",
    )
    .unwrap();
    std::fs::write(
        &contract_path,
        "package workspace; public interface Contract { void contract(); }",
    )
    .unwrap();
    std::fs::write(
        &java_child_path,
        r#"package workspace;
public abstract class JavaChild extends Base implements Contract {
    void use() { inherited(); contract(); int qualified = this.count; int bare = count; }
    Runnable reference() { return this::inherited; }
}"#,
    )
    .unwrap();
    std::fs::write(
        &kotlin_path,
        r#"package kotlinworkspace
abstract class KotlinBase {
    abstract fun inherited()
    val baseProp: Int = 1
}
interface KotlinContract { fun contract() }
abstract class KotlinChild : KotlinBase(), KotlinContract {
    fun use() { inherited(); contract(); val value = this.baseProp }
}"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    for path in [&base_path, &contract_path, &java_child_path, &kotlin_path] {
        index.index_file(path).await.unwrap();
    }

    for (target_fqn, expected_usages) in [
        ("workspace.Base.inherited", 2),
        ("workspace.Contract.contract", 1),
        ("workspace.Base.count", 2),
        ("kotlinworkspace.KotlinBase.inherited", 1),
        ("kotlinworkspace.KotlinContract.contract", 1),
        ("kotlinworkspace.KotlinBase.baseProp", 1),
    ] {
        let target = index
            .get_symbol(target_fqn)
            .unwrap_or_else(|| panic!("missing inherited target {target_fqn}"));
        assert_eq!(
            index.find_usages(&target.fqn).len(),
            expected_usages,
            "the subclass usage should be stored under {target_fqn}"
        );
    }
}

#[tokio::test]
async fn inherited_usage_resolution_crosses_external_dependency_boundary() {
    let temp = TempDir::new().unwrap();
    let java_path = temp.path().join("JavaChild.java");
    let kotlin_path = temp.path().join("KotlinChild.kt");
    std::fs::write(
        &java_path,
        "package app; import dependency.Base; public abstract class JavaChild extends Base { void use() { inherited(); } }",
    )
    .unwrap();
    std::fs::write(
        &kotlin_path,
        "package app\nimport dependency.Base\nabstract class KotlinChild : Base() { fun use() { inherited() } }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(vec![
        synthetic_type(
            "dependency.Base",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        ),
        synthetic_public_method("dependency.Base", "inherited"),
    ]);
    index.index_file(&java_path).await.unwrap();
    index.index_file(&kotlin_path).await.unwrap();

    let inherited = index
        .get_symbol("dependency.Base.inherited")
        .expect("external inherited method");
    assert_eq!(
        index.find_usages(&inherited.fqn).len(),
        2,
        "both JVM languages should attribute the call to the dependency declaration"
    );
}

/// Restoring the cached JDK image publishes hundreds of
/// owner-complete chunks, each advancing the coherent workspace generation.
/// Interactive readers capture that generation before resolving, so exact
/// equality made every concurrent navigation request fail for the whole
/// restore. An append to a source published for the first time cannot produce
/// a view that no single generation could have produced, so captured bindings
/// must survive it.
#[test]
fn additive_external_publication_preserves_captured_workspace_bindings() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Service.java");
    index
        .index_file_with_source(&path, "package p; public class Service {}")
        .unwrap();
    let captured = index.current_workspace_revision();

    for chunk in 0..3 {
        index.add_external_symbols_batch(vec![synthetic_type(
            &format!("dep.chunk{chunk}.Cached"),
            lsp_types::SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )]);
        assert_ne!(
            index.current_workspace_revision(),
            captured,
            "each published chunk is still its own exact generation"
        );
        assert!(
            index.workspace_bindings_are_current(captured),
            "chunk {chunk} must not invalidate a reader's captured bindings"
        );
        assert!(index.workspace_bindings_agree(captured, index.current_workspace_revision()));
    }
}

/// The relaxation is limited to appends: republishing a workspace source can
/// retract or rebind declarations, so readers that straddle it are abandoned
/// exactly as before.
#[test]
fn workspace_source_publication_invalidates_captured_workspace_bindings() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Service.java");
    index
        .index_file_with_source(&path, "package p; public class Service {}")
        .unwrap();
    let captured = index.current_workspace_revision();
    assert!(index.workspace_bindings_are_current(captured));

    index
        .index_file_with_source(&path, "package p; public class Service { void run() {} }")
        .unwrap();
    assert!(!index.workspace_bindings_are_current(captured));
    assert!(!index.workspace_bindings_agree(captured, index.current_workspace_revision()));

    let other = temp.path().join("Other.java");
    let captured = index.current_workspace_revision();
    index
        .index_file_with_source(&other, "package p; public class Other {}")
        .unwrap();
    assert!(!index.workspace_bindings_are_current(captured));
}

/// Replacing an already published external source is a retraction followed by
/// a registration, so it must invalidate captured bindings even though the
/// chunked append path shares its code.
#[test]
fn external_source_replacement_invalidates_captured_workspace_bindings() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let archive = temp.path().join("dep.jar");
    let publish = |fqn: &str| {
        let refresh = index.register_external_sources(std::slice::from_ref(&archive));
        let revision = refresh
            .revision_for(&archive)
            .expect("archive registration reserves a revision");
        assert!(index.add_external_symbols_responsive_for_revision(
            vec![synthetic_type(
                fqn,
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            )],
            revision,
        ));
    };

    publish("dep.Cached");
    let captured = index.current_workspace_revision();
    assert!(index.workspace_bindings_are_current(captured));

    publish("dep.Replaced");
    assert!(!index.workspace_bindings_are_current(captured));
}

/// Review: append-only is a claim about a publication's inputs, and
/// election decides the outcome. A classpath can rank a later external source
/// above an earlier one, so "registering new candidates" can replace a
/// declaration readers already see. That publication must retire their captured
/// generations even though it never explicitly retracted anything.
#[test]
fn outranking_external_publication_invalidates_captured_workspace_bindings() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let application = PathBuf::from("/deps/application.jar");
    let overriding = PathBuf::from("/deps/overriding.jar");

    let application_ticket = index.begin_external_source_refresh("gradle:application");
    let application_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &application_ticket,
            100,
            std::slice::from_ref(&application),
        )
        .expect("application classpath");
    assert!(index.add_external_symbols_responsive_for_revision(
        vec![synthetic_type(
            "javax.annotation.Nonnull",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        application_refresh
            .revision_for(&application)
            .expect("application revision"),
    ));
    let application_id = index
        .find_symbol("javax.annotation.Nonnull")
        .expect("duplicated declaration is visible")
        .file_id;

    // Registering the outranking classpath is itself a rebinding transaction;
    // capture afterwards so the test isolates the *publication*.
    let overriding_ticket = index.begin_external_source_refresh("gradle:overriding");
    let overriding_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &overriding_ticket,
            0,
            std::slice::from_ref(&overriding),
        )
        .expect("outranking classpath");
    let captured = index.current_workspace_revision();
    assert!(index.workspace_bindings_are_current(captured));

    assert!(index.add_external_symbols_responsive_for_revision(
        vec![synthetic_type(
            "javax.annotation.Nonnull",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        overriding_refresh
            .revision_for(&overriding)
            .expect("overriding revision"),
    ));
    assert_ne!(
        index
            .find_symbol("javax.annotation.Nonnull")
            .expect("declaration remains visible")
            .file_id,
        application_id,
        "the outranking source must win election for the duplicated FQN"
    );
    assert!(
        !index.workspace_bindings_are_current(captured),
        "a publication that displaced a visible declaration must retire \
         captured generations even when it only registered candidates"
    );
}

/// The mirror image: a lower-ranked source publishing the same FQN changes
/// nothing a reader can see, so the append-only classification stands.
#[test]
fn outranked_external_publication_preserves_captured_workspace_bindings() {
    let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
    let application = PathBuf::from("/deps/application.jar");
    let shadowed = PathBuf::from("/deps/shadowed.jar");

    let application_ticket = index.begin_external_source_refresh("gradle:application");
    let application_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &application_ticket,
            0,
            std::slice::from_ref(&application),
        )
        .expect("application classpath");
    assert!(index.add_external_symbols_responsive_for_revision(
        vec![synthetic_type(
            "javax.annotation.Nonnull",
            SymbolKind::CLASS,
            None,
            &[],
            crate::Modifiers::default(),
        )],
        application_refresh
            .revision_for(&application)
            .expect("application revision"),
    ));
    let application_id = index
        .find_symbol("javax.annotation.Nonnull")
        .expect("duplicated declaration is visible")
        .file_id;

    let shadowed_ticket = index.begin_external_source_refresh("gradle:shadowed");
    let shadowed_refresh = index
        .replace_external_sources_with_priority_for_refresh(
            &shadowed_ticket,
            200,
            std::slice::from_ref(&shadowed),
        )
        .expect("outranked classpath");
    let captured = index.current_workspace_revision();

    assert!(index.add_external_symbols_responsive_for_revision(
        vec![
            synthetic_type(
                "javax.annotation.Nonnull",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
            synthetic_type(
                "shadowed.OnlyHere",
                SymbolKind::CLASS,
                None,
                &[],
                crate::Modifiers::default(),
            ),
        ],
        shadowed_refresh
            .revision_for(&shadowed)
            .expect("shadowed revision"),
    ));
    assert_eq!(
        index
            .find_symbol("javax.annotation.Nonnull")
            .expect("declaration remains visible")
            .file_id,
        application_id,
        "the outranked source must not win election"
    );
    assert!(index.find_symbol("shadowed.OnlyHere").is_some());
    assert!(
        index.workspace_bindings_are_current(captured),
        "a publication that displaced nothing visible stays append-only"
    );
}

/// Every member lookup of a call site rebuilt the owner's whole
/// member list (O(n^2) over a large class). The list is now elected once per
/// published source revision, shared between lookups, and never survives a
/// republication of its file.
#[test]
fn owner_member_definitions_are_shared_per_revision_and_never_stale() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let path = temp.path().join("Big.java");
    let source = |extra: &str| {
        let mut text = String::from("package demo;\npublic class Big {\n");
        for i in 0..50 {
            text.push_str(&format!(
                "    public int m{i}(int a) {{ return a + {i}; }}\n"
            ));
        }
        text.push_str(extra);
        text.push_str("}\n");
        text
    };
    index
        .index_file_with_source(&path, &source(""))
        .expect("index Big");

    let first = {
        let inner = index.inner.read();
        selected_owner_member_definitions_shared_in(&inner, "demo.Big").expect("members")
    };
    let second = {
        let inner = index.inner.read();
        selected_owner_member_definitions_shared_in(&inner, "demo.Big").expect("members")
    };
    assert!(
        Arc::ptr_eq(&first, &second),
        "an unchanged revision must reuse the elected member list"
    );
    assert_eq!(
        index
            .find_member_definitions_by_name(
                "demo.Big",
                "m7",
                crate::DeclarationNamespace::Callable
            )
            .len(),
        1
    );

    index
        .index_file_with_source(&path, &source("    public int added() { return 1; }\n"))
        .expect("republish Big");
    let after = {
        let inner = index.inner.read();
        selected_owner_member_definitions_shared_in(&inner, "demo.Big").expect("members")
    };
    assert!(!Arc::ptr_eq(&first, &after), "a stale list was served");
    assert_eq!(
        index
            .find_member_definitions_by_name(
                "demo.Big",
                "added",
                crate::DeclarationNamespace::Callable
            )
            .len(),
        1,
        "a member added by the republication must be found"
    );
}
