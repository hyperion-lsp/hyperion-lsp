// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Termination of the recursive walk through included Gradle projects.
//!
//! The walk had no cycle check, so an include graph with a back-edge recursed
//! until the stack overflowed and the process aborted. In the language server
//! that surfaced as a session wedged on "Resolving Build System".

use hyperion_build::gradle::GradleParser;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

/// Long enough that a slow machine will not trip it, short enough that a
/// non-terminating parse fails the test instead of hanging the suite.
const TERMINATION_BUDGET: Duration = Duration::from_secs(30);

fn write_build(dir: &Path, settings: &str) {
    fs::create_dir_all(dir).expect("create project dir");
    fs::write(dir.join("settings.gradle"), settings).expect("write settings.gradle");
    fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").expect("write build.gradle");
}

/// `includeBuild` composite builds are allowed to include each other, so a
/// back-edge here is valid Gradle, not a malformed project.
#[tokio::test]
async fn mutually_including_composite_builds_terminate() {
    let temp = TempDir::new().expect("temp dir");
    let a = temp.path().join("a");
    let b = temp.path().join("b");

    write_build(&a, "rootProject.name = 'a'\nincludeBuild '../b'\n");
    write_build(&b, "rootProject.name = 'b'\nincludeBuild '../a'\n");

    let parsed = tokio::time::timeout(TERMINATION_BUDGET, GradleParser::parse_project(&a)).await;

    assert!(
        parsed.is_ok(),
        "mutually including composite builds must terminate"
    );
    assert!(
        parsed.expect("within budget").is_ok(),
        "cutting the cycle must still yield a parsed root project"
    );
}

/// A `projectDir` override may point outside its own directory, including at
/// an ancestor, which makes the subproject walk re-enter a parent.
#[tokio::test]
async fn project_dir_pointing_at_an_ancestor_terminates() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();
    let nested = root.join("nested");

    write_build(root, "rootProject.name = 'root'\ninclude ':nested'\n");
    write_build(
        &nested,
        "rootProject.name = 'nested'\ninclude ':loop'\nproject(':loop').projectDir = file('..')\n",
    );

    let parsed = tokio::time::timeout(TERMINATION_BUDGET, GradleParser::parse_project(root)).await;

    assert!(
        parsed.is_ok(),
        "a projectDir override aimed at an ancestor must terminate"
    );
    assert!(
        parsed.expect("within budget").is_ok(),
        "cutting the cycle must still yield a parsed root project"
    );
}

/// A build that includes itself is the degenerate case of the same shape.
#[tokio::test]
async fn self_including_build_terminates() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();

    write_build(root, "rootProject.name = 'root'\nincludeBuild '.'\n");

    let parsed = tokio::time::timeout(TERMINATION_BUDGET, GradleParser::parse_project(root)).await;

    assert!(parsed.is_ok(), "a self-including build must terminate");
}

/// The guard tracks the current recursion path, not every directory ever
/// visited, so a module reachable from two parents is still parsed for both.
/// Cutting on a global visited set instead would silently drop the second
/// parent's dependencies.
#[tokio::test]
async fn a_project_included_by_two_parents_is_parsed_for_both() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();

    fs::create_dir_all(root.join("left")).expect("left");
    fs::create_dir_all(root.join("right")).expect("right");
    fs::create_dir_all(root.join("shared")).expect("shared");

    write_build(
        root,
        "rootProject.name = 'root'\ninclude ':left'\ninclude ':right'\n",
    );

    for parent in ["left", "right"] {
        let dir = root.join(parent);
        fs::write(
            dir.join("settings.gradle"),
            format!("rootProject.name = '{parent}'\ninclude ':shared'\nproject(':shared').projectDir = file('../shared')\n"),
        )
        .expect("write settings");
        fs::write(dir.join("build.gradle"), "apply plugin: 'java'\n").expect("write build");
    }

    fs::write(
        root.join("shared/build.gradle"),
        "apply plugin: 'java'\ndependencies {\n    implementation 'com.example:shared:1.0'\n}\n",
    )
    .expect("write shared build");

    let parsed = tokio::time::timeout(TERMINATION_BUDGET, GradleParser::parse_project(root))
        .await
        .expect("within budget")
        .expect("root project parses");

    assert!(
        parsed
            .dependencies
            .iter()
            .any(|dependency| dependency.artifact == "shared"),
        "the shared module's dependencies must still reach the root, got: {:?}",
        parsed
            .dependencies
            .iter()
            .map(|dependency| dependency.artifact.clone())
            .collect::<Vec<_>>()
    );
}
