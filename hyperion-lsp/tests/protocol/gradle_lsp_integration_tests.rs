// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP-level integration tests for Gradle support
//!
//! These tests validate that the Gradle integration works correctly
//! within the LSP server context.

use hyperion_lsp::gradle_integration::{initialize_gradle_support, GradleIntegration};
use hyperion_workspace::WorkspaceIndex;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

/// Helper to get the workspace root
fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).parent().unwrap().to_path_buf()
}

/// Helper to get the example Gradle project path
fn example_project_path() -> PathBuf {
    workspace_root().join("examples").join("gradle-project")
}

/// Give every test a distinct project identity.
///
/// Project-classpath publication is intentionally process-global and rejects
/// tickets from a different `WorkspaceIndex` claiming authority over the same
/// root. Sharing the tracked fixture between parallel tests therefore tests an
/// impossible production topology and makes the correct supersession check
/// reject one of the test servers.
fn isolated_example_project() -> (TempDir, PathBuf) {
    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create isolated Gradle fixture directory");
        for entry in fs::read_dir(source).expect("read tracked Gradle fixture") {
            let entry = entry.expect("read tracked Gradle fixture entry");
            let name = entry.file_name();
            // Gradle's mutable local cache is neither source input nor safe to
            // share or copy while another test may be reading it.
            if name == ".gradle" {
                continue;
            }
            let source_path = entry.path();
            let destination_path = destination.join(name);
            if entry
                .file_type()
                .expect("inspect tracked Gradle fixture entry")
                .is_dir()
            {
                copy_tree(&source_path, &destination_path);
            } else {
                fs::copy(&source_path, &destination_path)
                    .expect("copy tracked Gradle fixture file");
            }
        }
    }

    let temp = TempDir::new().expect("isolated Gradle fixture");
    let project_path = temp.path().join("gradle-project");
    copy_tree(&example_project_path(), &project_path);
    (temp, project_path)
}

/// A dependency-free Gradle project whose classpath can be proven complete on
/// every development machine. The tracked example intentionally contains
/// external dependencies and is useful for parser coverage, but its exact
/// dependency graph is unavailable when those artifacts are absent locally.
fn complete_isolated_project() -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().expect("complete isolated Gradle fixture");
    let project_path = temp.path().join("gradle-project");
    let source_path = project_path.join("src/main/java/example");
    fs::create_dir_all(&source_path).expect("create complete Gradle source directory");
    fs::write(project_path.join("build.gradle"), "plugins { id 'java' }\n")
        .expect("write complete Gradle build");
    fs::write(
        source_path.join("CompleteFixture.java"),
        "package example; public class CompleteFixture {}\n",
    )
    .expect("write complete Gradle source");
    let java_root = project_path.join("src/main/java");
    (temp, project_path, java_root)
}

fn assert_example_project_exists(project_path: &std::path::Path) {
    assert!(
        project_path.join("settings.gradle").is_file(),
        "tracked Gradle fixture is missing at {}",
        project_path.display()
    );
}

fn expected_source_paths(project_path: &std::path::Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = ["core", "api", "service", "web"]
        .into_iter()
        .map(|module| project_path.join(module).join("src/main/java"))
        .collect();
    let generated_root = project_path.join("core/build/generated");
    if generated_root.is_dir() {
        paths.push(generated_root);
    }
    paths
}

#[tokio::test]
async fn test_gradle_detection() {
    let (_temp, project_path) = isolated_example_project();
    assert_example_project_exists(&project_path);

    let gradle_integration = GradleIntegration::new(project_path);

    assert!(
        gradle_integration.is_gradle_workspace(),
        "Should detect Gradle workspace (has build.gradle)"
    );

    println!("✓ Gradle workspace detected correctly");
}

#[tokio::test]
async fn test_initialize_gradle_support() {
    let (_temp, project_path, expected_source_path) = complete_isolated_project();

    // Create a workspace index
    let workspace_index = Arc::new(WorkspaceIndex::new(project_path.clone()));

    // Initialize Gradle support
    let paths = initialize_gradle_support(&project_path, workspace_index.clone())
        .await
        .expect("dependency-free Gradle fixture must initialize");

    assert!(
        paths.contains(&expected_source_path),
        "Gradle initialization omitted source root {} from {paths:?}",
        expected_source_path.display()
    );
    assert!(workspace_index
        .find_symbol("example.CompleteFixture")
        .is_some());
}

#[tokio::test]
async fn test_gradle_integration_lifecycle() {
    let (_temp, project_path) = isolated_example_project();
    assert_example_project_exists(&project_path);

    let mut gradle_integration = GradleIntegration::new(project_path.clone());

    // Test initialization
    let (source_paths, jar_paths, jars_with_docs) = gradle_integration
        .initialize()
        .await
        .expect("tracked Gradle fixture must initialize");
    assert_eq!(source_paths, expected_source_paths(&project_path));

    let project = gradle_integration
        .project()
        .expect("initialization must retain the parsed Gradle project");
    assert_eq!(project.name, "hyperion-gradle-example");
    assert_eq!(project.subprojects, ["core", "api", "service", "web"]);
    assert!(
        !project.source_sets.is_empty(),
        "project should have source sets"
    );

    let refreshed = gradle_integration
        .refresh()
        .await
        .expect("tracked Gradle fixture must refresh");
    assert_eq!(refreshed.0, source_paths);
    assert_eq!(refreshed.1, jar_paths);
    assert_eq!(refreshed.2, jars_with_docs);
}

#[tokio::test]
async fn test_non_gradle_workspace() {
    // Test with a path that's definitely not a Gradle workspace
    let non_gradle_path = workspace_root().join("hyperion-lsp").join("src");

    let mut gradle_integration = GradleIntegration::new(non_gradle_path.clone());

    assert!(
        !gradle_integration.is_gradle_workspace(),
        "Should NOT detect Gradle workspace in src directory"
    );

    // Initializing should return empty paths (not an error)
    let paths = gradle_integration
        .initialize()
        .await
        .expect("non-Gradle workspaces should be a successful no-op");
    assert_eq!(paths, (Vec::new(), Vec::new(), Vec::new()));
}

#[tokio::test]
async fn test_gradle_classpath_indexing() {
    let (_temp, project_path, expected_source_path) = complete_isolated_project();

    let workspace_index = Arc::new(WorkspaceIndex::new(project_path.clone()));

    let paths = initialize_gradle_support(&project_path, workspace_index.clone())
        .await
        .expect("dependency-free Gradle fixture must produce an indexable classpath");

    assert!(paths.contains(&expected_source_path));
    assert!(paths.iter().all(|path| {
        path.is_dir() || path.extension().is_some_and(|extension| extension == "jar")
    }));
}

fn no_arg_module_project(
    external_library: bool,
    shadow_library: bool,
) -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = TempDir::new().expect("isolated multi-module Gradle fixture");
    let project = temp.path().join("gradle-project");
    let library = if external_library {
        temp.path().join("shared-library")
    } else {
        project.join("library")
    };
    let app = project.join("app");
    let app_source = app.join("src/main/java/client/Use.java");
    let library_source = library.join("src/main/java/api/Factory.java");
    fs::create_dir_all(app_source.parent().unwrap()).expect("create app source directory");
    fs::create_dir_all(library_source.parent().unwrap()).expect("create library source directory");
    fs::write(project.join("build.gradle"), "plugins { id 'java' }\n").expect("write root build");
    let mut settings = "include ':app', ':library'\n".to_string();
    if external_library {
        settings.push_str("project(':library').projectDir = file('../shared-library')\n");
    }
    if shadow_library {
        settings.push_str("include ':a-shadow'\n");
    }
    fs::write(project.join("settings.gradle"), settings).expect("write settings");
    fs::write(
        app.join("build.gradle"),
        "plugins { id 'java' }\ndependencies { implementation project(':library') }\n",
    )
    .expect("write app build");
    fs::write(library.join("build.gradle"), "plugins { id 'java' }\n")
        .expect("write library build");
    fs::write(
        &app_source,
        "package client; import api.Factory; class Use { int value = Factory.make(); }\n",
    )
    .expect("write app source");
    fs::write(
        &library_source,
        "package api; public class Factory { public static int make() { return 1; } }\n",
    )
    .expect("write library source");
    if shadow_library {
        let shadow = project.join("a-shadow");
        let shadow_source = shadow.join("src/main/java/api/Factory.java");
        fs::create_dir_all(shadow_source.parent().unwrap())
            .expect("create shadow source directory");
        fs::write(shadow.join("build.gradle"), "plugins { id 'java' }\n")
            .expect("write shadow build");
        fs::write(&shadow_source, "package api; public class Factory {}\n")
            .expect("write shadow source");
    }
    (temp, project, app_source, library_source)
}

async fn assert_no_arg_module_navigation(project: &Path, app_source: &Path, library_source: &Path) {
    use hyperion_lsp::navigation::NavigationProvider;
    use hyperion_semantics::db::{Language, SourceDatabase};
    use hyperion_workspace::query::QueryEngine;

    use tower_lsp::lsp_types::{Position, Url};

    let index = Arc::new(WorkspaceIndex::new(project.to_path_buf()));
    let roots = initialize_gradle_support(project, Arc::clone(&index))
        .await
        .expect("complete multi-module Gradle model");
    for source in [app_source, library_source] {
        let canonical_source = source.canonicalize().expect("canonical source");
        assert!(
            roots.iter().any(|candidate| {
                canonical_source
                    .starts_with(candidate.canonicalize().expect("canonical source root"))
            }),
            "missing source root for {}: {roots:?}",
            source.display()
        );
        assert!(
            index.get_file_id(source).is_some(),
            "source was not indexed: {}",
            source.display()
        );
    }
    let shadow_source = project.join("a-shadow/src/main/java/api/Factory.java");
    if shadow_source.is_file() && shadow_source != library_source {
        let scope =
            hyperion_lsp::project_classpath::known_source_scope_for_file(project, app_source)
                .expect("published app source scope");
        let source_scope = hyperion_workspace::SourceScope::new(scope.visible_source_roots.clone());
        let scoped_owner = index
            .find_type_definition_in_scope("api.Factory", &source_scope)
            .expect("app dependency Factory owner");
        assert_eq!(
            scoped_owner.location.file_id,
            index
                .get_file_id(library_source)
                .expect("indexed library FileId")
        );
        assert!(
            index
                .find_member_definitions_in_scope("api.Factory", &source_scope)
                .expect("library Factory members")
                .iter()
                .any(|member| member.symbol.simple_name.as_ref() == "make"),
            "the scoped owner must carry its own make method"
        );
        let selected_owner = index
            .find_symbol("api.Factory")
            .expect("selected Factory type");
        assert_eq!(
            selected_owner.file_id,
            index
                .get_file_id(&shadow_source)
                .expect("indexed shadow FileId"),
            "global owner election should expose the shadow fixture"
        );
    }

    let source = fs::read_to_string(app_source).expect("read indexed app source");
    let file_id = index.get_file_id(app_source).expect("indexed app FileId");
    let host = hyperion_lsp::semantic_host::SemanticHost::new(Arc::clone(&index));
    host.with_db(|database| {
        database.set_file_text(file_id, Arc::new(source.clone()));
        database.set_file_language(file_id, Language::Java);
    });
    let txn = host.read_now();
    let provider = NavigationProvider::new(
        &txn,
        Arc::clone(&index),
        Arc::new(QueryEngine::new((*index).clone())),
        Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
    );
    let call_offset = source.find("make()").expect("static call");
    let position = Position::new(0, call_offset as u32);
    let uri = Url::from_file_path(app_source).expect("app file URI");
    let location = provider
        .goto_definition(file_id, position, &source, &uri)
        .expect("navigation request")
        .expect("static call definition");
    assert_eq!(
        location.uri.to_file_path().unwrap().canonicalize().unwrap(),
        library_source.canonicalize().unwrap()
    );
    let declaration = fs::read_to_string(library_source).expect("read library source");
    let declaration_offset = declaration.find("make()").expect("library declaration");
    assert_eq!(
        location.range.start,
        Position::new(0, declaration_offset as u32)
    );
}

#[tokio::test]
async fn static_no_arg_navigation_finds_internal_gradle_sibling() {
    let (_temp, project, app_source, library_source) = no_arg_module_project(false, false);
    assert_no_arg_module_navigation(&project, &app_source, &library_source).await;
}

#[tokio::test]
async fn static_no_arg_navigation_finds_external_gradle_sibling() {
    let (_temp, project, app_source, library_source) = no_arg_module_project(true, false);
    assert_no_arg_module_navigation(&project, &app_source, &library_source).await;
}

#[tokio::test]
async fn static_no_arg_navigation_ignores_unrelated_duplicate_gradle_type() {
    use hyperion_build::gradle::{DependencyConfiguration, GradleParser};

    let (_temp, project, app_source, library_source) = no_arg_module_project(false, true);
    let app_model = GradleParser::parse_project(&project.join("app"))
        .await
        .expect("parse app Gradle model");
    assert_eq!(
        app_model.project_dependencies[&DependencyConfiguration::Implementation],
        ["library"]
    );
    assert_no_arg_module_navigation(&project, &app_source, &library_source).await;
}

#[tokio::test]
async fn static_no_arg_navigation_uses_first_visible_gradle_type_owner() {
    let (_temp, project, app_source, library_source) = no_arg_module_project(false, true);
    let shadow_source = project.join("a-shadow/src/main/java/api/Factory.java");
    fs::write(
        &shadow_source,
        "package api; public class Factory { public static int make() { return 2; } }\n",
    )
    .expect("write visible shadow owner");
    fs::write(
        project.join("app/build.gradle"),
        "plugins { id 'java-library' }\ndependencies { api project(':library'); implementation project(':a-shadow') }\n",
    )
    .expect("write ordered app dependencies");

    assert_no_arg_module_navigation(&project, &app_source, &library_source).await;
    let scope = hyperion_lsp::project_classpath::known_source_scope_for_file(&project, &app_source)
        .expect("published app source scope");
    let library_root = library_source.parent().unwrap().parent().unwrap();
    let shadow_root = shadow_source.parent().unwrap().parent().unwrap();
    let library_rank = scope
        .visible_source_roots
        .iter()
        .position(|root| root == library_root)
        .expect("library visible to app");
    let shadow_rank = scope
        .visible_source_roots
        .iter()
        .position(|root| root == shadow_root)
        .expect("shadow visible to app");
    assert!(
        library_rank < shadow_rank,
        "Gradle classpath ranks library first"
    );
}

#[tokio::test]
async fn static_no_arg_navigation_finds_generated_gradle_sibling() {
    let (_temp, project, app_source, library_source) = no_arg_module_project(false, false);
    let generated_source = project
        .join("library/build/generated/sources/annotationProcessor/java/main/api/Factory.java");
    let generated_test = project
        .join("library/build/generated/sources/annotationProcessor/java/test/api/Factory.java");
    fs::create_dir_all(generated_source.parent().unwrap())
        .expect("create generated library source directory");
    fs::create_dir_all(generated_test.parent().unwrap())
        .expect("create generated test source directory");
    fs::rename(&library_source, &generated_source)
        .expect("move library declaration to generated root");
    fs::write(&generated_test, "package api; public class Factory {}\n")
        .expect("write generated test duplicate");

    assert_no_arg_module_navigation(&project, &app_source, &generated_source).await;
    let app_scope =
        hyperion_lsp::project_classpath::known_source_scope_for_file(&project, &app_source)
            .expect("app source scope");
    let test_root = generated_test.parent().unwrap().parent().unwrap();
    assert!(
        !app_scope
            .visible_source_roots
            .contains(&test_root.to_path_buf()),
        "library test outputs are not on app's main compile classpath"
    );
    let test_scope =
        hyperion_lsp::project_classpath::known_source_scope_for_file(&project, &generated_test)
            .expect("generated test source scope");
    assert!(test_scope
        .visible_source_roots
        .contains(&test_root.to_path_buf()));
}

#[tokio::test]
async fn static_no_arg_navigation_finds_thrift_generated_gradle_sibling() {
    let (_temp, project, app_source, library_source) = no_arg_module_project(false, false);
    let generated_root = project.join("library/build/generated/source/thrift/main/java");
    let generated_source = generated_root.join("api/Factory.java");
    fs::create_dir_all(generated_source.parent().unwrap()).expect("create Thrift output directory");
    fs::rename(&library_source, &generated_source)
        .expect("move library declaration to Thrift output");

    assert_no_arg_module_navigation(&project, &app_source, &generated_source).await;
    let scope = hyperion_lsp::project_classpath::known_source_scope_for_file(&project, &app_source)
        .expect("published app source scope");
    assert!(scope.visible_source_roots.contains(&generated_root));
}
