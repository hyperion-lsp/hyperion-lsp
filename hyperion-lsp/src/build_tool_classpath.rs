// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Opt-in classpaths straight from the build tool.
//!
//! By default Hyperion never runs the project's build: it reads the Gradle and
//! Maven caches, which are empty on a fresh clone. With
//! `hyperion.buildToolClasspath` enabled it instead asks the build tool for the
//! exact resolved classpaths (downloading what is missing):
//!
//! * Gradle: an init script (written to a temp file, never into the project)
//!   registers a task that prints every source set's compile and runtime
//!   classpath.
//! * Maven: `dependency:build-classpath` per module for compile and test scope.
//!
//! **Security.** Running Gradle or Maven executes the project's build scripts
//! and plugins: arbitrary code from the repository. The setting is therefore
//! `false` unless the *client* (initialization options, `workspace/
//! didChangeConfiguration`) or the user's environment enables it. A
//! `.hyperion.toml` inside the repository deliberately cannot.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tracing::{info, warn};

const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const GRADLE_TIMEOUT: Duration = Duration::from_mins(15);
const MAVEN_TIMEOUT: Duration = Duration::from_mins(15);

static ENABLED: AtomicBool = AtomicBool::new(false);
static FORCE_RERUN: AtomicBool = AtomicBool::new(false);

/// The last build-tool answer per workspace, valid while the build files are
/// unchanged. Refreshes triggered by unrelated events (the tool creating its
/// own `target/` directory, for one) reuse it instead of running the build again.
static LAST_RESULT: std::sync::Mutex<Option<CachedResult>> = std::sync::Mutex::new(None);

struct CachedResult {
    root: PathBuf,
    fingerprint: u64,
    result: ToolClasspaths,
}

/// Make the next resolution run the build tool even if no build file changed
/// (`hyperion.reloadProject`, or the setting being switched on).
pub fn request_rerun() {
    FORCE_RERUN.store(true, Ordering::Release);
}

/// Hash of every build-model input under `root` (names, sizes, mtimes).
fn build_files_fingerprint(root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        let Ok(read) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < 5 && !hyperion_layout::SKIPPED_DIRS.contains(&name.as_str()) {
                    pending.push((path, depth + 1));
                }
            } else if hyperion_layout::is_build_input(&path) {
                if let Ok(metadata) = entry.metadata() {
                    entries.push((
                        path,
                        metadata.len(),
                        metadata.modified().ok().map(|time| {
                            time.duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |elapsed| elapsed.as_nanos())
                        }),
                    ));
                }
            }
        }
    }
    entries.sort();
    for entry in &entries {
        entry.hash(&mut hasher);
    }
    hasher.finish()
}

fn cached_result(root: &Path, fingerprint: u64) -> Option<ToolClasspaths> {
    if FORCE_RERUN.swap(false, Ordering::AcqRel) {
        return None;
    }
    let cache = LAST_RESULT.lock().ok()?;
    let cached = cache.as_ref()?;
    (cached.root == root && cached.fingerprint == fingerprint).then(|| cached.result.clone())
}

fn store_result(root: &Path, fingerprint: u64, result: &ToolClasspaths) {
    if let Ok(mut cache) = LAST_RESULT.lock() {
        *cache = Some(CachedResult {
            root: root.to_path_buf(),
            fingerprint,
            result: result.clone(),
        });
    }
}

/// Whether the user has allowed Hyperion to run the project's build tool.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
        || std::env::var("HYPERION_BUILD_TOOL_CLASSPATH").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

/// Enable or disable running the build tool. Returns the previous value.
pub fn set_enabled(enabled: bool) -> bool {
    ENABLED.swap(enabled, Ordering::AcqRel)
}

/// Read the setting from `initializationOptions` / `didChangeConfiguration`
/// payloads: `{"hyperion": {"buildToolClasspath": true}}` or the flat
/// `{"buildToolClasspath": true}`.
pub fn setting_from_json(value: &serde_json::Value) -> Option<bool> {
    value
        .pointer("/hyperion/buildToolClasspath")
        .or_else(|| value.get("buildToolClasspath"))
        .and_then(serde_json::Value::as_bool)
}

/// Resolved classpaths of one source set of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSourceSet {
    pub compile: Vec<PathBuf>,
    pub runtime: Vec<PathBuf>,
}

/// What the build tool reported for one module (Gradle project / Maven module).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolModule {
    pub project_dir: PathBuf,
    pub main: ToolSourceSet,
    pub test: ToolSourceSet,
}

/// Everything one build-tool run produced.
#[derive(Debug, Clone, Default)]
pub struct ToolClasspaths {
    pub modules: Vec<ToolModule>,
    /// Per-configuration failures the tool itself reported.
    pub problems: Vec<String>,
}

impl ToolClasspaths {
    pub fn module_for(&self, project_dir: &Path) -> Option<&ToolModule> {
        let key = hyperion_workspace::path_lookup_key(project_dir);
        self.modules
            .iter()
            .find(|module| hyperion_workspace::path_lookup_key(&module.project_dir) == key)
    }
}

/// The Gradle init script: prints one `HYPERION` line per classpath.
const GRADLE_INIT_SCRIPT: &str = r#"
allprojects { proj ->
    proj.tasks.register("hyperionClasspaths") {
        notCompatibleWithConfigurationCache("prints resolved classpaths")
        doLast {
            def sourceSets = proj.extensions.findByName("sourceSets")
            if (sourceSets == null) return
            sourceSets.each { ss ->
                [compile: { ss.compileClasspath.files }, runtime: { ss.runtimeClasspath.files }].each { kind, files ->
                    try {
                        def line = ["HYPERION", kind, proj.projectDir.absolutePath, proj.path, ss.name]
                        files().each { line << it.absolutePath }
                        println(line.join("\t"))
                    } catch (Throwable t) {
                        println(["HYPERION_ERROR", proj.path, ss.name, kind, String.valueOf(t.message).replace("\n", " ")].join("\t"))
                    }
                }
            }
        }
    }
}
"#;

/// Parse the tab-separated lines printed by [`GRADLE_INIT_SCRIPT`].
pub fn parse_gradle_output(output: &str) -> ToolClasspaths {
    let mut modules: HashMap<PathBuf, ToolModule> = HashMap::new();
    let mut order = Vec::new();
    let mut problems = Vec::new();
    for line in output.lines() {
        let mut fields = line.split('\t');
        match fields.next() {
            Some("HYPERION") => {
                let (Some(kind), Some(project_dir), Some(_project_path), Some(source_set)) =
                    (fields.next(), fields.next(), fields.next(), fields.next())
                else {
                    continue;
                };
                let entries: Vec<PathBuf> = fields.map(PathBuf::from).collect();
                let project_dir = PathBuf::from(project_dir);
                if !modules.contains_key(&project_dir) {
                    order.push(project_dir.clone());
                }
                let module = modules
                    .entry(project_dir.clone())
                    .or_insert_with(|| ToolModule {
                        project_dir,
                        ..ToolModule::default()
                    });
                let target = match source_set {
                    "main" => &mut module.main,
                    "test" => &mut module.test,
                    _ => continue,
                };
                match kind {
                    "compile" => target.compile = entries,
                    "runtime" => target.runtime = entries,
                    _ => {}
                }
            }
            Some("HYPERION_ERROR") => {
                let details: Vec<&str> = fields.collect();
                problems.push(format!("Gradle could not resolve {}", details.join(" / ")));
            }
            _ => {}
        }
    }
    ToolClasspaths {
        modules: order
            .into_iter()
            .filter_map(|dir| modules.remove(&dir))
            .collect(),
        problems,
    }
}

/// Run Gradle (the project's wrapper when present) and collect the resolved
/// classpaths of every source set of every project.
pub async fn gradle_classpaths(root: &Path) -> Result<ToolClasspaths, String> {
    let fingerprint = build_files_fingerprint(root);
    if let Some(cached) = cached_result(root, fingerprint) {
        info!("Build files unchanged: reusing the last Gradle classpaths");
        return Ok(cached);
    }
    let script = tempfile::Builder::new()
        .prefix("hyperion-classpaths-")
        .suffix(".init.gradle")
        .tempfile()
        .map_err(|error| format!("cannot create the Gradle init script: {error}"))?;
    std::fs::write(script.path(), GRADLE_INIT_SCRIPT)
        .map_err(|error| format!("cannot write the Gradle init script: {error}"))?;

    let wrapper = root.join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    });
    let executable: PathBuf = if wrapper.is_file() {
        wrapper
    } else {
        PathBuf::from("gradle")
    };
    info!(
        "Running {} to resolve the Gradle classpaths (hyperion.buildToolClasspath is enabled)",
        executable.display()
    );
    let args = [
        "--console=plain".to_string(),
        "-q".to_string(),
        "--init-script".to_string(),
        script.path().display().to_string(),
        "hyperionClasspaths".to_string(),
    ];
    let output = run(&executable, &args, root, GRADLE_TIMEOUT).await?;
    let parsed = parse_gradle_output(&output);
    if parsed.modules.is_empty() {
        return Err("Gradle printed no source-set classpaths".to_string());
    }
    store_result(root, fingerprint, &parsed);
    Ok(parsed)
}

/// Run `dependency:build-classpath` for compile and test scope and read the
/// per-module result files.
pub async fn maven_classpaths(
    root: &Path,
    module_dirs: &[PathBuf],
) -> Result<ToolClasspaths, String> {
    let fingerprint = build_files_fingerprint(root);
    if let Some(cached) = cached_result(root, fingerprint) {
        info!("Build files unchanged: reusing the last Maven classpaths");
        return Ok(cached);
    }
    let wrapper = root.join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" });
    let executable: PathBuf = if wrapper.is_file() {
        wrapper
    } else {
        PathBuf::from("mvn")
    };
    info!(
        "Running {} to resolve the Maven classpaths (hyperion.buildToolClasspath is enabled)",
        executable.display()
    );
    let mut problems = Vec::new();
    for scope in ["compile", "test"] {
        let args = [
            "-q".to_string(),
            "-B".to_string(),
            "dependency:build-classpath".to_string(),
            format!("-Dmdep.includeScope={scope}"),
            format!("-Dmdep.outputFile=target/hyperion-classpath-{scope}.txt"),
        ];
        if let Err(error) = run(&executable, &args, root, MAVEN_TIMEOUT).await {
            problems.push(format!("Maven {scope}-scope classpath: {error}"));
        }
    }
    let mut modules = Vec::new();
    for dir in module_dirs {
        let read = |scope: &str| -> Vec<PathBuf> {
            match std::fs::read_to_string(
                dir.join(format!("target/hyperion-classpath-{scope}.txt")),
            ) {
                Ok(text) => parse_maven_classpath(&text),
                // Not written yet: the build tool has not run for this scope.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => {
                    tracing::warn!("cannot read the Maven classpath for {scope}: {error}");
                    Vec::new()
                }
            }
        };
        let compile = read("compile");
        let test = read("test");
        if compile.is_empty() && test.is_empty() {
            continue;
        }
        modules.push(ToolModule {
            project_dir: dir.clone(),
            main: ToolSourceSet {
                runtime: compile.clone(),
                compile,
            },
            test: ToolSourceSet {
                runtime: test.clone(),
                compile: test,
            },
        });
    }
    if modules.is_empty() {
        return Err(format!(
            "Maven produced no classpath files{}",
            if problems.is_empty() {
                String::new()
            } else {
                format!(": {}", problems.join("; "))
            }
        ));
    }
    let result = ToolClasspaths { modules, problems };
    store_result(root, fingerprint, &result);
    Ok(result)
}

/// `dependency:build-classpath` writes one line of entries separated by the
/// platform path separator.
pub fn parse_maven_classpath(text: &str) -> Vec<PathBuf> {
    text.trim()
        .split(if cfg!(windows) { ';' } else { ':' })
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Keep only JAR archives from a resolved classpath (directories are the
/// project's own outputs, which the static model already knows).
pub fn jar_entries(entries: &[PathBuf]) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        })
        .cloned()
        .collect()
}

async fn run(
    executable: &Path,
    args: &[String],
    directory: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", executable.display()))?;
    let stdout = child.stdout.take().ok_or("stdout pipe unavailable")?;
    let stderr = child.stderr.take().ok_or("stderr pipe unavailable")?;
    let stdout_task = tokio::spawn(read_bounded(stdout));
    let stderr_task = tokio::spawn(read_bounded(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status.map_err(|error| error.to_string())?,
        Err(_) => {
            let _ = child.kill().await;
            stdout_task.abort();
            stderr_task.abort();
            return Err(format!(
                "{} did not finish within {} seconds",
                executable.display(),
                timeout.as_secs()
            ));
        }
    };
    let stdout = stdout_task.await.map_err(|error| error.to_string())?;
    let stderr = stderr_task.await.map_err(|error| error.to_string())?;
    if !status.success() {
        let tail: String = stderr
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        warn!("{} failed: {tail}", executable.display());
        return Err(format!(
            "{} exited with {status}: {tail}",
            executable.display()
        ));
    }
    Ok(stdout)
}

async fn read_bounded(mut reader: impl AsyncRead + Unpin) -> String {
    let mut bytes = Vec::new();
    let mut limited = (&mut reader).take(MAX_OUTPUT_BYTES as u64);
    let _ = limited.read_to_end(&mut bytes).await;
    // Keep draining so the child never blocks on a full pipe.
    let mut sink = [0_u8; 8192];
    while matches!(reader.read(&mut sink).await, Ok(read) if read > 0) {}
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_is_read_from_client_options_only() {
        assert_eq!(
            setting_from_json(&serde_json::json!({"hyperion": {"buildToolClasspath": true}})),
            Some(true)
        );
        assert_eq!(
            setting_from_json(&serde_json::json!({"buildToolClasspath": false})),
            Some(false)
        );
        assert_eq!(setting_from_json(&serde_json::json!({"other": 1})), None);
    }

    #[test]
    fn gradle_output_is_grouped_per_module_and_source_set() {
        let output = "\
HYPERION\tcompile\t/w/app\t:app\tmain\t/w/core/build/classes\t/c/guava.jar
HYPERION\truntime\t/w/app\t:app\tmain\t/c/guava.jar\t/c/jackson.jar
HYPERION\tcompile\t/w/app\t:app\ttest\t/c/junit.jar
HYPERION\tcompile\t/w/app\t:app\tintegrationTest\t/c/ignored.jar
HYPERION_ERROR\t:core\tmain\truntime\tCould not resolve x:y:1
noise printed by a plugin
";
        let parsed = parse_gradle_output(output);

        assert_eq!(parsed.modules.len(), 1);
        let app = &parsed.modules[0];
        assert_eq!(app.project_dir, PathBuf::from("/w/app"));
        assert_eq!(
            jar_entries(&app.main.compile),
            vec![PathBuf::from("/c/guava.jar")]
        );
        assert_eq!(
            jar_entries(&app.main.runtime),
            vec![
                PathBuf::from("/c/guava.jar"),
                PathBuf::from("/c/jackson.jar")
            ]
        );
        assert_eq!(
            jar_entries(&app.test.compile),
            vec![PathBuf::from("/c/junit.jar")]
        );
        assert!(parsed.problems[0].contains("Could not resolve x:y:1"));
        assert!(parsed.module_for(Path::new("/w/app")).is_some());
    }

    #[test]
    fn maven_classpath_files_split_on_the_path_separator() {
        let text = if cfg!(windows) {
            "C:\\a.jar;C:\\b.jar\n"
        } else {
            "/a.jar:/b.jar\n"
        };
        assert_eq!(parse_maven_classpath(text).len(), 2);
        assert!(parse_maven_classpath("").is_empty());
    }

    #[test]
    fn fingerprint_tracks_build_files_and_ignores_build_output() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("pom.xml"), "<project/>").unwrap();
        let before = build_files_fingerprint(temp.path());

        // The tool's own output directory must not look like a change.
        std::fs::create_dir_all(temp.path().join("target")).unwrap();
        std::fs::write(temp.path().join("target/pom.xml"), "generated").unwrap();
        std::fs::write(temp.path().join("notes.txt"), "unrelated").unwrap();
        assert_eq!(build_files_fingerprint(temp.path()), before);

        std::fs::write(
            temp.path().join("pom.xml"),
            "<project><name>x</name></project>",
        )
        .unwrap();
        assert_ne!(build_files_fingerprint(temp.path()), before);

        std::fs::create_dir_all(temp.path().join("module")).unwrap();
        let with_pom = build_files_fingerprint(temp.path());
        std::fs::write(temp.path().join("module/build.gradle.kts"), "").unwrap();
        assert_ne!(build_files_fingerprint(temp.path()), with_pom);
    }

    #[test]
    fn fingerprint_sees_applied_scripts() {
        // `gradle/deps.gradle` is a model input for the file
        // watcher (`is_build_input`); a fingerprint that ignored it served a
        // stale classpath after such an edit.
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("gradle/deps.gradle");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "a").unwrap();
        assert!(hyperion_layout::is_build_input(&script));
        let before = build_files_fingerprint(temp.path());
        std::fs::write(&script, "changed contents").unwrap();
        assert_ne!(build_files_fingerprint(temp.path()), before);
    }

    #[test]
    fn the_flag_defaults_to_disabled() {
        // Never touch the process-wide flag here: other tests run in parallel.
        assert!(!ENABLED.load(Ordering::Acquire));
    }
}
