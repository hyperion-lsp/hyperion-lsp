// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use hyperion_workspace::WorkspaceIndex;
use std::path::Path;

fn index_source(index: &WorkspaceIndex, root: &Path, relative: &str, source: &str) {
    index
        .index_file_with_source(&root.join(relative), source)
        .unwrap_or_else(|error| panic!("failed to index {relative}: {error}"));
}

fn access(package: &str, owner: &str) -> MemberAccessContext {
    MemberAccessContext {
        caller_package: (!package.is_empty()).then(|| package.to_string()),
        caller_owner: Some(owner.to_string()),
        is_super_access: false,
    }
}

fn labels(
    index: &WorkspaceIndex,
    receiver: &str,
    static_access: bool,
    access: &MemberAccessContext,
) -> Vec<String> {
    MemberAccessCompletionGenerator::new()
        .get_type_members(receiver, &[], static_access, access, index, false)
        .expect("member completion")
        .into_iter()
        .map(|item| item.label)
        .collect()
}

fn generated_labels(
    index: Arc<WorkspaceIndex>,
    file_id: FileId,
    source: &str,
    completion_offset: usize,
) -> Vec<String> {
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::{Language, SourceDatabase};
    use lru::LruCache;
    use parking_lot::Mutex;
    use std::num::NonZeroUsize;

    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|database| {
        database.set_file_text(file_id, Arc::new(source.to_string()));
        database.set_file_language(file_id, Language::Java);
    });
    let txn = host.read_now();
    let context = CompletionGeneratorContext::new(
        txn.snapshot(),
        index,
        file_id,
        tower_lsp::lsp_types::Position::new(0, completion_offset as u32),
        source,
        Arc::new(Mutex::new(LruCache::new(
            NonZeroUsize::new(16).expect("non-zero cache size"),
        ))),
    );
    MemberAccessCompletionGenerator::new()
        .generate(&context)
        .expect("member access generation")
        .into_iter()
        .map(|item| item.label)
        .collect()
}

#[test]
fn test_count_method_arguments_string_with_comma() {
    assert_eq!(count_method_arguments(r#"foo("a,b")"#), Some(1));
}

#[test]
fn test_count_method_arguments_two_strings_one_with_comma() {
    assert_eq!(count_method_arguments(r#"foo("a", "b,c")"#), Some(2));
}

#[test]
fn test_count_method_arguments_empty() {
    assert_eq!(count_method_arguments("foo()"), Some(0));
}

#[test]
fn test_count_method_arguments_nested_call() {
    assert_eq!(count_method_arguments("foo(bar(x), y)"), Some(2));
}

#[test]
fn test_count_method_arguments_no_parens() {
    assert_eq!(count_method_arguments("foo"), None);
}

#[test]
fn test_count_method_arguments_escaped_quote_in_string() {
    assert_eq!(count_method_arguments(r#"foo("a\"b", c)"#), Some(2));
}

#[test]
fn caller_owner_at_preserves_nested_lexical_identity() {
    let source = "package p; class Outer { class Inner { void f() { this.; } } }";
    let tokens = hyperion_parser::Lexer::new(source).collect();
    let (green, _) = hyperion_parser::Parser::parse(tokens);
    let root = hyperion_parser::syntax::SyntaxNode::new_root(green);
    let offset = source.find("this.").expect("completion site") + "this.".len();
    assert_eq!(
        MemberAccessContext::caller_owner_at(&root, offset as u32, Some("p")),
        Some("p.Outer.Inner".to_string())
    );
}

#[test]
fn member_completion_elects_overrides_and_applies_java_access_context() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "a/Base.java",
        r#"
            package a;
            public class Base {
                protected int protectedValue;
                protected static int protectedStatic;
                int packageValue;
                private int privateValue;
                public void work() {}
                private void helper() {}
                public static void baseStatic() {}
            }
        "#,
    );
    index_source(
        &index,
        temp.path(),
        "b/Child.java",
        r#"
            package b;
            import a.Base;
            public class Child extends Base {
                @Override public void work() {}
            }
        "#,
    );
    index_source(
        &index,
        temp.path(),
        "a/Peer.java",
        "package a; public class Peer {}",
    );

    let child_access = access("b", "b.Child");
    let child = labels(&index, "b.Child", false, &child_access);
    assert_eq!(child.iter().filter(|label| *label == "work").count(), 1);
    assert!(child.iter().any(|label| label == "protectedValue"));
    assert!(!child.iter().any(|label| label == "packageValue"));
    assert!(!child.iter().any(|label| label == "privateValue"));
    assert!(!child.iter().any(|label| label == "helper"));

    let incorrectly_qualified = labels(&index, "a.Base", false, &child_access);
    assert!(
        !incorrectly_qualified
            .iter()
            .any(|label| label == "protectedValue"),
        "a cross-package subclass may not use a Base-typed qualifier"
    );
    let super_access = MemberAccessContext {
        is_super_access: true,
        ..access("b", "b.Child")
    };
    assert!(
        labels(&index, "a.Base", false, &super_access)
            .iter()
            .any(|label| label == "protectedValue"),
        "super is the protected qualifier exception"
    );

    let peer = labels(&index, "a.Base", false, &access("a", "a.Peer"));
    assert!(peer.iter().any(|label| label == "protectedValue"));
    assert!(peer.iter().any(|label| label == "packageValue"));
    assert!(!peer.iter().any(|label| label == "privateValue"));

    let owner = labels(&index, "a.Base", false, &access("a", "a.Base"));
    assert!(owner.iter().any(|label| label == "privateValue"));
    assert!(owner.iter().any(|label| label == "helper"));
    assert!(labels(&index, "b.Child", true, &child_access)
        .iter()
        .any(|label| label == "baseStatic"));
    assert!(
        labels(&index, "a.Base", true, &child_access)
            .iter()
            .any(|label| label == "protectedStatic"),
        "a cross-package subclass may qualify a protected static with its declaring type"
    );
    assert!(
        !labels(&index, "a.Base", true, &access("q", "q.Peer"))
            .iter()
            .any(|label| label == "protectedStatic"),
        "an unrelated cross-package caller may not access protected static state"
    );
}

#[test]
fn member_completion_resolves_this_and_super_at_real_completion_sites() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let base_path = temp.path().join("a/Base.java");
    let base = "package a; public class Base { protected int inherited; private int secret; void own() { this. } }";
    index
        .index_file_with_source(&base_path, base)
        .expect("base");
    let base_id = index.get_file_id(&base_path).expect("base file id");
    let base_labels = generated_labels(
        Arc::clone(&index),
        base_id,
        base,
        base.find("this.").expect("this completion") + "this.".len(),
    );
    assert!(base_labels.iter().any(|label| label == "secret"));

    let child_path = temp.path().join("b/Child.java");
    let child = "package b; public class Child extends a.Base { void child() { super. } }";
    index
        .index_file_with_source(&child_path, child)
        .expect("child");
    let child_id = index.get_file_id(&child_path).expect("child file id");
    let child_labels = generated_labels(
        Arc::clone(&index),
        child_id,
        child,
        child.find("super.").expect("super completion") + "super.".len(),
    );
    assert!(child_labels.iter().any(|label| label == "inherited"));
    assert!(!child_labels.iter().any(|label| label == "secret"));
}

#[test]
fn member_completion_does_not_resurrect_package_fields_across_packages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "a/Base.java",
        "package a; public class Base { int packageValue; }",
    );
    index_source(
        &index,
        temp.path(),
        "b/Middle.java",
        "package b; public class Middle extends a.Base {}",
    );
    index_source(
        &index,
        temp.path(),
        "a/Child.java",
        "package a; public class Child extends b.Middle {}",
    );

    let child = labels(&index, "a.Child", false, &access("a", "a.Child"));
    assert!(!child.iter().any(|label| label == "packageValue"));
}

#[test]
fn member_completion_preserves_field_ambiguity_before_access_filtering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "p/Flag.java",
        "package p; public interface Flag { int value = 1; }",
    );
    index_source(
        &index,
        temp.path(),
        "p/Base.java",
        "package p; public class Base { public static int value; }",
    );
    index_source(
        &index,
        temp.path(),
        "p/Ambiguous.java",
        "package p; public class Ambiguous extends Base implements Flag {}",
    );
    index_source(
        &index,
        temp.path(),
        "p/Resolved.java",
        "package p; public class Resolved extends Base implements Flag { private static int value; }",
    );
    index_source(
        &index,
        temp.path(),
        "q/Peer.java",
        "package q; public class Peer {}",
    );

    let ambiguous = labels(&index, "p.Ambiguous", true, &access("p", "p.Ambiguous"));
    assert_eq!(
        ambiguous.iter().filter(|label| *label == "value").count(),
        2,
        "distinct inherited declarations remain ambiguous"
    );

    let owner = labels(&index, "p.Resolved", true, &access("p", "p.Resolved"));
    assert_eq!(owner.iter().filter(|label| *label == "value").count(), 1);
    let peer = labels(&index, "p.Resolved", true, &access("q", "q.Peer"));
    assert_eq!(
        peer.iter().filter(|label| *label == "value").count(),
        0,
        "an inaccessible hiding field must not reveal hidden ancestors"
    );
}

#[test]
fn member_completion_obeys_interface_static_inheritance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "api/Root.java",
        r#"
            package api;
            public interface Root {
                static Root create() { return null; }
                default void run() {}
                int CONSTANT = 1;
            }
        "#,
    );
    index_source(
        &index,
        temp.path(),
        "api/Child.java",
        "package api; public interface Child extends Root {}",
    );
    index_source(
        &index,
        temp.path(),
        "impl/Impl.java",
        "package impl; public class Impl implements api.Child {}",
    );

    let root_static = labels(&index, "api.Root", true, &access("api", "api.Root"));
    assert!(root_static.iter().any(|label| label == "create"));
    assert!(root_static.iter().any(|label| label == "CONSTANT"));

    let child_static = labels(&index, "api.Child", true, &access("api", "api.Child"));
    assert!(!child_static.iter().any(|label| label == "create"));
    assert!(child_static.iter().any(|label| label == "CONSTANT"));

    let implementation_access = access("impl", "impl.Impl");
    let implementation = labels(&index, "impl.Impl", false, &implementation_access);
    assert!(implementation.iter().any(|label| label == "run"));
    assert!(!implementation.iter().any(|label| label == "create"));
    assert!(!implementation.iter().any(|label| label == "CONSTANT"));
    assert!(labels(&index, "impl.Impl", true, &implementation_access)
        .iter()
        .any(|label| label == "CONSTANT"));
}

#[test]
fn kotlin_interface_properties_remain_instance_members() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "api/KotlinContract.kt",
        "package api\ninterface KotlinContract { val value: String }",
    );

    let fields = index.find_effective_fields("api.KotlinContract");
    let value = fields
        .iter()
        .find(|field| field.simple_name.as_ref() == "value")
        .expect("Kotlin interface property should be indexed");
    assert!(
        !value.modifiers.is_static,
        "the regression must exercise the implicit-interface rule"
    );

    let site = access("api", "api.KotlinContract");
    assert!(labels(&index, "api.KotlinContract", false, &site)
        .iter()
        .any(|label| label == "value"));
    assert!(!labels(&index, "api.KotlinContract", true, &site)
        .iter()
        .any(|label| label == "value"));
}

#[test]
fn member_completion_renders_receiver_projected_method_and_field_types() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index_source(
        &index,
        temp.path(),
        "p/Types.java",
        concat!(
            "package p; class Text {} class List<E> {} ",
            "class Base<T> { T value; T get() { return value; } } ",
            "class Mid<U> extends Base<List<U>> {} ",
            "class Child<V> extends Mid<V> {}"
        ),
    );

    let items = MemberAccessCompletionGenerator::new()
        .get_type_members(
            "p.Child",
            &["p.Text".to_string()],
            false,
            &access("p", "p.Use"),
            &index,
            false,
        )
        .expect("member completion");
    for name in ["get", "value"] {
        let item = items
            .iter()
            .find(|item| item.label == name)
            .unwrap_or_else(|| panic!("missing {name} completion"));
        // The projected (receiver-relative) type travels in the metadata
        // fully qualified, and is shown shortened next to the label.
        let metadata =
            crate::completion::metadata::CompletionItemMetadata::from_item(item).expect("metadata");
        assert_eq!(metadata.item_type.as_deref(), Some("p.List<p.Text>"));
        assert_eq!(
            item.label_details
                .as_ref()
                .and_then(|details| details.description.as_deref()),
            Some("List<Text>")
        );
    }
}

#[test]
fn member_completion_has_no_depth_or_member_count_correctness_caps() {
    let temp = tempfile::tempdir().expect("tempdir");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let fields = (0..210)
        .map(|number| format!("public int field{number};"))
        .collect::<Vec<_>>()
        .join("\n");
    index_source(
        &index,
        temp.path(),
        "p/C0.java",
        &format!("package p; public class C0 {{ {fields} }}"),
    );
    for depth in 1..=10 {
        index_source(
            &index,
            temp.path(),
            &format!("p/C{depth}.java"),
            &format!(
                "package p; public class C{depth} extends C{} {{}}",
                depth - 1
            ),
        );
    }

    let completed = labels(&index, "p.C10", false, &access("p", "p.C10"));
    assert!(completed.iter().any(|label| label == "field0"));
    assert!(completed.iter().any(|label| label == "field209"));
    assert_eq!(
        completed
            .iter()
            .filter(|label| label.starts_with("field"))
            .count(),
        210
    );
}
