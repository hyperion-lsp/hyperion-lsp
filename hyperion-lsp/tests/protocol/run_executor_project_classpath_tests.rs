// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Proves the "▶ Run" project-classpath path: a class that calls a
//! sibling helper class can run from a project's real compiled output +
//! classpath even when that helper's *source* is no longer present -- which
//! the single-file `javac` fallback fundamentally cannot do, since it always
//! recompiles from source.
//!
//! Fixture: a two-class Gradle-shaped module (`Main` calls `Helper`), both
//! compiled once with `javac` into the conventional
//! `build/classes/java/main` output directory. `Helper.java` is then
//! deleted, leaving only its `.class` behind -- simulating "this was built
//! by Gradle a while ago" without needing a real Gradle invocation. Calls
//! `hyperion_lsp::run_executor::compile_and_run` directly (no LSP
//! JSON-RPC plumbing needed; that round-trip is already covered by
//! `run_command_tests.rs`).
//!
//! Self-skips (like `run_command_tests.rs`) when `javac` isn't on `PATH`.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hyperion_lsp::project_classpath;
use hyperion_lsp::run_executor::compile_and_run;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg(if tool == "jar" {
            "--version"
        } else {
            "-version"
        })
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write_file(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    path
}

fn compile_with_javac(out_dir: &Path, sources: &[std::path::PathBuf]) {
    std::fs::create_dir_all(out_dir).unwrap();
    let mut cmd = Command::new("javac");
    cmd.arg("-d").arg(out_dir).arg("-encoding").arg("UTF-8");
    for s in sources {
        cmd.arg(s);
    }
    let output = cmd.output().expect("failed to spawn javac");
    assert!(
        output.status.success(),
        "fixture setup: javac failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const MAIN_SOURCE: &str = r#"package com.example;

public class Main {
    public static void main(String[] args) {
        System.out.println("Result: " + Helper.help());
    }
}
"#;

const HELPER_SOURCE: &str = r#"package com.example;

public class Helper {
    public static String help() {
        return "cross-class-ok";
    }
}
"#;

#[tokio::test]
async fn project_classpath_runs_cross_class_code_the_single_file_path_cannot() {
    if !tool_available("javac") {
        eprintln!(
            "SKIP project_classpath_runs_cross_class_code_the_single_file_path_cannot: no javac"
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();

    let source_root = workspace_root.join("src/main/java");
    let main_java = write_file(&source_root, "com/example/Main.java", MAIN_SOURCE);
    let helper_java = write_file(&source_root, "com/example/Helper.java", HELPER_SOURCE);

    let out_dir = workspace_root.join("build/classes/java/main");
    compile_with_javac(&out_dir, &[main_java.clone(), helper_java.clone()]);

    // Simulate "Gradle built this a while ago, then its source moved on/was
    // cleaned up" -- only Main.java (the file the Run lens was invoked from)
    // remains; Helper's source is gone, but its .class is still in the
    // compiled output.
    std::fs::remove_file(&helper_java).unwrap();

    // Dependency resolution has run for this workspace and found zero
    // external JARs -- "known: empty", not "unknown".
    project_classpath::record_dependency_jars(&workspace_root, Vec::new());

    let source_text = std::fs::read_to_string(&main_java).unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(
            &main_java,
            &source_text,
            "com.example.Main",
            Some(&workspace_root),
        )
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        outcome.summary.contains("finished with exit code 0"),
        "expected a successful run, got summary: {}\nlog:\n{all_text}",
        outcome.summary
    );
    assert!(
        outcome.summary.contains("project classpath"),
        "summary must honestly state which mode ran: {}",
        outcome.summary
    );
    assert!(
        all_text.contains("Mode: project classpath"),
        "log must state the mode honestly:\n{all_text}"
    );
    assert!(
        all_text.contains("cross-class-ok"),
        "Helper.help() (only available as precompiled bytecode) must have executed:\n{all_text}"
    );
    assert!(
        !all_text.contains("Compiling"),
        "project classpath mode must not invoke javac at all:\n{all_text}"
    );
}

#[tokio::test]
async fn cached_included_project_without_build_file_is_selected_for_run() {
    if !tool_available("javac") {
        eprintln!("SKIP cached_included_project_without_build_file_is_selected_for_run: no javac");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();
    let project_root = workspace_root.join("included");
    let source_root = project_root.join("src/main/java");
    let main_java = write_file(
        &source_root,
        "com/example/Main.java",
        "package com.example; public class Main { public static void main(String[] a) { System.out.println(\"included-ok\"); } }\n",
    );
    let out_dir = project_root.join("build/classes/java/main");
    compile_with_javac(&out_dir, std::slice::from_ref(&main_java));
    project_classpath::record_project_classpath_scoped(
        &workspace_root,
        &project_root,
        "main",
        vec![out_dir.clone()],
        vec![out_dir],
        Vec::new(),
        Vec::new(),
    );

    let source_text = std::fs::read_to_string(&main_java).unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(
            &main_java,
            &source_text,
            "com.example.Main",
            Some(&workspace_root),
        )
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        all_text.contains("Mode: project classpath") && all_text.contains("included-ok"),
        "cached included project should run from its scoped classpath:\n{all_text}"
    );
}

#[tokio::test]
async fn single_file_fallback_cannot_run_the_same_fixture_once_helper_source_is_gone() {
    if !tool_available("javac") {
        eprintln!(
            "SKIP single_file_fallback_cannot_run_the_same_fixture_once_helper_source_is_gone: no javac"
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();

    let source_root = workspace_root.join("src/main/java");
    let main_java = write_file(&source_root, "com/example/Main.java", MAIN_SOURCE);
    let helper_java = write_file(&source_root, "com/example/Helper.java", HELPER_SOURCE);

    let out_dir = workspace_root.join("build/classes/java/main");
    compile_with_javac(&out_dir, &[main_java.clone(), helper_java.clone()]);
    std::fs::remove_file(&helper_java).unwrap();

    // Deliberately never record dependency data for this workspace root ->
    // "unknown" -> plan_project_run must fall back to single-file javac,
    // which only has Main.java (Helper.java is gone) and therefore cannot
    // compile.
    let source_text = std::fs::read_to_string(&main_java).unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(
            &main_java,
            &source_text,
            "com.example.Main",
            Some(&workspace_root),
        )
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_text.contains("Mode: single-file javac"),
        "must honestly report the fallback mode:\n{all_text}"
    );
    assert!(
        all_text.contains("unknown"),
        "must state *why* project classpath wasn't used:\n{all_text}"
    );
    assert!(
        outcome.summary.contains("Compilation failed") || outcome.summary.contains("exit code 1"),
        "single-file javac must fail without Helper's source (cannot find symbol): {}",
        outcome.summary
    );
}

#[tokio::test]
async fn stale_compiled_output_falls_back_to_single_file_honestly() {
    if !tool_available("javac") {
        eprintln!("SKIP stale_compiled_output_falls_back_to_single_file_honestly: no javac");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();

    let source_root = workspace_root.join("src/main/java");
    let main_java = write_file(
        &source_root,
        "Main.java",
        "public class Main { public static void main(String[] a) {} }\n",
    );

    let out_dir = workspace_root.join("build/classes/java/main");
    compile_with_javac(&out_dir, std::slice::from_ref(&main_java));
    project_classpath::record_dependency_jars(&workspace_root, Vec::new());

    // Edit the source after the compiled class exists, so the class is now
    // stale relative to it.
    tokio::time::sleep(Duration::from_millis(20)).await;
    std::fs::write(
        &main_java,
        "public class Main { public static void main(String[] a) { System.out.println(\"edited\"); } }\n",
    )
    .unwrap();

    let source_text = std::fs::read_to_string(&main_java).unwrap();
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(&main_java, &source_text, "Main", Some(&workspace_root))
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_text.contains("Mode: single-file javac"),
        "stale compiled output must fall back to single-file javac:\n{all_text}"
    );
    assert!(
        all_text.contains("stale"),
        "fallback reason must mention staleness:\n{all_text}"
    );
    // The edited source (recompiled fresh) must actually run, proving the
    // fallback path isn't just failing -- it's genuinely recompiling.
    assert!(
        outcome.summary.contains("finished with exit code 0"),
        "recompiled single-file run should still succeed: {}",
        outcome.summary
    );
    assert!(
        all_text.contains("edited"),
        "must run the freshly-recompiled source, not the stale class:\n{all_text}"
    );
}

#[tokio::test]
async fn unsaved_buffer_is_compiled_and_project_output_is_not_reused() {
    if !tool_available("javac") {
        eprintln!("SKIP unsaved_buffer_is_compiled_and_project_output_is_not_reused: no javac");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();

    let source_root = workspace_root.join("src/main/java");
    let main_java = write_file(
        &source_root,
        "Main.java",
        "public class Main { public static void main(String[] a) { System.out.println(\"disk\"); } }\n",
    );
    let out_dir = workspace_root.join("build/classes/java/main");
    compile_with_javac(&out_dir, std::slice::from_ref(&main_java));
    project_classpath::record_dependency_jars(&workspace_root, Vec::new());

    // Do not write this edit to disk. The run command receives it from the
    // open document buffer and must compile that content instead of reusing
    // the class built from the saved source.
    let source_text =
        "public class Main { public static void main(String[] a) { System.out.println(\"memory\"); } }\n";
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(&main_java, source_text, "Main", Some(&workspace_root))
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        all_text.contains("source has unsaved in-memory edits"),
        "must explain why project classpath mode was skipped:\n{all_text}"
    );
    assert!(
        outcome.summary.contains("finished with exit code 0"),
        "unsaved source should still run successfully: {}",
        outcome.summary
    );
    assert!(
        all_text.contains("memory") && !all_text.contains("disk"),
        "the in-memory buffer must be executed instead of the saved source:\n{all_text}"
    );
}

#[test]
fn kotlin_fallback_reports_that_project_output_is_required() {
    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();
    let source_root = workspace_root.join("src/main/kotlin");
    let main_kt = write_file(
        &source_root,
        "Main.kt",
        "fun main() { println(\"kotlin\") }\n",
    );
    let source_text = std::fs::read_to_string(&main_kt).unwrap();

    let outcome = compile_and_run(&main_kt, &source_text, "MainKt", Some(&workspace_root));
    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        outcome.summary.contains("Kotlin/Scala") && outcome.summary.contains("Build the project"),
        "fallback should clearly explain the required execution mode: {}",
        outcome.summary
    );
    assert!(
        !all_text.contains("Mode: single-file javac"),
        "Kotlin fallback must not invoke single-file javac:\n{all_text}"
    );
}

#[tokio::test]
async fn fallback_uses_known_dependency_jars_for_compile_and_runtime() {
    if !tool_available("javac") || !tool_available("jar") {
        eprintln!(
            "SKIP fallback_uses_known_dependency_jars_for_compile_and_runtime: javac/jar unavailable"
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let workspace_root = temp.path().to_path_buf();
    std::fs::write(workspace_root.join("build.gradle"), "plugins { id 'java' }").unwrap();

    let dependency_source = write_file(
        &workspace_root,
        "dependency-src/dep/Library.java",
        r#"package dep;

public class Library {
    public static String value() {
        return "jar-ok";
    }
}
"#,
    );
    let dependency_classes = workspace_root.join("dependency-classes");
    compile_with_javac(&dependency_classes, &[dependency_source]);
    let dependency_jar = workspace_root.join("library.jar");
    let jar_output = Command::new("jar")
        .args(["cf", dependency_jar.to_str().unwrap(), "-C"])
        .arg(&dependency_classes)
        .arg(".")
        .output()
        .expect("spawn jar");
    assert!(
        jar_output.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&jar_output.stderr)
    );

    let main_java = write_file(
        &workspace_root,
        "src/main/java/com/example/Main.java",
        "package com.example;\npublic class Main { public static void main(String[] a) { System.out.println(\"disk\"); } }\n",
    );
    project_classpath::record_dependency_jars(&workspace_root, vec![dependency_jar]);

    let source_text = r#"package com.example;

import dep.Library;

public class Main {
    public static void main(String[] args) {
        System.out.println(Library.value());
    }
}
"#;
    let outcome = tokio::task::spawn_blocking(move || {
        compile_and_run(
            &main_java,
            source_text,
            "com.example.Main",
            Some(&workspace_root),
        )
    })
    .await
    .expect("compile_and_run panicked");

    let all_text = outcome
        .log_lines
        .iter()
        .map(|(_, msg)| msg.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        outcome.summary.contains("finished with exit code 0"),
        "known dependency must be available at runtime: {}\n{all_text}",
        outcome.summary
    );
    assert!(
        all_text.contains("jar-ok"),
        "dependency output missing: {all_text}"
    );
    assert!(
        all_text.contains("single-file javac"),
        "fallback mode missing: {all_text}"
    );
}
