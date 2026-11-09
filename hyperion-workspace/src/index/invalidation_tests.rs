// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use tempfile::TempDir;

#[test]
fn same_package_superclass_is_a_declaration_dependent_without_an_import() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let base_path = temp.path().join("p/Base.java");
    let child_path = temp.path().join("p/Child.java");

    index
        .index_file_with_source(
            &base_path,
            "package p; public class Base { protected int inherited; }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &child_path,
            "package p; public class Child extends Base { int read() { return inherited; } }",
        )
        .unwrap();
    let child_id = index.get_file_id(&child_path).unwrap();

    let stats = index
        .index_file_with_source(&base_path, "package p; public class Base {}")
        .unwrap();

    assert_eq!(stats.dependent_files, vec![child_id]);
}

#[test]
fn fully_qualified_interface_is_a_declaration_dependent_without_an_import() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let contract_path = temp.path().join("api/Contract.java");
    let implementation_path = temp.path().join("app/Implementation.java");

    index
        .index_file_with_source(
            &contract_path,
            "package api; public interface Contract { void run(); }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &implementation_path,
            "package app; public abstract class Implementation implements api.Contract {}",
        )
        .unwrap();
    let implementation_id = index.get_file_id(&implementation_path).unwrap();

    let stats = index
        .index_file_with_source(
            &contract_path,
            "package api; public interface Contract { void changed(); }",
        )
        .unwrap();

    assert_eq!(stats.dependent_files, vec![implementation_id]);
}

#[test]
fn exact_member_usage_is_a_declaration_dependent_without_an_import() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("p/Target.java");
    let consumer_path = temp.path().join("p/Consumer.java");

    index
        .index_file_with_source(
            &target_path,
            "package p; public class Target { public int value; }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &consumer_path,
            "package p; class Consumer { int read(Target target) { return target.value; } }",
        )
        .unwrap();
    let consumer_id = index.get_file_id(&consumer_path).unwrap();
    assert!(index
        .find_usages("p.Target.value")
        .iter()
        .any(|usage| usage.file_id == consumer_id));

    let stats = index
        .index_file_with_source(&target_path, "package p; public class Target {}")
        .unwrap();

    assert_eq!(stats.dependent_files, vec![consumer_id]);
}

#[test]
fn publishing_a_missing_supertype_invalidates_contextual_and_qualified_subtypes() {
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let same_package_child_path = temp.path().join("late/SamePackageChild.java");
    let qualified_child_path = temp.path().join("other/QualifiedChild.java");
    let base_path = temp.path().join("late/Base.java");

    index
        .index_file_with_source(
            &same_package_child_path,
            "package late; public class SamePackageChild extends Base {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &qualified_child_path,
            "package other; public class QualifiedChild extends late.Base {}",
        )
        .unwrap();
    let mut expected = vec![
        index.get_file_id(&same_package_child_path).unwrap(),
        index.get_file_id(&qualified_child_path).unwrap(),
    ];
    expected.sort_unstable_by_key(|file_id| file_id.as_u32());

    let stats = index
        .index_file_with_source(&base_path, "package late; public class Base {}")
        .unwrap();

    assert_eq!(stats.dependent_files, expected);
}
