// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_build::gradle::{
    DependencyConfiguration, GradleClasspathBuilder, GradleDependency, GradleIncompleteReason,
    GradleParser, GradleProject, GradleResolver,
};
use std::path::Path;

fn write_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("gradle")).unwrap();
    std::fs::write(
        root.join("build.gradle"),
        r#"
apply from: "$rootDir/gradle/dependencies.gradle"

plugins { id 'java' }
repositories { mavenCentral() }
dependencies {
    implementation libs.alpha
    implementation libs.dynamic
    testImplementation libs.missing
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("gradle/dependencies.gradle"),
        r#"
def dynamicVersion = "2.0.0"
versions += [
    alpha: "1.2.3"
]
libs += [
    alpha: "com.example:alpha:$versions.alpha",
    dynamic: "com.example:dynamic:$dynamicVersion",
    missing: "com.example:missing:9.9.9"
]
"#,
    )
    .unwrap();
}

fn write_gradle_artifact(
    cache: &Path,
    group: &str,
    artifact: &str,
    version: &str,
    pom: Option<&str>,
) {
    let artifact_dir = cache
        .join(group)
        .join(artifact)
        .join(version)
        .join("fixture-hash");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(
        artifact_dir.join(format!("{artifact}-{version}.jar")),
        b"fixture jar",
    )
    .unwrap();
    if let Some(pom) = pom {
        std::fs::write(artifact_dir.join(format!("{artifact}-{version}.pom")), pom).unwrap();
    }
}

#[tokio::test]
async fn applied_legacy_catalog_declares_dependencies() {
    let fixture = tempfile::tempdir().unwrap();
    write_fixture(fixture.path());

    let project = GradleParser::parse_project(fixture.path()).await.unwrap();
    let coordinates: std::collections::HashSet<_> = project
        .dependencies
        .iter()
        .map(|dependency| dependency.coordinates())
        .collect();

    assert_eq!(project.dependencies.len(), 3);
    assert!(coordinates.contains("com.example:alpha:1.2.3"));
    assert!(coordinates.contains("com.example:dynamic:2.0.0"));
    assert!(coordinates.contains("com.example:missing:9.9.9"));
}

#[tokio::test]
async fn declared_to_resolved_accounting_uses_fake_gradle_cache() {
    let fixture = tempfile::tempdir().unwrap();
    write_fixture(fixture.path());
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");

    write_gradle_artifact(
        &cache,
        "com.example",
        "alpha",
        "1.2.3",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>transitive</artifactId><version>3.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(&cache, "com.example", "dynamic", "2.0.0", None);
    write_gradle_artifact(&cache, "com.example", "transitive", "3.0.0", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let project = GradleParser::parse_project(fixture.path()).await.unwrap();
    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 1)
        .await
        .unwrap();

    assert_eq!(report.declared, 3);
    assert_eq!(report.resolved, 2);
    assert_eq!(report.missing_count(), 1);
    assert_eq!(report.missing, vec!["com.example:missing:9.9.9"]);
    assert_eq!(report.transitive_jars, 1);
    assert!(!report.complete);
    assert_eq!(report.jar_paths.len(), 3);
    assert!(report
        .jar_paths
        .iter()
        .any(|path| path.ends_with("alpha-1.2.3.jar")));
    assert!(report
        .jar_paths
        .iter()
        .any(|path| path.ends_with("dynamic-2.0.0.jar")));
    assert!(report
        .jar_paths
        .iter()
        .any(|path| path.ends_with("transitive-3.0.0.jar")));
    assert_eq!(
        report.explain_path("com.example:alpha:1.2.3"),
        Some(vec!["com.example:alpha:1.2.3".to_owned()])
    );
    assert_eq!(
        report.explain_path("com.example:transitive:3.0.0"),
        Some(vec![
            "com.example:alpha:1.2.3".to_owned(),
            "com.example:transitive:3.0.0".to_owned(),
        ])
    );
}

#[tokio::test]
async fn missing_transitive_jar_marks_resolution_incomplete() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>missing-transitive</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new(
        "missing-transitive".to_string(),
        fixture.path().to_path_buf(),
    );
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(!report.complete);
}

#[tokio::test]
async fn depth_limit_marks_untraversed_transitive_graph_incomplete() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>middle</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "middle",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>leaf</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "leaf",
        "1.0.0",
        Some("<project><dependencies></dependencies></project>"),
    );
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("depth-limited".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 1)
        .await
        .unwrap();
    assert!(!report.complete);
    assert_eq!(
        report.explain_path("com.example:middle:1.0.0"),
        Some(vec![
            "com.example:root:1.0.0".to_owned(),
            "com.example:middle:1.0.0".to_owned(),
        ])
    );
    assert_eq!(report.explain_path("com.example:leaf:1.0.0"), None);

    let complete_report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(complete_report.complete);
    assert_eq!(
        complete_report.explain_path("com.example:leaf:1.0.0"),
        Some(vec![
            "com.example:root:1.0.0".to_owned(),
            "com.example:middle:1.0.0".to_owned(),
            "com.example:leaf:1.0.0".to_owned(),
        ])
    );
}

#[tokio::test]
async fn pom_exclusions_are_applied_to_transitive_resolution() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>middle</artifactId><version>1.0.0</version>
<exclusions><exclusion><groupId>com.example</groupId><artifactId>excluded</artifactId></exclusion></exclusions>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "middle",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>excluded</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "excluded",
        "1.0.0",
        Some("<project><dependencies></dependencies></project>"),
    );
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("exclusions".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(report.complete);
    assert!(report
        .jar_paths
        .iter()
        .all(|path| !path.ends_with("excluded-1.0.0.jar")));
}

#[tokio::test]
async fn pom_classifier_marks_resolution_incomplete_without_guessing_a_jar() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>fixture</artifactId><version>1.0.0</version><classifier>tests</classifier>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(&cache, "com.example", "fixture", "1.0.0", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("classifier".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(!report.complete);
    assert!(report.incomplete_reasons.iter().any(|reason| matches!(
        reason,
        GradleIncompleteReason::UnresolvedTransitive { declared_by, dependency, .. }
            if declared_by == "com.example:root:1.0.0" && dependency == "com.example:fixture"
    )));
    assert!(report
        .jar_paths
        .iter()
        .all(|path| !path.ends_with("fixture-1.0.0.jar")));
}

#[tokio::test]
async fn versionless_test_dependency_keeps_test_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(&cache, "com.example", "test-lib", "4.5.6", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project =
        GradleProject::new("versionless-test".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "test-lib".to_string(),
        version: None,
        configuration: DependencyConfiguration::TestImplementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let builder =
        GradleClasspathBuilder::with_resolver(GradleResolver::with_cache_dirs(cache, maven_local));
    let classpath = builder.build(&project).await.unwrap();
    let jar = fixture
        .path()
        .join("gradle-cache/com.example/test-lib/4.5.6/fixture-hash/test-lib-4.5.6.jar");

    assert!(classpath.test_dependency_jars.contains(&jar));
    assert!(!classpath.dependency_jars.contains(&jar));
}

#[tokio::test]
async fn versionless_test_dependency_reuses_the_version_declared_for_the_same_module() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(&cache, "com.example", "shared", "1.0.0", None);
    write_gradle_artifact(&cache, "com.example", "shared", "2.0.0", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("same-artifact".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "shared".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "shared".to_string(),
        version: None,
        configuration: DependencyConfiguration::TestImplementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let builder =
        GradleClasspathBuilder::with_resolver(GradleResolver::with_cache_dirs(cache, maven_local));
    let classpath = builder.build(&project).await.unwrap();
    let main_jar = fixture
        .path()
        .join("gradle-cache/com.example/shared/1.0.0/fixture-hash/shared-1.0.0.jar");
    let test_jar = fixture
        .path()
        .join("gradle-cache/com.example/shared/2.0.0/fixture-hash/shared-2.0.0.jar");

    // A versionless declaration merges with the version another declaration
    // supplies; it must not guess the newest installed version.
    assert!(classpath.dependency_jars.contains(&main_jar));
    assert!(!classpath.dependency_jars.contains(&test_jar));
    assert!(!classpath.test_dependency_jars.contains(&test_jar));
    assert_eq!(classpath.resolution.resolved, 2);
}

#[tokio::test]
async fn dependency_configurations_keep_compile_and_runtime_scopes_separate() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    for artifact in [
        "api",
        "compile-only",
        "annotation",
        "runtime-only",
        "test-compile-only",
        "test-runtime-only",
    ] {
        write_gradle_artifact(&cache, "com.example", artifact, "1.0.0", None);
    }
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("scoped".to_string(), fixture.path().to_path_buf());
    for (artifact, configuration) in [
        ("api", DependencyConfiguration::Api),
        ("compile-only", DependencyConfiguration::CompileOnly),
        ("annotation", DependencyConfiguration::AnnotationProcessor),
        ("runtime-only", DependencyConfiguration::RuntimeOnly),
        (
            "test-compile-only",
            DependencyConfiguration::TestCompileOnly,
        ),
        (
            "test-runtime-only",
            DependencyConfiguration::TestRuntimeOnly,
        ),
    ] {
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "com.example".to_string(),
            artifact: artifact.to_string(),
            version: Some("1.0.0".to_string()),
            configuration,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
    }

    let builder =
        GradleClasspathBuilder::with_resolver(GradleResolver::with_cache_dirs(cache, maven_local));
    let classpath = builder.build(&project).await.unwrap();
    let jar = |artifact: &str| {
        fixture.path().join(format!(
            "gradle-cache/com.example/{artifact}/1.0.0/fixture-hash/{artifact}-1.0.0.jar"
        ))
    };

    assert!(classpath
        .compile_dependency_jars
        .contains(&jar("compile-only")));
    assert!(classpath.api_dependency_jars.contains(&jar("api")));
    assert!(classpath
        .compile_dependency_jars
        .contains(&jar("annotation")));
    assert!(!classpath.dependency_jars.contains(&jar("compile-only")));
    assert!(!classpath.dependency_jars.contains(&jar("annotation")));
    assert!(classpath.dependency_jars.contains(&jar("runtime-only")));
    assert!(!classpath
        .compile_dependency_jars
        .contains(&jar("runtime-only")));
    assert!(classpath
        .test_compile_dependency_jars
        .contains(&jar("test-compile-only")));
    assert!(!classpath
        .test_dependency_jars
        .contains(&jar("test-compile-only")));
    assert!(classpath
        .test_dependency_jars
        .contains(&jar("test-runtime-only")));
    assert!(!classpath
        .test_compile_dependency_jars
        .contains(&jar("test-runtime-only")));
}

#[tokio::test]
async fn pom_runtime_transitives_stay_out_of_compile_classpath() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>runtime-transitive</artifactId><version>1.0.0</version><scope>runtime</scope>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(&cache, "com.example", "runtime-transitive", "1.0.0", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("pom-scopes".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let builder =
        GradleClasspathBuilder::with_resolver(GradleResolver::with_cache_dirs(cache, maven_local));
    let classpath = builder.build(&project).await.unwrap();
    let runtime_jar = fixture.path().join(
        "gradle-cache/com.example/runtime-transitive/1.0.0/fixture-hash/runtime-transitive-1.0.0.jar",
    );

    assert!(classpath.runtime_dependency_jars.contains(&runtime_jar));
    assert!(!classpath.compile_dependency_jars.contains(&runtime_jar));
}

#[tokio::test]
async fn public_depth_api_fails_closed_for_an_incomplete_report() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>missing</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("fail-closed".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    assert!(resolver
        .resolve_dependencies_with_depth(&project, 2)
        .await
        .is_err());
    assert!(resolver
        .resolve_dependencies_with_docs(&project)
        .await
        .is_err());
}

#[tokio::test]
async fn conflicting_transitive_versions_resolve_to_the_highest_version() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project><dependencies>
<dependency><groupId>com.example</groupId><artifactId>shared</artifactId><version>1.0.0</version></dependency>
<dependency><groupId>com.example</groupId><artifactId>shared</artifactId><version>2.0.0</version></dependency>
</dependencies></project>"#,
        ),
    );
    for version in ["1.0.0", "2.0.0"] {
        write_gradle_artifact(
            &cache,
            "com.example",
            "shared",
            version,
            Some("<project><dependencies></dependencies></project>"),
        );
    }
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("conflict".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    // Gradle keeps one version per module: the highest requested.
    assert!(report.complete, "{:?}", report.incomplete_reasons);
    let shared: Vec<_> = report
        .jar_paths
        .iter()
        .filter(|path| path.to_string_lossy().contains("/shared/"))
        .collect();
    assert_eq!(shared.len(), 1, "{shared:?}");
    assert!(shared[0].ends_with("shared-2.0.0.jar"));
}

#[tokio::test]
async fn explicit_dependency_version_inherits_managed_scope_metadata() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project>
<dependencyManagement><dependencies><dependency>
<groupId>com.example</groupId><artifactId>managed</artifactId><version>1.0.0</version><scope>runtime</scope>
</dependency></dependencies></dependencyManagement>
<dependencies><dependency>
<groupId>com.example</groupId><artifactId>managed</artifactId><version>2.0.0</version>
</dependency></dependencies>
</project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "managed",
        "2.0.0",
        Some("<project><dependencies></dependencies></project>"),
    );
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("managed-scope".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 1)
        .await
        .unwrap();
    let managed_jar = fixture
        .path()
        .join("gradle-cache/com.example/managed/2.0.0/fixture-hash/managed-2.0.0.jar");
    assert!(report.complete);
    assert!(!report.main_compile_jars.contains(&managed_jar));
    assert!(report.main_runtime_jars.contains(&managed_jar));
}

#[tokio::test]
async fn pom_properties_apply_to_optional_dependencies_and_exclusions() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("gradle-cache");
    let maven_local = fixture.path().join("maven-local");
    write_gradle_artifact(
        &cache,
        "com.example",
        "root",
        "1.0.0",
        Some(
            r#"<project>
<properties><optional.flag>true</optional.flag><excluded.group>com.example</excluded.group><excluded.artifact>excluded</excluded.artifact></properties>
<dependencies>
<dependency><groupId>com.example</groupId><artifactId>optional</artifactId><version>1.0.0</version><optional>${optional.flag}</optional></dependency>
<dependency><groupId>com.example</groupId><artifactId>middle</artifactId><version>1.0.0</version><exclusions><exclusion><groupId>${excluded.group}</groupId><artifactId>${excluded.artifact}</artifactId></exclusion></exclusions></dependency>
</dependencies></project>"#,
        ),
    );
    write_gradle_artifact(
        &cache,
        "com.example",
        "middle",
        "1.0.0",
        Some(
            r#"<project><dependencies><dependency>
<groupId>com.example</groupId><artifactId>excluded</artifactId><version>1.0.0</version>
</dependency></dependencies></project>"#,
        ),
    );
    write_gradle_artifact(&cache, "com.example", "optional", "1.0.0", None);
    write_gradle_artifact(&cache, "com.example", "excluded", "1.0.0", None);
    std::fs::create_dir_all(&maven_local).unwrap();

    let mut project = GradleProject::new("properties".to_string(), fixture.path().to_path_buf());
    project.dependencies.push(GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".to_string(),
        artifact: "root".to_string(),
        version: Some("1.0.0".to_string()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    });

    let resolver = GradleResolver::with_cache_dirs(cache, maven_local);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(report.complete);
    assert!(report
        .jar_paths
        .iter()
        .all(|path| !path.ends_with("optional-1.0.0.jar")));
    assert!(report
        .jar_paths
        .iter()
        .all(|path| !path.ends_with("excluded-1.0.0.jar")));
}

fn graph_dependency(artifact: &str) -> GradleDependency {
    GradleDependency {
        exclusions: Vec::new(),
        transitive: true,
        forced: false,
        group: "com.example".into(),
        artifact: artifact.into(),
        version: Some("1.0.0".into()),
        configuration: DependencyConfiguration::Implementation,
        classifier: None,
        packaging: None,
        is_platform: false,
    }
}

fn write_graph(cache: &Path, graph: &[(&str, &[&str])]) {
    for (artifact, children) in graph {
        let dependencies: String = children.iter().map(|child| format!(
            "<dependency><groupId>com.example</groupId><artifactId>{child}</artifactId><version>1.0.0</version></dependency>"
        )).collect();
        write_gradle_artifact(
            cache,
            "com.example",
            artifact,
            "1.0.0",
            Some(&format!(
                "<project><dependencies>{dependencies}</dependencies></project>"
            )),
        );
    }
}

#[tokio::test]
async fn shortest_dependency_paths_are_expanded_regardless_of_declaration_order() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(
        &cache,
        &[
            ("root", &["middle"]),
            ("middle", &["shared"]),
            ("shared", &["leaf"]),
            ("leaf", &[]),
        ],
    );
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    for roots in [["root", "shared"], ["shared", "root"]] {
        let mut project = GradleProject::new("shortest".into(), fixture.path().into());
        project.dependencies = roots.into_iter().map(graph_dependency).collect();
        let report = resolver
            .resolve_dependencies_with_report(&project, 2)
            .await
            .unwrap();
        assert!(
            report.complete,
            "shorter direct paths must finish the graph: {report:?}"
        );
        assert_eq!(report.jar_paths.len(), 4);
        assert_eq!(
            report.explain_path("com.example:leaf:1.0.0"),
            Some(vec![
                "com.example:shared:1.0.0".into(),
                "com.example:leaf:1.0.0".into()
            ])
        );
    }
}

#[tokio::test]
async fn default_classpath_includes_deep_transitives_and_preserves_scope() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(
        &cache,
        &[
            ("a", &["b"]),
            ("b", &["c"]),
            ("c", &["d"]),
            ("d", &["e"]),
            ("e", &[]),
        ],
    );
    let mut project = GradleProject::new("deep".into(), fixture.path().into());
    project.dependencies = vec![graph_dependency("a")];
    project.dependencies[0].configuration = DependencyConfiguration::TestImplementation;
    let builder = GradleClasspathBuilder::with_resolver(GradleResolver::with_cache_dirs(
        cache,
        fixture.path().join("maven"),
    ));
    let classpath = builder.build(&project).await.unwrap();
    assert!(classpath.complete);
    assert_eq!(classpath.resolution.jar_paths.len(), 5);
    assert!(classpath.main_classpath().is_empty());
    assert!(classpath
        .test_classpath()
        .iter()
        .any(|path| path.ends_with("e-1.0.0.jar")));
}

#[tokio::test]
async fn cycles_at_depth_boundary_do_not_make_a_complete_graph_incomplete() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(&cache, &[("a", &["b"]), ("b", &["a"])]);
    let mut project = GradleProject::new("cycle".into(), fixture.path().into());
    project.dependencies = vec![graph_dependency("a")];
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    let report = resolver
        .resolve_dependencies_with_report(&project, 1)
        .await
        .unwrap();
    assert!(report.complete);
    assert_eq!(report.jar_paths.len(), 2);
    assert_eq!(report.dependency_edges.len(), 3);
}

#[tokio::test]
async fn shared_metadata_preserves_scopes_and_is_reread_after_refresh() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(&cache, &[("root", &["old"]), ("old", &[])]);
    let mut project = GradleProject::new("refresh".into(), fixture.path().into());
    let mut test_dependency = graph_dependency("root");
    test_dependency.configuration = DependencyConfiguration::TestImplementation;
    project.dependencies = vec![graph_dependency("root"), test_dependency];
    let resolver = GradleResolver::with_cache_dirs(cache.clone(), fixture.path().join("maven"));
    let first = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(first.complete);
    assert_eq!(first.main_compile_jars, first.test_compile_jars);
    write_graph(&cache, &[("root", &["new"]), ("new", &[])]);
    let refreshed = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(refreshed.complete);
    assert_eq!(refreshed.main_compile_jars, refreshed.test_compile_jars);
    assert!(refreshed
        .jar_paths
        .iter()
        .any(|path| path.ends_with("new-1.0.0.jar")));
    assert!(!refreshed
        .jar_paths
        .iter()
        .any(|path| path.ends_with("old-1.0.0.jar")));
}

#[tokio::test]
async fn available_transitive_docs_survive_an_incomplete_graph() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(&cache, &[("root", &["leaf", "missing"]), ("leaf", &[])]);
    let docs = cache.join("com.example/leaf/1.0.0/fixture-hash/leaf-1.0.0-javadoc.jar");
    std::fs::write(&docs, b"fixture docs").unwrap();
    let mut project = GradleProject::new("docs".into(), fixture.path().into());
    project.dependencies = vec![graph_dependency("root")];
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(!report.complete);
    let attached = resolver
        .resolve_artifact_docs(&report.resolved_artifacts)
        .await;
    assert_eq!(
        attached,
        vec![(docs.with_file_name("leaf-1.0.0.jar"), docs)]
    );
}

#[tokio::test]
async fn invalid_coordinates_cannot_escape_dependency_cache_roots() {
    let fixture = tempfile::tempdir().unwrap();
    let resolver =
        GradleResolver::with_cache_dirs(fixture.path().join("cache"), fixture.path().join("maven"));
    for (field, value) in [
        ("group", "../outside"),
        ("group", "com..example"),
        ("artifact", "/outside"),
        ("artifact", "../outside"),
        ("version", "../../outside"),
    ] {
        let mut dep = graph_dependency("root");
        match field {
            "group" => dep.group = value.into(),
            "artifact" => dep.artifact = value.into(),
            _ => dep.version = Some(value.into()),
        }
        assert!(matches!(
            resolver.resolve_dependency(&dep).await,
            Err(hyperion_build::gradle::GradleResolverError::InvalidDependency(_))
        ));
        assert!(resolver.resolve_javadoc(&dep).await.is_err());
        let mut project = GradleProject::new("invalid".into(), fixture.path().into());
        project.dependencies = vec![dep];
        let report = resolver
            .resolve_dependencies_with_report(&project, 2)
            .await
            .unwrap();
        assert!(!report.complete);
        assert_eq!(report.missing.len(), 1);
        assert!(report
            .incomplete_reasons
            .iter()
            .any(|reason| matches!(reason, GradleIncompleteReason::UnsupportedIdentity(_))));
        assert!(report.jar_paths.is_empty());
    }
}

#[tokio::test]
async fn installed_version_guess_never_authorizes_a_complete_classpath() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(&cache, &[("root", &[])]);
    let mut project = GradleProject::new("guess".into(), fixture.path().into());
    let mut dependency = graph_dependency("root");
    dependency.version = None;
    project.dependencies.push(dependency);
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert_eq!(report.cache_selected_versions, 1);
    assert_eq!(
        report.jar_paths.len(),
        1,
        "navigation still gets the cached artifact"
    );
    assert!(
        !report.complete,
        "installed versions are not Gradle's selection result"
    );
    assert!(report
        .incomplete_reasons
        .contains(&GradleIncompleteReason::UnprovenVersion(
            "com.example:root:1.0.0".into()
        )));
}

#[tokio::test]
async fn metadata_is_loaded_from_the_cache_that_supplied_the_artifact() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    let maven = fixture.path().join("maven");
    write_graph(&cache, &[("root", &["actual"]), ("actual", &[])]);
    let local = maven.join("com/example/root/1.0.0");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("root-1.0.0.pom"), "<project><dependencies><dependency><groupId>com.example</groupId><artifactId>stale</artifactId><version>1.0.0</version></dependency></dependencies></project>").unwrap();
    let mut project = GradleProject::new("provenance".into(), fixture.path().into());
    project.dependencies = vec![graph_dependency("root")];
    let resolver = GradleResolver::with_cache_dirs(cache.clone(), maven);
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(report.complete);
    assert!(report
        .jar_paths
        .iter()
        .any(|path| path.ends_with("actual-1.0.0.jar")));
    std::fs::remove_file(cache.join("com.example/root/1.0.0/fixture-hash/root-1.0.0.pom")).unwrap();
    let missing_metadata = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(!missing_metadata.complete);
    assert_eq!(missing_metadata.jar_paths.len(), 1);
    assert!(
        missing_metadata
            .explain_path("com.example:stale:1.0.0")
            .is_none(),
        "must not use another repository's metadata"
    );
}

#[tokio::test]
async fn ambiguous_checksum_variants_and_directories_are_not_selected_as_jars() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    write_graph(&cache, &[("root", &[])]);
    let other = cache.join("com.example/root/1.0.0/other-hash");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("root-1.0.0.jar"), b"different repository bytes").unwrap();
    let dep = graph_dependency("root");
    let resolver = GradleResolver::with_cache_dirs(cache.clone(), fixture.path().join("maven"));
    assert!(resolver.resolve_dependency(&dep).await.is_err());
    let mut project = GradleProject::new("ambiguous".into(), fixture.path().into());
    project.dependencies.push(dep.clone());
    let report = resolver
        .resolve_dependencies_with_report(&project, 1)
        .await
        .unwrap();
    assert!(report.incomplete_reasons.iter().any(|reason| matches!(
        reason,
        GradleIncompleteReason::ArtifactResolutionFailed { error, .. }
            if error.contains("ambiguous cached artifact")
    )));
    std::fs::remove_file(other.join("root-1.0.0.jar")).unwrap();
    std::fs::remove_file(cache.join("com.example/root/1.0.0/fixture-hash/root-1.0.0.jar")).unwrap();
    std::fs::create_dir(other.join("root-1.0.0.jar")).unwrap();
    assert!(resolver.resolve_dependency(&dep).await.is_err());
}

#[tokio::test]
async fn large_shared_graph_preserves_unique_scope_entries() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    let children: Vec<_> = (0..500).map(|i| format!("leaf{i}")).collect();
    let child_names: Vec<_> = children.iter().map(String::as_str).collect();
    write_graph(&cache, &[("root", &child_names)]);
    for child in &child_names {
        write_graph(&cache, &[(child, &[])]);
    }
    let mut project = GradleProject::new("shared".into(), fixture.path().into());
    for configuration in [
        DependencyConfiguration::Implementation,
        DependencyConfiguration::Api,
        DependencyConfiguration::TestImplementation,
    ] {
        let mut dep = graph_dependency("root");
        dep.configuration = configuration;
        project.dependencies.push(dep);
    }
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    let start = std::time::Instant::now();
    for _ in 0..10 {
        let report = resolver
            .resolve_dependencies_with_report(&project, 64)
            .await
            .unwrap();
        assert!(report.complete);
        assert_eq!(report.jar_paths.len(), 501);
        assert_eq!(report.main_compile_jars.len(), 501);
        assert_eq!(report.main_api_jars.len(), 501);
        assert_eq!(report.test_compile_jars.len(), 501);
        assert_eq!(report.direct_jars.len(), 3);
    }
    eprintln!(
        "Shared 501-artifact graph, three scopes, ten loads: {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn inline_dependency_blocks_are_audited_without_losing_closing_boundaries() {
    for (file, content) in [
        (
            "build.gradle",
            "plugins { id 'java' }\ndependencies { implementation 'com.example:root:1.0.0' }\n",
        ),
        (
            "build.gradle.kts",
            "plugins { java }\ndependencies { implementation(\"com.example:root:1.0.0\") }\n",
        ),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join(file), content).unwrap();
        let project = GradleParser::parse_project(fixture.path()).await.unwrap();
        assert_eq!(project.dependencies.len(), 1);
        assert!(GradleParser::find_unaccounted_dependency_syntax(&project)
            .unwrap()
            .is_empty());
        std::fs::write(
            fixture.path().join(file),
            format!("{content}\ndependencies {{ implementation computedCoordinate }}"),
        )
        .unwrap();
        assert!(!GradleParser::find_unaccounted_dependency_syntax(&project)
            .unwrap()
            .is_empty());
    }
}

#[tokio::test]
async fn dense_shared_graph_counts_contexts_instead_of_duplicate_queue_entries() {
    let fixture = tempfile::tempdir().unwrap();
    let cache = fixture.path().join("cache");
    let parents: Vec<_> = (0..130).map(|i| format!("parent{i}")).collect();
    let leaves: Vec<_> = (0..130).map(|i| format!("leaf{i}")).collect();
    let parent_names: Vec<_> = parents.iter().map(String::as_str).collect();
    let leaf_names: Vec<_> = leaves.iter().map(String::as_str).collect();
    write_graph(&cache, &[("root", &parent_names)]);
    for parent in &parent_names {
        write_graph(&cache, &[(parent, &leaf_names)]);
    }
    for leaf in &leaf_names {
        write_graph(&cache, &[(leaf, &[])]);
    }
    let mut project = GradleProject::new("dense".into(), fixture.path().into());
    project.dependencies = vec![graph_dependency("root")];
    let resolver = GradleResolver::with_cache_dirs(cache, fixture.path().join("maven"));
    let report = resolver
        .resolve_dependencies_with_report(&project, 2)
        .await
        .unwrap();
    assert!(
        report.complete,
        "duplicate edges must not exhaust the pending-context budget"
    );
    assert_eq!(report.jar_paths.len(), 261);
    assert_eq!(report.dependency_edges.len(), 1 + 130 + 130 * 130);
}
