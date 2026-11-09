// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive tests for ImportResolver

use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::{ImportMap, Modifiers, Symbol, TextRange, WorkspaceIndex};
use lsp_types::SymbolKind;
use std::path::PathBuf;

fn make_test_workspace() -> WorkspaceIndex {
    WorkspaceIndex::new(PathBuf::from("/tmp/test"))
}

fn make_class_symbol(fqn: &str, simple_name: &str) -> Symbol {
    Symbol {
        fqn: fqn.into(),
        simple_name: simple_name.into(),
        range: TextRange::new(0, 100),
        kind: SymbolKind::CLASS,
        modifiers: Modifiers {
            is_public: true,
            ..Modifiers::default()
        },
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
    }
}

fn make_static_method_symbol(owner: &str, name: &str) -> Symbol {
    Symbol {
        fqn: format!("{owner}.{name}()").into(),
        simple_name: name.into(),
        range: TextRange::new(0, 1),
        kind: SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: true,
            ..Modifiers::default()
        },
        container: Some(owner.into()),
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
    }
}

fn add_static_method(workspace: &WorkspaceIndex, owner: &str, name: &str) {
    let simple_name = owner.rsplit('.').next().unwrap_or(owner);
    workspace.add_external_symbol(make_class_symbol(owner, simple_name));
    workspace.add_external_symbol(make_static_method_symbol(owner, name));
}

fn make_basic_imports() -> ImportMap {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example".into());
    imports.add_import("List".to_string(), "java.util.List".to_string());
    imports.add_import("Map".to_string(), "java.util.Map".to_string());
    imports.add_wildcard("com.example.utils".to_string());
    imports.add_static_import("max".to_string(), "java.lang.Math.max".to_string());
    imports
}

#[test]
fn test_resolve_explicit_import_single_result() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("List");

    // Explicit import should be definitive and return only one result
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "java.util.List");
}

#[test]
fn test_resolve_explicit_import_takes_precedence() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    // Even though List exists in java.util wildcard and other places,
    // explicit import should be the only result
    let result = resolver.resolve("List");
    assert_eq!(result, vec!["java.util.List"]);
}

#[test]
fn test_resolve_java_lang_class() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("String");

    // Should include java.lang.String
    assert!(result.contains(&"java.lang.String".to_string()));
}

#[test]
fn test_resolve_java_lang_object() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("Object");
    assert!(result.contains(&"java.lang.Object".to_string()));
}

#[test]
fn test_resolve_java_lang_integer() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("Integer");
    assert!(result.contains(&"java.lang.Integer".to_string()));
}

#[test]
fn test_resolve_java_lang_exception() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("Exception");
    assert!(result.contains(&"java.lang.Exception".to_string()));
}

#[test]
fn test_resolve_current_package() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("MyClass");

    // Should include current package
    assert!(result.contains(&"com.example.MyClass".to_string()));
}

#[test]
fn test_resolve_with_default_package() {
    let mut imports = ImportMap::new();
    imports.current_package = None; // Default package

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("MyClass");

    // In default package, should just return the simple name
    assert!(result.contains(&"MyClass".to_string()));
}

#[test]
fn test_resolve_wildcard_import() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("Helper");

    // Should include wildcard import candidate
    assert!(result.contains(&"com.example.utils.Helper".to_string()));
}

#[test]
fn test_resolve_multiple_wildcard_candidates() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.app".into());
    imports.add_wildcard("com.example.a".to_string());
    imports.add_wildcard("com.example.b".to_string());
    imports.add_wildcard("com.example.c".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Foo");

    // Should have candidates from current package and all wildcards
    assert!(result.contains(&"com.app.Foo".to_string()));
    assert!(result.contains(&"com.example.a.Foo".to_string()));
    assert!(result.contains(&"com.example.b.Foo".to_string()));
    assert!(result.contains(&"com.example.c.Foo".to_string()));
}

#[test]
fn test_resolve_no_matches() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example".into());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("UnknownClass");

    // Should still have current package as candidate
    assert!(result.contains(&"com.example.UnknownClass".to_string()));
}

#[test]
fn test_resolve_static_explicit() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    add_static_method(&workspace, "java.lang.Math", "max");
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve_static("max");

    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "java.lang.Math.max");
}

#[test]
fn test_resolve_static_wildcard() {
    let mut imports = ImportMap::new();
    imports.add_static_wildcard("java.util.Collections".to_string());
    imports.add_static_wildcard("java.lang.Math".to_string());

    let workspace = make_test_workspace();
    add_static_method(&workspace, "java.lang.Math", "max");
    add_static_method(&workspace, "java.util.Collections", "sort");
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve_static("sort");

    // Only the indexed, viable member is an exact binding.
    assert_eq!(result, ["java.util.Collections.sort"]);
}

#[test]
fn test_resolve_static_multiple() {
    let mut imports = ImportMap::new();
    imports.add_static_import("max".to_string(), "java.lang.Math.max".to_string());
    imports.add_static_import("min".to_string(), "java.lang.Math.min".to_string());

    let workspace = make_test_workspace();
    add_static_method(&workspace, "java.lang.Math", "max");
    add_static_method(&workspace, "java.lang.Math", "min");
    let resolver = ImportResolver::new(&imports, &workspace);

    let result1 = resolver.resolve_static("max");
    assert_eq!(result1, vec!["java.lang.Math.max"]);

    let result2 = resolver.resolve_static("min");
    assert_eq!(result2, vec!["java.lang.Math.min"]);
}

#[test]
fn test_resolve_static_no_matches() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve_static("unknownMethod");
    assert_eq!(result.len(), 0);
}

#[test]
fn test_is_imported_explicit() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert!(resolver.is_imported("java.util.List", "List"));
    assert!(resolver.is_imported("java.util.Map", "Map"));
    assert!(!resolver.is_imported("java.util.Set", "Set")); // Not imported
}

#[test]
fn test_is_imported_java_lang() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert!(resolver.is_imported("java.lang.String", "String"));
    assert!(resolver.is_imported("java.lang.Object", "Object"));
    assert!(resolver.is_imported("java.lang.Integer", "Integer"));
}

#[test]
fn test_is_imported_current_package() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert!(resolver.is_imported("com.example.MyClass", "MyClass"));
    assert!(resolver.is_imported("com.example.Helper", "Helper"));
    assert!(!resolver.is_imported("com.other.MyClass", "MyClass"));
}

#[test]
fn test_is_imported_wildcard() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert!(resolver.is_imported("com.example.utils.Helper", "Helper"));
    assert!(resolver.is_imported("com.example.utils.Tool", "Tool"));
    assert!(!resolver.is_imported("com.other.utils.Helper", "Helper"));
}

#[test]
fn test_is_imported_wrong_fqn() {
    let imports = make_basic_imports();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    // List is imported from java.util, not java.awt
    assert!(!resolver.is_imported("java.awt.List", "List"));
}

#[test]
fn test_resolution_priority() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example".into());
    imports.add_import("Test".to_string(), "org.junit.Test".to_string());
    imports.add_wildcard("com.example.test".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Test");

    // Explicit import should be the only result (highest priority)
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "org.junit.Test");
}

#[test]
fn test_resolve_with_empty_imports() {
    let imports = ImportMap::new();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let result = resolver.resolve("String");

    // Should still resolve java.lang classes
    assert!(result.contains(&"java.lang.String".to_string()));
}

#[test]
fn test_resolve_common_java_lang_classes() {
    let imports = ImportMap::new();
    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    let classes = vec![
        "Object",
        "String",
        "Integer",
        "Long",
        "Double",
        "Float",
        "Boolean",
        "Character",
        "Byte",
        "Short",
        "Math",
        "System",
        "Thread",
        "Exception",
        "RuntimeException",
        "Error",
        "Class",
    ];

    for class in classes {
        let result = resolver.resolve(class);
        let expected_fqn = format!("java.lang.{}", class);
        assert!(
            result.contains(&expected_fqn),
            "Failed to resolve {} to {}",
            class,
            expected_fqn
        );
    }
}

#[test]
fn test_resolve_with_multiple_packages() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.acme.app.services".into());
    imports.add_import("UserService".into(), "com.acme.user.UserService".into());
    imports.add_wildcard("com.acme.models".to_string());
    imports.add_wildcard("com.acme.utils".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Service");

    // Should include current package and wildcard candidates
    assert!(result.contains(&"com.acme.app.services.Service".to_string()));
    assert!(result.contains(&"com.acme.models.Service".to_string()));
    assert!(result.contains(&"com.acme.utils.Service".to_string()));
}

#[test]
fn test_resolve_nested_package_names() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example.app.services.impl.v2".into());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("MyService");

    assert!(result.contains(&"com.example.app.services.impl.v2.MyService".to_string()));
}

#[test]
fn test_static_import_explicit_takes_precedence() {
    let mut imports = ImportMap::new();
    imports.add_static_import("max".to_string(), "com.example.Utils.max".to_string());
    imports.add_static_wildcard("java.lang.Math".to_string());

    let workspace = make_test_workspace();
    add_static_method(&workspace, "com.example.Utils", "max");
    add_static_method(&workspace, "java.lang.Math", "max");
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve_static("max");

    // Explicit static import should appear
    assert!(result.contains(&"com.example.Utils.max".to_string()));
}

#[test]
fn test_resolve_handles_duplicate_wildcards() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example".into());
    imports.add_wildcard("java.util".to_string());
    imports.add_wildcard("java.util".to_string()); // Duplicate

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("ArrayList");

    // Should not have duplicates
    let java_util_count = result
        .iter()
        .filter(|s| *s == "java.util.ArrayList")
        .count();
    assert_eq!(java_util_count, 1);
}

#[test]
fn test_is_imported_case_sensitive() {
    let mut imports = ImportMap::new();
    imports.add_import("List".to_string(), "java.util.List".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    // Java is case-sensitive
    assert!(resolver.is_imported("java.util.List", "List"));
    assert!(!resolver.is_imported("java.util.List", "list")); // lowercase
    assert!(!resolver.is_imported("java.util.List", "LIST")); // uppercase
}

#[test]
fn test_resolve_with_single_level_package() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("example".into());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("MyClass");

    assert!(result.contains(&"example.MyClass".to_string()));
}

#[test]
fn test_complex_resolution_scenario() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.acme.app".into());
    imports.add_import("User".to_string(), "com.acme.models.User".to_string());
    imports.add_import("List".to_string(), "java.util.List".to_string());
    imports.add_wildcard("com.acme.utils".to_string());
    imports.add_wildcard("java.io".to_string());
    imports.add_static_import("max".to_string(), "java.lang.Math.max".to_string());
    imports.add_static_wildcard("java.util.Collections".to_string());

    let workspace = make_test_workspace();
    add_static_method(&workspace, "java.lang.Math", "max");
    add_static_method(&workspace, "java.util.Collections", "sort");
    let resolver = ImportResolver::new(&imports, &workspace);

    // Test explicit imports
    assert_eq!(resolver.resolve("User"), vec!["com.acme.models.User"]);
    assert_eq!(resolver.resolve("List"), vec!["java.util.List"]);

    // Test java.lang
    let string_result = resolver.resolve("String");
    assert!(string_result.contains(&"java.lang.String".to_string()));

    // Test current package and wildcards
    let helper_result = resolver.resolve("Helper");
    assert!(helper_result.contains(&"com.acme.app.Helper".to_string()));
    assert!(helper_result.contains(&"com.acme.utils.Helper".to_string()));

    // Test static imports
    let max_result = resolver.resolve_static("max");
    assert_eq!(max_result, ["java.lang.Math.max"]); // explicit import shadows wildcard imports
    let sort_result = resolver.resolve_static("sort");
    assert_eq!(sort_result, ["java.util.Collections.sort"]);
}

#[test]
fn test_resolve_with_no_current_package() {
    let mut imports = ImportMap::new();
    imports.current_package = None;
    imports.add_import("List".to_string(), "java.util.List".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    // Explicit import should work
    let list_result = resolver.resolve("List");
    assert_eq!(list_result, vec!["java.util.List"]);

    // Unknown class should resolve to simple name (default package)
    let unknown_result = resolver.resolve("MyClass");
    assert!(unknown_result.contains(&"MyClass".to_string()));
}

#[test]
fn test_static_wildcard_multiple_candidates() {
    let mut imports = ImportMap::new();
    imports.add_static_wildcard("java.lang.Math".to_string());
    imports.add_static_wildcard("java.lang.StrictMath".to_string());
    imports.add_static_wildcard("com.example.MathUtils".to_string());

    let workspace = make_test_workspace();
    for owner in [
        "java.lang.Math",
        "java.lang.StrictMath",
        "com.example.MathUtils",
    ] {
        add_static_method(&workspace, owner, "sqrt");
    }
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve_static("sqrt");

    assert!(result.is_empty(), "multiple viable owners are ambiguous");
}

// JLS §6.5.5 — types in the compilation unit's own package shadow
// the implicit java.lang import-on-demand. A workspace class com.foo.Record
// referenced from com.foo must win over java.lang.Record.
#[test]
fn test_same_package_class_shadows_java_lang() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.foo".into());

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));
    workspace.add_external_symbol(make_class_symbol("com.foo.Record", "Record"));

    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Record");

    assert_eq!(
        result[0], "com.foo.Record",
        "same-package class must shadow java.lang (JLS §6.5.5), got {result:?}"
    );
    assert!(result.contains(&"java.lang.Record".to_string()));
}

// When no same-package class actually exists, the verified
// java.lang candidate stays first so consumers that take the first
// candidate unfiltered keep preferring the JDK type.
#[test]
fn test_java_lang_first_when_no_same_package_class() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.foo".into());

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));

    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Record");

    assert_eq!(result[0], "java.lang.Record");
    assert!(result.contains(&"com.foo.Record".to_string()));
}

// Default-package types shadow java.lang too.
#[test]
fn test_default_package_class_shadows_java_lang() {
    let imports = ImportMap::new(); // current_package = None

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));
    workspace.add_external_symbol(make_class_symbol("Record", "Record"));

    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Record");

    assert_eq!(
        result[0], "Record",
        "default-package class must shadow java.lang, got {result:?}"
    );
    assert!(result.contains(&"java.lang.Record".to_string()));
}

#[test]
fn test_resolution_order_correctness() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.example".into());
    imports.add_wildcard("java.util".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("UnknownClass");

    // First candidate should be java.lang (if applicable)
    // Then current package
    // Then wildcard imports

    // For UnknownClass (not in java.lang):
    // Should have current package first
    assert!(result[0].starts_with("com.example."));
}

// `resolve` is a guessing API, but a guess must never outrank a
// candidate the workspace can prove exists. `import lib.*;` with an indexed
// `lib.Helper` and no `app.Helper` used to hand callers `app.Helper` first
// purely because the current-package tier is stronger.
#[test]
fn an_indexed_wildcard_candidate_outranks_a_nonexistent_same_package_guess() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());
    imports.add_wildcard("lib".to_string());

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("lib.Helper", "Helper"));

    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Helper");

    assert_eq!(
        result[0], "lib.Helper",
        "an existing wildcard candidate must precede a same-package guess, got {result:?}"
    );
    assert!(
        result.contains(&"app.Helper".to_string()),
        "the same-package guess must remain available as a fallback"
    );
}

// The same rule for a written fully qualified name. `java.util.List`
// used to yield `app.java.util.List` as the first candidate.
#[test]
fn a_written_fully_qualified_name_outranks_its_package_prefixed_guess() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("java.util.List", "List"));

    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("java.util.List");

    assert_eq!(
        result[0], "java.util.List",
        "an indexed fully qualified spelling must win over a prefixed guess, got {result:?}"
    );
}

// Same-package shadowing stays intact under the existence-first rule: a same-package class
// that exists still shadows java.lang, and when it does not exist the
// existence-verified java.lang candidate leads.
#[test]
fn existence_ordering_preserves_the_java_lang_shadowing_rule() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("com.foo".into());

    let shadowed = make_test_workspace();
    shadowed.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));
    shadowed.add_external_symbol(make_class_symbol("com.foo.Record", "Record"));
    assert_eq!(
        ImportResolver::new(&imports, &shadowed).resolve("Record")[0],
        "com.foo.Record"
    );

    let jdk_only = make_test_workspace();
    jdk_only.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));
    assert_eq!(
        ImportResolver::new(&imports, &jdk_only).resolve("Record")[0],
        "java.lang.Record"
    );
}

// With nothing indexed every candidate is a guess, so the JLS tier
// order is the whole answer and the current package still leads.
#[test]
fn tier_order_is_preserved_when_no_candidate_exists() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());
    imports.add_wildcard("lib".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);
    let result = resolver.resolve("Unknown");

    assert_eq!(
        result,
        vec!["app.Unknown".to_string(), "lib.Unknown".to_string()]
    );
}

// The "is this name merely cold?" question now lives on
// the resolver instead of being re-derived from candidates by the semantics
// layer. Exactly one reading whose package has nothing indexed is not
// evidence of a typo.
#[test]
fn a_sole_reading_of_an_unindexed_package_is_reported_as_cold() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert_eq!(
        resolver.sole_reading_while_dependency_is_cold("Ghost"),
        Some("app.Ghost".to_string())
    );
}

// Two readings are a genuine ambiguity, so the caller must report.
#[test]
fn two_readings_are_never_cold() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());
    imports.add_wildcard("missing.dep".to_string());

    let workspace = make_test_workspace();
    let resolver = ImportResolver::new(&imports, &workspace);

    assert_eq!(
        resolver.sole_reading_while_dependency_is_cold("Ghost"),
        None
    );
}

// An indexed package is evidence: the spelling is simply wrong.
#[test]
fn a_sole_reading_of_an_indexed_package_is_not_cold() {
    let mut imports = ImportMap::new();
    imports.current_package = Some("app".into());

    let workspace = make_test_workspace();
    workspace.add_external_symbol(make_class_symbol("app.Widget", "Widget"));
    let resolver = ImportResolver::new(&imports, &workspace);

    assert_eq!(
        resolver.sole_reading_while_dependency_is_cold("Widgit"),
        None
    );
}
