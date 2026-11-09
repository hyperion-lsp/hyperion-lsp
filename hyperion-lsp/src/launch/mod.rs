// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `hyperion.resolveLaunch`.
//!
//! Turns an editor position into a complete, ready-to-run description of how
//! to build and launch (or test) the enclosing entry point. See
//! `docs/editor-integration.md` for the contract.

mod project;
pub(crate) mod scan;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tower_lsp::lsp_types::Position;

use crate::project_classpath::ProjectClasspathStore;
use project::{
    conventional_outputs, gradle_executable, gradle_project_path, maven_executable, path_separator,
    BuildTool, ModuleContext,
};
use scan::{MainKind, ScannedFile};

/// Which kind of launch the client asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTarget {
    Auto,
    Main,
    Test,
}

impl LaunchTarget {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("main") => Self::Main,
            Some("test") => Self::Test,
            _ => Self::Auto,
        }
    }
}

pub struct LaunchRequest<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub position: Option<Position>,
    pub target: LaunchTarget,
    pub workspace_root: &'a Path,
    pub store: &'a ProjectClasspathStore,
}

enum Selection {
    Main {
        /// Binary class name (package-less) that owns the launched `main`.
        class: String,
        kind: MainKind,
    },
    Test {
        class: String,
        method: Option<String>,
    },
}

/// Resolve the launch description for `request`, or `None` when the file has
/// no entry point and no test for the requested target.
pub fn resolve_launch(request: &LaunchRequest<'_>) -> Option<Value> {
    let file = scan_file(request.path, request.text)?;
    let offset = request.position.and_then(|position| {
        hyperion_workspace::position_utils::position_to_offset(request.text, position).ok()
    });
    let selection = select(&file, offset, request.target)?;
    let ctx = ModuleContext::locate(
        request.path,
        file.package.as_deref(),
        request.workspace_root,
    );
    let mut warnings = Vec::new();
    let value = match selection {
        Selection::Main { class, kind } => {
            if kind == MainKind::Extended {
                warnings.push(
                    "this entry point uses the instance/no-argument `main` form: it needs JDK 25+ \
                     (or --enable-preview on JDK 21-24)"
                        .to_owned(),
                );
            }
            main_launch(request, &file, &ctx, &class, &mut warnings)
        }
        Selection::Test { class, method } => {
            test_launch(request, &file, &ctx, &class, method, &mut warnings)?
        }
    };
    Some(with_warnings(value, warnings))
}

fn with_warnings(mut value: Value, warnings: Vec<String>) -> Value {
    value["warnings"] = json!(warnings);
    value
}

fn scan_file(path: &Path, text: &str) -> Option<ScannedFile> {
    match path.extension()?.to_str()? {
        "java" => Some(scan::scan_java(text)),
        "kt" => {
            let stem = path.file_stem()?.to_str()?;
            Some(scan::scan_kotlin(text, stem))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

fn select(file: &ScannedFile, offset: Option<usize>, target: LaunchTarget) -> Option<Selection> {
    let test = || select_test(file, offset);
    let main = || select_main(file, offset);
    match target {
        LaunchTarget::Test => test(),
        LaunchTarget::Main => main(),
        LaunchTarget::Auto => {
            // A test method under the cursor wins, then a `main` method under
            // the cursor, then the enclosing test class; otherwise the file's
            // entry point, and a test class only when there is no entry point.
            if let Some(selection) = offset.and_then(|offset| test_method_at(file, offset)) {
                return Some(selection);
            }
            if let Some(offset) = offset {
                let in_main = file.classes.iter().any(|class| {
                    class.methods.iter().any(|method| {
                        method.main.is_some() && contains(method.start, method.end, offset)
                    })
                });
                if in_main {
                    return main();
                }
            }
            if let Some(selection) = offset.and_then(|offset| test_class_at(file, offset)) {
                return Some(selection);
            }
            main().or_else(test)
        }
    }
}

const fn contains(start: usize, end: usize, offset: usize) -> bool {
    start <= offset && offset <= end
}

/// Innermost test method around the position.
fn test_method_at(file: &ScannedFile, offset: usize) -> Option<Selection> {
    let method = file
        .classes
        .iter()
        .flat_map(|class| class.methods.iter().map(move |method| (class, method)))
        .filter(|(_, method)| method.is_test && contains(method.start, method.end, offset))
        .min_by_key(|(_, method)| method.end - method.start);
    method.map(|(class, method)| Selection::Test {
        class: class.binary_name.clone(),
        method: Some(method.name.clone()),
    })
}

/// Innermost class around the position that has tests.
fn test_class_at(file: &ScannedFile, offset: usize) -> Option<Selection> {
    file.classes
        .iter()
        .filter(|class| {
            !class.is_facade
                && contains(class.start, class.end, offset)
                && file.class_has_tests(class)
        })
        .min_by_key(|class| class.end - class.start)
        .map(|class| Selection::Test {
            class: class.binary_name.clone(),
            method: None,
        })
}

fn select_test(file: &ScannedFile, offset: Option<usize>) -> Option<Selection> {
    if let Some(selection) = offset
        .and_then(|offset| test_method_at(file, offset).or_else(|| test_class_at(file, offset)))
    {
        return Some(selection);
    }
    // No (usable) position: the first top-level test class.
    file.classes
        .iter()
        .find(|class| !class.is_facade && file.class_has_tests(class))
        .map(|class| Selection::Test {
            class: class.binary_name.clone(),
            method: None,
        })
}

fn select_main(file: &ScannedFile, offset: Option<usize>) -> Option<Selection> {
    struct Candidate<'a> {
        class: &'a scan::ScannedClass,
        method: &'a scan::ScannedMethod,
        kind: MainKind,
    }
    let candidates: Vec<Candidate<'_>> = file
        .classes
        .iter()
        .flat_map(|class| {
            class.methods.iter().filter_map(move |method| {
                method.main.map(|kind| Candidate {
                    class,
                    method,
                    kind,
                })
            })
        })
        .collect();
    let launch_class = |candidate: &Candidate<'_>| {
        candidate
            .method
            .launch_class
            .clone()
            .unwrap_or_else(|| candidate.class.binary_name.clone())
    };
    let chosen = offset
        .and_then(|offset| {
            // The `main` method itself, then the class around the position.
            candidates
                .iter()
                .filter(|c| contains(c.method.start, c.method.end, offset))
                .min_by_key(|c| c.method.end - c.method.start)
                .or_else(|| {
                    candidates
                        .iter()
                        .filter(|c| {
                            !c.class.is_facade && contains(c.class.start, c.class.end, offset)
                        })
                        .min_by_key(|c| c.class.end - c.class.start)
                })
        })
        // Classic entry points beat extended ones when nothing else decides.
        .or_else(|| candidates.iter().find(|c| c.kind == MainKind::Classic))
        .or_else(|| candidates.first())?;
    Some(Selection::Main {
        class: launch_class(chosen),
        kind: chosen.kind,
    })
}

// ---------------------------------------------------------------------------
// Result construction
// ---------------------------------------------------------------------------

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn module_label(ctx: &ModuleContext, workspace_root: &Path) -> String {
    let dir = if ctx.module_dir == workspace_root {
        workspace_root
    } else {
        &ctx.module_dir
    };
    dir.file_name().map_or_else(
        || "project".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Runtime classpath of the module's source set, plus any warnings about how
/// complete it is.
fn runtime_classpath(
    request_root: &Path,
    store: &ProjectClasspathStore,
    ctx: &ModuleContext,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut entries = conventional_outputs(ctx);
    if ctx.build_tool == BuildTool::None {
        entries.push(plain_output_dir(ctx));
        entries.extend(plain_lib_jars(ctx));
    }
    if ctx.build_tool != BuildTool::None {
        let snapshot = store
            .execution_classpath_snapshot(request_root, Some((&ctx.module_dir, &ctx.source_set)));
        match snapshot {
            Some(snapshot) => {
                if let Some(project) = snapshot.project.clone() {
                    for dir in project.output_dirs {
                        // Depended-on Gradle modules: their resources belong
                        // next to their compiled classes.
                        let resources = gradle_resources_sibling(&dir);
                        entries.push(dir);
                        entries.extend(resources);
                    }
                    entries.extend(project.dependency_jars);
                } else if let Some(jars) = snapshot.runtime_dependency_jars.clone() {
                    warnings.push(
                        "no per-module dependency model for this module; using the workspace-wide \
                         runtime dependency set"
                            .to_owned(),
                    );
                    entries.extend(jars);
                } else {
                    warnings.push(
                        "dependency jars are not resolved yet (the build model may still be loading; see \
                         hyperion.reloadProject); the launch classpath contains module outputs only"
                            .to_owned(),
                    );
                }
                if !snapshot.is_current() {
                    warnings.push(
                        "the dependency model changed while the launch was being resolved; \
                         resolve again"
                            .to_owned(),
                    );
                }
            }
            None => warnings.push(
                "the build model is still loading or failed to load; the launch classpath \
                 contains module outputs only (see hyperion.reloadProject)"
                    .to_owned(),
            ),
        }
        if ctx.build_tool == BuildTool::Gradle {
            if let Some(report) =
                store.known_gradle_resolution_for_path(request_root, &ctx.module_dir)
            {
                if !report.complete {
                    warnings.push(format!(
                        "classpath incomplete: {} dependencies not in the local cache{}",
                        report.missing.len().max(report.incomplete_reasons.len()),
                        report
                            .missing
                            .first()
                            .map(|first| format!(" (e.g. {first})"))
                            .unwrap_or_default()
                    ));
                }
            }
        }
    }
    dedupe(entries)
}

/// `<m>/build/classes/<lang>/<set>` -> `<m>/build/resources/<set>`.
fn gradle_resources_sibling(output_dir: &Path) -> Option<PathBuf> {
    let set = output_dir.file_name()?;
    let lang_dir = output_dir.parent()?;
    let classes = lang_dir.parent()?;
    if classes.file_name()? != "classes" {
        return None;
    }
    let build = classes.parent()?;
    (build.file_name()? == "build").then(|| build.join("resources").join(set))
}

fn dedupe(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn join_classpath(entries: &[PathBuf]) -> String {
    entries
        .iter()
        .map(|entry| display(entry))
        .collect::<Vec<_>>()
        .join(path_separator())
}

fn plain_output_dir(ctx: &ModuleContext) -> PathBuf {
    ctx.module_dir.join("out/production")
}

fn plain_lib_jars(ctx: &ModuleContext) -> Vec<PathBuf> {
    let mut jars: Vec<PathBuf> = std::fs::read_dir(ctx.module_dir.join("lib"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jar"))
        .collect();
    jars.sort();
    jars
}

fn main_launch(
    request: &LaunchRequest<'_>,
    file: &ScannedFile,
    ctx: &ModuleContext,
    class: &str,
    warnings: &mut Vec<String>,
) -> Value {
    let main_class = file.qualify(class);
    let classpath = runtime_classpath(request.workspace_root, request.store, ctx, warnings);
    let simple = class.rsplit('$').next().unwrap_or(class);
    let label = if file
        .classes
        .iter()
        .any(|c| c.is_facade && c.binary_name == class)
    {
        request
            .path
            .file_stem()
            .map_or_else(|| simple.to_owned(), |s| s.to_string_lossy().into_owned())
    } else {
        simple.to_owned()
    };
    let build = build_argv(request, file, ctx, &classpath, warnings);
    json!({
        "name": format!("{label} ({})", module_label(ctx, request.workspace_root)),
        "kind": "main",
        "language": language_name(file),
        "projectRoot": display(request.workspace_root),
        "moduleDir": display(&ctx.module_dir),
        "buildTool": ctx.build_tool.as_str(),
        "build": build,
        "launch": {
            "mainClass": main_class,
            "classpath": join_classpath(&classpath),
            "args": [],
            "jvmArgs": [],
            "cwd": display(&ctx.module_dir),
            "projectRoot": display(request.workspace_root),
            "env": {},
        },
    })
}

const fn language_name(file: &ScannedFile) -> &'static str {
    match file.language {
        hyperion_workspace::Language::Kotlin => "kotlin",
        _ => "java",
    }
}

fn build_argv(
    request: &LaunchRequest<'_>,
    file: &ScannedFile,
    ctx: &ModuleContext,
    classpath: &[PathBuf],
    warnings: &mut Vec<String>,
) -> Value {
    match ctx.build_tool {
        BuildTool::Gradle => {
            let path = gradle_project_path(&ctx.build_root, &ctx.module_dir);
            let task = if ctx.is_test_set() {
                format!("{}Classes", ctx.source_set)
            } else {
                "classes".to_owned()
            };
            json!({
                "argv": [
                    gradle_executable(&ctx.build_root),
                    gradle_task(&path, &task),
                    "--console=plain",
                ],
                "cwd": display(&ctx.build_root),
            })
        }
        BuildTool::Maven => {
            let goal = if ctx.is_test_set() {
                "test-compile"
            } else {
                "compile"
            };
            let mut argv = vec![maven_executable(&ctx.build_root), "-q".to_owned()];
            argv.extend(maven_module_args(ctx));
            argv.push(goal.to_owned());
            json!({ "argv": argv, "cwd": display(&ctx.build_root) })
        }
        BuildTool::None => {
            if file.language != hyperion_workspace::Language::Java {
                warnings.push(
                    "no Gradle or Maven build was found and Kotlin sources cannot be compiled \
                     without one; run must use classes that are already built"
                        .to_owned(),
                );
                return Value::Null;
            }
            let source_root = ctx
                .source_root
                .clone()
                .or_else(|| request.path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| ctx.module_dir.clone());
            let out = plain_output_dir(ctx);
            let mut argv = vec![
                "javac".to_owned(),
                // Debug info: without it a debugger sees no local variables.
                "-g".to_owned(),
                "-d".to_owned(),
                display(&out),
                "-encoding".to_owned(),
                "UTF-8".to_owned(),
                "-sourcepath".to_owned(),
                display(&source_root),
            ];
            let libs = plain_lib_jars(ctx);
            if !libs.is_empty() {
                argv.push("-cp".to_owned());
                argv.push(join_classpath(&libs));
            }
            argv.push(display(request.path));
            let _ = classpath;
            json!({ "argv": argv, "cwd": display(&ctx.module_dir) })
        }
    }
}

fn gradle_task(project_path: &str, task: &str) -> String {
    if project_path == ":" {
        format!(":{task}")
    } else {
        format!("{project_path}:{task}")
    }
}

/// `-pl <module> -am` when the module is not the reactor root.
fn maven_module_args(ctx: &ModuleContext) -> Vec<String> {
    match ctx.module_dir.strip_prefix(&ctx.build_root) {
        Ok(relative) if !relative.as_os_str().is_empty() => vec![
            "-pl".to_owned(),
            relative.to_string_lossy().replace('\\', "/"),
            "-am".to_owned(),
        ],
        _ => Vec::new(),
    }
}

fn test_launch(
    request: &LaunchRequest<'_>,
    file: &ScannedFile,
    ctx: &ModuleContext,
    class: &str,
    method: Option<String>,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    if ctx.build_tool == BuildTool::None {
        // Nothing can run a test without Gradle or Maven.
        return None;
    }
    let class_name = file.qualify(class);
    let classpath = runtime_classpath(request.workspace_root, request.store, ctx, warnings);
    let label_class = class.rsplit('$').next().unwrap_or(class);
    let name = match &method {
        Some(method) => format!("{label_class}.{method}"),
        None => label_class.to_owned(),
    };
    let module = module_label(ctx, request.workspace_root);

    let (gradle_task_name, argv, debug_argv, reports_dir) = match ctx.build_tool {
        BuildTool::Gradle => {
            let path = gradle_project_path(&ctx.build_root, &ctx.module_dir);
            let task_name = if ctx.source_set == "test" {
                "test".to_owned()
            } else {
                ctx.source_set.clone()
            };
            let task = gradle_task(&path, &task_name);
            let filter = match &method {
                Some(method) => format!("{class_name}.{method}"),
                None => class_name.clone(),
            };
            // `cleanTest` first: Gradle would otherwise skip an unchanged,
            // already-passed test task instead of running it again.
            let mut argv = vec![
                gradle_executable(&ctx.build_root),
                gradle_task(&path, &format!("clean{}", capitalize(&task_name))),
                task.clone(),
                "--tests".to_owned(),
                filter,
            ];
            let mut debug_argv = argv.clone();
            argv.push("--console=plain".to_owned());
            debug_argv.push("--debug-jvm".to_owned());
            debug_argv.push("--console=plain".to_owned());
            (
                Some(task),
                argv,
                debug_argv,
                ctx.module_dir.join("build/test-results").join(task_name),
            )
        }
        _ => {
            let selector = match &method {
                Some(method) => format!("{class_name}#{method}"),
                None => class_name.clone(),
            };
            let mut argv = vec![
                maven_executable(&ctx.build_root),
                format!("-Dtest={selector}"),
            ];
            let modules = maven_module_args(ctx);
            if !modules.is_empty() {
                // Upstream modules in the reactor have no such test.
                argv.extend(modules);
                argv.push("-Dsurefire.failIfNoSpecifiedTests=false".to_owned());
                argv.push("-DfailIfNoTests=false".to_owned());
            }
            argv.push("test".to_owned());
            let mut debug_argv = argv.clone();
            debug_argv.insert(2, "-Dmaven.surefire.debug".to_owned());
            (
                None,
                argv,
                debug_argv,
                ctx.module_dir.join("target/surefire-reports"),
            )
        }
    };

    Some(json!({
        "name": format!("{name} ({module})"),
        "kind": "test",
        "language": language_name(file),
        "projectRoot": display(request.workspace_root),
        "moduleDir": display(&ctx.module_dir),
        "buildTool": ctx.build_tool.as_str(),
        "build": Value::Null,
        // Informational for tests: tests run through the build tool, not DAP `launch`.
        "launch": {
            "mainClass": class_name,
            "classpath": join_classpath(&classpath),
            "args": [],
            "jvmArgs": [],
            "cwd": display(&ctx.module_dir),
            "projectRoot": display(request.workspace_root),
            "env": {},
        },
        "test": {
            "className": class_name,
            "methodName": method,
            "gradleTask": gradle_task_name,
            "argv": argv,
            "debugArgv": debug_argv,
            "cwd": display(&ctx.build_root),
            "reportsDir": display(&reports_dir),
        },
    }))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Run configurations
// ---------------------------------------------------------------------------

/// Fill `classpath`, `cwd`, `projectRoot` (and `build`/`warnings`) into an
/// Application run configuration, resolved like `hyperion.resolveLaunch`.
pub fn enrich_application_config(
    config: &mut Value,
    workspace_root: &Path,
    store: &ProjectClasspathStore,
) {
    let Some(main_class) = config
        .get("mainClass")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let mut warnings = Vec::new();
    let located = find_source_for_class(workspace_root, &main_class);
    let (ctx, file, path) = match &located {
        Some((path, file)) => (
            ModuleContext::locate(path, file.package.as_deref(), workspace_root),
            Some(file),
            Some(path.as_path()),
        ),
        None => {
            warnings.push(format!(
                "could not find the source file of {main_class}; using the workspace root's \
                 classpath"
            ));
            (
                ModuleContext::for_dir(workspace_root, workspace_root),
                None,
                None,
            )
        }
    };
    let classpath = runtime_classpath(workspace_root, store, &ctx, &mut warnings);
    config["classpath"] = json!(join_classpath(&classpath));
    config["projectRoot"] = json!(display(workspace_root));
    let configured_cwd = config
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|cwd| !cwd.contains("$MODULE_WORKING_DIR$"));
    config["cwd"] = json!(configured_cwd.unwrap_or_else(|| display(&ctx.module_dir)));
    config["moduleDir"] = json!(display(&ctx.module_dir));
    config["buildTool"] = json!(ctx.build_tool.as_str());
    if let (Some(file), Some(path)) = (file, path) {
        let request = LaunchRequest {
            path,
            text: "",
            position: None,
            target: LaunchTarget::Main,
            workspace_root,
            store,
        };
        config["build"] = build_argv(&request, file, &ctx, &classpath, &mut warnings);
    }
    config["warnings"] = json!(warnings);
}

/// Locate and scan the source file declaring `main_class` (a binary name such
/// as `com.example.Main`, `com.example.Main$Inner` or `com.example.AppKt`).
fn find_source_for_class(
    workspace_root: &Path,
    main_class: &str,
) -> Option<(PathBuf, ScannedFile)> {
    let top = main_class.split('$').next().unwrap_or(main_class);
    let simple = top.rsplit('.').next().unwrap_or(top);
    let mut names = vec![format!("{simple}.java"), format!("{simple}.kt")];
    if let Some(stem) = simple.strip_suffix("Kt") {
        names.push(format!("{stem}.kt"));
    }
    let mut pending = vec![workspace_root.to_path_buf()];
    let mut visited = 0usize;
    let mut kotlin_candidates = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 100_000 {
                return None;
            }
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !hyperion_layout::SKIPPED_DIRS.contains(&name.as_ref()) {
                    pending.push(path);
                }
            } else if names.iter().any(|candidate| candidate == name.as_ref()) {
                if let Some(found) = scan_matching(&path, main_class) {
                    return Some(found);
                }
            } else if name.ends_with(".kt") {
                kotlin_candidates.push(path);
            }
        }
    }
    // `@file:JvmName` facades do not follow the file name.
    kotlin_candidates
        .into_iter()
        .take(5_000)
        .find_map(|path| scan_matching(&path, main_class))
}

fn scan_matching(path: &Path, main_class: &str) -> Option<(PathBuf, ScannedFile)> {
    let text = std::fs::read_to_string(path).ok()?;
    let file = scan_file(path, &text)?;
    file.classes
        .iter()
        .any(|class| file.qualify(&class.binary_name) == main_class)
        .then(|| (path.to_path_buf(), file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, relative: &str, text: &str) -> PathBuf {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// Store with one published Gradle-style module classpath per entry.
    fn store_with(
        workspace: &Path,
        modules: &[(&Path, &str, Vec<PathBuf>, Vec<PathBuf>)],
    ) -> ProjectClasspathStore {
        let store = ProjectClasspathStore::new();
        let index = hyperion_workspace::WorkspaceIndex::new(workspace.to_path_buf());
        let workspace_ticket = index.begin_external_source_refresh("build:gradle");
        let ticket = store
            .begin_project_classpath_refresh(workspace, &workspace_ticket)
            .expect("begin refresh");
        for (module, set, outputs, jars) in modules {
            assert!(
                crate::project_classpath::record_project_classpath_scoped_for_refresh(
                    &ticket,
                    module,
                    set,
                    outputs.clone(),
                    outputs.clone(),
                    jars.clone(),
                    jars.clone(),
                )
            );
        }
        let _ = index
            .replace_external_sources_for_refresh(&workspace_ticket, &[])
            .expect("commit");
        assert!(
            crate::project_classpath::publish_gradle_dependency_snapshot_for_refresh(
                &ticket,
                Vec::new(),
                Vec::new()
            )
        );
        store
    }

    fn resolve(
        root: &Path,
        store: &ProjectClasspathStore,
        file: &Path,
        needle: Option<&str>,
        target: LaunchTarget,
    ) -> Option<Value> {
        let text = fs::read_to_string(file).unwrap();
        let position = needle.map(|needle| {
            let index = text.find(needle).expect("needle in fixture");
            let line = text[..index].matches('\n').count();
            let character = index - text[..index].rfind('\n').map_or(0, |n| n + 1);
            Position {
                line: line as u32,
                character: character as u32,
            }
        });
        resolve_launch(&LaunchRequest {
            path: file,
            text: &text,
            position,
            target,
            workspace_root: root,
            store,
        })
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    fn gradle_fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        write(&root, "settings.gradle", "include ':app', ':libs:core'\n");
        write(&root, "build.gradle", "");
        write(&root, "app/build.gradle", "plugins { id 'java' }");
        write(&root, "libs/core/build.gradle", "plugins { id 'java' }");
        write(&root, "gradle/wrapper/gradle-wrapper.jar", "");
        write(&root, "gradlew", "");
        (temp, root)
    }

    #[test]
    fn gradle_multi_module_main_uses_module_path_outputs_and_dependency_jars() {
        let (_temp, root) = gradle_fixture();
        let app = root.join("app");
        let core = root.join("libs/core");
        let main = write(
            &app,
            "src/main/java/com/acme/Main.java",
            "package com.acme;\npublic class Main {\n  public static void main(String[] args) {}\n}\n",
        );
        let jar = PathBuf::from("/cache/guava.jar");
        let store = store_with(
            &root,
            &[(
                &app,
                "main",
                vec![
                    app.join("build/classes/java/main"),
                    core.join("build/classes/java/main"),
                ],
                vec![jar.clone()],
            )],
        );

        let result = resolve(&root, &store, &main, Some("main"), LaunchTarget::Auto).unwrap();
        assert_eq!(result["kind"], "main");
        assert_eq!(result["language"], "java");
        assert_eq!(result["buildTool"], "gradle");
        assert_eq!(result["name"], "Main (app)");
        assert_eq!(result["projectRoot"], root.to_string_lossy().as_ref());
        assert_eq!(result["moduleDir"], app.to_string_lossy().as_ref());
        assert_eq!(result["launch"]["mainClass"], "com.acme.Main");
        assert_eq!(result["launch"]["cwd"], app.to_string_lossy().as_ref());
        let classpath = result["launch"]["classpath"].as_str().unwrap();
        let entries: Vec<&str> = classpath.split(path_separator()).collect();
        let expected = [
            app.join("build/classes/java/main"),
            app.join("build/classes/kotlin/main"),
            app.join("build/resources/main"),
            core.join("build/classes/java/main"),
            core.join("build/resources/main"),
            jar,
        ];
        assert_eq!(
            entries,
            expected
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            strings(&result["build"]["argv"]),
            [
                root.join("gradlew").to_string_lossy().as_ref(),
                ":app:classes",
                "--console=plain"
            ]
        );
        assert_eq!(result["build"]["cwd"], root.to_string_lossy().as_ref());
        assert_eq!(result["warnings"], json!([]));
        assert!(result.get("test").is_none());
    }

    #[test]
    fn nested_gradle_module_uses_its_settings_path_and_path_gradle_without_a_wrapper_jar() {
        let (_temp, root) = gradle_fixture();
        fs::remove_file(root.join("gradle/wrapper/gradle-wrapper.jar")).unwrap();
        let core = root.join("libs/core");
        let main = write(
            &core,
            "src/main/java/Tool.java",
            "public class Tool { public static void main(String[] a) {} }",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &main,
            None,
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(
            strings(&result["build"]["argv"]),
            ["gradle", ":libs:core:classes", "--console=plain"]
        );
        assert_eq!(result["launch"]["mainClass"], "Tool");
        // No model published yet: best effort plus a warning, never an error.
        let warnings = strings(&result["warnings"]);
        assert!(
            warnings.iter().any(|w| w.contains("not resolved")),
            "{warnings:?}"
        );
    }

    #[test]
    fn nested_classes_use_binary_names_and_instance_main_warns() {
        let (_temp, root) = gradle_fixture();
        let app = root.join("app");
        let file = write(
            &app,
            "src/main/java/p/Outer.java",
            "package p;\npublic class Outer {\n  static class Inner {\n    public static void main(String[] a) {}\n  }\n}\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &file,
            Some("static class"),
            LaunchTarget::Main,
        )
        .unwrap();
        assert_eq!(result["launch"]["mainClass"], "p.Outer$Inner");
        assert_eq!(result["name"], "Inner (app)");

        let instance = write(
            &app,
            "src/main/java/p/Modern.java",
            "package p;\nclass Modern {\n  void main() {}\n}\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &instance,
            None,
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(result["launch"]["mainClass"], "p.Modern");
        assert!(strings(&result["warnings"])
            .iter()
            .any(|w| w.contains("JDK 25")));
    }

    #[test]
    fn kotlin_top_level_main_resolves_to_the_file_facade_or_jvm_name() {
        let (_temp, root) = gradle_fixture();
        let app = root.join("app");
        let plain = write(
            &app,
            "src/main/kotlin/demo/Server.kt",
            "package demo\n\nfun main(args: Array<String>) {\n    println(args)\n}\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &plain,
            Some("println"),
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(result["language"], "kotlin");
        assert_eq!(result["launch"]["mainClass"], "demo.ServerKt");
        assert_eq!(result["name"], "Server (app)");

        let renamed = write(
            &app,
            "src/main/kotlin/demo/Boot.kt",
            "@file:JvmName(\"Launcher\")\npackage demo\n\nfun main() {}\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &renamed,
            None,
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(result["launch"]["mainClass"], "demo.Launcher");

        let companion = write(
            &app,
            "src/main/kotlin/demo/App.kt",
            "package demo\n\nclass App {\n    companion object {\n        @JvmStatic\n        fun main(args: Array<String>) {}\n    }\n}\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &companion,
            None,
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(result["launch"]["mainClass"], "demo.App");
    }

    #[test]
    fn junit_methods_parameterized_and_nested_classes_resolve_to_test_selectors() {
        let (_temp, root) = gradle_fixture();
        let app = root.join("app");
        let file = write(
            &app,
            "src/test/java/p/CalcTest.java",
            "package p;\nclass CalcTest {\n  @Test void adds() {}\n  @ParameterizedTest @ValueSource(ints = {1})\n  void squares(int x) {}\n  void helper() {}\n  @Nested class Deep {\n    @Test void nested() {}\n  }\n}\n",
        );
        let store = ProjectClasspathStore::new();
        let at = |needle: &str, target| resolve(&root, &store, &file, Some(needle), target);

        let method = at("adds", LaunchTarget::Auto).unwrap();
        assert_eq!(method["kind"], "test");
        assert_eq!(method["test"]["className"], "p.CalcTest");
        assert_eq!(method["test"]["methodName"], "adds");
        assert_eq!(method["test"]["gradleTask"], ":app:test");
        assert_eq!(
            strings(&method["test"]["argv"]),
            [
                root.join("gradlew").to_string_lossy().as_ref(),
                ":app:cleanTest",
                ":app:test",
                "--tests",
                "p.CalcTest.adds",
                "--console=plain"
            ]
        );
        assert!(strings(&method["test"]["debugArgv"]).contains(&"--debug-jvm".to_owned()));
        assert_eq!(
            method["test"]["reportsDir"],
            app.join("build/test-results/test")
                .to_string_lossy()
                .as_ref()
        );
        assert_eq!(method["build"], Value::Null);
        // Test classpath = test outputs first, then main outputs.
        assert!(method["launch"]["classpath"].as_str().unwrap().starts_with(
            &app.join("build/classes/java/test")
                .to_string_lossy()
                .into_owned()
        ));

        let parameterized = at("squares", LaunchTarget::Auto).unwrap();
        assert_eq!(parameterized["test"]["methodName"], "squares");

        // Inside the class but outside any test method: the whole class.
        let class = at("helper", LaunchTarget::Auto).unwrap();
        assert_eq!(class["test"]["className"], "p.CalcTest");
        assert_eq!(class["test"]["methodName"], Value::Null);

        let nested_method = at("nested()", LaunchTarget::Auto).unwrap();
        assert_eq!(nested_method["test"]["className"], "p.CalcTest$Deep");
        assert_eq!(nested_method["test"]["methodName"], "nested");
        let nested_class = at("class Deep", LaunchTarget::Auto).unwrap();
        assert_eq!(nested_class["test"]["className"], "p.CalcTest$Deep");
        assert_eq!(nested_class["test"]["methodName"], Value::Null);

        // `main` was requested but the file has no entry point.
        assert_eq!(at("adds", LaunchTarget::Main), None);
    }

    #[test]
    fn auto_prefers_a_test_under_the_cursor_over_the_files_main() {
        let (_temp, root) = gradle_fixture();
        let file = write(
            &root.join("app"),
            "src/test/java/Both.java",
            "public class Both {\n  public static void main(String[] a) {}\n  @Test void t() {}\n}\n",
        );
        let store = ProjectClasspathStore::new();
        let on_test = resolve(&root, &store, &file, Some("void t()"), LaunchTarget::Auto).unwrap();
        assert_eq!(on_test["kind"], "test");
        let on_main = resolve(&root, &store, &file, Some("void main"), LaunchTarget::Auto).unwrap();
        assert_eq!(on_main["kind"], "main");
        // A `main` in the test source set builds the test classes.
        assert_eq!(strings(&on_main["build"]["argv"])[1], ":app:testClasses");
        let forced = resolve(&root, &store, &file, Some("void main"), LaunchTarget::Test).unwrap();
        assert_eq!(forced["kind"], "test");
    }

    #[test]
    fn maven_module_builds_and_tests_through_the_reactor() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        write(&root, "pom.xml", "<project/>");
        write(&root, "svc/pom.xml", "<project/>");
        write(&root, ".mvn/wrapper/maven-wrapper.properties", "");
        write(&root, "mvnw", "");
        let svc = root.join("svc");
        let main = write(
            &svc,
            "src/main/java/x/App.java",
            "package x;\npublic class App { public static void main(String[] a) {} }\n",
        );
        let test = write(
            &svc,
            "src/test/java/x/AppTest.java",
            "package x;\nclass AppTest { @Test void works() {} }\n",
        );
        let store = ProjectClasspathStore::new();
        let run = resolve(&root, &store, &main, None, LaunchTarget::Auto).unwrap();
        assert_eq!(run["buildTool"], "maven");
        assert_eq!(
            strings(&run["build"]["argv"]),
            [
                root.join("mvnw").to_string_lossy().as_ref(),
                "-q",
                "-pl",
                "svc",
                "-am",
                "compile"
            ]
        );
        assert_eq!(run["build"]["cwd"], root.to_string_lossy().as_ref());
        assert!(run["launch"]["classpath"]
            .as_str()
            .unwrap()
            .starts_with(&svc.join("target/classes").to_string_lossy().into_owned()));

        let test = resolve(&root, &store, &test, Some("works"), LaunchTarget::Auto).unwrap();
        assert_eq!(test["test"]["gradleTask"], Value::Null);
        assert_eq!(
            strings(&test["test"]["argv"]),
            [
                root.join("mvnw").to_string_lossy().as_ref(),
                "-Dtest=x.AppTest#works",
                "-pl",
                "svc",
                "-am",
                "-Dsurefire.failIfNoSpecifiedTests=false",
                "-DfailIfNoTests=false",
                "test"
            ]
        );
        assert!(strings(&test["test"]["debugArgv"]).contains(&"-Dmaven.surefire.debug".to_owned()));
        assert_eq!(
            test["test"]["reportsDir"],
            svc.join("target/surefire-reports")
                .to_string_lossy()
                .as_ref()
        );
    }

    #[test]
    fn plain_java_compiles_with_javac_and_files_without_entry_points_are_null() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let main = write(
            &root,
            "src/app/Hello.java",
            "package app;\npublic class Hello { public static void main(String[] a) {} }\n",
        );
        let result = resolve(
            &root,
            &ProjectClasspathStore::new(),
            &main,
            None,
            LaunchTarget::Auto,
        )
        .unwrap();
        assert_eq!(result["buildTool"], "none");
        assert_eq!(result["launch"]["mainClass"], "app.Hello");
        let argv = strings(&result["build"]["argv"]);
        assert_eq!(argv[0], "javac");
        assert_eq!(argv.last().unwrap(), main.to_string_lossy().as_ref());
        assert!(argv.contains(&root.join("src").to_string_lossy().into_owned()));
        assert_eq!(
            result["launch"]["classpath"],
            root.join("out/production").to_string_lossy().as_ref()
        );

        let helper = write(&root, "src/app/Util.java", "package app;\nclass Util {}\n");
        assert_eq!(
            resolve(
                &root,
                &ProjectClasspathStore::new(),
                &helper,
                None,
                LaunchTarget::Auto
            ),
            None
        );
        // There is nothing that could run a test without a build tool.
        let test = write(
            &root,
            "src/app/HelloTest.java",
            "package app;\nclass HelloTest { @Test void t() {} }\n",
        );
        assert_eq!(
            resolve(
                &root,
                &ProjectClasspathStore::new(),
                &test,
                None,
                LaunchTarget::Auto
            ),
            None
        );
    }

    #[test]
    fn application_run_configurations_gain_classpath_cwd_and_project_root() {
        let (_temp, root) = gradle_fixture();
        let app = root.join("app");
        write(
            &app,
            "src/main/java/com/acme/Main.java",
            "package com.acme;\npublic class Main { public static void main(String[] a) {} }\n",
        );
        let jar = PathBuf::from("/cache/lib.jar");
        let store = store_with(
            &root,
            &[(
                &app,
                "main",
                vec![app.join("build/classes/java/main")],
                vec![jar],
            )],
        );
        let mut config = json!({
            "name": "Main", "type": "java", "request": "launch",
            "mainClass": "com.acme.Main", "jvmArgs": [], "args": [],
            "cwd": root.to_string_lossy(),
        });
        enrich_application_config(&mut config, &root, &store);
        assert!(config["classpath"]
            .as_str()
            .unwrap()
            .ends_with("/cache/lib.jar"));
        assert_eq!(config["projectRoot"], root.to_string_lossy().as_ref());
        assert_eq!(config["cwd"], root.to_string_lossy().as_ref());
        assert_eq!(config["moduleDir"], app.to_string_lossy().as_ref());

        // Unknown class: still returns a usable (workspace-root) description.
        let mut missing = json!({"request": "launch", "mainClass": "nope.Gone"});
        enrich_application_config(&mut missing, &root, &store);
        assert!(missing["classpath"].is_string());
        assert!(!missing["warnings"].as_array().unwrap().is_empty());
    }
}
