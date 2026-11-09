// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

fn external_type(fqn: &str) -> crate::Symbol {
    crate::Symbol {
        fqn: fqn.into(),
        simple_name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
        range: crate::TextRange::new(0, 0),
        kind: lsp_types::SymbolKind::CLASS,
        modifiers: crate::Modifiers {
            is_public: true,
            ..crate::Modifiers::default()
        },
        container: None,
        return_type: None,
        parameters: Vec::new(),
        parent_class: None,
        interfaces: Vec::new(),
        documentation: None,
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

#[test]
fn descriptorless_callable_definition_requires_one_overload() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Util.java"),
            "package p; class Util { void one() {} void pick(int x) {} void pick(String x) {} }",
        )
        .unwrap();

    let callable = |fqn: &str| crate::SymbolTarget::new(fqn, crate::DeclarationNamespace::Callable);
    assert!(index
        .find_symbol_definition(&callable("p.Util.one"))
        .is_some());
    assert!(index
        .find_symbol_definition(&callable("p.Util.pick"))
        .is_none());
    assert!(index
        .find_symbol_definition(&callable("p.Util.pick(int)"))
        .is_some());
}

#[test]
fn virtual_deletion_reveals_the_shadowed_source_publication() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbol(external_type("p.Target"));
    index
        .index_file_with_source(
            &temp.path().join("Target.java"),
            "package p; public class Target {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Use.java"),
            "package q; import p.Target; class Use { Target value; }",
        )
        .unwrap();

    let target = crate::SymbolTarget::new("p.Target", crate::DeclarationNamespace::Type);
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                &[target],
                index.current_workspace_revision(),
            )
            .unwrap(),
        0,
        "removing the workspace publication must reveal the dependency publication",
    );
}

#[test]
fn import_dependency_identity_retains_each_alias_spelling() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Target.java"),
            "package p; public class Target {}",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Use.groovy"),
            "package q\nimport p.Target as First\nimport p.Target as Second\nclass Use {}",
        )
        .unwrap();

    let target = crate::SymbolTarget::new("p.Target", crate::DeclarationNamespace::Type);
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                &[target],
                index.current_workspace_revision(),
            )
            .unwrap(),
        2,
    );
}

#[test]
fn deleting_one_static_overload_keeps_a_valid_single_static_import() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let utility_path = temp.path().join("Util.java");
    let consumer_path = temp.path().join("Consumer.java");
    index
        .index_file_with_source(
            &utility_path,
            concat!(
                "package p; public class Util { ",
                "public static void pick(int value) {} ",
                "public static void pick(String value) {} }",
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &consumer_path,
            "package q; import static p.Util.pick; class Consumer {}",
        )
        .unwrap();

    let methods = index.find_methods_by_name("p.Util", "pick");
    assert_eq!(methods.len(), 2);
    let first = crate::SymbolTarget::for_symbol(&methods[0]);
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                std::slice::from_ref(&first),
                index.current_workspace_revision(),
            )
            .unwrap(),
        0,
        "the other accessible overload keeps the import declaration valid"
    );

    let all = methods
        .iter()
        .map(|method| crate::SymbolTarget::for_symbol(method))
        .collect::<Vec<_>>();
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                &all,
                index.current_workspace_revision(),
            )
            .unwrap(),
        1,
    );
}

#[test]
fn deleting_a_hiding_static_field_reveals_the_inherited_binding() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Parent.java"),
            "package p; public class Parent { public static int value; }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Parent { ",
                "public static int value; }",
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Consumer.java"),
            "package q; import static p.Child.value; class Consumer {}",
        )
        .unwrap();

    let child = index
        .find_member_symbols_by_name("p.Child", "value", crate::DeclarationNamespace::Value)
        .pop()
        .expect("child field");
    let parent = index
        .find_member_symbols_by_name("p.Parent", "value", crate::DeclarationNamespace::Value)
        .pop()
        .expect("parent field");
    let child = crate::SymbolTarget::for_symbol(&child);
    let parent = crate::SymbolTarget::for_symbol(&parent);

    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                std::slice::from_ref(&child),
                index.current_workspace_revision(),
            )
            .unwrap(),
        0,
        "removing the hiding declaration must reveal the inherited field",
    );
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                &[child, parent],
                index.current_workspace_revision(),
            )
            .unwrap(),
        1,
    );
}

#[test]
fn single_static_import_survives_while_any_imported_namespace_remains() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Util.java"),
            concat!(
                "package p; public class Util { ",
                "public static int same; public static void same() {} }",
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &temp.path().join("Consumer.java"),
            "package q; import static p.Util.same; class Consumer {}",
        )
        .unwrap();

    let value = index
        .find_member_symbols_by_name("p.Util", "same", crate::DeclarationNamespace::Value)
        .pop()
        .map(|symbol| crate::SymbolTarget::for_symbol(&symbol))
        .expect("static field");
    let callable = index
        .find_methods_by_name("p.Util", "same")
        .pop()
        .map(|symbol| crate::SymbolTarget::for_symbol(&symbol))
        .expect("static method");

    for remaining_namespace in [&value, &callable] {
        assert_eq!(
            index
                .count_current_import_dependencies_for_symbol_targets(
                    std::slice::from_ref(remaining_namespace),
                    index.current_workspace_revision(),
                )
                .unwrap(),
            0,
        );
    }
    assert_eq!(
        index
            .count_current_import_dependencies_for_symbol_targets(
                &[value, callable],
                index.current_workspace_revision(),
            )
            .unwrap(),
        1,
    );
}

#[test]
fn lexical_identifier_digest_is_compact_and_fails_open_for_unicode_escapes() {
    let hashes = super::super::core::lexical_identifier_hashes(
        "class Haystack { Needle value; Needle make() { return value; } }",
    )
    .expect("ordinary Java source is filterable");
    let needle = super::super::core::ascii_identifier_hash("Needle").unwrap();
    assert!(hashes.might_contain(needle));
    assert!(std::mem::size_of_val(hashes.as_ref()) <= 512);
    assert!(
        super::super::core::lexical_identifier_hashes(r"class Escaped { Object \u006eame; }")
            .is_none()
    );
}

#[test]
fn exact_usage_scan_skips_java_files_that_cannot_mention_the_target() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("Needle.java");
    let use_path = temp.path().join("Use.java");
    let noise_path = temp.path().join("Noise.java");
    index
        .index_file_with_source(
            &target_path,
            "package p; public class Needle { public Needle() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &use_path,
            "package p; class Use { Needle value = new Needle(); }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &noise_path,
            "package p; class Noise { String value = \"haystack\"; }",
        )
        .unwrap();

    let noise_id = index.get_file_id(&noise_path).unwrap();
    let current = index.find_current_usages("p.Needle").unwrap();
    assert!(current
        .locations()
        .any(|location| location.file_id == index.get_file_id(&use_path).unwrap()));
    assert!(current
        .scanned_revisions()
        .iter()
        .all(|revision| revision.file_id() != noise_id));
    assert!(index.current_usage_locations_are_current(&current));
}

#[test]
fn exact_usage_scan_prunes_a_large_irrelevant_java_workspace() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("Needle.java");
    let use_path = temp.path().join("Use.java");
    for number in 0..1_000 {
        index
            .index_file_with_source(
                &temp.path().join(format!("Noise{number}.java")),
                &format!("package noise; class Noise{number} {{ String haystack{number}; }}"),
            )
            .unwrap();
    }
    // Publish the two candidates last so this intentionally synthetic series
    // of 1,002 independent revisions does not evict their retained bytes.
    index
        .index_file_with_source(&target_path, "package p; public class Needle {}")
        .unwrap();
    index
        .index_file_with_source(&use_path, "package p; class Use { Needle value; }")
        .unwrap();

    let started = std::time::Instant::now();
    let current = index.find_current_usages("p.Needle").unwrap();
    let elapsed = started.elapsed();

    assert_eq!(
        current.scanned_revisions().len(),
        2,
        "only the declaration and lexically relevant use should be parsed"
    );
    assert_eq!(current.locations().count(), 1);
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "digest-pruned exact query took {elapsed:?}"
    );
    assert!(index.current_usage_locations_are_current(&current));
}

#[test]
fn exact_usage_extraction_is_reused_only_within_its_workspace_revision() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("Needle.java");
    let use_path = temp.path().join("Use.java");
    index
        .index_file_with_source(
            &target_path,
            "package p; public class Needle { public void touch() {} }",
        )
        .unwrap();
    index
        .index_file_with_source(
            &use_path,
            "package p; class Use { Needle value; void run() { value.touch(); } }",
        )
        .unwrap();

    let first = index.find_current_usages("p.Needle").unwrap();
    let use_revision = first
        .scanned_revisions()
        .iter()
        .copied()
        .find(|revision| revision.file_id() == index.get_file_id(&use_path).unwrap())
        .expect("usage file participates in the exact scan");
    let first_key = (first.workspace_revision, use_revision);
    let first_cached = index
        .current_usage_cache
        .lock()
        .entries
        .peek(&first_key)
        .map(|(usages, _)| Arc::clone(usages))
        .expect("first query caches the complete extraction");

    let second = index.find_current_usages("p.Needle").unwrap();
    let second_cached = index
        .current_usage_cache
        .lock()
        .entries
        .peek(&first_key)
        .map(|(usages, _)| Arc::clone(usages))
        .expect("same-revision query retains the cached extraction");
    assert!(Arc::ptr_eq(&first_cached, &second_cached));
    assert_eq!(first.locations().count(), second.locations().count());

    index
        .index_file_with_source(
            &temp.path().join("Other.java"),
            "package p; public class Other {}",
        )
        .unwrap();
    let rebound = index.find_current_usages("p.Needle").unwrap();
    let rebound_key = (rebound.workspace_revision, use_revision);
    let rebound_cached = index
        .current_usage_cache
        .lock()
        .entries
        .peek(&rebound_key)
        .map(|(usages, _)| Arc::clone(usages))
        .expect("new workspace revision receives a distinct extraction");
    assert!(!Arc::ptr_eq(&first_cached, &rebound_cached));
}

#[test]
fn exact_usage_scan_observes_cancellation_across_candidate_files() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Needle.java"),
            "package p; public class Needle {}",
        )
        .unwrap();
    for number in 0..32 {
        index
            .index_file_with_source(
                &temp.path().join(format!("Use{number}.java")),
                &format!("package p; class Use{number} {{ Needle value; }}"),
            )
            .unwrap();
    }

    let checks = std::sync::atomic::AtomicUsize::new(0);
    let result = index.find_current_usages_for_targets(&[Arc::from("p.Needle")], None, || {
        checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 4
    });
    assert_eq!(result.unwrap_err(), CurrentUsageQueryError::Cancelled);
}

#[test]
fn exact_usage_scan_resolves_calls_on_constructed_receivers() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let target_path = temp.path().join("Target.java");
    let use_path = temp.path().join("Use.java");
    index
        .index_file_with_source(
            &target_path,
            concat!(
                "package p; public class Target { ",
                "public Target self() { return this; } ",
                "public void target() {} }",
            ),
        )
        .unwrap();
    index
        .index_file_with_source(
            &use_path,
            concat!(
                "package p; class Use { void run() { ",
                "new Target().target(); ",
                "new Target().self().target(); ",
                "new p.Target().target(); } }",
            ),
        )
        .unwrap();

    let target =
        crate::SymbolTarget::new("p.Target.target()", crate::DeclarationNamespace::Callable);
    let current = index
        .find_current_usages_for_symbol_target(&target)
        .expect("constructed-receiver usages are exact");
    let locations = current.locations().collect::<Vec<_>>();
    assert_eq!(locations.len(), 3, "locations: {locations:?}");
    assert!(locations.iter().all(|location| {
        location.file_id == index.get_file_id(&use_path).unwrap()
            && location.kind == SymbolKind::METHOD
            && location.name.as_ref() == "target"
    }));
    assert!(index.current_usage_locations_are_current(&current));
}

#[test]
fn field_target_never_absorbs_same_named_method_calls() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Mixed.value"),
        namespace: Some(crate::DeclarationNamespace::Value),
        kind: CurrentUsageTargetKind::Exact,
    };

    assert_eq!(
        target.classify("p.Mixed.value", SymbolKind::FIELD),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Mixed.value", SymbolKind::METHOD),
        CurrentUsageMatch::No
    );
    assert_eq!(
        target.classify("p.Mixed.value()", SymbolKind::METHOD),
        CurrentUsageMatch::No
    );
}

#[test]
fn unqualified_method_call_is_not_indexed_as_same_named_field_read() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Mixed.java"),
            concat!(
                "package p; class Mixed { int value; void value() {} void use() { ",
                "value(); int copy = value; } }",
            ),
        )
        .expect("index colliding field and method");

    let field = crate::SymbolTarget::new("p.Mixed.value", crate::DeclarationNamespace::Value);
    let method = crate::SymbolTarget::new("p.Mixed.value()", crate::DeclarationNamespace::Callable);

    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&field)
            .expect("query field usages")
            .locations()
            .count(),
        1,
        "only the value expression is a field read"
    );
    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&method)
            .expect("query method usages")
            .locations()
            .count(),
        1,
        "the call must remain in the callable namespace"
    );
}

#[test]
fn exact_overload_rejects_descriptorless_or_placeholder_usages() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Mixed.pick(String)"),
        namespace: Some(crate::DeclarationNamespace::Callable),
        kind: CurrentUsageTargetKind::MethodOverload {
            descriptorless: Arc::from("p.Mixed.pick"),
            inherited_descriptorless: Vec::new(),
            overloads: vec![
                CallableOverload {
                    fqn: Arc::from("p.Mixed.pick(String)"),
                    parameter_count: 1,
                    variadic: false,
                },
                CallableOverload {
                    fqn: Arc::from("p.Mixed.pick(int)"),
                    parameter_count: 1,
                    variadic: false,
                },
            ],
        },
    };

    assert_eq!(
        target.classify("p.Mixed.pick(String)", SymbolKind::METHOD),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Mixed.pick(int)", SymbolKind::METHOD),
        CurrentUsageMatch::No
    );
    assert_eq!(
        target.classify("p.Mixed.pick", SymbolKind::METHOD),
        CurrentUsageMatch::AmbiguousMethod
    );
    assert_eq!(
        target.classify("p.Mixed.pick(_)", SymbolKind::METHOD),
        CurrentUsageMatch::AmbiguousMethod
    );
}

#[test]
fn exact_unique_overload_accepts_descriptorless_and_unique_arity_usages() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Mixed.pick(String)"),
        namespace: Some(crate::DeclarationNamespace::Callable),
        kind: CurrentUsageTargetKind::MethodOverload {
            descriptorless: Arc::from("p.Mixed.pick"),
            inherited_descriptorless: Vec::new(),
            overloads: vec![
                CallableOverload {
                    fqn: Arc::from("p.Mixed.pick(String)"),
                    parameter_count: 1,
                    variadic: false,
                },
                CallableOverload {
                    fqn: Arc::from("p.Mixed.pick(int,int)"),
                    parameter_count: 2,
                    variadic: false,
                },
            ],
        },
    };

    assert_eq!(
        target.classify("p.Mixed.pick", SymbolKind::METHOD),
        CurrentUsageMatch::AmbiguousMethod,
        "descriptorless usage cannot distinguish a multi-overload family"
    );
    assert_eq!(
        target.classify("p.Mixed.pick(_)", SymbolKind::METHOD),
        CurrentUsageMatch::Exact,
        "a placeholder descriptor proves the only arity-compatible overload"
    );
}

#[test]
fn explicit_method_family_accepts_every_overload_but_not_a_field() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Mixed.pick"),
        namespace: Some(crate::DeclarationNamespace::Callable),
        kind: CurrentUsageTargetKind::MethodFamily {
            descriptor_prefix: "p.Mixed.pick(".to_owned(),
        },
    };

    assert_eq!(
        target.classify("p.Mixed.pick", SymbolKind::METHOD),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Mixed.pick(String)", SymbolKind::METHOD),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Mixed.pick", SymbolKind::FIELD),
        CurrentUsageMatch::No
    );
    assert_eq!(
        target.classify("p.Mixed.picker()", SymbolKind::METHOD),
        CurrentUsageMatch::No
    );
}

#[test]
fn type_target_accepts_constructors_but_not_same_named_methods() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Widget"),
        namespace: Some(crate::DeclarationNamespace::Type),
        kind: CurrentUsageTargetKind::Type {
            constructor_prefix: "p.Widget.Widget(".to_owned(),
        },
    };

    assert_eq!(
        target.classify("p.Widget", SymbolKind::CLASS),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Widget.Widget()", SymbolKind::CONSTRUCTOR),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Widget.Widget()", SymbolKind::METHOD),
        CurrentUsageMatch::No
    );
}

#[test]
fn method_family_does_not_absorb_same_spelling_constructor() {
    let target = CurrentUsageTarget {
        fqn: Arc::from("p.Widget.Widget"),
        namespace: Some(crate::DeclarationNamespace::Callable),
        kind: CurrentUsageTargetKind::MethodFamily {
            descriptor_prefix: "p.Widget.Widget(".to_owned(),
        },
    };

    assert_eq!(
        target.classify("p.Widget.Widget()", SymbolKind::METHOD),
        CurrentUsageMatch::Exact
    );
    assert_eq!(
        target.classify("p.Widget.Widget()", SymbolKind::CONSTRUCTOR),
        CurrentUsageMatch::No
    );
}

#[test]
fn type_usage_scan_excludes_an_ordinary_method_named_like_its_class() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    let widget_path = temp.path().join("Widget.java");
    let use_path = temp.path().join("Use.java");
    let widget = concat!(
        "package p; public class Widget { ",
        "public Widget() {} public int Widget() { return 1; } }",
    );
    let use_source = concat!(
        "package q; import p.Widget; class Use { void run(Widget widget) { ",
        "Widget made = new Widget(); int value = widget.Widget(); } }",
    );
    index
        .index_file_with_source(&widget_path, widget)
        .expect("index declaration");
    index
        .index_file_with_source(&use_path, use_source)
        .expect("index usages");
    let use_id = index.get_file_id(&use_path).expect("usage file id");
    let imports = index.get_imports(use_id).expect("usage imports");
    let extracted =
        crate::usage_extractor::UsageExtractor::extract(use_source, use_id, &imports, &index)
            .expect("extract exact usages");
    assert!(extracted.iter().any(|usage| {
        usage.fqn == "p.Widget.Widget()" && usage.location.kind == SymbolKind::CONSTRUCTOR
    }));
    assert!(
        extracted.iter().any(|usage| {
            usage.fqn == "p.Widget.Widget()" && usage.location.kind == SymbolKind::METHOD
        }),
        "extracted usages: {extracted:?}"
    );

    let current = index
        .find_current_usages("p.Widget")
        .expect("current type usages");
    assert!(current
        .locations()
        .any(|location| location.kind == SymbolKind::CONSTRUCTOR));
    assert!(
        current
            .locations()
            .all(|location| location.kind != SymbolKind::METHOD),
        "a same-spelling ordinary method is not a usage of the type declaration"
    );
}

#[test]
fn nested_class_call_keeps_its_lexical_owner_when_top_level_name_collides() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Worker.java"),
            "package p; public class Worker { public void target() {} }",
        )
        .expect("index top-level collision");
    index
        .index_file_with_source(
            &temp.path().join("Outer.java"),
            concat!(
                "package p; public class Outer { public static class Worker { ",
                "public void target() {} public void invoke() { target(); } } }",
            ),
        )
        .expect("index nested declaration and call");

    let nested = crate::SymbolTarget::new(
        "p.Outer.Worker.target()",
        crate::DeclarationNamespace::Callable,
    );
    let top_level =
        crate::SymbolTarget::new("p.Worker.target()", crate::DeclarationNamespace::Callable);
    let nested_usages = index
        .find_current_usages_for_symbol_target(&nested)
        .expect("query nested method usages");
    let top_level_usages = index
        .find_current_usages_for_symbol_target(&top_level)
        .expect("query top-level method usages");

    assert_eq!(
        nested_usages.locations().count(),
        1,
        "the unqualified call belongs to the nested declaration"
    );
    assert_eq!(
        top_level_usages.locations().count(),
        0,
        "the nested call must not be attributed to the colliding top-level class"
    );
}

#[test]
fn explicit_super_call_is_attributed_to_the_superclass_declaration() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base { public void target() {} }",
        )
        .expect("index superclass");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base { ",
                "public void target() {} public void invoke() { super.target(); } }",
            ),
        )
        .expect("index subclass and super call");

    let superclass =
        crate::SymbolTarget::new("p.Base.target()", crate::DeclarationNamespace::Callable);
    let subclass =
        crate::SymbolTarget::new("p.Child.target()", crate::DeclarationNamespace::Callable);
    let superclass_usages = index
        .find_current_usages_for_symbol_target(&superclass)
        .expect("query superclass method usages");
    let subclass_usages = index
        .find_current_usages_for_symbol_target(&subclass)
        .expect("query subclass method usages");

    assert_eq!(superclass_usages.locations().count(), 1);
    assert_eq!(
        subclass_usages.locations().count(),
        0,
        "super.target() invokes the superclass implementation"
    );
}

#[test]
fn explicit_super_field_access_is_attributed_to_the_superclass_declaration() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base { public int value; }",
        )
        .expect("index superclass field");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base { public int value; ",
                "public int read() { return super.value; } }",
            ),
        )
        .expect("index subclass and super field access");

    let superclass = crate::SymbolTarget::new("p.Base.value", crate::DeclarationNamespace::Value);
    let subclass = crate::SymbolTarget::new("p.Child.value", crate::DeclarationNamespace::Value);
    let superclass_usages = index
        .find_current_usages_for_symbol_target(&superclass)
        .expect("query superclass field usages");
    let subclass_usages = index
        .find_current_usages_for_symbol_target(&subclass)
        .expect("query subclass field usages");

    assert_eq!(superclass_usages.locations().count(), 1);
    assert_eq!(
        subclass_usages.locations().count(),
        0,
        "super.value reads the superclass field"
    );
}

#[test]
fn super_method_reference_is_attributed_to_the_superclass_declaration() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base { public void target() {} }",
        )
        .expect("index superclass");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base { public void target() {} ",
                "public Runnable reference() { return super::target; } }",
            ),
        )
        .expect("index subclass and super method reference");

    let superclass =
        crate::SymbolTarget::new("p.Base.target()", crate::DeclarationNamespace::Callable);
    let subclass =
        crate::SymbolTarget::new("p.Child.target()", crate::DeclarationNamespace::Callable);
    let superclass_usages = index
        .find_current_usages_for_symbol_target(&superclass)
        .expect("query superclass method usages");
    let subclass_usages = index
        .find_current_usages_for_symbol_target(&subclass)
        .expect("query subclass method usages");

    assert_eq!(superclass_usages.locations().count(), 1);
    assert_eq!(subclass_usages.locations().count(), 0);
}

#[test]
fn receiver_variable_uses_its_declared_superclass_method() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base { public void target() {} }",
        )
        .expect("index superclass");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            "package p; public class Child extends Base { public void target() {} }",
        )
        .expect("index subclass override");
    index
        .index_file_with_source(
            &temp.path().join("Use.java"),
            concat!(
                "package p; public class Use { public void invoke(Base receiver) { ",
                "receiver.target(); } }",
            ),
        )
        .expect("index superclass-typed receiver call");

    let superclass =
        crate::SymbolTarget::new("p.Base.target()", crate::DeclarationNamespace::Callable);
    let subclass =
        crate::SymbolTarget::new("p.Child.target()", crate::DeclarationNamespace::Callable);
    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&superclass)
            .expect("query superclass usages")
            .locations()
            .count(),
        1
    );
    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&subclass)
            .expect("query subclass usages")
            .locations()
            .count(),
        0
    );
}

#[test]
fn receiver_variable_uses_its_declared_interface_method() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Contract.java"),
            "package p; public interface Contract { void target(); }",
        )
        .expect("index interface");
    index
        .index_file_with_source(
            &temp.path().join("Implementation.java"),
            concat!(
                "package p; public class Implementation implements Contract { ",
                "public void target() {} }",
            ),
        )
        .expect("index implementing override");
    index
        .index_file_with_source(
            &temp.path().join("Use.java"),
            concat!(
                "package p; public class Use { public void invoke(Contract receiver) { ",
                "receiver.target(); } }",
            ),
        )
        .expect("index interface-typed receiver call");

    let interface =
        crate::SymbolTarget::new("p.Contract.target()", crate::DeclarationNamespace::Callable);
    let implementation = crate::SymbolTarget::new(
        "p.Implementation.target()",
        crate::DeclarationNamespace::Callable,
    );
    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&interface)
            .expect("query interface usages")
            .locations()
            .count(),
        1
    );
    assert_eq!(
        index
            .find_current_usages_for_symbol_target(&implementation)
            .expect("query implementation usages")
            .locations()
            .count(),
        0
    );
}

#[test]
fn inherited_field_variable_keeps_its_declared_receiver_type() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index receiver type");
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base { protected Service service; }",
        )
        .expect("index inherited field");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base { public void invoke() { ",
                "service.target(); } }",
            ),
        )
        .expect("index inherited field receiver call");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query receiver method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "service.target() must use the inherited field's declared type"
    );
}

#[test]
fn inherited_interface_field_keeps_its_declared_receiver_type() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index receiver type");
    index
        .index_file_with_source(
            &temp.path().join("HasService.java"),
            concat!(
                "package p; public interface HasService { ",
                "Service service = new Service(); }",
            ),
        )
        .expect("index inherited interface field");
    index
        .index_file_with_source(
            &temp.path().join("Use.java"),
            concat!(
                "package p; public class Use implements HasService { public void invoke() { ",
                "service.target(); } }",
            ),
        )
        .expect("index inherited interface field receiver call");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query receiver method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "service.target() must use the inherited interface field's declared type"
    );
}

#[test]
fn inherited_generic_field_uses_its_projected_receiver_type() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index receiver type");
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base<T> { protected T service; }",
        )
        .expect("index generic inherited field");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base<Service> { public void invoke() { ",
                "service.target(); } }",
            ),
        )
        .expect("index projected inherited field receiver call");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query receiver method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "service.target() must project the inherited field's type argument"
    );
}

#[test]
fn inherited_generic_method_projects_its_chained_receiver_type() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index chained receiver type");
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            concat!(
                "package p; public class Base<T> { protected T service() { ",
                "return null; } }",
            ),
        )
        .expect("index generic inherited method");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base<Service> { public void invoke() { ",
                "service().target(); } }",
            ),
        )
        .expect("index projected inherited method chain");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query chained method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "service().target() must project the inherited return type"
    );
}

#[test]
fn inherited_parameterized_field_preserves_its_chained_receiver_arguments() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index final receiver type");
    index
        .index_file_with_source(
            &temp.path().join("Factory.java"),
            concat!(
                "package p; public class Factory<T> { public T service() { ",
                "return null; } }",
            ),
        )
        .expect("index generic factory");
    index
        .index_file_with_source(
            &temp.path().join("Base.java"),
            "package p; public class Base<T> { protected T factory; }",
        )
        .expect("index inherited generic field");
    index
        .index_file_with_source(
            &temp.path().join("Child.java"),
            concat!(
                "package p; public class Child extends Base<Factory<Service>> { ",
                "public void invoke() { factory.service().target(); } }",
            ),
        )
        .expect("index inherited parameterized receiver chain");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query final method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "the projected Factory<Service> field type must retain Service"
    );
}

#[test]
fn parameterized_variable_projects_chained_method_return_type() {
    let temp = tempfile::tempdir().expect("temporary workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index
        .index_file_with_source(
            &temp.path().join("Service.java"),
            "package p; public class Service { public void target() {} }",
        )
        .expect("index chained receiver type");
    index
        .index_file_with_source(
            &temp.path().join("Factory.java"),
            concat!(
                "package p; public class Factory<T> { public T service() { ",
                "return null; } }",
            ),
        )
        .expect("index generic factory");
    index
        .index_file_with_source(
            &temp.path().join("Use.java"),
            concat!(
                "package p; public class Use { void invoke(Factory<Service> factory) { ",
                "factory.service().target(); } }",
            ),
        )
        .expect("index parameterized receiver chain");

    let target =
        crate::SymbolTarget::new("p.Service.target()", crate::DeclarationNamespace::Callable);
    let usages = index
        .find_current_usages_for_symbol_target(&target)
        .expect("query chained method usages");

    assert_eq!(
        usages.locations().count(),
        1,
        "Factory<Service>.service() must project T to Service"
    );
}
