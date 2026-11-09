// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;
use hyperion_build::gradle::DependencyEdge;
use std::sync::Barrier;

#[test]
fn unknown_root_returns_none() {
    let root = PathBuf::from("/nonexistent/hyperion-test-workspace-never-recorded");
    assert_eq!(known_dependency_jars(&root), None);
}

#[test]
fn stores_are_isolated_for_the_same_root() {
    let temp = tempfile::tempdir().expect("temporary shared-root workspace");
    let workspace = temp.path();
    let first_store = ProjectClasspathStore::new();
    let second_store = ProjectClasspathStore::new();
    let first_index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let second_index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());

    let first_workspace = first_index.begin_external_source_refresh("build:maven");
    let first = first_store
        .begin_project_classpath_refresh(workspace, &first_workspace)
        .expect("first session owns its registry");
    let _ = first_index
        .replace_external_sources_for_refresh(&first_workspace, &[])
        .expect("first session commits");
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &first,
        vec![PathBuf::from("/libs/first.jar")],
    ));

    let second_workspace = second_index.begin_external_source_refresh("build:maven");
    let second = second_store
        .begin_project_classpath_refresh(workspace, &second_workspace)
        .expect("second session has independent authority");
    let _ = second_index
        .replace_external_sources_for_refresh(&second_workspace, &[])
        .expect("second session commits");
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &second,
        vec![PathBuf::from("/libs/second.jar")],
    ));

    assert_eq!(
        first_store.known_dependency_jars(workspace),
        Some(vec![PathBuf::from("/libs/first.jar")])
    );
    assert_eq!(
        second_store.known_dependency_jars(workspace),
        Some(vec![PathBuf::from("/libs/second.jar")])
    );
}

#[test]
fn workspace_aliases_share_one_refresh_generation() {
    let temp = tempfile::tempdir().expect("temporary aliased workspace");
    let workspace = temp.path();
    let alias = workspace.join(".");
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());

    let old_workspace = index.begin_external_source_refresh("build:maven");
    let old = store
        .begin_project_classpath_refresh(&alias, &old_workspace)
        .expect("alias begins old refresh");
    let new_workspace = index.begin_external_source_refresh("build:maven");
    let new = store
        .begin_project_classpath_refresh(workspace, &new_workspace)
        .expect("real spelling supersedes alias refresh");
    let _ = index
        .replace_external_sources_for_refresh(&new_workspace, &[])
        .expect("new refresh commits");

    assert!(!publish_maven_dependency_snapshot_for_refresh(
        &old,
        vec![PathBuf::from("/libs/stale.jar")],
    ));
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &new,
        vec![PathBuf::from("/libs/current.jar")],
    ));
    assert_eq!(
        store.known_dependency_jars(&alias),
        Some(vec![PathBuf::from("/libs/current.jar")])
    );
}

#[test]
fn dropped_attempt_releases_unpublished_build_authority() {
    let temp = tempfile::tempdir().expect("temporary aborted classpath workspace");
    let workspace = temp.path();
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let gradle = index.begin_external_source_refresh("build:gradle");
    let attempt = store
        .begin_project_classpath_refresh_guard(workspace, &gradle)
        .expect("Gradle begins side publication");

    drop(attempt);

    let maven = index.begin_external_source_refresh("build:maven");
    assert!(store
        .begin_project_classpath_refresh(workspace, &maven)
        .is_some());
}

#[test]
fn guard_cannot_claim_an_existing_attempt() {
    let temp = tempfile::tempdir().expect("temporary guarded classpath workspace");
    let workspace = temp.path();
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let gradle = index.begin_external_source_refresh("build:gradle");
    let attempt = store
        .begin_project_classpath_refresh(workspace, &gradle)
        .expect("first observer installs the attempt");

    assert!(store
        .begin_project_classpath_refresh_guard(workspace, &gradle)
        .is_none());
    let _ = index
        .replace_external_sources_for_refresh(&gradle, &[])
        .expect("workspace generation commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &attempt,
        Vec::new(),
        Vec::new(),
    ));
}

#[test]
fn late_attempt_abort_cannot_erase_newer_staging() {
    let temp = tempfile::tempdir().expect("temporary delayed-abort workspace");
    let workspace = temp.path();
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let old_workspace = index.begin_external_source_refresh("build:gradle");
    let old = store
        .begin_project_classpath_refresh_guard(workspace, &old_workspace)
        .expect("old attempt begins");
    let new_workspace = index.begin_external_source_refresh("build:gradle");
    let new = store
        .begin_project_classpath_refresh(workspace, &new_workspace)
        .expect("new attempt supersedes old");
    assert!(stage_project_ownership_for_refresh(&new, workspace, "main"));

    drop(old);

    let _ = index
        .replace_external_sources_for_refresh(&new_workspace, &[])
        .expect("new workspace generation commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &new,
        Vec::new(),
        Vec::new(),
    ));
    assert_eq!(
        store.known_project_for_source_root(workspace, &workspace.join("src/main/java")),
        Some((workspace.to_path_buf(), "main".to_owned()))
    );
}

#[test]
fn project_and_build_file_aliases_share_identity() {
    let temp = tempfile::tempdir().expect("temporary aliased project");
    let workspace = temp.path();
    let project = workspace.join("service");
    std::fs::create_dir(&project).expect("create project directory");
    let project_alias = project.join(".");
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin project refresh");

    assert!(record_project_classpath_scoped_for_refresh(
        &ticket,
        &project_alias,
        "main",
        vec![project.join("build/classes")],
        vec![project.join("build/classes")],
        Vec::new(),
        Vec::new(),
    ));
    assert!(record_gradle_resolution_for_refresh(
        &ticket,
        &project_alias,
        GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct("org.example:alias:1")],
            ..GradleResolutionReport::default()
        },
    ));
    let _ = index
        .replace_external_sources_for_refresh(&workspace_ticket, &[])
        .expect("workspace refresh commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &ticket,
        Vec::new(),
        Vec::new(),
    ));

    assert!(store
        .known_project_classpath(workspace, &project, "main")
        .is_some());
    let report = store
        .known_gradle_resolution_for_path(workspace, &project.join("build.gradle"))
        .expect("real build path sees aliased report");
    assert_eq!(
        report.explain_path("org.example:alias:1"),
        Some(vec!["org.example:alias:1".to_owned()])
    );
}

#[cfg(unix)]
#[test]
fn retargeted_project_alias_does_not_redirect_cached_ownership() {
    let temp = tempfile::tempdir().expect("temporary retargeted project");
    let workspace = temp.path();
    let old_project = workspace.join("old-service");
    let new_project = workspace.join("new-service");
    std::fs::create_dir_all(old_project.join("src/main/java")).expect("create old source root");
    std::fs::create_dir_all(new_project.join("src/main/java")).expect("create new source root");
    let project_alias = workspace.join("service");
    std::os::unix::fs::symlink(&old_project, &project_alias).expect("create project alias");
    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin project refresh");
    assert!(stage_project_ownership_for_refresh(
        &ticket,
        &project_alias,
        "main",
    ));
    let _ = index
        .replace_external_sources_for_refresh(&workspace_ticket, &[])
        .expect("workspace refresh commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &ticket,
        Vec::new(),
        Vec::new(),
    ));

    std::fs::remove_file(&project_alias).expect("remove old alias");
    std::os::unix::fs::symlink(&new_project, &project_alias).expect("retarget alias");

    assert_eq!(
        store.known_project_for_source_root(workspace, &old_project.join("src/main/java")),
        Some((
            hyperion_workspace::path_lookup_key(&old_project)
                .as_path()
                .to_path_buf(),
            "main".to_owned(),
        ))
    );
}

#[test]
fn dependency_aliases_are_deduplicated_in_first_seen_order() {
    let temp = tempfile::tempdir().expect("temporary dependency directory");
    let jar = temp.path().join("dependency.jar");
    std::fs::write(&jar, b"jar").expect("create dependency");
    let alias = temp.path().join(".").join("dependency.jar");
    let root = temp.path().join("workspace");

    record_dependency_jars(&root, vec![jar.clone(), alias]);

    assert_eq!(known_dependency_jars(&root), Some(vec![jar]));
    reset_project_classpaths(&root);
}

#[test]
fn recorded_root_is_known_even_when_empty() {
    let root = PathBuf::from("/tmp/hyperion-test-project-classpath-empty-marker");
    record_dependency_jars(&root, Vec::new());
    assert_eq!(known_dependency_jars(&root), Some(Vec::new()));
}

#[test]
fn recorded_jars_are_deduped_in_resolver_order() {
    let root = PathBuf::from("/tmp/hyperion-test-project-classpath-dedup-marker");
    record_dependency_jars(
        &root,
        vec![
            PathBuf::from("/libs/b.jar"),
            PathBuf::from("/libs/a.jar"),
            PathBuf::from("/libs/b.jar"),
        ],
    );
    assert_eq!(
        known_dependency_jars(&root),
        Some(vec![
            PathBuf::from("/libs/b.jar"),
            PathBuf::from("/libs/a.jar")
        ])
    );
}

#[test]
fn project_classpaths_are_scoped_to_module_and_source_set() {
    let workspace = PathBuf::from("/tmp/hyperion-test-project-classpath-scope");
    let module = workspace.join("app");
    record_project_classpath(
        &workspace,
        &module,
        "main",
        vec![module.join("build/classes/java/main")],
        vec![PathBuf::from("/libs/app.jar")],
    );

    assert_eq!(
        known_project_classpath(&workspace, &module, "main"),
        Some(KnownProjectClasspath {
            output_dirs: vec![module.join("build/classes/java/main")],
            compile_output_dirs: vec![module.join("build/classes/java/main")],
            dependency_jars: vec![PathBuf::from("/libs/app.jar")],
            compile_dependency_jars: vec![PathBuf::from("/libs/app.jar")],
        })
    );
    assert_eq!(known_project_classpath(&workspace, &module, "test"), None);
}

#[test]
fn gradle_resolution_lookup_uses_the_deepest_owning_module() {
    let workspace = PathBuf::from("/tmp/hyperion-test-gradle-resolution-scope");
    let module = workspace.join("service");
    record_gradle_resolution(
        &workspace,
        &workspace,
        GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct("org.example:root:1.0")],
            ..GradleResolutionReport::default()
        },
    );
    record_gradle_resolution(
        &workspace,
        &module,
        GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct("org.example:service:1.0")],
            ..GradleResolutionReport::default()
        },
    );

    let root_report =
        known_gradle_resolution_for_path(&workspace.join("build.gradle")).expect("root report");
    assert_eq!(
        root_report.explain_path("org.example:root:1.0"),
        Some(vec!["org.example:root:1.0".to_owned()])
    );
    let module_report =
        known_gradle_resolution_for_path(&module.join("build.gradle")).expect("module report");
    assert_eq!(
        module_report.explain_path("org.example:service:1.0"),
        Some(vec!["org.example:service:1.0".to_owned()])
    );

    reset_project_classpaths(&workspace);
    assert!(known_gradle_resolution_for_path(&module.join("build.gradle")).is_none());
}

#[test]
fn module_ownership_survives_an_unavailable_refresh_marker() {
    let workspace = PathBuf::from("/tmp/hyperion-test-project-classpath-ownership");
    let module = workspace.join("included");
    record_project_classpath_scoped(
        &workspace,
        &module,
        "main",
        vec![module.join("build/classes/java/main")],
        vec![module.join("build/classes/java/main")],
        Vec::new(),
        Vec::new(),
    );
    clear_project_classpaths(&workspace);

    assert_eq!(
        known_project_for_source_root(&workspace, &module.join("src/main/java")),
        Some((module, "main".to_string()))
    );
    reset_project_classpaths(&workspace);
}

#[test]
fn pending_module_ownership_is_visible_during_an_unavailable_refresh() {
    let workspace = PathBuf::from("/tmp/hyperion-test-project-classpath-pending-ownership");
    let module = workspace.join("new-included");
    clear_project_classpaths(&workspace);
    record_project_classpath_scoped(
        &workspace,
        &module,
        "main",
        vec![module.join("build/classes/java/main")],
        vec![module.join("build/classes/java/main")],
        Vec::new(),
        Vec::new(),
    );

    assert_eq!(known_project_classpath(&workspace, &module, "main"), None);
    assert_eq!(
        known_project_for_source_root(&workspace, &module.join("src/main/java")),
        Some((module, "main".to_string()))
    );
    reset_project_classpaths(&workspace);
}

#[test]
fn parsed_module_ownership_can_be_staged_without_a_classpath() {
    let workspace = PathBuf::from("/tmp/hyperion-test-project-classpath-staged-ownership");
    let module = workspace.join("failed-included");
    clear_project_classpaths(&workspace);
    stage_project_ownership(&workspace, &module, "main");

    assert_eq!(known_project_classpath(&workspace, &module, "main"), None);
    assert_eq!(
        known_project_for_source_root(&workspace, &module.join("src/main/java")),
        Some((module, "main".to_string()))
    );
    reset_project_classpaths(&workspace);
}

#[test]
fn re_recording_replaces_the_previous_list() {
    // Simulates a build-file-triggered refresh changing the resolved
    // dependency set: the newer call must win, not merge with the old.
    let root = PathBuf::from("/tmp/hyperion-test-project-classpath-refresh-marker");
    record_dependency_jars(&root, vec![PathBuf::from("/libs/old.jar")]);
    record_dependency_jars(&root, vec![PathBuf::from("/libs/new.jar")]);
    assert_eq!(
        known_dependency_jars(&root),
        Some(vec![PathBuf::from("/libs/new.jar")])
    );
}

#[test]
fn snapshot_cannot_finalize_before_its_workspace_commit() {
    let temp = tempfile::tempdir().expect("temporary gated classpath workspace");
    let workspace = temp.path().to_path_buf();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let workspace_ticket = index.begin_external_source_refresh("maven:/workspace");
    let side_ticket = begin_project_classpath_refresh(&workspace, &workspace_ticket)
        .expect("workspace ticket owns the side refresh");

    assert!(!publish_maven_dependency_snapshot_for_refresh(
        &side_ticket,
        vec![PathBuf::from("/libs/new.jar")],
    ));
    assert_eq!(known_dependency_jars(&workspace), None);

    let _ = index
        .replace_external_sources_for_refresh(&workspace_ticket, &[])
        .expect("workspace classpath commits");
    assert_eq!(known_dependency_jars(&workspace), None);
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &side_ticket,
        vec![PathBuf::from("/libs/new.jar")],
    ));
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new.jar")])
    );
    assert!(reset_project_classpaths_for_refresh(&side_ticket));
}

#[test]
fn active_build_owner_and_same_name_impostor_cannot_take_over() {
    let temp = tempfile::tempdir().expect("temporary authority workspace");
    let workspace = temp.path().to_path_buf();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let gradle_owner = format!("gradle:{}", workspace.display());
    let maven_owner = format!("maven:{}", workspace.display());

    let gradle_workspace_ticket = index.begin_external_source_refresh(&gradle_owner);
    let gradle_side_ticket = begin_project_classpath_refresh(&workspace, &gradle_workspace_ticket)
        .expect("Gradle owns the initial side refresh");
    let _ = index
        .replace_external_sources_for_refresh(&gradle_workspace_ticket, &[])
        .expect("Gradle workspace classpath commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &gradle_side_ticket,
        vec![PathBuf::from("/libs/gradle.jar")],
        vec![PathBuf::from("/libs/gradle-runtime.jar")],
    ));

    let active_maven_ticket = index.begin_external_source_refresh(&maven_owner);
    assert!(begin_project_classpath_refresh(&workspace, &active_maven_ticket).is_none());

    let impostor_index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let impostor_ticket = impostor_index.begin_external_source_refresh(&gradle_owner);
    let _ = impostor_index
        .replace_external_sources_for_refresh(&impostor_ticket, &[])
        .expect("impostor owns only its private WorkspaceIndex");
    assert!(begin_project_classpath_refresh(&workspace, &impostor_ticket).is_none());
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/gradle.jar")])
    );
    assert!(reset_project_classpaths_for_refresh(&gradle_side_ticket));
}

#[test]
fn inactive_gradle_owner_hands_off_to_maven_and_late_cleanup_is_harmless() {
    let temp = tempfile::tempdir().expect("temporary Gradle to Maven workspace");
    let workspace = temp.path().to_path_buf();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let gradle_owner = format!("gradle:{}", workspace.display());
    let maven_owner = format!("maven:{}", workspace.display());

    let old_workspace_ticket = index.begin_external_source_refresh(&gradle_owner);
    let old_side_ticket = begin_project_classpath_refresh(&workspace, &old_workspace_ticket)
        .expect("Gradle owns the initial side refresh");
    let _ = index
        .replace_external_sources_for_refresh(&old_workspace_ticket, &[])
        .expect("initial Gradle classpath commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &old_side_ticket,
        vec![PathBuf::from("/libs/old-gradle.jar")],
        vec![PathBuf::from("/libs/old-gradle-runtime.jar")],
    ));

    let gradle_cleanup_ticket = index.begin_external_source_refresh(&gradle_owner);
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/old-gradle.jar")])
    );
    let _ = index
        .replace_external_sources_for_refresh(&gradle_cleanup_ticket, &[])
        .expect("empty Gradle classpath commits");
    assert_eq!(known_dependency_jars(&workspace), None);

    // Both retained Gradle provenances are now inactive, so Maven may take
    // over even if the Gradle cleanup task has not reached its side state.
    let maven_workspace_ticket = index.begin_external_source_refresh(&maven_owner);
    let maven_side_ticket = begin_project_classpath_refresh(&workspace, &maven_workspace_ticket)
        .expect("inactive Gradle authority permits the Maven handoff");

    let cleanup_ready = Arc::new(Barrier::new(2));
    let release_cleanup = Arc::new(Barrier::new(2));
    let cleanup_task = {
        let workspace = workspace.clone();
        let cleanup_ready = Arc::clone(&cleanup_ready);
        let release_cleanup = Arc::clone(&release_cleanup);
        std::thread::spawn(move || {
            cleanup_ready.wait();
            release_cleanup.wait();
            retire_project_classpath_for_external_refresh(&workspace, &gradle_cleanup_ticket)
        })
    };

    cleanup_ready.wait();
    let _ = index
        .replace_external_sources_for_refresh(&maven_workspace_ticket, &[])
        .expect("Maven workspace classpath commits");
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &maven_side_ticket,
        vec![PathBuf::from("/libs/new-maven.jar")],
    ));
    release_cleanup.wait();

    assert!(cleanup_task.join().expect("delayed Gradle cleanup"));
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new-maven.jar")])
    );
    assert!(reset_project_classpaths_for_refresh(&maven_side_ticket));
}

#[test]
fn retired_maven_owner_hands_off_to_gradle() {
    let temp = tempfile::tempdir().expect("temporary Maven to Gradle workspace");
    let workspace = temp.path().to_path_buf();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let maven_owner = format!("maven:{}", workspace.display());
    let gradle_owner = format!("gradle:{}", workspace.display());

    let old_workspace_ticket = index.begin_external_source_refresh(&maven_owner);
    let old_side_ticket = begin_project_classpath_refresh(&workspace, &old_workspace_ticket)
        .expect("Maven owns the initial side refresh");
    let _ = index
        .replace_external_sources_for_refresh(&old_workspace_ticket, &[])
        .expect("initial Maven classpath commits");
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &old_side_ticket,
        vec![PathBuf::from("/libs/old-maven.jar")],
    ));

    let maven_cleanup_ticket = index.begin_external_source_refresh(&maven_owner);
    let _ = index
        .replace_external_sources_for_refresh(&maven_cleanup_ticket, &[])
        .expect("empty Maven classpath commits");
    assert_eq!(known_dependency_jars(&workspace), None);
    assert!(retire_project_classpath_for_external_refresh(
        &workspace,
        &maven_cleanup_ticket,
    ));

    let gradle_workspace_ticket = index.begin_external_source_refresh(&gradle_owner);
    let gradle_side_ticket = begin_project_classpath_refresh(&workspace, &gradle_workspace_ticket)
        .expect("retired Maven authority permits the Gradle handoff");
    let _ = index
        .replace_external_sources_for_refresh(&gradle_workspace_ticket, &[])
        .expect("Gradle workspace classpath commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &gradle_side_ticket,
        vec![PathBuf::from("/libs/new-gradle.jar")],
        vec![PathBuf::from("/libs/new-gradle-runtime.jar")],
    ));

    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new-gradle.jar")])
    );
    assert_eq!(
        known_runtime_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new-gradle-runtime.jar")])
    );
    assert!(reset_project_classpaths_for_refresh(&gradle_side_ticket));
}

#[test]
fn superseded_refresh_cannot_publish_any_project_side_state() {
    let temp = tempfile::tempdir().expect("temporary classpath workspace");
    let workspace = temp.path().to_path_buf();
    let old_module = workspace.join("old-service");
    let new_module = workspace.join("new-service");
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.clone());
    let owner = format!("gradle:{}", workspace.display());

    let baseline_workspace_ticket = index.begin_external_source_refresh(&owner);
    let baseline_ticket = begin_project_classpath_refresh(&workspace, &baseline_workspace_ticket)
        .expect("baseline workspace ticket owns the side refresh");
    assert!(record_project_classpath_scoped_for_refresh(
        &baseline_ticket,
        &old_module,
        "main",
        vec![old_module.join("baseline-output")],
        vec![old_module.join("baseline-output")],
        vec![PathBuf::from("/libs/baseline-compile.jar")],
        vec![PathBuf::from("/libs/baseline-runtime.jar")],
    ));
    assert!(record_gradle_resolution_for_refresh(
        &baseline_ticket,
        &old_module,
        GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct("baseline:dependency:0")],
            ..GradleResolutionReport::default()
        },
    ));
    let _ = index
        .replace_external_sources_for_refresh(&baseline_workspace_ticket, &[])
        .expect("baseline workspace classpath commits");
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &baseline_ticket,
        vec![PathBuf::from("/libs/baseline-compile.jar")],
        vec![PathBuf::from("/libs/baseline-runtime.jar")],
    ));

    let old_workspace_ticket = index.begin_external_source_refresh(&owner);
    let old_ticket = begin_project_classpath_refresh(&workspace, &old_workspace_ticket)
        .expect("next workspace ticket owns the staged side refresh");
    assert!(record_project_classpath_scoped_for_refresh(
        &old_ticket,
        &old_module,
        "main",
        vec![old_module.join("superseded-output")],
        vec![old_module.join("superseded-output")],
        vec![PathBuf::from("/libs/superseded-compile.jar")],
        vec![PathBuf::from("/libs/superseded-runtime.jar")],
    ));
    // Returning an error at this point performs no rollback: all request
    // reads still come from the untouched baseline snapshot.
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/baseline-compile.jar")])
    );
    assert_eq!(
        known_runtime_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/baseline-runtime.jar")])
    );
    assert_eq!(
        known_project_classpath(&workspace, &old_module, "main")
            .expect("last-good classpath remains visible")
            .output_dirs,
        vec![old_module.join("baseline-output")]
    );
    assert_eq!(
        known_project_for_source_root(&workspace, &old_module.join("src/main/java")),
        Some((old_module.clone(), "main".to_string()))
    );
    assert_eq!(
        known_gradle_resolution_for_path(&old_module.join("build.gradle"))
            .expect("last-good resolution remains visible")
            .explain_path("baseline:dependency:0"),
        Some(vec!["baseline:dependency:0".to_owned()])
    );

    let old_ready = Arc::new(Barrier::new(2));
    let release_old = Arc::new(Barrier::new(2));
    let old_task = {
        let old_ready = Arc::clone(&old_ready);
        let release_old = Arc::clone(&release_old);
        let old_module = old_module.clone();
        std::thread::spawn(move || {
            old_ready.wait();
            release_old.wait();
            let classpath_published = record_project_classpath_scoped_for_refresh(
                &old_ticket,
                &old_module,
                "main",
                vec![old_module.join("late-output")],
                vec![old_module.join("late-output")],
                vec![PathBuf::from("/libs/old-compile.jar")],
                vec![PathBuf::from("/libs/old-runtime.jar")],
            );
            let resolution_published = record_gradle_resolution_for_refresh(
                &old_ticket,
                &old_module,
                GradleResolutionReport {
                    dependency_edges: vec![DependencyEdge::direct("old:dependency:1")],
                    ..GradleResolutionReport::default()
                },
            );
            let snapshot_published = publish_gradle_dependency_snapshot_for_refresh(
                &old_ticket,
                vec![PathBuf::from("/libs/old-compile.jar")],
                vec![PathBuf::from("/libs/old-runtime.jar")],
            );
            (
                classpath_published,
                resolution_published,
                snapshot_published,
            )
        })
    };

    old_ready.wait();
    let new_workspace_ticket = index.begin_external_source_refresh(&owner);
    let new_ticket = begin_project_classpath_refresh(&workspace, &new_workspace_ticket)
        .expect("newer workspace ticket supersedes the side refresh");
    assert!(begin_project_classpath_refresh(&workspace, &old_workspace_ticket).is_none());
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/baseline-compile.jar")])
    );
    assert!(record_project_classpath_scoped_for_refresh(
        &new_ticket,
        &new_module,
        "main",
        vec![new_module.join("new-output")],
        vec![new_module.join("new-output")],
        vec![PathBuf::from("/libs/new-compile.jar")],
        vec![PathBuf::from("/libs/new-runtime.jar")],
    ));
    assert!(record_gradle_resolution_for_refresh(
        &new_ticket,
        &new_module,
        GradleResolutionReport {
            dependency_edges: vec![DependencyEdge::direct("new:dependency:2")],
            ..GradleResolutionReport::default()
        },
    ));
    assert_eq!(
        known_project_classpath(&workspace, &old_module, "main")
            .expect("staging does not mutate the last-good snapshot")
            .output_dirs,
        vec![old_module.join("baseline-output")]
    );
    assert_eq!(
        known_project_classpath(&workspace, &new_module, "main"),
        None
    );
    assert_eq!(
        known_project_for_source_root(&workspace, &new_module.join("src/main/java")),
        None
    );
    let _ = index
        .replace_external_sources_for_refresh(&new_workspace_ticket, &[])
        .expect("newest workspace classpath commits");
    assert_eq!(known_dependency_jars(&workspace), None);
    assert_eq!(known_runtime_dependency_jars(&workspace), None);
    assert_eq!(
        known_project_classpath(&workspace, &old_module, "main"),
        None
    );
    assert!(known_gradle_resolution_for_path(&old_module.join("build.gradle")).is_none());
    assert_eq!(
        known_project_for_source_root(&workspace, &old_module.join("src/main/java")),
        None
    );
    assert_eq!(
        known_project_for_source_root(&workspace, &new_module.join("src/main/java")),
        None
    );
    assert!(publish_gradle_dependency_snapshot_for_refresh(
        &new_ticket,
        vec![PathBuf::from("/libs/new-compile.jar")],
        vec![PathBuf::from("/libs/new-runtime.jar")],
    ));
    release_old.wait();

    assert_eq!(
        old_task.join().expect("old refresh task"),
        (false, false, false)
    );
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new-compile.jar")])
    );
    assert_eq!(
        known_runtime_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new-runtime.jar")])
    );
    assert_eq!(
        known_project_classpath(&workspace, &old_module, "main"),
        None
    );
    assert!(known_gradle_resolution_for_path(&old_module.join("build.gradle")).is_none());
    assert_eq!(
        known_project_classpath(&workspace, &new_module, "main"),
        Some(KnownProjectClasspath {
            output_dirs: vec![new_module.join("new-output")],
            compile_output_dirs: vec![new_module.join("new-output")],
            dependency_jars: vec![PathBuf::from("/libs/new-runtime.jar")],
            compile_dependency_jars: vec![PathBuf::from("/libs/new-compile.jar")],
        })
    );
    assert_eq!(
        known_project_for_source_root(&workspace, &new_module.join("src/main/java")),
        Some((new_module.clone(), "main".to_string()))
    );
    let report = known_gradle_resolution_for_path(&new_module.join("build.gradle"))
        .expect("new resolution report");
    assert_eq!(
        report.explain_path("new:dependency:2"),
        Some(vec!["new:dependency:2".to_owned()])
    );
    assert!(std::mem::size_of::<ProjectClasspathRefreshTicket>() <= 48);
    assert!(reset_project_classpaths_for_refresh(&new_ticket));
}

#[test]
fn standalone_refreshes_share_one_per_root_generation_order() {
    let temp = tempfile::tempdir().expect("temporary standalone classpath workspace");
    let workspace = temp.path().to_path_buf();
    let first = begin_standalone_project_classpath_refresh(&workspace);
    let second = begin_standalone_project_classpath_refresh(&workspace);

    assert_ne!(first, second);
    assert!(!publish_maven_dependency_snapshot_for_refresh(
        &first,
        vec![PathBuf::from("/libs/old.jar")],
    ));
    assert!(publish_maven_dependency_snapshot_for_refresh(
        &second,
        vec![PathBuf::from("/libs/new.jar")],
    ));
    assert_eq!(
        known_dependency_jars(&workspace),
        Some(vec![PathBuf::from("/libs/new.jar")])
    );
    assert!(reset_project_classpaths_for_refresh(&second));
}

#[test]
fn archive_replacement_invalidates_every_versioned_classpath_lookup() {
    let temp = tempfile::tempdir().expect("temporary version-fenced classpath workspace");
    let workspace = temp.path();
    let project = workspace.join("module");
    std::fs::create_dir(&project).expect("create project directory");
    let archive_path = workspace.join("dependency.jar");
    std::fs::write(&archive_path, b"same bytes").expect("write staged archive");
    let archive = hyperion_workspace::ExternalSourceArchive::capture_staged(&archive_path, 1024)
        .expect("stage archive generation");

    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin classpath refresh");
    assert!(record_project_classpath_scoped_for_refresh(
        &ticket,
        &project,
        "main",
        vec![project.join("build/classes")],
        vec![project.join("build/classes")],
        vec![archive_path.clone()],
        vec![archive_path.clone()],
    ));
    let (compile, runtime, archives) = bind_staged_gradle_classpaths_to_archives_for_refresh(
        &ticket,
        vec![archive_path.clone()],
        vec![archive_path.clone()],
        std::slice::from_ref(&archive),
    )
    .expect("bind production Gradle classpath generation");
    let workspace_refresh = index
        .replace_external_archives_for_refresh(&workspace_ticket, std::slice::from_ref(&archive))
        .expect("commit workspace generation");
    assert!(
        workspace_refresh
            .revision_for_identity(archive.identity())
            .is_some(),
        "the workspace and execution stores must name the same archive generation"
    );
    assert!(
        publish_gradle_dependency_snapshot_with_archives_for_refresh(
            &ticket, compile, runtime, archives,
        )
    );

    assert!(store.known_dependency_jars(workspace).is_some());
    assert!(store.known_runtime_dependency_jars(workspace).is_some());
    assert!(store
        .known_project_classpath(workspace, &project, "main")
        .is_some());

    let replacement = workspace.join("replacement.jar");
    std::fs::write(&replacement, b"same bytes").expect("write replacement archive");
    std::fs::remove_file(&archive_path).expect("unlink staged archive");
    std::fs::rename(replacement, &archive_path).expect("install same-path replacement");

    assert_eq!(store.known_dependency_jars(workspace), None);
    assert_eq!(store.known_runtime_dependency_jars(workspace), None);
    assert_eq!(
        store.known_project_classpath(workspace, &project, "main"),
        None
    );
}

#[cfg(unix)]
#[test]
fn gradle_project_classpath_rebinds_alias_to_captured_canonical_archive() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temporary alias-bound classpath workspace");
    let workspace = temp.path();
    let project = workspace.join("module");
    std::fs::create_dir(&project).expect("create project directory");
    let first = workspace.join("first.jar");
    let second = workspace.join("second.jar");
    let alias = workspace.join("dependency.jar");
    std::fs::write(&first, b"first generation").expect("write first archive");
    std::fs::write(&second, b"second generation").expect("write second archive");
    symlink(&first, &alias).expect("create dependency alias");
    let archive =
        ExternalSourceArchive::capture_staged(&alias, 1024).expect("stage alias target generation");
    let canonical = archive
        .identity()
        .archive_path()
        .expect("archive identity has a canonical path")
        .to_path_buf();

    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin classpath refresh");
    assert!(record_project_classpath_scoped_for_refresh(
        &ticket,
        &project,
        "main",
        Vec::new(),
        Vec::new(),
        vec![alias.clone()],
        vec![alias.clone()],
    ));
    let (compile, runtime, used) = bind_staged_gradle_classpaths_to_archives_for_refresh(
        &ticket,
        vec![alias.clone()],
        vec![alias.clone()],
        std::slice::from_ref(&archive),
    )
    .expect("bind staged Gradle paths");
    assert_eq!(compile, vec![canonical.clone()]);
    assert_eq!(runtime, vec![canonical.clone()]);
    assert_eq!(used.len(), 1);

    std::fs::remove_file(&alias).expect("remove old alias");
    symlink(&second, &alias).expect("retarget dependency alias");
    let _ = index
        .replace_external_archives_for_refresh(&workspace_ticket, &[archive])
        .expect("commit workspace generation");
    assert!(
        publish_gradle_dependency_snapshot_with_archives_for_refresh(
            &ticket, compile, runtime, used,
        )
    );

    let published = store
        .known_project_classpath(workspace, &project, "main")
        .expect("canonical project classpath remains current");
    assert_eq!(published.compile_dependency_jars, vec![canonical.clone()]);
    assert_eq!(published.dependency_jars, vec![canonical]);
    assert!(!published.dependency_jars.contains(&alias));
}

#[test]
fn gradle_fences_project_only_archives_but_not_unpublished_extras() {
    let temp = tempfile::tempdir().expect("temporary exact-fence classpath workspace");
    let workspace = temp.path();
    let project = workspace.join("module");
    std::fs::create_dir(&project).expect("create project directory");
    let project_jar = workspace.join("project-only.jar");
    let unused_jar = workspace.join("unused.jar");
    std::fs::write(&project_jar, b"project dependency").expect("write project archive");
    std::fs::write(&unused_jar, b"unused dependency").expect("write unused archive");
    let project_archive =
        ExternalSourceArchive::capture_staged(&project_jar, 1024).expect("stage project archive");
    let unused_archive =
        ExternalSourceArchive::capture_staged(&unused_jar, 1024).expect("stage unused archive");

    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin classpath refresh");
    assert!(record_project_classpath_scoped_for_refresh(
        &ticket,
        &project,
        "main",
        Vec::new(),
        Vec::new(),
        vec![project_jar.clone()],
        vec![project_jar.clone()],
    ));
    let (compile, runtime, used) = bind_staged_gradle_classpaths_to_archives_for_refresh(
        &ticket,
        Vec::new(),
        Vec::new(),
        &[project_archive.clone(), unused_archive],
    )
    .expect("bind staged project-only archive");
    assert_eq!(used.len(), 1, "only published paths should be fenced");
    let _ = index
        .replace_external_archives_for_refresh(
            &workspace_ticket,
            std::slice::from_ref(&project_archive),
        )
        .expect("commit workspace generation");
    assert!(
        publish_gradle_dependency_snapshot_with_archives_for_refresh(
            &ticket, compile, runtime, used,
        )
    );

    std::fs::write(&unused_jar, b"changed but still unused").expect("mutate unused archive");
    assert!(store
        .known_project_classpath(workspace, &project, "main")
        .is_some());

    std::fs::write(&project_jar, b"changed project dependency").expect("mutate project archive");
    assert!(store
        .known_project_classpath(workspace, &project, "main")
        .is_none());
}

#[cfg(unix)]
#[test]
fn raw_writer_after_gradle_binding_cannot_join_the_prepared_generation() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temporary post-bind writer workspace");
    let workspace = temp.path();
    let project = workspace.join("module");
    std::fs::create_dir(&project).expect("create project directory");
    let archive_path = workspace.join("real.jar");
    let alias = workspace.join("alias.jar");
    std::fs::write(&archive_path, b"archive").expect("write archive");
    symlink(&archive_path, &alias).expect("create archive alias");
    let archive = ExternalSourceArchive::capture_staged(&alias, 1024).expect("stage archive");

    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let workspace_ticket = index.begin_external_source_refresh("build:gradle");
    let ticket = store
        .begin_project_classpath_refresh(workspace, &workspace_ticket)
        .expect("begin classpath refresh");
    let (compile, runtime, used) = bind_staged_gradle_classpaths_to_archives_for_refresh(
        &ticket,
        vec![alias.clone()],
        vec![alias.clone()],
        std::slice::from_ref(&archive),
    )
    .expect("prepare canonical generation");
    assert!(record_project_classpath_scoped_for_refresh(
        &ticket,
        &project,
        "main",
        Vec::new(),
        Vec::new(),
        vec![alias.clone()],
        vec![alias],
    ));
    let _ = index
        .replace_external_archives_for_refresh(&workspace_ticket, &[archive])
        .expect("commit workspace generation");

    assert!(
        !publish_gradle_dependency_snapshot_with_archives_for_refresh(
            &ticket, compile, runtime, used,
        ),
        "the final in-memory fence must reject raw staging added after binding"
    );
}

#[test]
fn execution_snapshot_is_one_generation_and_rejects_refresh_or_archive_drift() {
    let temp = tempfile::tempdir().expect("temporary execution snapshot workspace");
    let workspace = temp.path();
    let first_jar = workspace.join("first.jar");
    let second_jar = workspace.join("second.jar");
    std::fs::write(&first_jar, b"first").expect("write first archive");
    std::fs::write(&second_jar, b"second").expect("write second archive");
    let first_archive =
        ExternalSourceArchive::capture_staged(&first_jar, 1024).expect("stage first archive");

    let store = ProjectClasspathStore::new();
    let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
    let first_workspace = index.begin_external_source_refresh("build:maven");
    let first = store
        .begin_project_classpath_refresh(workspace, &first_workspace)
        .expect("begin first classpath refresh");
    let _ = index
        .replace_external_archives_for_refresh(
            &first_workspace,
            std::slice::from_ref(&first_archive),
        )
        .expect("commit first workspace generation");
    let first_path = first_archive
        .identity()
        .archive_path()
        .expect("archive identity path")
        .to_path_buf();
    assert!(publish_maven_dependency_snapshot_with_archives_for_refresh(
        &first,
        vec![first_path.clone()],
        vec![first_archive],
    ));
    let snapshot = store
        .execution_classpath_snapshot(workspace, None)
        .expect("capture first execution generation");
    assert_eq!(snapshot.dependency_jars, Some(vec![first_path]));
    assert!(snapshot.is_current());

    let second_archive =
        ExternalSourceArchive::capture_staged(&second_jar, 1024).expect("stage second archive");
    let second_workspace = index.begin_external_source_refresh("build:maven");
    let second = store
        .begin_project_classpath_refresh(workspace, &second_workspace)
        .expect("begin second classpath refresh");
    let _ = index
        .replace_external_archives_for_refresh(
            &second_workspace,
            std::slice::from_ref(&second_archive),
        )
        .expect("commit second workspace generation");
    let second_path = second_archive
        .identity()
        .archive_path()
        .expect("archive identity path")
        .to_path_buf();
    assert!(publish_maven_dependency_snapshot_with_archives_for_refresh(
        &second,
        vec![second_path],
        vec![second_archive],
    ));
    assert!(
        !snapshot.is_current(),
        "a request may not mix its retained R1 paths with an R2 publication"
    );

    let current = store
        .execution_classpath_snapshot(workspace, None)
        .expect("capture second execution generation");
    std::fs::write(&second_jar, b"changed second").expect("mutate second archive");
    assert!(
        !current.is_current(),
        "use-time validation must reject dependency byte drift"
    );
}
