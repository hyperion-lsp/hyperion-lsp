// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Deterministic contracts for the checked-in Gradle example.
//!
//! These tests never invoke Gradle and never depend on the developer's artifact
//! caches. Missing fixtures, parse failures, and unexpected public model changes
//! are failures rather than successful diagnostic probes.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use hyperion_build::gradle::{
    DependencyConfiguration, GradleClasspathBuilder, GradleDependency, GradleParser, GradleProject,
    GradleResolver, GradleResolverError,
};
use tempfile::TempDir;

fn example_project_path() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("hyperion-build must be inside the workspace")
        .join("examples/gradle-project");
    assert!(
        path.is_dir(),
        "checked-in Gradle fixture is missing: {}",
        path.display()
    );
    path
}

fn assert_dependency(
    project: &GradleProject,
    group: &str,
    artifact: &str,
    version: &str,
    configuration: DependencyConfiguration,
) {
    assert!(
        project.dependencies.iter().any(|dependency| {
            dependency.group == group
                && dependency.artifact == artifact
                && dependency.version.as_deref() == Some(version)
                && dependency.configuration == configuration
        }),
        "missing {configuration:?} dependency {group}:{artifact}:{version}; parsed dependencies: {:?}",
        project
            .dependencies
            .iter()
            .map(GradleDependency::coordinates)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn checked_in_multimodule_project_exposes_its_declared_model() {
    let root = example_project_path();
    let project = GradleParser::parse_project(&root)
        .await
        .expect("checked-in Gradle fixture must parse");

    assert_eq!(project.name, "hyperion-gradle-example");
    assert_eq!(project.project_path, ":");
    assert_eq!(project.root_dir, root);
    assert_eq!(project.group.as_deref(), Some("com.hyperion.example"));
    assert_eq!(project.version.as_deref(), Some("1.0.0-SNAPSHOT"));
    assert_eq!(project.subprojects, ["core", "api", "service", "web"]);
    assert!(project.has_plugin("java"));
    assert!(project.has_plugin("java-library"));
    assert!(project.has_wrapper);

    for module in &project.subprojects {
        assert_eq!(
            project.subproject_dirs.get(module),
            Some(&project.root_dir.join(module)),
            "wrong directory for subproject {module}"
        );
    }

    assert_dependency(
        &project,
        "org.junit.jupiter",
        "junit-jupiter-api",
        "5.10.0",
        DependencyConfiguration::TestImplementation,
    );
    assert_dependency(
        &project,
        "org.springframework.boot",
        "spring-boot-starter-web",
        "3.5.0",
        DependencyConfiguration::Implementation,
    );
}

#[tokio::test]
async fn web_module_preserves_external_and_project_dependency_semantics() {
    let web_root = example_project_path().join("web");
    let project = GradleParser::parse_project(&web_root)
        .await
        .expect("web fixture must parse");

    let plugin_ids = project
        .plugins
        .iter()
        .map(|plugin| plugin.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(plugin_ids, ["java", "application"]);
    assert_eq!(project.dependencies.len(), 8);

    assert_dependency(
        &project,
        "org.springframework.boot",
        "spring-boot-starter-web",
        "3.5.0",
        DependencyConfiguration::Implementation,
    );
    assert_dependency(
        &project,
        "org.springframework.boot",
        "spring-boot-devtools",
        "3.5.0",
        DependencyConfiguration::DevelopmentOnly,
    );
    assert_dependency(
        &project,
        "org.projectlombok",
        "lombok",
        "1.18.30",
        DependencyConfiguration::AnnotationProcessor,
    );

    assert_eq!(
        project
            .project_dependencies
            .get(&DependencyConfiguration::Implementation),
        Some(&vec![
            "service".to_string(),
            "api".to_string(),
            "core".to_string(),
        ])
    );
}

#[tokio::test]
async fn classpath_contract_keeps_main_and_test_layout_and_order() {
    let root = example_project_path();
    let project = GradleParser::parse_project(&root)
        .await
        .expect("checked-in Gradle fixture must parse");
    let classpath = GradleClasspathBuilder::new()
        .build(&project)
        .await
        .expect("checked-in Gradle fixture classpath must build");

    let main_source = root.join("src/main/java");
    let main_resource = root.join("src/main/resources");
    let main_output = root.join("build/classes/java/main");
    let test_source = root.join("src/test/java");
    let test_resource = root.join("src/test/resources");
    let test_output = root.join("build/classes/java/test");

    assert_eq!(classpath.project_name, "hyperion-gradle-example");
    assert_eq!(classpath.project_path, ":");
    assert_eq!(classpath.project_root, root);
    assert_eq!(classpath.source_dirs, [main_source]);
    assert_eq!(classpath.resource_dirs, [main_resource]);
    assert_eq!(
        classpath.output_dirs.as_slice(),
        std::slice::from_ref(&main_output)
    );
    assert_eq!(classpath.test_source_dirs, [test_source]);
    assert_eq!(classpath.test_resource_dirs, [test_resource]);
    assert_eq!(
        classpath.test_output_dirs.as_slice(),
        std::slice::from_ref(&test_output)
    );

    let main_classpath = classpath.main_classpath();
    let test_classpath = classpath.test_classpath();
    assert_eq!(main_classpath.first(), Some(&main_output));
    assert_eq!(test_classpath.first(), Some(&test_output));
    assert!(
        main_classpath
            .iter()
            .all(|entry| test_classpath.contains(entry)),
        "test classpath must contain the complete main classpath"
    );
    assert_eq!(
        main_classpath.iter().collect::<HashSet<_>>().len(),
        main_classpath.len(),
        "main classpath contains duplicate entries"
    );
    assert_eq!(
        test_classpath.iter().collect::<HashSet<_>>().len(),
        test_classpath.len(),
        "test classpath contains duplicate entries"
    );
}

#[tokio::test]
async fn settings_only_root_aggregates_subproject_dependencies() {
    let temp = TempDir::new().expect("temp fixture");
    let root = temp.path();
    fs::write(
        root.join("settings.gradle"),
        "rootProject.name = 'root'\ninclude ':app'",
    )
    .expect("write settings");

    let app_dir = root.join("app");
    fs::create_dir_all(&app_dir).expect("create app");
    fs::write(
        app_dir.join("build.gradle"),
        r#"
            plugins { id 'java' }
            dependencies {
                implementation 'org.slf4j:slf4j-api:2.0.9'
            }
        "#,
    )
    .expect("write app build");

    let project = GradleParser::parse_project(root)
        .await
        .expect("settings-only root must parse");

    assert_eq!(project.name, "root");
    assert_eq!(project.subprojects, ["app"]);
    assert_dependency(
        &project,
        "org.slf4j",
        "slf4j-api",
        "2.0.9",
        DependencyConfiguration::Implementation,
    );
}

#[tokio::test]
async fn missing_project_and_dependency_fail_with_specific_errors() {
    let temp = TempDir::new().expect("temp fixture");
    let missing_project = temp.path().join("missing-project");
    let project_error = GradleParser::parse_project(&missing_project)
        .await
        .expect_err("missing project must not parse");
    assert!(
        project_error.to_string().contains("not found"),
        "unexpected project error: {project_error}"
    );

    let dependency = GradleDependency::from_coordinates(
        "com.example.missing:never-published:999.999.999",
        DependencyConfiguration::Implementation,
    )
    .expect("valid coordinates");
    let resolver = GradleResolver::with_cache_dirs(
        temp.path().join("empty-gradle-cache"),
        temp.path().join("empty-maven-cache"),
    );
    let dependency_error = resolver
        .resolve_dependency(&dependency)
        .await
        .expect_err("empty isolated caches must not resolve a dependency");
    assert!(
        matches!(
            dependency_error,
            GradleResolverError::DependencyNotFound(ref coordinates)
                if coordinates == "com.example.missing:never-published:999.999.999"
        ),
        "unexpected dependency error: {dependency_error}"
    );
}

#[test]
fn dependency_coordinates_have_strict_minimum_shape() {
    for malformed in ["", "invalid"] {
        assert!(
            GradleDependency::from_coordinates(malformed, DependencyConfiguration::Implementation)
                .is_none(),
            "malformed coordinates were accepted: {malformed:?}"
        );
    }

    let versionless =
        GradleDependency::from_coordinates("group:artifact", DependencyConfiguration::CompileOnly)
            .expect("versionless coordinates are supported");
    assert_eq!(versionless.group, "group");
    assert_eq!(versionless.artifact, "artifact");
    assert_eq!(versionless.version, None);
    assert_eq!(
        versionless.configuration,
        DependencyConfiguration::CompileOnly
    );
}
