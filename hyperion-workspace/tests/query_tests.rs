// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive tests for QueryEngine

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::query::QueryEngine;
use hyperion_workspace::FileId;
use lsp_types::{SymbolInformation, SymbolKind};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

async fn workspace_query(files: &[(&str, &str)]) -> (TempDir, QueryEngine) {
    let temp = TempDir::new().expect("create query test workspace");
    let index = WorkspaceIndex::new(temp.path().to_path_buf());

    for (relative_path, source) in files {
        let path = temp.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        fs::write(&path, source)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        index
            .index_file(&path)
            .await
            .unwrap_or_else(|error| panic!("index {}: {error}", path.display()));
    }

    (temp, QueryEngine::new(index))
}

#[allow(deprecated)]
fn workspace_symbols(engine: &QueryEngine, query: &str) -> Vec<SymbolInformation> {
    engine
        .workspace_symbols(query)
        .unwrap_or_else(|error| panic!("workspace symbol query {query:?} failed: {error}"))
}

#[allow(deprecated)]
fn only_workspace_symbol(engine: &QueryEngine, query: &str) -> SymbolInformation {
    let actual = workspace_symbols(engine, query);
    assert_eq!(
        actual.len(),
        1,
        "workspace symbol query {query:?} returned {actual:#?}"
    );
    actual.into_iter().next().expect("length checked above")
}

fn path_suffix(path: &Path) -> Vec<String> {
    let mut components: Vec<_> = path
        .components()
        .rev()
        .take(3)
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    components.reverse();
    components
}

#[tokio::test]
async fn test_goto_definition_with_explicit_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create definition file
    let list_file = root.join("List.java");
    fs::write(&list_file, "package java.util;\npublic interface List {}").unwrap();

    // Create usage file
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nimport java.util.List;\nclass User { List list; }",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    index.index_file(&list_file).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    // Query
    let query_engine = QueryEngine::new(index);
    let locations = query_engine.goto_definition("List", user_file_id).unwrap();

    assert_eq!(locations.len(), 1);
    assert_eq!(&*locations[0].name, "List");
}

#[tokio::test]
async fn test_goto_definition_with_wildcard_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create definition file
    let helper_file = root.join("Helper.java");
    fs::write(
        &helper_file,
        "package com.example.utils;\npublic class Helper {}",
    )
    .unwrap();

    // Create usage file with wildcard import
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nimport com.example.utils.*;\nclass User { Helper helper; }",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    index.index_file(&helper_file).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    // Query
    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("Helper", user_file_id)
        .unwrap();

    assert_eq!(locations.len(), 1);
    assert_eq!(&*locations[0].name, "Helper");
}

#[tokio::test]
async fn test_goto_definition_same_package() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create two classes in the same package
    let service_file = root.join("Service.java");
    fs::write(
        &service_file,
        "package com.example;\npublic class Service {}",
    )
    .unwrap();

    let controller_file = root.join("Controller.java");
    fs::write(
        &controller_file,
        "package com.example;\nclass Controller { Service service; }",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    index.index_file(&service_file).await.unwrap();
    index.index_file(&controller_file).await.unwrap();

    let controller_file_id = index.get_file_id(&controller_file).unwrap();

    // Query
    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("Service", controller_file_id)
        .unwrap();

    assert_eq!(locations.len(), 1);
    assert_eq!(&*locations[0].name, "Service");
}

#[tokio::test]
async fn test_goto_definition_java_lang_class() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create String class (normally from JDK)
    let string_file = root.join("String.java");
    fs::write(&string_file, "package java.lang;\npublic class String {}").unwrap();

    // Create usage file (String is implicitly imported from java.lang)
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nclass User { String name; }",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    index.index_file(&string_file).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    // Query
    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("String", user_file_id)
        .unwrap();

    // Should find String from java.lang
    assert!(!locations.is_empty());
    let string_loc = locations
        .iter()
        .find(|l| l.name.as_ref() == "String")
        .unwrap();
    assert_eq!(&*string_loc.name, "String");
}

#[tokio::test]
async fn test_goto_definition_multiple_candidates() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create two classes with the same simple name in different packages
    let helper1_file = root.join("Helper1.java");
    fs::write(
        &helper1_file,
        "package com.example.utils;\npublic class Helper {}",
    )
    .unwrap();

    let helper2_file = root.join("Helper2.java");
    fs::write(
        &helper2_file,
        "package com.example.tools;\npublic class Helper {}",
    )
    .unwrap();

    // Create usage file with wildcard imports for both packages
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nimport com.example.utils.*;\nimport com.example.tools.*;\nclass User { Helper helper; }",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    index.index_file(&helper1_file).await.unwrap();
    index.index_file(&helper2_file).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    // Query
    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("Helper", user_file_id)
        .unwrap();

    // Should find both Helper classes
    assert!(locations.len() >= 2);
}

#[tokio::test]
async fn test_goto_definition_no_results() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nclass User { NonExistent obj; }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("NonExistent", user_file_id)
        .unwrap();

    // Won't find NonExistent since it's not defined
    assert_eq!(locations.len(), 0);
}

#[tokio::test]
async fn test_goto_definition_with_no_imports() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let user_file = root.join("User.java");
    fs::write(&user_file, "package com.example;\nclass User {}").unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    let query_engine = QueryEngine::new(index);
    let locations = query_engine
        .goto_definition("SomeClass", user_file_id)
        .unwrap();

    // Should get results based on current package
    // but SomeClass doesn't exist, so empty
    assert_eq!(locations.len(), 0);
}

#[tokio::test]
async fn test_find_references() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let service_file = root.join("Service.java");
    fs::write(
        &service_file,
        "package com.example;\npublic class Service {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&service_file).await.unwrap();

    let query_engine = QueryEngine::new(index);
    let refs = query_engine.find_references("com.example.Service").unwrap();

    // Currently just returns the definition
    assert_eq!(refs.len(), 1);
    assert_eq!(&*refs[0].name, "Service");
}

#[tokio::test]
async fn workspace_symbol_name_search_is_case_insensitive_partial_and_exact() {
    let (_temp, engine) = workspace_query(&[
        (
            "com/example/UserService.java",
            "package com.example;\npublic class UserService {}\n",
        ),
        (
            "com/example/OrderService.java",
            "package com.example;\npublic class OrderService {}\n",
        ),
        (
            "com/example/PaymentGateway.java",
            "package com.example;\npublic class PaymentGateway {}\n",
        ),
    ])
    .await;

    let mut services: Vec<_> = workspace_symbols(&engine, "service")
        .into_iter()
        .map(|symbol| (symbol.name, symbol.kind))
        .collect();
    services.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(
        services,
        vec![
            ("OrderService".to_owned(), SymbolKind::CLASS),
            ("UserService".to_owned(), SymbolKind::CLASS),
        ]
    );

    for query in ["USER", "user", "UserS"] {
        let symbol = only_workspace_symbol(&engine, query);
        assert_eq!(symbol.name, "UserService", "query {query:?}");
        assert_eq!(symbol.kind, SymbolKind::CLASS, "query {query:?}");
    }

    assert!(workspace_symbols(&engine, "NonExistentSymbol12345").is_empty());
    assert!(workspace_symbols(&engine, &"a".repeat(1_000)).is_empty());
}

#[tokio::test]
async fn workspace_symbols_preserve_member_identity_container_and_utf16_range() {
    let source = concat!(
        "package com.example;\n",
        "\n",
        "public class Catalog {\n",
        "    private String apiKey;\n",
        "    public void loadData() {}\n",
        "}\n",
    );
    let (_temp, engine) = workspace_query(&[("com/example/Catalog.java", source)]).await;

    let field = only_workspace_symbol(&engine, "apiKey");
    assert_eq!(field.name, "apiKey");
    assert_eq!(field.kind, SymbolKind::FIELD);
    assert_eq!(field.container_name.as_deref(), Some("com.example.Catalog"));
    assert_eq!(field.location.range.start.line, 3);
    assert_eq!(field.location.range.start.character, 19);
    assert_eq!(field.location.range.end.character, 25);

    let method = only_workspace_symbol(&engine, "loadData");
    assert_eq!(method.name, "loadData");
    assert_eq!(method.kind, SymbolKind::METHOD);
    assert_eq!(
        method.container_name.as_deref(),
        Some("com.example.Catalog")
    );
    assert_eq!(method.location.range.start.line, 4);
    assert_eq!(method.location.range.start.character, 16);
    assert_eq!(method.location.range.end.character, 24);
}

#[tokio::test]
async fn workspace_symbols_preserve_same_named_declarations_and_fqn_search() {
    let (_temp, engine) = workspace_query(&[
        (
            "com/alpha/User.java",
            "package com.alpha;\npublic class User {}\n",
        ),
        (
            "com/beta/User.java",
            "package com.beta;\npublic class User {}\n",
        ),
    ])
    .await;

    let users: BTreeSet<_> = workspace_symbols(&engine, "User")
        .into_iter()
        .map(|symbol| {
            assert_eq!(symbol.kind, SymbolKind::CLASS);
            (
                symbol.name,
                path_suffix(Path::new(symbol.location.uri.path())),
            )
        })
        .collect();
    assert_eq!(
        users,
        BTreeSet::from([
            (
                "User".to_owned(),
                vec!["com".to_owned(), "alpha".to_owned(), "User.java".to_owned()],
            ),
            (
                "User".to_owned(),
                vec!["com".to_owned(), "beta".to_owned(), "User.java".to_owned()],
            ),
        ])
    );

    let alpha = only_workspace_symbol(&engine, "com.alpha");
    assert!(alpha.location.uri.path().ends_with("/com/alpha/User.java"));

    let all_names: Vec<_> = workspace_symbols(&engine, "")
        .into_iter()
        .map(|symbol| symbol.name)
        .collect();
    assert_eq!(all_names.len(), 2);
    assert!(all_names.iter().all(|name| name == "User"));
}

#[tokio::test]
async fn test_goto_definition_with_static_import() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create Math class with max method (simplified)
    let math_file = root.join("Math.java");
    fs::write(&math_file, "package java.lang;\npublic class Math {}").unwrap();

    // Create usage file with static import
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nimport static java.lang.Math.max;\nclass User {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&math_file).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    let query_engine = QueryEngine::new(index);

    // Try to find Math (the class should still be resolvable)
    let locations = query_engine.goto_definition("Math", user_file_id).unwrap();

    // Should find Math from java.lang
    assert!(!locations.is_empty());
}

#[tokio::test]
async fn test_query_engine_with_complex_workspace() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a complex workspace structure
    fs::create_dir_all(root.join("src/main/java/com/example/models")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/example/services")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/example/controllers")).unwrap();

    // Models
    fs::write(
        root.join("src/main/java/com/example/models/User.java"),
        "package com.example.models;\npublic class User {}",
    )
    .unwrap();

    // Services
    fs::write(
        root.join("src/main/java/com/example/services/UserService.java"),
        "package com.example.services;\nimport com.example.models.User;\npublic class UserService {}"
    ).unwrap();

    // Controllers
    fs::write(
        root.join("src/main/java/com/example/controllers/UserController.java"),
        "package com.example.controllers;\nimport com.example.services.UserService;\npublic class UserController {}"
    ).unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    let query_engine = QueryEngine::new(index);

    let user_names: BTreeSet<_> = workspace_symbols(&query_engine, "User")
        .into_iter()
        .map(|symbol| symbol.name)
        .collect();
    assert_eq!(
        user_names,
        BTreeSet::from([
            "User".to_owned(),
            "UserController".to_owned(),
            "UserService".to_owned(),
        ])
    );

    // Test goto definition from UserService
    let service_file = root.join("src/main/java/com/example/services/UserService.java");
    let service_file_id = query_engine.index().get_file_id(&service_file).unwrap();
    let user_def = query_engine
        .goto_definition("User", service_file_id)
        .unwrap();
    assert_eq!(user_def.len(), 1);
    assert_eq!(user_def[0].name.as_ref(), "User");
    assert_eq!(user_def[0].kind, SymbolKind::CLASS);
}

#[tokio::test]
async fn test_goto_definition_invalid_file_id() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root);
    let query_engine = QueryEngine::new(index);

    let locations = query_engine.goto_definition("Test", FileId(999)).unwrap();

    // Should return empty (no imports for invalid file id)
    assert_eq!(locations.len(), 0);
}

#[tokio::test]
async fn test_goto_definition_explicit_import_overrides_same_package() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create List in same package
    let local_list = root.join("List.java");
    fs::write(&local_list, "package com.example;\npublic class List {}").unwrap();

    // Create List in java.util
    let util_list = root.join("UtilList.java");
    fs::write(&util_list, "package java.util;\npublic class List {}").unwrap();

    // Create usage with explicit import (should override same-package List)
    let user_file = root.join("User.java");
    fs::write(
        &user_file,
        "package com.example;\nimport java.util.List;\nclass User { List list; }",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.index_file(&local_list).await.unwrap();
    index.index_file(&util_list).await.unwrap();
    index.index_file(&user_file).await.unwrap();

    let user_file_id = index.get_file_id(&user_file).unwrap();

    let query_engine = QueryEngine::new(index);
    let locations = query_engine.goto_definition("List", user_file_id).unwrap();

    // Should only find java.util.List (explicit import takes precedence)
    assert_eq!(locations.len(), 1);
}

#[tokio::test]
async fn test_index_accessor() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let index = WorkspaceIndex::new(root);
    let query_engine = QueryEngine::new(index);

    // Should be able to access the index
    let index_ref = query_engine.index();
    let stats = index_ref.stats();
    assert_eq!(stats.symbols_indexed, 0);
}

#[tokio::test]
async fn workspace_symbol_ranges_use_utf16_after_unicode_and_all_line_endings() {
    for newline in ["\n", "\r\n", "\r"] {
        let source = format!("// header{newline}/* 😀界 */ class Café {{}}");
        let (_temp, engine) = workspace_query(&[("Café.java", &source)]).await;
        let symbol = only_workspace_symbol(&engine, "Café");
        // Literal UTF-16 columns, independent of the production converter.
        assert_eq!(symbol.location.range.start, lsp_types::Position::new(1, 16));
        assert_eq!(symbol.location.range.end, lsp_types::Position::new(1, 20));
    }
}
