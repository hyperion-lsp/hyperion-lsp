// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_build::gradle::parser::GradleParser;
use hyperion_build::gradle::sniff_report::{SniffOutcome, SniffSkipReason};
use tempfile::TempDir;

fn write(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write file");
}

#[tokio::test]
async fn report_mentions_missing_internal_plugin_bytecode_with_remediation() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    write(
        &root.join("settings.gradle"),
        r#"
rootProject.name = "demo"
include(":app")
includeBuild("build-logic")
"#,
    );

    write(
        &root.join("build.gradle"),
        r#"
plugins {
  id "com.example.internal"
}
"#,
    );

    write(&root.join("app/build.gradle.kts"), "plugins { java }\n");

    // Composite build descriptor exists, but no compiled bytecode exists.
    write(
        &root.join("build-logic/build/resources/main/META-INF/gradle-plugins/com.example.internal.properties"),
        "implementation-class=com.example.InternalPlugin\n",
    );

    let (_project, report) = GradleParser::parse_project_with_report(root)
        .await
        .expect("parse_project_with_report");

    let entry = report
        .plugin_resolutions
        .iter()
        .find(|r| r.plugin_id == "com.example.internal")
        .expect("plugin resolution entry");

    match &entry.outcome {
        SniffOutcome::Skipped {
            reason: SniffSkipReason::MissingBytecode { remediation },
        } => {
            assert!(
                remediation.contains("Run Gradle once"),
                "expected actionable remediation; got: {remediation}"
            );
        }
        other => panic!("expected MissingBytecode, got: {other:?}"),
    }
}
