// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Changes made behind the editor's back (git checkout, other tools) must
//! reach the diagnostics of open files that depend on them.

use super::refactor_harness::Harness;

const CIRCLE: &str =
    "package demo.core;\n\npublic class Circle {\n    public double area() { return 1.0; }\n}\n";
const CIRC: &str =
    "package demo.core;\n\npublic class Circ {\n    public double area() { return 1.0; }\n}\n";
const APP: &str = "package demo.app;\n\nimport demo.core.Circle;\n\npublic class App {\n    Circle c = new Circle();\n}\n";

async fn wait_for(h: &Harness, what: &str, accept: impl Fn(&[String]) -> bool) {
    mutate_until(h, what, || {}, accept).await;
}

/// Repeat `mutate` until `accept` sees the resulting diagnostics: the file
/// watcher starts only after the workspace finished its initial indexing, and
/// changes made earlier are (correctly) covered by that scan instead.
async fn mutate_until(
    h: &Harness,
    what: &str,
    mutate: impl Fn(),
    accept: impl Fn(&[String]) -> bool,
) {
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    let mut next_mutation = tokio::time::Instant::now();
    loop {
        if tokio::time::Instant::now() >= next_mutation {
            mutate();
            next_mutation = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        }
        if let Some(diagnostics) = h.latest_diagnostics("demo/app/App.java") {
            if accept(&diagnostics) {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what}: last diagnostics {:?}",
            h.latest_diagnostics("demo/app/App.java")
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn git(h: &Harness, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
        .args(args)
        .current_dir(h.temp.path())
        .status()
        .expect("git is required for this test");
    assert!(status.success(), "git {args:?} failed");
}

/// Real `git checkout` replaces a file by unlinking and recreating it and
/// touching it again; the resulting burst of events used to leave the open
/// dependent's diagnostics stale because a later no-op event superseded the
/// refresh and nothing re-ran it.
#[tokio::test(flavor = "multi_thread")]
async fn git_checkout_of_an_unopened_file_refreshes_open_dependents() {
    let h = Harness::start(&[
        ("demo/core/Circle.java", CIRCLE),
        ("demo/app/App.java", APP),
    ])
    .await;
    git(&h, &["init", "-q", "-b", "main"]);
    git(&h, &["add", "."]);
    git(&h, &["commit", "-q", "-m", "circle"]);
    git(&h, &["checkout", "-q", "-b", "renamed"]);
    std::fs::write(h.path("demo/core/Circle.java"), CIRC).unwrap();
    git(&h, &["commit", "-q", "-am", "rename class"]);
    git(&h, &["checkout", "-q", "main"]);

    h.open("demo/app/App.java").await;
    wait_for(&h, "no unresolved symbol initially", |d| {
        !d.iter().any(|m| m.contains("cannot find symbol"))
    })
    .await;

    let circle = h.path("demo/core/Circle.java");
    // The watcher starts only after initial indexing; a plain rewrite, repeated
    // until it lands, proves it is running.
    mutate_until(
        &h,
        "watcher is running",
        || std::fs::write(&circle, CIRC).unwrap(),
        |d| d.iter().any(|m| m.contains("cannot find symbol")),
    )
    .await;
    std::fs::write(&circle, CIRCLE).unwrap();
    wait_for(&h, "resolved after restoring", |d| {
        !d.iter().any(|m| m.contains("cannot find symbol"))
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Exactly one checkout of the other branch, no retries.
    git(&h, &["checkout", "-q", "-f", "renamed"]);
    wait_for(&h, "unresolved after `git checkout renamed`", |d| {
        d.iter().any(|m| m.contains("cannot find symbol"))
    })
    .await;
    git(&h, &["checkout", "-q", "-f", "main"]);
    wait_for(&h, "resolved after `git checkout main`", |d| {
        !d.iter().any(|m| m.contains("cannot find symbol"))
    })
    .await;
}
