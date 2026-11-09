// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Backs the "▶ Run" code lens command.
//!
//! Two execution modes, chosen automatically by [`compile_and_run`]:
//!
//! - **Project classpath** (preferred): if the file's module has *already
//!   been built* by Gradle/Maven (a fresh compiled-output directory exists
//!   for `main_class`), run the precompiled `.class` directly against the
//!   project's real classpath (the module's output dir + every dependency
//!   JAR known from `gradle_integration`/`maven_integration`, see
//!   `project_classpath`). No `javac` invocation needed.
//! - **Single-file javac** (fallback): compile every Java file under the
//!   file's Maven/Gradle-style source root with `javac`, using known
//!   dependency JARs and verified project outputs, and run the result. This is
//!   the original (and only) mode before project-classpath support was
//!   added; it's still what runs whenever project-classpath preconditions
//!   aren't met, and it always says so honestly in its output (spec: no
//!   silent guessing about which mode ran).
//!
//! Either way, `main`/the run target executes in the Hyperion bytecode VM
//! (`hyperion_bytecode::interpreter`).
//!
//! # Why this doesn't call `hyperion_eval::class_runner::run_class`
//!
//! `hyperion-eval` already has a `class_runner::run_class` that does almost
//! exactly this (eagerly load every `.class` on a classpath, then
//! `invoke_static(..., "main", ...)`). It was deliberately *not* reused here:
//! `run_class` echoes the executed program's `System.out`/`System.err`
//! straight to the *real* process stdout/stderr, which is the right thing
//! for a standalone CLI but wrong for an in-process LSP embedding -- when
//! `hyperion-lsp` talks JSON-RPC over stdio (the common transport), a stray
//! `println!` from user code would corrupt the protocol stream.
//! `VirtualMachine::set_suppress_console` + `captured_stdout()`/
//! `captured_stderr()` buffer that output instead so it can be forwarded to
//! the client as `window/logMessage` text, which is the entire reason this
//! module talks to `hyperion_bytecode` directly rather than depending on
//! `hyperion-eval` (which, as it happens, would not have introduced a
//! dependency cycle either -- `hyperion-eval` depends on `hyperion-parser`,
//! `hyperion-kotlin`, `hyperion-groovy` and `hyperion-bytecode`, none of
//! which depend back on `hyperion-lsp` -- the blocker is the missing output
//! capture, not a cycle). The bounded directory/JAR loading itself is shared
//! through `hyperion_bytecode::classpath`; only VM invocation and output
//! handling differ.
//!
//! # Project-classpath assembly: known artifacts only, never live Gradle
//!
//! Dependency JARs come exclusively from [`project_classpath`]'s cache of
//! whatever `gradle_integration`/`maven_integration` already resolved during
//! normal workspace indexing (see that module's docs for why the cache
//! exists at all). This module never shells out to Gradle/Maven itself at
//! request time. If that cache has no entry yet for the workspace (dependency
//! resolution hasn't happened, or the LSP has no workspace root), the
//! project-classpath path is skipped entirely and single-file javac runs
//! instead, with the reason logged -- per spec, "cannot determine" must be
//! reported, not silently treated as "no dependencies".
//!
//! # Scope
//!
//! - Only Java's `javac`/`.class` output is understood here (Kotlin/Scala
//!   source roots are recognized for source-set/output-dir *shape* matching,
//!   but this module never invokes `kotlinc`/`scalac`, so a Kotlin/Scala
//!   `main` only ever runs via project classpath, and only if some other
//!   process already compiled it).
//! - The source root is the file's Maven/Gradle-style
//!   `src/{main,test}/{java,kotlin,scala}` root when one can be found;
//!   otherwise it falls back to deriving the root from the `package`
//!   declaration (or, failing that, compiles just the single file). Project
//!   classpath mode requires the standard layout (it needs to locate the
//!   module's build file); the package-derived fallback always runs
//!   single-file.
//! - Timeouts are enforced by the *caller* (see `server/misc.rs`), which
//!   must run `compile_and_run` inside `tokio::task::spawn_blocking` and
//!   apply `tokio::time::timeout` around it: the VM has no cancellation
//!   hook, so a timeout abandons the blocking-pool thread rather than
//!   stopping the VM (an infinite loop keeps spinning on that thread
//!   forever; it just stops holding up the LSP's async event loop).
//!   `DEFAULT_TIMEOUT_MS` is a single `pub const`, so raising it (e.g. for a
//!   demo where a run is expected to take longer) is a one-line change.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime};

use hyperion_bytecode::classpath::load_classpath_classes;
use hyperion_bytecode::interpreter::{MemoryConfig, VMError, VirtualMachine};
use sha2::{Digest, Sha256};
use tower_lsp::lsp_types::MessageType;

use crate::navigation_util::extract_package_from_source;
use crate::project_classpath;
use crate::server::find_nearest_source_root;

mod source_output_layout;

use source_output_layout::{ensure_cached_output_dirs_fresh, ensure_source_files_fresh};

/// Default hard cap on end-to-end (compile + run) execution time.
///
/// A user's infinite loop must not wedge the LSP indefinitely. A single
/// `pub const` so it's a one-line change to raise for a demo that needs more
/// headroom.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_JAVAC_OUTPUT_BYTES: usize = 64 * 1024;

/// The result of a "Run" command invocation, ready to be streamed to the client.
///
/// `log_lines` are sent one at a time via `window/logMessage`; `summary` is sent as a final
/// `window/showMessage`.
pub struct RunOutcome {
    pub log_lines: Vec<(MessageType, String)>,
    pub summary_type: MessageType,
    pub summary: String,
}

impl RunOutcome {
    fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            log_lines: vec![(MessageType::ERROR, message.clone())],
            summary_type: MessageType::ERROR,
            summary: message,
        }
    }
}

/// Run `main_class`'s `public static void main(String[] args)`.
///
/// Prefers the project's real compiled output + classpath and falls back to
/// compiling `file_path`'s source root with `javac` when that isn't
/// possible. See the module docs for the exact decision rule and why.
///
/// Blocking: may shell out to `javac` synchronously and always runs the VM
/// to completion on the calling thread. See the module docs for why callers
/// must wrap this in `spawn_blocking` + a timeout.
pub fn compile_and_run(
    file_path: &Path,
    source_text: &str,
    main_class: &str,
    workspace_root: Option<&Path>,
) -> RunOutcome {
    compile_and_run_with_project_classpaths(
        file_path,
        source_text,
        main_class,
        workspace_root,
        &project_classpath::compatibility_store(),
    )
}

pub(crate) fn compile_and_run_with_project_classpaths(
    file_path: &Path,
    source_text: &str,
    main_class: &str,
    workspace_root: Option<&Path>,
    project_classpaths: &project_classpath::ProjectClasspathStore,
) -> RunOutcome {
    let (source_root, sources) = match resolve_sources(file_path, source_text) {
        Ok(v) => v,
        Err(e) => return RunOutcome::error(format!("Cannot run: {e}")),
    };

    let has_unsaved_edits = source_differs_from_disk(file_path, source_text);
    let decision = if has_unsaved_edits {
        ProjectRunDecision::Fallback {
            reason: "source has unsaved in-memory edits".to_string(),
        }
    } else {
        plan_project_run_with_project_classpaths(
            &source_root,
            &sources,
            main_class,
            workspace_root,
            project_classpaths,
        )
    };

    match decision {
        ProjectRunDecision::Run {
            classpath,
            generation,
        } => run_with_project_classpath(main_class, classpath, generation),
        ProjectRunDecision::Fallback { reason } => {
            if is_non_java_source(file_path, &source_root) {
                return RunOutcome::error(format!(
                    "Cannot run Kotlin/Scala source with single-file javac: {reason}. Build the project first so its compiled output can be reused."
                ));
            }

            run_single_file(
                &source_root,
                sources,
                file_path,
                source_text,
                has_unsaved_edits,
                main_class,
                workspace_root,
                project_classpaths,
                Some(reason),
            )
        }
    }
}

// ---------------------------------------------------------------------
// Project-classpath mode
// ---------------------------------------------------------------------

/// The build tool that produced a module's compiled output, and therefore
/// which conventional output directory to look under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectKind {
    Gradle,
    Maven,
}

enum ProjectRunDecision {
    Run {
        classpath: Vec<PathBuf>,
        generation: Option<Box<project_classpath::ExecutionClasspathSnapshot>>,
    },
    Fallback {
        reason: String,
    },
}

/// Decide whether `main_class` can run from the project's precompiled output
/// rather than `javac`-ing `sources` from scratch, per spec 1/2/3: compiled
/// output must exist, be fresh relative to every file in `sources`, and the
/// workspace's dependency JARs must be known (not merely "assumed empty").
#[cfg(test)]
fn plan_project_run(
    source_root: &Path,
    sources: &[PathBuf],
    main_class: &str,
    workspace_root: Option<&Path>,
) -> ProjectRunDecision {
    plan_project_run_with_project_classpaths(
        source_root,
        sources,
        main_class,
        workspace_root,
        &project_classpath::compatibility_store(),
    )
}

fn plan_project_run_with_project_classpaths(
    source_root: &Path,
    sources: &[PathBuf],
    main_class: &str,
    workspace_root: Option<&Path>,
    project_classpaths: &project_classpath::ProjectClasspathStore,
) -> ProjectRunDecision {
    let Some((project_root, kind, lang, set_name)) =
        find_module_project_with_cache_in(source_root, workspace_root, project_classpaths)
    else {
        return ProjectRunDecision::Fallback {
            reason: "no build.gradle(.kts)/pom.xml found for this file's module".to_string(),
        };
    };

    let local_output_dirs = output_dirs_for_kind(&project_root, kind, &lang, &set_name);
    let output_dir = local_output_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| output_dir_for_kind(&project_root, kind, &lang, &set_name));
    if !output_dir.is_dir() {
        return ProjectRunDecision::Fallback {
            reason: format!(
                "no compiled output directory found at {} (module not built yet)",
                output_dir.display()
            ),
        };
    }

    let mut class_file = output_dir.clone();
    for part in main_class.split('.') {
        class_file.push(part);
    }
    class_file.set_extension("class");

    if fs::metadata(&class_file)
        .and_then(|m| m.modified())
        .is_err()
    {
        return ProjectRunDecision::Fallback {
            reason: format!(
                "class '{main_class}' not found in compiled output at {}",
                class_file.display()
            ),
        };
    }

    let source_sets =
        match collect_project_source_sets(&project_root, &set_name, source_root, sources) {
            Ok(source_sets) => source_sets,
            Err(reason) => return ProjectRunDecision::Fallback { reason },
        };
    if let Err(reason) = ensure_project_outputs_fresh(
        &project_root,
        kind,
        &set_name,
        &local_output_dirs,
        &source_sets,
    ) {
        return ProjectRunDecision::Fallback { reason };
    }
    if set_name == "test" {
        let main_source_sets =
            match collect_project_source_sets(&project_root, "main", source_root, &[]) {
                Ok(source_sets) => source_sets,
                Err(reason) => return ProjectRunDecision::Fallback { reason },
            };
        let main_output_dirs = output_dirs_for_kind(&project_root, kind, &lang, "main");
        if let Err(reason) = ensure_project_outputs_fresh(
            &project_root,
            kind,
            "main",
            &main_output_dirs,
            &main_source_sets,
        ) {
            return ProjectRunDecision::Fallback { reason };
        }
    }

    let generation = workspace_root.and_then(|workspace_root| {
        project_classpaths
            .execution_classpath_snapshot(workspace_root, Some((&project_root, &set_name)))
    });
    let cached_classpath = generation
        .as_ref()
        .and_then(|snapshot| snapshot.project.clone());
    let (classpath_outputs, dependency_jars) = if let Some(cached) = cached_classpath {
        if let Err(reason) = ensure_cached_output_dirs_fresh(&cached.output_dirs) {
            return ProjectRunDecision::Fallback { reason };
        }
        (cached.output_dirs, cached.dependency_jars)
    } else {
        let Some(dependency_jars) = generation
            .as_ref()
            .and_then(|snapshot| snapshot.runtime_dependency_jars.clone())
        else {
            return ProjectRunDecision::Fallback {
                reason: "project dependency JARs are unknown (Gradle/Maven integration has not \
                         resolved this workspace's dependencies yet)"
                    .to_string(),
            };
        };

        let mut classpath = local_output_dirs;
        // Test source sets run against main output too (standard Gradle/Maven
        // test classpath behavior): a test's `main` might reference production
        // classes.
        if set_name == "test" {
            for main_output in output_dirs_for_kind(&project_root, kind, &lang, "main") {
                if !classpath.contains(&main_output) {
                    classpath.push(main_output);
                }
            }
        }
        (classpath, dependency_jars)
    };

    let mut classpath = classpath_outputs;
    classpath.extend(dependency_jars);

    if generation
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_current())
    {
        return ProjectRunDecision::Fallback {
            reason: "project dependency generation changed while planning the run".to_string(),
        };
    }

    ProjectRunDecision::Run {
        classpath,
        generation: generation.map(Box::new),
    }
}

/// Verify every source-set output that will be placed on the project
/// classpath. A missing source/class mtime is a reason to fall back:
/// project-classpath mode must never run on unverifiable output.
fn ensure_project_outputs_fresh(
    project_root: &Path,
    kind: ProjectKind,
    set_name: &str,
    output_dirs: &[PathBuf],
    source_sets: &[(PathBuf, Vec<PathBuf>)],
) -> Result<(), String> {
    for (source_root, sources) in source_sets {
        if sources.is_empty() {
            continue;
        }
        let lang = source_root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "source root has no language name: {}",
                    source_root.display()
                )
            })?;
        let output_dir = output_dir_for_kind(project_root, kind, lang, set_name);
        if !output_dirs.contains(&output_dir) {
            return Err(format!(
                "compiled output for source root {} is missing or not included in the project classpath",
                source_root.display()
            ));
        }
        ensure_source_files_fresh(&output_dir, source_root, sources)?;
    }

    for output_dir in output_dirs {
        let matched = source_sets.iter().any(|(source_root, sources)| {
            source_root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|lang| {
                    output_dir_for_kind(project_root, kind, lang, set_name) == *output_dir
                        && !sources.is_empty()
                })
        });
        if !matched {
            return Err(format!(
                "cannot verify compiled output freshness for {}: no source set is available",
                output_dir.display()
            ));
        }
    }

    Ok(())
}

fn collect_project_source_sets(
    project_root: &Path,
    set_name: &str,
    selected_root: &Path,
    selected_sources: &[PathBuf],
) -> Result<Vec<(PathBuf, Vec<PathBuf>)>, String> {
    let mut source_sets = Vec::new();
    for language in VERIFIABLE_LANGUAGES {
        let source_root = project_root.join("src").join(set_name).join(language);
        if !source_root.is_dir() {
            continue;
        }
        let sources = if source_root == selected_root {
            selected_sources.to_vec()
        } else {
            let mut sources = Vec::new();
            collect_language_files(&source_root, language, &mut sources)?;
            sources
        };
        source_sets.push((source_root, sources));
    }
    if source_sets.is_empty() && !selected_sources.is_empty() {
        source_sets.push((selected_root.to_path_buf(), selected_sources.to_vec()));
    }
    Ok(source_sets)
}

#[cfg(test)]
fn verified_project_output_dirs(
    workspace_root: Option<&Path>,
    source_root: &Path,
    sources: &[PathBuf],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    verified_project_output_dirs_in(
        workspace_root,
        source_root,
        sources,
        &project_classpath::compatibility_store(),
        None,
    )
}

fn verified_project_output_dirs_in(
    workspace_root: Option<&Path>,
    source_root: &Path,
    sources: &[PathBuf],
    project_classpaths: &project_classpath::ProjectClasspathStore,
    generation: Option<&project_classpath::ExecutionClasspathSnapshot>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Some((project_root, kind, lang, set_name)) =
        find_module_project_with_cache_in(source_root, workspace_root, project_classpaths)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut sets = vec![set_name.clone()];
    if set_name == "test" {
        sets.push("main".to_string());
    }
    let mut verified_compile = Vec::new();
    let mut verified_runtime = Vec::new();
    for set in sets {
        let Ok(source_sets) = collect_project_source_sets(
            &project_root,
            &set,
            source_root,
            if set == set_name { sources } else { &[] },
        ) else {
            continue;
        };
        for output_dir in output_dirs_for_kind(
            &project_root,
            kind,
            source_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("java"),
            &set,
        ) {
            let selected_language = source_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("java");
            if set == set_name
                && output_dir_for_kind(&project_root, kind, selected_language, &set) == output_dir
            {
                if kind == ProjectKind::Maven {
                    // Maven shares one output directory across languages. The
                    // fallback cannot separate fresh Kotlin/Scala classes
                    // from stale selected-language classes, so fail closed
                    // instead of exposing the shared directory to javac/VM.
                }
                // The fallback recompiles the selected source set into its
                // isolated directory; stale classes from that output must not
                // leak back in as dependencies.
                continue;
            }
            let mut matched = false;
            let mut fresh = true;
            for (root, files) in &source_sets {
                let Some(language) = root.file_name().and_then(|name| name.to_str()) else {
                    fresh = false;
                    break;
                };
                if output_dir_for_kind(&project_root, kind, language, &set) == output_dir {
                    matched = true;
                    if files.is_empty()
                        || ensure_source_files_fresh(&output_dir, root, files).is_err()
                    {
                        fresh = false;
                    }
                }
            }
            if matched && fresh {
                if !verified_compile.contains(&output_dir) {
                    verified_compile.push(output_dir.clone());
                }
                if !verified_runtime.contains(&output_dir) {
                    verified_runtime.push(output_dir);
                }
            }
        }
    }
    let cached = match generation {
        Some(snapshot) => snapshot.project.clone(),
        None => workspace_root.and_then(|workspace_root| {
            project_classpaths.known_project_classpath(workspace_root, &project_root, &set_name)
        }),
    };
    if let Some(cached) = cached {
        let selected_output = output_dir_for_kind(&project_root, kind, &lang, &set_name);
        for output_dir in &cached.compile_output_dirs {
            if *output_dir == selected_output || verified_compile.contains(output_dir) {
                continue;
            }
            if ensure_cached_output_dirs_fresh(std::slice::from_ref(output_dir)).is_ok() {
                verified_compile.push(output_dir.clone());
            }
        }
        for output_dir in &cached.output_dirs {
            if *output_dir == selected_output || verified_runtime.contains(output_dir) {
                continue;
            }
            if ensure_cached_output_dirs_fresh(std::slice::from_ref(output_dir)).is_ok() {
                verified_runtime.push(output_dir.clone());
            }
        }
    }
    (verified_compile, verified_runtime)
}

/// Locate the nearest module root for `source_root` (a `.../src/{main,test}/
/// {java,kotlin,scala}` directory, e.g. as returned by
/// `find_nearest_source_root`) and identify which build tool owns it (the
/// shared `hyperion_layout::ModuleContext`).
///
/// Returns `(module_root, kind, lang, set_name)` where `lang` is
/// `java`/`kotlin`/`scala` and `set_name` is `main`/`test`.
fn find_module_project_with_cache_in(
    source_root: &Path,
    workspace_root: Option<&Path>,
    project_classpaths: &project_classpath::ProjectClasspathStore,
) -> Option<(PathBuf, ProjectKind, String, String)> {
    let language = source_root.file_name()?.to_str()?.to_string();
    if !VERIFIABLE_LANGUAGES.contains(&language.as_str()) {
        return find_module_project(source_root);
    }
    if let Some((project_root, source_set)) = workspace_root.and_then(|workspace| {
        project_classpaths.known_project_for_source_root(workspace, source_root)
    }) {
        return Some((project_root, ProjectKind::Gradle, language, source_set));
    }
    find_module_project(source_root)
}

/// Languages whose sources the freshness checker can map to class files; a
/// Groovy source root is not verifiable, so it never takes the reuse path.
const VERIFIABLE_LANGUAGES: [&str; 3] = ["java", "kotlin", "scala"];

/// The module of a `src/{main,test}/<language>` source root, from the shared
/// locator. Custom source sets are not run from prebuilt output.
fn find_module_project(source_root: &Path) -> Option<(PathBuf, ProjectKind, String, String)> {
    let lang = source_root.file_name()?.to_str()?.to_string();
    if !VERIFIABLE_LANGUAGES.contains(&lang.as_str()) {
        return None;
    }
    let module = hyperion_layout::ModuleContext::from_source_root(source_root, None)?;
    if !matches!(module.source_set.as_str(), "main" | "test") {
        return None;
    }
    let kind = match module.build_tool {
        hyperion_layout::BuildTool::Gradle => ProjectKind::Gradle,
        hyperion_layout::BuildTool::Maven => ProjectKind::Maven,
        hyperion_layout::BuildTool::None => return None,
    };
    Some((module.module_dir, kind, lang, module.source_set))
}

/// The conventional compiled-output directory for a module, per build tool:
/// Gradle's Java plugin writes to `build/classes/<lang>/<set>`; Maven writes
/// main output to `target/classes` and test output to `target/test-classes`
/// regardless of source language.
fn output_dir_for_kind(
    project_root: &Path,
    kind: ProjectKind,
    lang: &str,
    set_name: &str,
) -> PathBuf {
    match kind {
        ProjectKind::Gradle => hyperion_layout::gradle_class_dir(project_root, lang, set_name),
        ProjectKind::Maven => hyperion_layout::maven_class_dir(project_root, set_name),
    }
}

/// Return all existing compiled output directories for a source set, keeping
/// the current language first so its main class lookup and classpath precedence
/// remain unchanged. Gradle modules commonly mix Java, Kotlin and Scala in one
/// source set, and each language plugin writes to its own directory.
fn output_dirs_for_kind(
    project_root: &Path,
    kind: ProjectKind,
    lang: &str,
    set_name: &str,
) -> Vec<PathBuf> {
    match kind {
        ProjectKind::Gradle => {
            let mut languages = vec![lang.to_string()];
            for candidate in VERIFIABLE_LANGUAGES {
                if !languages.iter().any(|existing| existing == candidate) {
                    languages.push(candidate.to_string());
                }
            }
            languages
                .into_iter()
                .filter_map(|language| {
                    let source_root = project_root.join("src").join(set_name).join(&language);
                    let output_dir =
                        hyperion_layout::gradle_class_dir(project_root, &language, set_name);
                    (source_root.is_dir() && output_dir.is_dir()).then_some(output_dir)
                })
                .collect()
        }
        ProjectKind::Maven => vec![output_dir_for_kind(project_root, kind, lang, set_name)],
    }
}

/// Run `main_class` directly from `classpath` (module output dir(s) +
/// dependency JARs) -- no compilation. Reports the mode and classpath entry
/// count honestly.
fn run_with_project_classpath(
    main_class: &str,
    classpath: Vec<PathBuf>,
    generation: Option<Box<project_classpath::ExecutionClasspathSnapshot>>,
) -> RunOutcome {
    let mut log_lines = vec![(
        MessageType::INFO,
        format!(
            "Mode: project classpath ({} entries; reusing existing compiled output, no compilation needed)",
            classpath.len()
        ),
    )];

    if generation
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_current())
    {
        return RunOutcome::error(
            "Cannot run: project dependency generation changed before VM startup",
        );
    }
    let started = Instant::now();
    let classes = match load_vm_classes_for_generation(&classpath, generation.as_deref(), || {}) {
        Ok(classes) => classes,
        Err(GenerationBoundClassLoadError::Classpath(error)) => {
            return RunOutcome::error(format!("Cannot run: {error}"));
        }
        Err(GenerationBoundClassLoadError::GenerationChanged) => {
            return RunOutcome::error(
                "Cannot run: project dependency generation changed while VM classes were loading",
            );
        }
    };
    // `classes` owns every byte the VM will consume. The generation fence in
    // `load_vm_classes_for_generation` ran after the last classpath read, so
    // invocation cannot reopen a dependency path from a different generation.
    let (exit_code, stdout, stderr) = run_loaded_in_vm(main_class, &classpath, &classes);
    let elapsed = started.elapsed();

    if !stdout.is_empty() {
        log_lines.push((MessageType::LOG, format!("[stdout]\n{stdout}")));
    }
    if !stderr.is_empty() {
        log_lines.push((MessageType::LOG, format!("[stderr]\n{stderr}")));
    }

    let summary_type = if exit_code == 0 {
        MessageType::INFO
    } else {
        MessageType::ERROR
    };
    let summary = format!(
        "{main_class} finished with exit code {exit_code} in {elapsed:.1?} \
         (project classpath, {} entries)",
        classpath.len()
    );

    RunOutcome {
        log_lines,
        summary_type,
        summary,
    }
}

// ---------------------------------------------------------------------
// Single-file javac mode (fallback)
// ---------------------------------------------------------------------

struct InvocationCleanup {
    directories: Vec<PathBuf>,
}

fn dedupe_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn spawn_bounded_reader<R>(mut reader: R) -> Receiver<Vec<u8>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 8192];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let remaining = MAX_JAVAC_OUTPUT_BYTES.saturating_sub(output.len());
            if remaining > 0 {
                output.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
        let _ = sender.send(output);
    });
    receiver
}

fn receive_reader(receiver: &Receiver<Vec<u8>>) -> Option<Vec<u8>> {
    receiver.recv_timeout(Duration::from_millis(250)).ok()
}

struct ProcessSupervisor {
    #[cfg(windows)]
    job: Option<WindowsJob>,
}

impl ProcessSupervisor {
    const fn for_child(_child: &Child) -> Self {
        Self {
            #[cfg(windows)]
            job: WindowsJob::new_for_child(_child),
        }
    }

    fn terminate(&self, child: &mut Child) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        terminate_process_tree(child);
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn new_for_child(child: &Child) -> Option<Self> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut limits = JobObjectExtendedLimitInformation::default();
            limits.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                &mut limits as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle() as *mut _) == 0
            {
                CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
    }

    fn terminate(&self) {
        unsafe {
            let _ = TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operations: u64,
    write_operations: u64,
    other_operations: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectExtendedLimitInformation {
    basic: JobObjectBasicLimitInformation,
    io: IoCounters,
    process_memory_limit: usize,
    peak_process_memory_used: usize,
    job_memory_limit: usize,
    peak_job_memory_used: usize,
}

#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
#[cfg(windows)]
const JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(
        attributes: *mut std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: u32,
        information: *mut std::ffi::c_void,
        length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

fn terminate_process_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(unix)]
    {
        let process_group = format!("-{pid}");
        let _ = Command::new("kill")
            .args(["-TERM", &process_group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(25));
        let _ = Command::new("kill")
            .args(["-KILL", &process_group])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl InvocationCleanup {
    fn new(directory: PathBuf) -> Self {
        Self {
            directories: vec![directory],
        }
    }

    fn add_directory(&mut self, directory: PathBuf) {
        self.directories.push(directory);
    }
}

impl Drop for InvocationCleanup {
    fn drop(&mut self) {
        for directory in &self.directories {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

/// Compile `sources` (every Java file under `source_root`) with `javac`
/// and run `main_class`'s `main` in the in-process VM. `fallback_reason`,
/// when present, is logged first so the user knows project classpath mode
/// was considered and specifically why it wasn't used (spec 3: no silent
/// mode-guessing).
fn run_single_file(
    source_root: &Path,
    sources: Vec<PathBuf>,
    file_path: &Path,
    source_text: &str,
    has_unsaved_edits: bool,
    main_class: &str,
    workspace_root: Option<&Path>,
    project_classpaths: &project_classpath::ProjectClasspathStore,
    fallback_reason: Option<String>,
) -> RunOutcome {
    let out_dir = run_output_dir_for(workspace_root, source_root);
    if let Err(e) = fs::create_dir_all(&out_dir) {
        return RunOutcome::error(format!(
            "Cannot run: failed to create build output directory {}: {e}",
            out_dir.display()
        ));
    }
    let mut cleanup = InvocationCleanup::new(out_dir.clone());

    let source_files_for_validation = sources.clone();
    let mut compile_sources = sources;
    if has_unsaved_edits {
        match write_source_override(workspace_root, source_root, file_path, source_text) {
            Ok(path) => {
                if let Some(parent) = path.parent() {
                    cleanup.add_directory(parent.to_path_buf());
                }
                if let Some(index) = compile_sources
                    .iter()
                    .position(|source| source == file_path)
                {
                    compile_sources[index] = path;
                } else {
                    compile_sources.push(path);
                }
            }
            Err(e) => return RunOutcome::error(format!("Cannot run: {e}")),
        }
    }

    let mut log_lines = vec![(
        MessageType::INFO,
        match &fallback_reason {
            Some(reason) => {
                format!("Mode: single-file javac (project classpath not used: {reason})")
            }
            None => "Mode: single-file javac".to_string(),
        },
    )];
    log_lines.push((
        MessageType::INFO,
        format!(
            "Compiling {} source file(s) from {} -> {}",
            compile_sources.len(),
            source_root.display(),
            out_dir.display()
        ),
    ));

    let mut cmd = Command::new("javac");
    cmd.arg("-d").arg(&out_dir).arg("-encoding").arg("UTF-8");
    let project =
        find_module_project_with_cache_in(source_root, workspace_root, project_classpaths);
    let generation = workspace_root.and_then(|workspace| {
        let project = project
            .as_ref()
            .map(|(root, _, _, source_set)| (root.as_path(), source_set.as_str()));
        project_classpaths.execution_classpath_snapshot(workspace, project)
    });
    let (verified_compile_outputs, verified_runtime_outputs) = verified_project_output_dirs_in(
        workspace_root,
        source_root,
        &source_files_for_validation,
        project_classpaths,
        generation.as_ref(),
    );
    let mut compile_dependency_classpath = verified_compile_outputs;
    let mut runtime_dependency_classpath = verified_runtime_outputs;
    if project.is_some() {
        if let Some(cached) = generation
            .as_ref()
            .and_then(|snapshot| snapshot.project.clone())
        {
            compile_dependency_classpath.extend(cached.compile_dependency_jars);
            runtime_dependency_classpath.extend(cached.dependency_jars);
        } else {
            if let Some(dependency_jars) = generation
                .as_ref()
                .and_then(|snapshot| snapshot.dependency_jars.clone())
            {
                compile_dependency_classpath.extend(dependency_jars);
            }
            if let Some(dependency_jars) = generation
                .as_ref()
                .and_then(|snapshot| snapshot.runtime_dependency_jars.clone())
            {
                runtime_dependency_classpath.extend(dependency_jars);
            }
        }
    } else {
        if let Some(dependency_jars) = generation
            .as_ref()
            .and_then(|snapshot| snapshot.dependency_jars.clone())
        {
            compile_dependency_classpath.extend(dependency_jars);
        }
        if let Some(dependency_jars) = generation
            .as_ref()
            .and_then(|snapshot| snapshot.runtime_dependency_jars.clone())
        {
            runtime_dependency_classpath.extend(dependency_jars);
        }
    }
    let compile_dependency_classpath = dedupe_paths(compile_dependency_classpath);
    if !compile_dependency_classpath.is_empty() {
        let joined = match std::env::join_paths(&compile_dependency_classpath) {
            Ok(joined) => joined,
            Err(error) => {
                return RunOutcome::error(format!(
                    "Cannot run: failed to assemble javac dependency classpath: {error}"
                ));
            }
        };
        cmd.arg("-classpath").arg(joined);
    }
    for s in &compile_sources {
        cmd.arg(s);
    }

    #[cfg(unix)]
    cmd.process_group(0);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if generation
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_current())
    {
        return RunOutcome::error(
            "Cannot run: project dependency generation changed before javac startup",
        );
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RunOutcome::error(
                "Cannot run: `javac` was not found on PATH. Install a JDK and make sure `javac` is on PATH.",
            );
        }
        Err(error) => {
            return RunOutcome::error(format!("Cannot run: failed to spawn javac: {error}"));
        }
    };
    let supervisor = ProcessSupervisor::for_child(&child);
    let Some(child_stdout) = child.stdout.take() else {
        supervisor.terminate(&mut child);
        return RunOutcome::error("Cannot run: javac stdout pipe was not available");
    };
    let Some(child_stderr) = child.stderr.take() else {
        supervisor.terminate(&mut child);
        return RunOutcome::error("Cannot run: javac stderr pipe was not available");
    };
    let stdout_reader = spawn_bounded_reader(child_stdout);
    let stderr_reader = spawn_bounded_reader(child_stderr);
    let compile_deadline = Duration::from_millis(DEFAULT_TIMEOUT_MS.saturating_sub(100).max(1));
    let compile_started = Instant::now();
    let compile_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if compile_started.elapsed() >= compile_deadline => {
                supervisor.terminate(&mut child);
                let _ = receive_reader(&stdout_reader);
                let _ = receive_reader(&stderr_reader);
                return RunOutcome::error(format!(
                    "Cannot run: javac timed out after {}ms",
                    compile_deadline.as_millis()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                supervisor.terminate(&mut child);
                let _ = receive_reader(&stdout_reader);
                let _ = receive_reader(&stderr_reader);
                return RunOutcome::error(format!(
                    "Cannot run: failed while waiting for javac: {error}"
                ));
            }
        }
    };
    let Some(_compile_stdout) = receive_reader(&stdout_reader) else {
        supervisor.terminate(&mut child);
        let _ = receive_reader(&stdout_reader);
        let _ = receive_reader(&stderr_reader);
        return RunOutcome::error(
            "Cannot run: javac stdout did not close after the process exited",
        );
    };
    let Some(compile_stderr) = receive_reader(&stderr_reader) else {
        supervisor.terminate(&mut child);
        let _ = receive_reader(&stdout_reader);
        let _ = receive_reader(&stderr_reader);
        return RunOutcome::error(
            "Cannot run: javac stderr did not close after the process exited",
        );
    };

    // The compiler process has exited and both pipes are closed. Clean up its
    // process group/job before entering the VM so compiler descendants cannot
    // outlive this invocation or retain the Windows job object.
    supervisor.terminate(&mut child);
    #[cfg(windows)]
    drop(supervisor);

    if !compile_stderr.is_empty() {
        log_lines.push((
            MessageType::WARNING,
            format!(
                "javac output:\n{}",
                String::from_utf8_lossy(&compile_stderr)
            ),
        ));
    }

    if !compile_status.success() {
        let msg = format!(
            "Compilation failed (javac exit code {:?})",
            compile_status.code()
        );
        let outcome = RunOutcome {
            log_lines,
            summary_type: MessageType::ERROR,
            summary: msg,
        };
        return outcome;
    }

    let mut runtime_classpath = vec![out_dir.clone()];
    runtime_classpath.extend(dedupe_paths(runtime_dependency_classpath));
    if generation
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_current())
    {
        return RunOutcome::error(
            "Cannot run: project dependency generation changed before VM startup",
        );
    }
    let started = Instant::now();
    let classes =
        match load_vm_classes_for_generation(&runtime_classpath, generation.as_ref(), || {}) {
            Ok(classes) => classes,
            Err(GenerationBoundClassLoadError::Classpath(error)) => {
                return RunOutcome::error(format!("Cannot run: {error}"));
            }
            Err(GenerationBoundClassLoadError::GenerationChanged) => {
                return RunOutcome::error(
                "Cannot run: project dependency generation changed while VM classes were loading",
            );
            }
        };
    // The VM receives all class bytes it needs, so temporary compiler output
    // and unsaved-source overrides can be removed before user code starts.
    // This is important when the caller abandons the blocking VM thread after
    // its request timeout expires.
    drop(cleanup);
    if generation
        .as_ref()
        .is_some_and(|snapshot| !snapshot.is_current())
    {
        return RunOutcome::error(
            "Cannot run: project dependency generation changed before VM invocation",
        );
    }
    let (exit_code, stdout, stderr) = run_loaded_in_vm(main_class, &runtime_classpath, &classes);
    let elapsed = started.elapsed();

    if !stdout.is_empty() {
        log_lines.push((MessageType::LOG, format!("[stdout]\n{stdout}")));
    }
    if !stderr.is_empty() {
        log_lines.push((MessageType::LOG, format!("[stderr]\n{stderr}")));
    }

    let summary_type = if exit_code == 0 {
        MessageType::INFO
    } else {
        MessageType::ERROR
    };
    let summary = format!(
        "{main_class} finished with exit code {exit_code} in {elapsed:.1?} \
         (single-file javac, {} classpath entries)",
        runtime_classpath.len()
    );

    RunOutcome {
        log_lines,
        summary_type,
        summary,
    }
}

fn source_differs_from_disk(file_path: &Path, source_text: &str) -> bool {
    hyperion_workspace::read_source_file(file_path).map_or(true, |on_disk| on_disk != source_text)
}

fn is_non_java_source(file_path: &Path, source_root: &Path) -> bool {
    file_path
        .extension()
        .is_some_and(|extension| matches!(extension.to_str(), Some("kt" | "kts" | "scala")))
        || source_root
            .file_name()
            .is_some_and(|name| matches!(name.to_str(), Some("kotlin" | "scala")))
}

fn write_source_override(
    workspace_root: Option<&Path>,
    source_root: &Path,
    file_path: &Path,
    source_text: &str,
) -> Result<PathBuf, String> {
    let mut hasher = Sha256::new();
    if let Some(root) = workspace_root {
        hasher.update(root.to_string_lossy().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(source_root.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(file_path.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(source_text.as_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(
        RUN_INVOCATION_COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .to_le_bytes(),
    );
    hasher.update(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| format!("failed to create source override name: {e}"))?
            .as_nanos()
            .to_le_bytes(),
    );
    let digest = hasher.finalize();
    let override_dir = output_dir_for(workspace_root, source_root)
        .join("source-overrides")
        .join(hex::encode(&digest[..16]));
    fs::create_dir_all(&override_dir).map_err(|e| {
        format!(
            "failed to create temporary source override directory {}: {e}",
            override_dir.display()
        )
    })?;

    let file_name = file_path
        .file_name()
        .ok_or_else(|| format!("source path has no file name: {}", file_path.display()))?;
    let override_path = override_dir.join(file_name);
    if let Err(error) = fs::write(&override_path, source_text) {
        let _ = fs::remove_dir_all(&override_dir);
        return Err(format!(
            "failed to write temporary source override {}: {error}",
            override_path.display()
        ));
    }
    Ok(override_path)
}

/// Find the source root for `file_path` and every same-language source file
/// in that standard source root. The fallback compiler consumes only Java
/// sources; Kotlin/Scala roots are handled by project output mode.
///
/// Prefers the Maven/Gradle-standard `src/{main,test}/{java,kotlin,scala}`
/// root (`find_nearest_source_root`), compiling every `.java` sibling under
/// it so cross-class references resolve. Falls back to deriving the root
/// from the file's `package` declaration when no standard layout is found;
/// in that fallback case only the single file is compiled, so sibling helper
/// classes require a standard source layout or project output mode.
fn resolve_sources(file_path: &Path, source_text: &str) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if let Some(root) = find_nearest_source_root(file_path) {
        let mut sources = Vec::new();
        let language = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("java");
        collect_language_files(&root, language, &mut sources)?;
        if sources.is_empty() {
            sources.push(file_path.to_path_buf());
        }
        return Ok((root, sources));
    }

    // Fallback: derive the source root from the package declaration (e.g. a
    // file at `.../com/example/Foo.java` with `package com.example;` has
    // its source root two directories up). No package -> the file's own
    // directory is the root, and only the single file is compiled.
    let mut root = file_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("file has no parent directory: {}", file_path.display()))?;
    if let Some(package) = extract_package_from_source(source_text) {
        for _ in package.split('.') {
            root = match root.parent() {
                Some(p) => p.to_path_buf(),
                None => break,
            };
        }
    }
    Ok((root, vec![file_path.to_path_buf()]))
}

/// Recursively collect source files for one standard language root.
fn collect_language_files(
    dir: &Path,
    language: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_language_files(&path, language, out)?;
        } else if path.extension().is_some_and(|ext| match language {
            "kotlin" => matches!(ext.to_str(), Some("kt" | "kts")),
            "scala" => ext == "scala",
            _ => ext == "java",
        }) {
            out.push(path);
        }
    }
    Ok(())
}

/// A stable, per-source-root temp output directory, reused across runs so
/// `javac` can do incremental recompilation instead of a clean rebuild every
/// time. Keyed by a hash of `workspace_root` + `source_root` so distinct
/// workspaces/source-roots never collide.
fn output_dir_for(workspace_root: Option<&Path>, source_root: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    if let Some(root) = workspace_root {
        hasher.update(root.to_string_lossy().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(source_root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(&digest[..8]);
    std::env::temp_dir().join("hyperion-lsp-run").join(hex)
}

static RUN_INVOCATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique output directory for one fallback invocation. Reusing a directory
/// lets removed or renamed classes survive into later runs, and concurrent
/// invocations can otherwise overwrite one another's bytecode.
fn run_output_dir_for(workspace_root: Option<&Path>, source_root: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(
        output_dir_for(workspace_root, source_root)
            .to_string_lossy()
            .as_bytes(),
    );
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(
        RUN_INVOCATION_COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .to_le_bytes(),
    );
    hasher.update(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
            .to_le_bytes(),
    );
    std::env::temp_dir()
        .join("hyperion-lsp-run")
        .join("invocations")
        .join(hex::encode(&hasher.finalize()[..16]))
}

// ---------------------------------------------------------------------
// Shared VM execution
// ---------------------------------------------------------------------

#[derive(Debug, Eq, PartialEq)]
enum GenerationBoundClassLoadError {
    Classpath(String),
    GenerationChanged,
}

/// Read the complete classpath into owned bytes, then validate the exact
/// dependency generation which named those paths.
///
/// The hook exists solely to make the otherwise narrow read/revalidation race
/// deterministic in tests. Production passes a no-op closure. Once this
/// returns, callers must invoke the VM with the returned bytes rather than
/// reopening `classpath`.
fn load_vm_classes_for_generation(
    classpath: &[PathBuf],
    generation: Option<&project_classpath::ExecutionClasspathSnapshot>,
    after_load: impl FnOnce(),
) -> Result<Vec<(String, Vec<u8>)>, GenerationBoundClassLoadError> {
    let classes = load_vm_classes(classpath).map_err(GenerationBoundClassLoadError::Classpath)?;
    after_load();
    if generation.is_some_and(|snapshot| !snapshot.is_current()) {
        return Err(GenerationBoundClassLoadError::GenerationChanged);
    }
    Ok(classes)
}

fn load_vm_classes(classpath: &[PathBuf]) -> Result<Vec<(String, Vec<u8>)>, String> {
    load_classpath_classes(classpath).map_err(|error| error.to_string())
}

fn run_loaded_in_vm(
    main_class: &str,
    classpath: &[PathBuf],
    classes: &[(String, Vec<u8>)],
) -> (i32, String, String) {
    if classes.is_empty() {
        let entries = classpath
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return (
            1,
            String::new(),
            format!("Error: no .class files found on classpath [{entries}]"),
        );
    }

    let config = MemoryConfig {
        // DEFAULT_TIMEOUT_MS is enforced by the caller's tokio::time::timeout
        // around spawn_blocking; this instruction cap is a secondary,
        // coarser safety net against runaway loops within that budget.
        max_instructions: DEFAULT_TIMEOUT_MS.saturating_mul(10_000),
        ..MemoryConfig::default()
    };
    let mut vm = VirtualMachine::new(config);
    vm.set_suppress_console(true);

    let main_internal = main_class.replace('.', "/");
    if !classes.iter().any(|(name, _)| name == &main_internal) {
        let entries = classpath
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return (
            1,
            String::new(),
            format!("Error: class '{main_class}' not found on classpath [{entries}]"),
        );
    }

    // Parse every definition into private staging, then link structural
    // dependencies parent-first. Unrelated malformed helpers remain isolated,
    // but the main class and its complete superclass/interface closure must be
    // valid before any user code executes.
    let load_report = vm.load_classes_from_bytes(classes);
    if !load_report.is_loaded(&main_internal) {
        let detail = load_report
            .failure_for(&main_internal)
            .map_or("no linked definition was produced", |failure| {
                failure.error.as_str()
            });
        return (
            1,
            String::new(),
            format!("Error: failed to link class '{main_class}': {detail}"),
        );
    }

    let args_value = match vm.make_string_array(&[]) {
        Ok(v) => v,
        Err(e) => {
            return (
                1,
                String::new(),
                format!("Error: failed to build args: {e}"),
            )
        }
    };

    // `invoke_static` errors (uncaught exceptions, missing `main`, ...) don't
    // go through `Heap::write_program_output` -- the VM has no public API to
    // inject text into its own capture buffer, so diagnostics for these
    // cases are appended to the captured stderr here instead, mirroring the
    // messages `class_runner::run_class` prints straight to the real stderr.
    let (exit_code, diagnostic) = match vm.invoke_static(
        &main_internal,
        "main",
        "([Ljava/lang/String;)V",
        &[args_value],
    ) {
        Ok(_) => (0, String::new()),
        Err(VMError::UncaughtException {
            exception_class,
            stack_trace,
            ..
        }) => {
            let mut trailer = format!(
                "Exception in thread \"main\" {}",
                exception_class.replace('/', ".")
            );
            if !stack_trace.is_empty() {
                trailer.push('\n');
                trailer.push_str(&stack_trace);
            }
            (1, trailer)
        }
        Err(VMError::NoSuchMethod(m)) => (
            1,
            format!(
                "Error: no such method: {m} (expected `public static void main(String[] args)` in {main_class})"
            ),
        ),
        Err(e) => (1, format!("Error: {e}")),
    };

    let mut stderr = vm.captured_stderr().to_string();
    if !diagnostic.is_empty() {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(&diagnostic);
    }

    (exit_code, vm.captured_stdout().to_string(), stderr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn javac_available() -> bool {
        Command::new("javac")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn path_deduplication_preserves_first_seen_order() {
        let paths = ["alpha", "beta", "alpha", "gamma", "beta"]
            .into_iter()
            .map(PathBuf::from);

        assert_eq!(
            dedupe_paths(paths),
            ["alpha", "beta", "gamma"].map(PathBuf::from)
        );
    }

    #[test]
    fn class_loading_rejects_dependency_replacement_after_owning_the_bytes() {
        let temp = tempfile::tempdir().expect("temporary generation-bound classpath");
        let workspace = temp.path();
        let dependency = workspace.join("dependency.jar");
        zip::ZipWriter::new(std::fs::File::create(&dependency).unwrap())
            .finish()
            .expect("write an empty but valid dependency JAR");
        let archive =
            hyperion_workspace::ExternalSourceArchive::capture_staged(&dependency, 1024 * 1024)
                .expect("stage dependency generation");
        let canonical_dependency = archive
            .identity()
            .archive_path()
            .expect("archive identity has a path")
            .to_path_buf();

        let store = project_classpath::ProjectClasspathStore::new();
        let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
        let workspace_ticket = index.begin_external_source_refresh("build:maven");
        let classpath_ticket = store
            .begin_project_classpath_refresh(workspace, &workspace_ticket)
            .expect("begin classpath publication");
        let _ = index
            .replace_external_archives_for_refresh(
                &workspace_ticket,
                std::slice::from_ref(&archive),
            )
            .expect("commit workspace archive generation");
        assert!(
            project_classpath::publish_maven_dependency_snapshot_with_archives_for_refresh(
                &classpath_ticket,
                vec![canonical_dependency.clone()],
                vec![archive],
            )
        );
        let generation = store
            .execution_classpath_snapshot(workspace, None)
            .expect("capture execution generation");

        let error = load_vm_classes_for_generation(
            std::slice::from_ref(&canonical_dependency),
            Some(&generation),
            || {
                std::fs::write(&canonical_dependency, b"replacement generation")
                    .expect("replace dependency after class loading");
            },
        )
        .expect_err("post-load generation fence must reject replacement");

        assert_eq!(error, GenerationBoundClassLoadError::GenerationChanged);
    }

    fn write(dir: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }

    // ---- find_module_project -------------------------------------------

    #[test]
    fn find_module_project_detects_gradle_main() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        fs::create_dir_all(&source_root).unwrap();

        let (project_root, kind, lang, set_name) =
            find_module_project(&source_root).expect("should detect Gradle module");
        assert_eq!(project_root, root);
        assert_eq!(kind, ProjectKind::Gradle);
        assert_eq!(lang, "java");
        assert_eq!(set_name, "main");
    }

    #[test]
    fn find_module_project_detects_gradle_kts() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("build.gradle.kts"), "").unwrap();
        let source_root = root.join("src/test/java");
        fs::create_dir_all(&source_root).unwrap();

        let (_, kind, _, set_name) =
            find_module_project(&source_root).expect("should detect Gradle module");
        assert_eq!(kind, ProjectKind::Gradle);
        assert_eq!(set_name, "test");
    }

    #[test]
    fn find_module_project_detects_maven() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("pom.xml"), "").unwrap();
        let source_root = root.join("src/main/java");
        fs::create_dir_all(&source_root).unwrap();

        let (project_root, kind, _, _) =
            find_module_project(&source_root).expect("should detect Maven module");
        assert_eq!(project_root, root);
        assert_eq!(kind, ProjectKind::Maven);
    }

    #[test]
    fn find_module_project_returns_none_without_build_file() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        fs::create_dir_all(&source_root).unwrap();

        assert!(find_module_project(&source_root).is_none());
    }

    #[test]
    fn find_module_project_returns_none_for_non_standard_layout() {
        // e.g. the package-derived fallback root from `resolve_sources`,
        // which never has this shape.
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("some/random/dir");
        fs::create_dir_all(&source_root).unwrap();

        assert!(find_module_project(&source_root).is_none());
    }

    // The locator is the shared `hyperion_layout::ModuleContext`; run adds its
    // own policy on top (main/test sets, verifiable languages) and always
    // names the module directory, not an ancestor that holds a build file.

    #[test]
    fn find_module_project_rejects_custom_source_sets() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("build.gradle"), "").unwrap();
        let source_root = temp.path().join("src/integrationTest/java");
        fs::create_dir_all(&source_root).unwrap();
        assert!(find_module_project(&source_root).is_none());
    }

    #[test]
    fn find_module_project_rejects_groovy_roots() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("build.gradle"), "").unwrap();
        let source_root = temp.path().join("src/main/groovy");
        fs::create_dir_all(&source_root).unwrap();
        assert!(find_module_project(&source_root).is_none());
    }

    #[test]
    fn find_module_project_reports_the_module_dir_for_a_nested_module() {
        let temp = tempfile::tempdir().unwrap();
        let module = temp.path().join("libs/core");
        fs::create_dir_all(module.join("src/main/kotlin")).unwrap();
        fs::write(module.join("build.gradle.kts"), "").unwrap();
        let (project_root, kind, lang, set) =
            find_module_project(&module.join("src/main/kotlin")).unwrap();
        assert_eq!(
            (project_root, kind, lang.as_str(), set.as_str()),
            (module, ProjectKind::Gradle, "kotlin", "main")
        );
    }

    #[test]
    fn find_module_project_names_a_module_without_its_own_build_file_not_its_ancestor() {
        // Behaviour change with the shared locator: the module's outputs live
        // under the module, even when only an ancestor has the build file.
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("build.gradle"), "").unwrap();
        let module = temp.path().join("a/b");
        fs::create_dir_all(module.join("src/main/java")).unwrap();
        assert_eq!(
            find_module_project(&module.join("src/main/java")).map(|found| found.0),
            Some(module)
        );
    }

    #[test]
    fn find_module_project_counts_a_settings_file_as_a_gradle_module_marker() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("settings.gradle"), "").unwrap();
        let source_root = temp.path().join("src/main/java");
        fs::create_dir_all(&source_root).unwrap();
        let (_, kind, _, _) = find_module_project(&source_root).unwrap();
        assert_eq!(kind, ProjectKind::Gradle);
    }

    // ---- output_dir_for_kind --------------------------------------------

    #[test]
    fn output_dir_matches_gradle_convention() {
        let root = Path::new("/proj");
        assert_eq!(
            output_dir_for_kind(root, ProjectKind::Gradle, "java", "main"),
            Path::new("/proj/build/classes/java/main")
        );
        assert_eq!(
            output_dir_for_kind(root, ProjectKind::Gradle, "java", "test"),
            Path::new("/proj/build/classes/java/test")
        );
    }

    #[test]
    fn output_dir_matches_maven_convention() {
        let root = Path::new("/proj");
        assert_eq!(
            output_dir_for_kind(root, ProjectKind::Maven, "java", "main"),
            Path::new("/proj/target/classes")
        );
        assert_eq!(
            output_dir_for_kind(root, ProjectKind::Maven, "java", "test"),
            Path::new("/proj/target/test-classes")
        );
    }

    // ---- project output freshness ----------------------------------------

    #[test]
    fn freshness_detects_a_stale_helper_even_when_main_is_newer() {
        if !javac_available() {
            eprintln!("SKIP freshness_detects_a_stale_helper_even_when_main_is_newer: no javac");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        let main = write(
            &source_root,
            "Main.java",
            "class Main { static void main(String[] a) {} }",
        );
        let helper = write(&source_root, "Helper.java", "class Helper {}");
        let output = temp.path().join("build/classes/java/main");
        fs::create_dir_all(&output).unwrap();
        compile_with_javac(&output, &[main.clone(), helper.clone()]);

        // Make only the helper newer. Comparing every source to Main.class
        // would incorrectly treat this output as fresh.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&helper, "class Helper { int changed; }").unwrap();

        let reason = ensure_source_files_fresh(&output, &source_root, &[main, helper]).unwrap_err();
        assert!(reason.contains("Helper.java"), "reason was: {reason}");
        assert!(reason.contains("stale"), "reason was: {reason}");
    }

    #[test]
    fn freshness_fails_closed_when_a_source_class_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        let source = write(&source_root, "Missing.java", "class Missing {}");
        let output = temp.path().join("build/classes/java/main");
        fs::create_dir_all(&output).unwrap();

        let reason = ensure_source_files_fresh(&output, &source_root, &[source]).unwrap_err();
        assert!(
            reason.contains("no corresponding class"),
            "reason was: {reason}"
        );
    }

    // ---- plan_project_run: end-to-end classpath assembly decision -------

    fn compile_with_javac(out_dir: &Path, sources: &[PathBuf]) {
        let mut cmd = Command::new("javac");
        cmd.arg("-d").arg(out_dir);
        for s in sources {
            cmd.arg(s);
        }
        let output = cmd.output().expect("failed to spawn javac");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn compile_with_javac_classpath(out_dir: &Path, classpath: &Path, sources: &[PathBuf]) {
        let mut cmd = Command::new("javac");
        cmd.arg("-cp").arg(classpath).arg("-d").arg(out_dir);
        for source in sources {
            cmd.arg(source);
        }
        let output = cmd.output().expect("failed to spawn javac");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn vm_run_links_workspace_subclass_to_dependency_hierarchy() {
        if !javac_available() {
            eprintln!("SKIP vm_run_links_workspace_subclass_to_dependency_hierarchy: no javac");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let dependency_sources = temp.path().join("dependency-src");
        let workspace_sources = temp.path().join("workspace-src");
        let dependency_output = temp.path().join("dependency-classes");
        let workspace_output = temp.path().join("workspace-classes");
        fs::create_dir_all(&dependency_output).unwrap();
        fs::create_dir_all(&workspace_output).unwrap();

        let service = write(
            &dependency_sources,
            "dependency/Service.java",
            "package dependency; public interface Service { default int bonus() { return 1; } }",
        );
        let base = write(
            &dependency_sources,
            "dependency/AbstractBase.java",
            "package dependency; public abstract class AbstractBase implements Service { protected int value = 41; }",
        );
        compile_with_javac(&dependency_output, &[service, base]);
        let main = write(
            &workspace_sources,
            "workspace/Main.java",
            "package workspace; public final class Main extends dependency.AbstractBase { public static void main(String[] args) { Main value = new Main(); System.out.println(value.value + value.bonus()); } }",
        );
        compile_with_javac_classpath(&workspace_output, &dependency_output, &[main]);

        let classpath = vec![workspace_output, dependency_output];
        let mut classes = load_vm_classes(&classpath).expect("load compiled classpath");
        classes.push(("broken/Unrelated".to_string(), vec![0xca, 0xfe]));
        let (exit_code, stdout, stderr) = run_loaded_in_vm("workspace.Main", &classpath, &classes);

        assert_eq!(exit_code, 0, "stderr: {stderr}");
        assert_eq!(stdout, "42\n");
        assert!(stderr.is_empty(), "stderr: {stderr}");
    }

    #[test]
    fn vm_run_rejects_main_when_its_dependency_superclass_is_missing() {
        if !javac_available() {
            eprintln!(
                "SKIP vm_run_rejects_main_when_its_dependency_superclass_is_missing: no javac"
            );
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let dependency_sources = temp.path().join("dependency-src");
        let workspace_sources = temp.path().join("workspace-src");
        let dependency_output = temp.path().join("dependency-classes");
        let workspace_output = temp.path().join("workspace-classes");
        fs::create_dir_all(&dependency_output).unwrap();
        fs::create_dir_all(&workspace_output).unwrap();

        let base = write(
            &dependency_sources,
            "dependency/Base.java",
            "package dependency; public class Base {}",
        );
        compile_with_javac(&dependency_output, &[base]);
        let main = write(
            &workspace_sources,
            "workspace/Main.java",
            "package workspace; public final class Main extends dependency.Base { public static void main(String[] args) { System.out.println(\"WRONG\"); } }",
        );
        compile_with_javac_classpath(&workspace_output, &dependency_output, &[main]);

        let classpath = vec![workspace_output];
        let classes = load_vm_classes(&classpath).expect("load workspace classes");
        let (exit_code, stdout, stderr) = run_loaded_in_vm("workspace.Main", &classpath, &classes);

        assert_eq!(exit_code, 1);
        assert!(stdout.is_empty());
        assert!(stderr.contains("failed to link class 'workspace.Main'"));
        assert!(stderr.contains("dependency.Base"), "stderr: {stderr}");
    }

    #[test]
    fn plan_project_run_falls_back_when_dependency_jars_unknown() {
        if !javac_available() {
            eprintln!("SKIP plan_project_run_falls_back_when_dependency_jars_unknown: no javac");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        let main_java = write(
            &source_root,
            "Main.java",
            "public class Main { public static void main(String[] a) {} }",
        );

        let out_dir = root.join("build/classes/java/main");
        fs::create_dir_all(&out_dir).unwrap();
        compile_with_javac(&out_dir, std::slice::from_ref(&main_java));

        // Deliberately never call `project_classpath::record_dependency_jars`
        // for this root -- dependency data is "unknown".
        let unique_root = root.join("never-recorded-marker");
        let decision = plan_project_run(&source_root, &[main_java], "Main", Some(&unique_root));
        match decision {
            ProjectRunDecision::Fallback { reason } => {
                assert!(reason.contains("unknown"), "reason was: {reason}");
            }
            ProjectRunDecision::Run { .. } => {
                panic!("must fall back when dependency JARs are unknown for the workspace")
            }
        }
    }

    #[test]
    fn plan_project_run_uses_project_classpath_when_fresh_and_known() {
        if !javac_available() {
            eprintln!(
                "SKIP plan_project_run_uses_project_classpath_when_fresh_and_known: no javac"
            );
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        let main_java = write(
            &source_root,
            "Main.java",
            "public class Main { public static void main(String[] a) {} }",
        );

        let out_dir = root.join("build/classes/java/main");
        fs::create_dir_all(&out_dir).unwrap();
        compile_with_javac(&out_dir, std::slice::from_ref(&main_java));

        project_classpath::record_dependency_jars(&root, vec![PathBuf::from("/libs/guava.jar")]);

        let decision = plan_project_run(&source_root, &[main_java], "Main", Some(&root));
        match decision {
            ProjectRunDecision::Run { classpath, .. } => {
                assert_eq!(classpath.len(), 2, "output dir + 1 known dependency jar");
                assert_eq!(classpath[0], out_dir);
            }
            ProjectRunDecision::Fallback { reason } => {
                panic!("expected project classpath mode, got fallback: {reason}")
            }
        }
    }

    #[test]
    fn plan_project_run_includes_existing_mixed_gradle_language_outputs() {
        if !javac_available() {
            eprintln!(
                "SKIP plan_project_run_includes_existing_mixed_gradle_language_outputs: no javac"
            );
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        let main_java = write(
            &source_root,
            "Main.java",
            "public class Main { public static void main(String[] a) {} }",
        );

        let java_output = root.join("build/classes/java/main");
        let kotlin_source_root = root.join("src/main/kotlin");
        let kotlin_source = write(&kotlin_source_root, "Helper.kt", "fun helper() {}\n");
        let kotlin_output = root.join("build/classes/kotlin/main");
        fs::create_dir_all(&java_output).unwrap();
        fs::create_dir_all(&kotlin_output).unwrap();
        compile_with_javac(&java_output, std::slice::from_ref(&main_java));
        fs::write(kotlin_output.join("HelperKt.class"), b"fixture class").unwrap();
        project_classpath::record_dependency_jars(&root, Vec::new());

        let decision = plan_project_run(&source_root, &[main_java], "Main", Some(&root));
        match decision {
            ProjectRunDecision::Run { classpath, .. } => {
                assert_eq!(classpath[0], java_output);
                assert!(classpath.contains(&kotlin_output));
                assert!(kotlin_source.exists());
            }
            ProjectRunDecision::Fallback { reason } => {
                panic!("expected mixed output directories in classpath, got: {reason}")
            }
        }
    }

    #[test]
    fn plan_project_run_accepts_kotlin_file_facade_output() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/kotlin");
        let source = write(&source_root, "Main.kt", "fun main() {}\n");
        let output = root.join("build/classes/kotlin/main");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("MainKt.class"), b"fixture class").unwrap();
        project_classpath::record_dependency_jars(&root, Vec::new());

        let decision = plan_project_run(&source_root, &[source], "MainKt", Some(&root));
        match decision {
            ProjectRunDecision::Run { .. } => {}
            ProjectRunDecision::Fallback { reason } => {
                panic!("a normal Kotlin file facade should be recognized: {reason}")
            }
        }
    }

    #[test]
    fn freshness_requires_all_kotlin_top_level_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/kotlin");
        let source = write(&source_root, "Helper.kt", "class Helper\nfun main() {}\n");
        let output = temp.path().join("build/classes/kotlin/main");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("HelperKt.class"), b"fixture class").unwrap();

        let reason = ensure_source_files_fresh(&output, &source_root, &[source]).unwrap_err();
        assert!(reason.contains("Helper.class"), "reason was: {reason}");
    }

    #[test]
    fn freshness_reads_kotlin_file_jvm_name_annotation_from_cst() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/kotlin");
        let source = write(
            &source_root,
            "main.kt",
            "@file:JvmName /* not a string \"Fake\" */ (\"Runner\\u0024Generated\")\nfun main() {}\n",
        );
        let output = temp.path().join("build/classes/kotlin/main");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("Runner$Generated.class"), b"fixture class").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_requires_kotlin_multifile_part_and_facade() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/kotlin");
        let source = write(
            &source_root,
            "main.kt",
            "@file:JvmName(\"Util\")\n@file:JvmMultifileClass\nfun main() {}\n",
        );
        let output = temp.path().join("build/classes/kotlin/main");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("Util.class"), b"fixture facade").unwrap();
        fs::write(output.join("Util__MainKt.class"), b"fixture part").unwrap();

        ensure_source_files_fresh(&output, &source_root, std::slice::from_ref(&source)).unwrap();
        fs::remove_file(output.join("Util__MainKt.class")).unwrap();
        let reason = ensure_source_files_fresh(&output, &source_root, &[source]).unwrap_err();
        assert!(
            reason.contains("Util__MainKt.class"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn freshness_uses_declared_packages_instead_of_physical_directories() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        let source = write(
            &source_root,
            "nested/Main.java",
            "/* package fake */\npackage declared.path; class Main {}\n",
        );
        let output = temp.path().join("build/classes/java/main");
        fs::create_dir_all(output.join("declared/path")).unwrap();
        fs::write(output.join("declared/path/Main.class"), b"fixture class").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_requires_all_java_top_level_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        let source = write(
            &source_root,
            "physical/Types.java",
            "/* package fake */\npackage\tdeclared.path;\nclass Types {}\nclass Helper {}\n",
        );
        let output = temp.path().join("build/classes/java/main");
        fs::create_dir_all(output.join("declared/path")).unwrap();
        fs::write(output.join("declared/path/Types.class"), b"fixture class").unwrap();

        let reason = ensure_source_files_fresh(&output, &source_root, &[source]).unwrap_err();
        assert!(reason.contains("Helper.class"), "reason was: {reason}");
    }

    #[test]
    fn freshness_handles_kotlin_annotation_before_package_and_default_layout() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/kotlin");
        let source = write(
            &source_root,
            "physical/Main.kt",
            "@file:JvmName (\"Runner\")\npackage declared.path\nfun main() {}\n",
        );
        let output = temp.path().join("build/classes/kotlin/main");
        fs::create_dir_all(output.join("declared/path")).unwrap();
        fs::write(output.join("declared/path/Runner.class"), b"fixture class").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_handles_kotlin_escaped_type_identifiers() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/kotlin");
        let source = write(
            &source_root,
            "Names.kt",
            "package a.companion\nclass `foo-bar`\n",
        );
        let output = temp.path().join("build/classes/kotlin/main");
        fs::create_dir_all(output.join("a/companion")).unwrap();
        fs::write(output.join("a/companion/foo-bar.class"), b"fixture class").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_maps_scala_top_level_declarations_without_comment_or_nested_leaks() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/scala");
        let source = write(
            &source_root,
            "physical/Main.scala",
            "/* outer /* object Fake */ */\npackage declared.path:\n  object Main:\n    class Inner\n",
        );
        let output = temp.path().join("build/classes/scala/main");
        fs::create_dir_all(output.join("declared/path")).unwrap();
        fs::write(output.join("declared/path/Main.class"), b"fixture class").unwrap();
        fs::write(output.join("declared/path/Main$.class"), b"fixture module").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_maps_scala_nested_packages_and_package_objects() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/scala");
        let nested = write(
            &source_root,
            "Nested.scala",
            "package first; package second; object Main {}\n",
        );
        let package_object = write(
            &source_root,
            "Package.scala",
            "package object helpers { val answer = 42 }\n",
        );
        let output = temp.path().join("build/classes/scala/main");
        fs::create_dir_all(output.join("first/second")).unwrap();
        fs::create_dir_all(output.join("helpers")).unwrap();
        fs::write(output.join("first/second/Main.class"), b"fixture class").unwrap();
        fs::write(output.join("first/second/Main$.class"), b"fixture module").unwrap();
        fs::write(output.join("helpers/package.class"), b"fixture package").unwrap();
        fs::write(output.join("helpers/package$.class"), b"fixture module").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[nested, package_object]).unwrap();
    }

    #[test]
    fn freshness_accepts_scala_three_package_parts() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/scala");
        let source = write(
            &source_root,
            "Main.scala",
            "package declared.path\ndef main = println(\"ok\")\n",
        );
        let output = temp.path().join("build/classes/scala/main");
        fs::create_dir_all(output.join("declared/path")).unwrap();
        fs::write(
            output.join("declared/path/Main$package.class"),
            b"fixture package",
        )
        .unwrap();
        fs::write(
            output.join("declared/path/Main$package$.class"),
            b"fixture package module",
        )
        .unwrap();

        ensure_source_files_fresh(&output, &source_root, std::slice::from_ref(&source)).unwrap();
        fs::remove_file(output.join("declared/path/Main$package$.class")).unwrap();
        let reason =
            ensure_source_files_fresh(&output, &source_root, std::slice::from_ref(&source))
                .unwrap_err();
        assert!(
            reason.contains("Main$package$.class"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn freshness_maps_scala_sibling_package_blocks_separately() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/scala");
        let source = write(
            &source_root,
            "Sibling.scala",
            "package first { class First }; package second { class Second }\n",
        );
        let output = temp.path().join("build/classes/scala/main");
        fs::create_dir_all(output.join("first")).unwrap();
        fs::create_dir_all(output.join("second")).unwrap();
        fs::write(output.join("first/First.class"), b"fixture first").unwrap();
        fs::write(output.join("second/Second.class"), b"fixture second").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn freshness_restores_parent_after_nested_scala_colon_package() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/scala");
        let source = write(
            &source_root,
            "Nested.scala",
            "package outer:\n  package inner:\n    class Inner\n  class Outer\n",
        );
        let output = temp.path().join("build/classes/scala/main");
        fs::create_dir_all(output.join("outer/inner")).unwrap();
        fs::create_dir_all(output.join("outer")).unwrap();
        fs::write(output.join("outer/inner/Inner.class"), b"fixture inner").unwrap();
        fs::write(output.join("outer/Outer.class"), b"fixture outer").unwrap();

        ensure_source_files_fresh(&output, &source_root, &[source]).unwrap();
    }

    #[test]
    fn fallback_skips_empty_mixed_language_source_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        let source = write(&source_root, "Main.java", "class Main {}\n");
        let kotlin_root = root.join("src/main/kotlin");
        fs::create_dir_all(&kotlin_root).unwrap();
        let kotlin_output = root.join("build/classes/kotlin/main");
        fs::create_dir_all(&kotlin_output).unwrap();
        fs::write(kotlin_output.join("StaleKt.class"), b"stale").unwrap();

        let (verified_compile, verified_runtime) =
            verified_project_output_dirs(None, &source_root, &[source]);
        assert!(!verified_compile.contains(&kotlin_output));
        assert!(!verified_runtime.contains(&kotlin_output));
    }

    #[test]
    fn fallback_output_directories_are_unique_per_invocation() {
        let temp = tempfile::tempdir().unwrap();
        let first = run_output_dir_for(Some(temp.path()), temp.path());
        let second = run_output_dir_for(Some(temp.path()), temp.path());
        assert_ne!(first, second);
    }

    #[test]
    fn plan_project_run_falls_back_when_class_is_stale() {
        if !javac_available() {
            eprintln!("SKIP plan_project_run_falls_back_when_class_is_stale: no javac");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("build.gradle"), "").unwrap();
        let source_root = root.join("src/main/java");
        let main_java = write(
            &source_root,
            "Main.java",
            "public class Main { public static void main(String[] a) {} }",
        );

        let out_dir = root.join("build/classes/java/main");
        fs::create_dir_all(&out_dir).unwrap();
        compile_with_javac(&out_dir, std::slice::from_ref(&main_java));

        project_classpath::record_dependency_jars(&root, Vec::new());

        // Edit the source after compiling, forcing its mtime to be newer
        // than the already-compiled .class file.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            &main_java,
            "public class Main { public static void main(String[] a) { System.out.println(1); } }",
        )
        .unwrap();

        let decision = plan_project_run(&source_root, &[main_java], "Main", Some(&root));
        match decision {
            ProjectRunDecision::Fallback { reason } => {
                assert!(reason.contains("stale"), "reason was: {reason}");
            }
            ProjectRunDecision::Run { .. } => panic!("must fall back for a stale compiled class"),
        }
    }

    #[test]
    fn plan_project_run_falls_back_when_no_build_file() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("src/main/java");
        fs::create_dir_all(&source_root).unwrap();

        let decision = plan_project_run(&source_root, &[], "Main", Some(temp.path()));
        assert!(matches!(decision, ProjectRunDecision::Fallback { .. }));
    }
}
