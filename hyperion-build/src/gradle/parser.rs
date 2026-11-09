// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle build file parser for extracting dependencies and project information.
//!
//! This module provides parsers for both Groovy DSL (build.gradle) and Kotlin DSL
//! (build.gradle.kts) build files. The parsing is primarily regex-based, which
//! handles the majority of common Gradle patterns without requiring a full
//! Groovy or Kotlin parser.

use super::model::{
    DependencyConfiguration, GradleDependency, GradlePlugin, GradleProject, GradleRepository,
    SourceSet, VersionCatalog,
};
use super::sniff_cache;
use super::sniff_report::{
    GradleDslKind, GradlePluginResolution, GradleSniffReport, SniffOutcome, SniffSkipReason,
    VmExecutionSummary,
};
use super::syntax::{find_named_block, strip_comments as strip_gradle_comments};
use crate::io::{read_utf8_file_bounded, MAX_BUILD_FILE_BYTES};
use hyperion_bytecode::gradle::SecurityPolicy;
use hyperion_bytecode::gradle_executor::GradleExecutor;
use hyperion_bytecode::gradle_plugin_executor::{GradlePluginExecutionError, GradlePluginExecutor};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;
use tracing::{debug, info, warn};

// Configuration names shared across multiple regex patterns
const GRADLE_CONFIGS: &str = r"implementation|api|compileOnly|runtimeOnly|annotationProcessor|testImplementation|testCompileOnly|testRuntimeOnly|developmentOnly";

// ---- Project properties ----
static GROUP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"group\s*=\s*['"]([^'"]+)['"]"#).unwrap());
static VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"version\s*=\s*['"]([^'"]+)['"]"#).unwrap());

// ---- Plugins ----
static PLUGIN_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"id\s*(?:\(\s*)?['"]([^'"]+)['"]\s*\)?"#).unwrap());
static PLUGIN_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"id\s*(?:\(\s*)?['"]([^'"]+)['"]\s*\)?\s+version\s*(?:\(\s*)?['"]([^'"]+)['"]\s*\)?"#,
    )
    .unwrap()
});
static KOTLIN_PLUGIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"kotlin\s*\(\s*['"]([^'"]+)['"]\s*\)(?:\s+version\s*(?:\(\s*)?['"]([^'"]+)['"]\s*\)?)?"#,
    )
    .unwrap()
});
static APPLY_PLUGIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"apply\s+plugin:\s*['"]([^'"]+)['"]"#).unwrap());
static APPLY_PLUGIN_CONSTRUCT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bapply\s*(?:plugin\s*:|\(\s*plugin\s*=)"#).unwrap());
static STATIC_KNOWN_PLUGIN_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?:\s+version\s*(?:\(\s*)?['"][^'"$]+['"]\s*\)?)?(?:\s*apply\s+(?:true|false))?\s*$"#,
    )
    .unwrap()
});
static STATIC_CORE_PLUGIN_SUFFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(?:\s+apply\s+(?:true|false))?\s*$"#).unwrap());

// ---- Repositories ----
static MAVEN_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"maven\s*\{[^}]*url\s*=\s*uri\(['"]([^'"]+)['"]\)"#).unwrap());

// ---- Groovy dependencies ----
static STRING_DEP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r#"({})\s+['"]([^'"]+)['"]"#, GRADLE_CONFIGS)).unwrap());
static STRING_DEP_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s*\(\s*['"]([^'"]+)['"]\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static PLATFORM_DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s+platform\s*\(\s*['"]([^'"]+)['"]\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
/// `platform(SpringBootPlugin.BOM_COORDINATES)`: the Spring Boot Gradle plugin's
/// constant for `org.springframework.boot:spring-boot-dependencies:<plugin version>`.
/// The version is not written in the build: it is the plugin's, filled in by the
/// resolver when it builds the platform map.
static SPRING_BOOT_BOM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s*\(?\s*(?:enforced)?[Pp]latform\s*\(\s*(?:[A-Za-z_][\w.]*\.)?SpringBootPlugin\.BOM_COORDINATES\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static CATALOG_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s*\(\s*libs\.([a-zA-Z0-9._-]+)\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static CATALOG_SPACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s+libs\.([a-zA-Z0-9._-]+)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});

// ---- Kotlin dependencies ----
static KOTLIN_DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"({})\s*\(['"]([^'"]+)['"]\)"#, GRADLE_CONFIGS)).unwrap()
});
static KOTLIN_PLATFORM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s*\(\s*(?:enforced)?[Pp]latform\s*\(\s*['"]([^'"]+)['"]\s*\)\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static KOTLIN_PROJECT_DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s*\(\s*project\s*\(\s*['"]([^'"]+)['"]\s*\)\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static GROOVY_PROJECT_DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"({})\s+project\s*\(\s*['"]([^'"]+)['"]\s*\)"#,
        GRADLE_CONFIGS
    ))
    .unwrap()
});
static STATIC_DEPENDENCY_ARGUMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^(?:['"][^'"$\r\n]+['"]|\(\s*['"][^'"$\r\n]+['"]\s*\)|(?:enforced)?[Pp]latform\s*\(\s*['"][^'"$\r\n]+['"]\s*\)|\(\s*(?:enforced)?[Pp]latform\s*\(\s*['"][^'"$\r\n]+['"]\s*\)\s*\)|project\s*\(\s*['"][^'"$\r\n]+['"]\s*\)|\(\s*project\s*\(\s*['"][^'"$\r\n]+['"]\s*\)\s*\)|libs\.[A-Za-z0-9._-]+|\(\s*libs\.[A-Za-z0-9._-]+\s*\))\s*$"#,
    )
    .expect("valid static Gradle dependency argument regex")
});

/// Platform declarations that name the Spring Boot BOM through the plugin's
/// `BOM_COORDINATES` constant (versionless; see `SPRING_BOOT_BOM_RE`).
fn spring_boot_bom_platforms(content: &str, project: &mut GradleProject) {
    for cap in SPRING_BOOT_BOM_RE.captures_iter(content) {
        let config = DependencyConfiguration::from_config_str(&cap[1]);
        if let Some(dep) = GradleDependency::from_coordinates_with_platform(
            "org.springframework.boot:spring-boot-dependencies",
            config,
            true,
        ) {
            if !project
                .dependencies
                .iter()
                .any(|d| d.same_declaration(&dep))
            {
                project.dependencies.push(dep);
            }
        }
    }
}

// ---- Settings file ----
static ROOT_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"rootProject\.name\s*=\s*['"]([^'"]+)['"]"#).unwrap());
static PROJECT_DIR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"project\(\s*["']?:(?P<name>[^"']+)["']?\s*\)\s*\.projectDir\s*=\s*(?:file|File)\(\s*['"](?P<path>[^'"]+)['"]\s*\)"#,
    ).unwrap()
});
static INCLUDE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"include\s*\(?\s*((?:['"][^'"]+['"]\s*,?\s*)+)\)?"#).unwrap());
static MODULE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"]([^'"]+)['"]"#).unwrap());
static INCLUDE_BUILD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"includeBuild\s*\(?\s*['"]([^'"]+)['"]\s*\)?"#).unwrap());
static SETTINGS_MODEL_CONSTRUCT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:include|includeBuild|includeFlat)\b|\.projectDir\b"#).unwrap()
});

// ---- Applied Groovy dependency catalogs ----
static APPLY_FROM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(?:\bapply\s+from\s*:\s*(?:file\s*\(\s*)?['\"]([^'\"]+)['\"]\s*\)?|\bapply\s*\(\s*from\s*=\s*['\"]([^'\"]+)['\"]\s*\))"#,
    )
    .unwrap()
});
static APPLY_FROM_DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:\bapply\s+from\s*:\s*([^\r\n;]+)|\bapply\s*\(\s*from\s*=\s*([^\r\n;]+))"#)
        .unwrap()
});
static LEGACY_LITERAL_VAR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:def|String)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*['\"]([^'\"]+)['\"]"#)
        .unwrap()
});
static LEGACY_MAP_ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^\s*(?:['\"]([^'\"]+)['\"]|([A-Za-z_][A-Za-z0-9_.-]*))\s*:\s*['\"]([^'\"]+)['\"]\s*,?"#,
    )
    .unwrap()
});
static LEGACY_VERSION_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)\bversions(?:\[\s*['\"]([^'\"]+)['\"]\s*\]|\.([A-Za-z_][A-Za-z0-9_]*))\s*=\s*(?:['\"]([^'\"]+)['\"]|([A-Za-z_][A-Za-z0-9_]*))"#,
    )
    .unwrap()
});
static LEGACY_INTERPOLATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\$\{?((?:versions\.)?[A-Za-z_][A-Za-z0-9_.]*)\}?"#).unwrap());

const MAX_APPLIED_CATALOG_FILES: usize = 16;
const MAX_APPLIED_CATALOG_BYTES: u64 = 2 * 1024 * 1024;

// ---- BuildSrc ----
static THRIFT_OUTPUT_DIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\boutputDir(?:\s*=\s*|\s*\(\s*|\s+)['"]([^'"]+)['"]"#).unwrap());

fn exec_error_summary(err: &GradlePluginExecutionError) -> String {
    match err {
        GradlePluginExecutionError::SandboxViolation(msg) => format!("sandbox_violation: {}", msg),
        GradlePluginExecutionError::ClassNotFound(class) => format!("class_not_found: {}", class),
        GradlePluginExecutionError::ApplyMethodMissing(class) => {
            format!("apply_method_missing: {}", class)
        }
        GradlePluginExecutionError::Vm(vm) => format!("vm_error: {}", vm),
        GradlePluginExecutionError::Io(e) => format!("io_error: {}", e),
        GradlePluginExecutionError::Zip(e) => format!("zip_error: {}", e),
        GradlePluginExecutionError::Execution(msg) => format!("execution_error: {}", msg),
    }
}

/// Errors that can occur during Gradle parsing
#[derive(Debug, thiserror::Error)]
pub enum GradleParserError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid build file: {0}")]
    InvalidBuildFile(String),

    #[error("Build file not found: {0}")]
    BuildFileNotFound(PathBuf),

    #[error("Parse error: {0}")]
    ParseError(String),
}

pub type Result<T> = std::result::Result<T, GradleParserError>;

#[derive(Clone, Debug)]
enum GroovyValue {
    Str(String),
    List(Vec<GroovyValue>),
    Map(BTreeMap<String, GroovyValue>),
}

/// Gradle build file parser
pub struct GradleParser;

impl GradleParser {
    /// Parse a Gradle project from a directory
    ///
    /// This method detects whether the project uses Groovy or Kotlin DSL and
    /// parses the appropriate build file.
    pub async fn parse_project(project_dir: &Path) -> Result<GradleProject> {
        let (project, _report) = Self::parse_project_with_report(project_dir).await?;
        Ok(project)
    }

    /// Parse a Gradle project and return a structured sniff report.
    ///
    /// Emits exactly one INFO summary line per call (root parse), with full details at DEBUG.
    pub async fn parse_project_with_report(
        project_dir: &Path,
    ) -> Result<(GradleProject, GradleSniffReport)> {
        // Seed the guard with the root so a subproject that includes the root
        // back is cut at the first back-edge rather than one level later.
        let mut guard = ProjectParseGuard::rooted_at(project_dir);
        Self::parse_project_internal(project_dir, true, None, &mut guard)
            .await
            .map(|(project, report)| (project, report.expect("report")))
    }

    /// The Gradle projects a build root's settings file declares, as
    /// `(project path such as ":libs:core", directory)`, from `include` and
    /// `project(':x').projectDir = file(...)` statements. Nothing is executed
    /// and no build file is read; a root without a settings file yields none.
    pub fn settings_module_dirs(build_root: &Path) -> Vec<(String, PathBuf)> {
        let Some(settings) = ["settings.gradle", "settings.gradle.kts"]
            .iter()
            .map(|name| build_root.join(name))
            .find(|path| path.is_file())
        else {
            return Vec::new();
        };
        let mut project = GradleProject::new(String::new(), build_root.to_path_buf());
        if Self::parse_settings_file(&settings, &mut project, None).is_err() {
            return Vec::new();
        }
        let mut modules: Vec<_> = project
            .subproject_dirs
            .into_iter()
            .filter(|(name, _)| !name.starts_with("@build:"))
            .map(|(name, dir)| (format!(":{name}"), dir))
            .collect();
        modules.sort();
        modules
    }

    /// Find dependency declarations which the static Gradle model cannot prove
    /// it accounted for.
    ///
    /// The parser intentionally supports a bounded set of literal, project, and
    /// version-catalog forms. Returning an empty list is therefore a proof about
    /// syntax coverage, not about Gradle execution. Callers may use partial
    /// models for navigation, but destructive operations must fail closed when
    /// this list is non-empty.
    pub fn find_unaccounted_dependency_syntax(project: &GradleProject) -> Result<Vec<String>> {
        let mut project_dirs: Vec<_> = std::iter::once(project.root_dir.clone())
            .chain(project.subproject_dirs.values().cloned())
            .collect();
        project_dirs.sort_unstable();
        project_dirs.dedup();

        let mut unaccounted = Vec::new();
        for project_dir in project_dirs {
            let build_file = [
                project_dir.join("build.gradle"),
                project_dir.join("build.gradle.kts"),
            ]
            .into_iter()
            .find(|path| path.is_file());
            let Some(build_file) = build_file else {
                continue;
            };
            Self::audit_dependency_script_graph(&build_file, &project.root_dir, &mut unaccounted)?;
        }
        if let Some(settings_file) = [
            project.root_dir.join("settings.gradle"),
            project.root_dir.join("settings.gradle.kts"),
        ]
        .into_iter()
        .find(|path| path.is_file())
        {
            Self::audit_settings_model_syntax(&settings_file, &project.root_dir, &mut unaccounted)?;
        }
        Ok(unaccounted)
    }

    /// Prove that every settings construct which can add or relocate a project
    /// was represented by the static model. Regex extraction is intentionally
    /// useful for common literal forms, but a partial match (for example
    /// `include("fixed", computedName)`) must not be mistaken for completeness.
    fn audit_settings_model_syntax(
        settings_file: &Path,
        project_root: &Path,
        unaccounted: &mut Vec<String>,
    ) -> Result<()> {
        let content = read_utf8_file_bounded(settings_file, MAX_BUILD_FILE_BYTES)?;
        let content = strip_gradle_comments(&content);
        let has_applied_settings_script = APPLY_FROM_DIRECTIVE_RE.is_match(&content);
        let mut modeled = Vec::new();

        for capture in INCLUDE_BLOCK_RE.captures_iter(&content) {
            let Some(whole) = capture.get(0) else {
                continue;
            };
            let module_arguments = capture.get(1).map_or("", |value| value.as_str());
            if !module_arguments.contains('$')
                && static_settings_match_is_complete(
                    &content,
                    whole.start(),
                    whole.end(),
                    "include",
                )
            {
                modeled.push(whole.start()..whole.end());
            }
        }
        for capture in INCLUDE_BUILD_RE.captures_iter(&content) {
            let (Some(whole), Some(path)) = (capture.get(0), capture.get(1)) else {
                continue;
            };
            let resolved = resolve_gradle_path_variables(path.as_str(), project_root);
            if !resolved.as_os_str().as_encoded_bytes().contains(&b'$')
                && static_settings_match_is_complete(
                    &content,
                    whole.start(),
                    whole.end(),
                    "includeBuild",
                )
            {
                modeled.push(whole.start()..whole.end());
            }
        }
        for capture in PROJECT_DIR_RE.captures_iter(&content) {
            let (Some(whole), Some(name), Some(path)) =
                (capture.get(0), capture.name("name"), capture.name("path"))
            else {
                continue;
            };
            let resolved = resolve_gradle_path_variables(path.as_str(), project_root);
            if !name.as_str().contains('$')
                && !resolved.as_os_str().as_encoded_bytes().contains(&b'$')
            {
                modeled.push(whole.start()..whole.end());
            }
        }

        let mut residual = content.into_bytes();
        for range in modeled {
            residual[range].fill(b' ');
        }
        mask_quoted_gradle_text(&mut residual);
        let residual = String::from_utf8(residual)
            .expect("masking an existing UTF-8 Gradle script preserves UTF-8");
        for construct in SETTINGS_MODEL_CONSTRUCT_RE.find_iter(&residual) {
            let line = residual[..construct.start()]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            unaccounted.push(format!(
                "{}:{line}: unmodeled settings project-graph construct: {}",
                settings_file.display(),
                construct.as_str()
            ));
        }
        if has_applied_settings_script {
            unaccounted.push(format!(
                "{}: applied settings scripts are not modeled",
                settings_file.display()
            ));
        }
        Ok(())
    }

    fn audit_dependency_script_graph(
        build_file: &Path,
        workspace_root: &Path,
        unaccounted: &mut Vec<String>,
    ) -> Result<()> {
        let workspace_root =
            fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let project_root = build_file.parent().unwrap_or(workspace_root.as_path());
        let mut pending = vec![(build_file.to_path_buf(), false)];
        let mut visited = HashSet::new();

        while let Some((script, applied)) = pending.pop() {
            if visited.contains(&script) {
                continue;
            }
            if visited.len() >= MAX_APPLIED_CATALOG_FILES {
                unaccounted.push(format!(
                    "{}: applied-script graph exceeds the {}-file audit bound",
                    build_file.display(),
                    MAX_APPLIED_CATALOG_FILES
                ));
                break;
            }
            visited.insert(script.clone());
            let maximum = if applied {
                MAX_APPLIED_CATALOG_BYTES as usize
            } else {
                MAX_BUILD_FILE_BYTES
            };
            let content = match read_utf8_file_bounded(&script, maximum) {
                Ok(content) => content,
                Err(error) => {
                    unaccounted.push(format!(
                        "{}: applied script could not be audited: {}",
                        script.display(),
                        error
                    ));
                    continue;
                }
            };
            Self::audit_dependency_syntax_in_content(&script, &content, applied, unaccounted);
            Self::audit_plugin_syntax_in_content(&script, &content, unaccounted);

            for directive in APPLY_FROM_DIRECTIVE_RE.captures_iter(&content) {
                let Some(full_directive) = directive.get(0) else {
                    continue;
                };
                let Some(reference) = APPLY_FROM_RE
                    .captures(full_directive.as_str())
                    .and_then(|capture| capture.get(1).or_else(|| capture.get(2)))
                else {
                    unaccounted.push(format!(
                        "{}:{}: dynamic applied-script path: {}",
                        script.display(),
                        content[..full_directive.start()].lines().count() + 1,
                        full_directive.as_str().trim()
                    ));
                    continue;
                };
                let Some(candidate) =
                    Self::resolve_applied_script_path(reference.as_str(), project_root)
                else {
                    unaccounted.push(format!(
                        "{}: dynamic applied-script path: {}",
                        script.display(),
                        reference.as_str()
                    ));
                    continue;
                };
                let canonical = match fs::canonicalize(&candidate) {
                    Ok(canonical) => canonical,
                    Err(error) => {
                        unaccounted.push(format!(
                            "{}: applied script {} is unavailable: {}",
                            script.display(),
                            candidate.display(),
                            error
                        ));
                        continue;
                    }
                };
                if !canonical.starts_with(&workspace_root) {
                    unaccounted.push(format!(
                        "{}: applied script is outside the audited workspace: {}",
                        script.display(),
                        canonical.display()
                    ));
                    continue;
                }
                pending.push((canonical, true));
            }
        }
        Ok(())
    }

    fn audit_dependency_syntax_in_content(
        script: &Path,
        content: &str,
        applied: bool,
        unaccounted: &mut Vec<String>,
    ) {
        let content = strip_gradle_comments(content);
        Self::audit_unknown_dependency_block_statements(script, &content, unaccounted);
        for (line_index, line) in content.lines().enumerate() {
            for statement in super::syntax::split_statements(line).map(str::trim) {
                if statement.is_empty() {
                    continue;
                }
                if statement.starts_with("add(") || statement.starts_with("add (") {
                    unaccounted.push(format!(
                        "{}:{}: {}",
                        script.display(),
                        line_index + 1,
                        statement
                    ));
                    continue;
                }

                let name_end = statement
                    .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .unwrap_or(statement.len());
                let name = &statement[..name_end];
                if !is_dependency_configuration_name(name) {
                    continue;
                }
                let argument = statement[name_end..].trim_start();
                if applied || !STATIC_DEPENDENCY_ARGUMENT_RE.is_match(argument) {
                    unaccounted.push(format!(
                        "{}:{}: {}",
                        script.display(),
                        line_index + 1,
                        statement
                    ));
                }
            }
        }
    }

    fn audit_plugin_syntax_in_content(script: &Path, content: &str, unaccounted: &mut Vec<String>) {
        let content = strip_gradle_comments(content);
        let mut remainder = content.as_str();
        while let Some(block) = find_named_block(remainder, "plugins") {
            let block_offset = block.as_ptr() as usize - content.as_ptr() as usize;
            let first_line = content[..block_offset].lines().count();
            for (relative_line, line) in block.lines().enumerate() {
                for statement in line.split(';').map(str::trim) {
                    if !statement.is_empty() && !is_modeled_core_plugin_statement(statement) {
                        unaccounted.push(format!(
                            "{}:{}: unmodeled plugin declaration: {}",
                            script.display(),
                            first_line + relative_line + 1,
                            statement
                        ));
                    }
                }
            }
            let next = block_offset + block.len();
            if next >= content.len() {
                break;
            }
            remainder = &content[next..];
        }

        for construct in APPLY_PLUGIN_CONSTRUCT_RE.find_iter(&content) {
            let line_start = content[..construct.start()]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            let line_end = content[construct.end()..]
                .find(['\n', ';'])
                .map_or(content.len(), |end| construct.end() + end);
            let statement = content[line_start..line_end].trim();
            let modeled = APPLY_PLUGIN_RE.captures(statement).is_some_and(|capture| {
                let Some(whole) = capture.get(0) else {
                    return false;
                };
                (is_modeled_core_plugin(&capture[1])
                    || is_modeled_dependency_neutral_plugin(&capture[1]))
                    && statement[whole.end()..].trim().is_empty()
            });
            if !modeled {
                let line = content[..construct.start()].lines().count() + 1;
                unaccounted.push(format!(
                    "{}:{line}: unmodeled plugin application: {statement}",
                    script.display()
                ));
            }
        }
    }

    fn audit_unknown_dependency_block_statements(
        script: &Path,
        content: &str,
        unaccounted: &mut Vec<String>,
    ) {
        let mut remainder = content;
        while let Some(block) = find_named_block(remainder, "dependencies") {
            let block_offset = block.as_ptr() as usize - content.as_ptr() as usize;
            let first_line = content[..block_offset].lines().count();
            for (relative_line, line) in block.lines().enumerate() {
                for statement in super::syntax::split_statements(line).map(str::trim) {
                    let name_end = statement
                        .find(|character: char| {
                            !character.is_ascii_alphanumeric() && character != '_'
                        })
                        .unwrap_or(statement.len());
                    let name = &statement[..name_end];
                    if name.is_empty() || is_dependency_configuration_name(name) {
                        continue;
                    }
                    // Inside a dependencies block, an arbitrary plugin-defined
                    // configuration call is an edge declaration. Other unknown
                    // statements (constraints, variables, helper calls) are
                    // equally unmodeled, so all must fail completeness closed.
                    unaccounted.push(format!(
                        "{}:{}: unmodeled dependencies-block statement: {}",
                        script.display(),
                        first_line + relative_line + 1,
                        statement
                    ));
                }
            }
            let next = block_offset + block.len();
            if next >= content.len() {
                break;
            }
            remainder = &content[next..];
        }
    }

    async fn parse_project_nested_with_catalog(
        project_dir: &Path,
        parent_catalog: Option<&VersionCatalog>,
        guard: &mut ProjectParseGuard,
    ) -> Result<GradleProject> {
        let (project, _report) =
            Self::parse_project_internal(project_dir, false, parent_catalog, guard).await?;
        Ok(project)
    }

    async fn parse_project_internal(
        project_dir: &Path,
        emit_report_log: bool,
        parent_catalog: Option<&VersionCatalog>,
        guard: &mut ProjectParseGuard,
    ) -> Result<(GradleProject, Option<GradleSniffReport>)> {
        debug!("Parsing Gradle project at: {:?}", project_dir);

        let project_name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut project = GradleProject::new(project_name.clone(), project_dir.to_path_buf());
        let mut report = GradleSniffReport::new(project_dir.to_path_buf());

        // Check for Gradle wrapper
        project.has_wrapper =
            project_dir.join("gradlew").exists() || project_dir.join("gradlew.bat").exists();

        // Parse version catalog if it exists (own catalog takes precedence over parent)
        let version_catalog_path = project_dir.join("gradle/libs.versions.toml");
        if version_catalog_path.exists() {
            debug!("Parsing version catalog: {:?}", version_catalog_path);
            report.set_version_catalog_path(version_catalog_path.clone());
            project.version_catalog = Some(Self::parse_version_catalog(&version_catalog_path)?);
            let catalog = project.version_catalog.as_ref().unwrap();
            debug!(
                "Loaded version catalog with {} libraries and {} bundles",
                catalog.libraries.len(),
                catalog.bundles.len()
            );
        } else if let Some(catalog) = parent_catalog {
            debug!("Inheriting version catalog from parent project");
            project.version_catalog = Some(catalog.clone());
        }

        // Parse build file (Groovy or Kotlin)
        let build_gradle = project_dir.join("build.gradle");
        let build_gradle_kts = project_dir.join("build.gradle.kts");

        // Large Groovy builds often keep coordinates in an applied script and
        // reference only `libs.foo` from the root build. Load bounded,
        // workspace-local static maps before parsing those references. This is
        // file inspection only and never invokes Gradle.
        let selected_build_file = if build_gradle.exists() {
            Some(build_gradle.as_path())
        } else if build_gradle_kts.exists() {
            Some(build_gradle_kts.as_path())
        } else {
            None
        };
        if let Some(build_file) = selected_build_file {
            Self::merge_applied_dependency_catalogs(build_file, &mut project)?;
        }

        let has_build_file = if build_gradle.exists() {
            debug!("Parsing build.gradle");
            report.set_build_file(build_gradle.clone(), GradleDslKind::Groovy);
            Self::parse_groovy_build_file(&build_gradle, &mut project, Some(&mut report)).await?;
            true
        } else if build_gradle_kts.exists() {
            debug!("Parsing build.gradle.kts");
            report.set_build_file(build_gradle_kts.clone(), GradleDslKind::Kotlin);
            Self::parse_kotlin_build_file(&build_gradle_kts, &mut project).await?;
            true
        } else {
            false
        };

        // Parse settings file for subprojects
        let settings_gradle = project_dir.join("settings.gradle");
        let settings_gradle_kts = project_dir.join("settings.gradle.kts");

        if settings_gradle.exists() {
            debug!("Parsing settings.gradle");
            report.set_settings_file(settings_gradle.clone(), GradleDslKind::Groovy);
            Self::parse_settings_file(&settings_gradle, &mut project, Some(&mut report))?;
        } else if settings_gradle_kts.exists() {
            debug!("Parsing settings.gradle.kts");
            report.set_settings_file(settings_gradle_kts.clone(), GradleDslKind::Kotlin);
            Self::parse_settings_file(&settings_gradle_kts, &mut project, Some(&mut report))?;
        }

        // This parser invocation represents the workspace root. Nested
        // projects are assigned canonical paths by the multi-module
        // classpath builder, where the settings include path is available.
        project.project_path = ":".to_string();

        if !report.include_build_roots.is_empty() {
            Self::sniff_composite_internal_plugins(&mut project, &mut report);
        }

        if !has_build_file && project.subprojects.is_empty() {
            return Err(GradleParserError::BuildFileNotFound(
                project_dir.to_path_buf(),
            ));
        }

        if !project.subprojects.is_empty() {
            report.subprojects = project.subprojects.clone();
            Self::parse_subprojects(&mut project, guard).await?;
        }

        // Offline fallback for buildSrc script plugins (e.g. buildSrc/src/main/groovy/<pluginId>.gradle)
        // when bytecode execution cannot fully evaluate the plugin.
        Self::parse_buildsrc_script_plugins(&mut project);

        // Bytecode extraction and regex fallback intentionally overlap. Keep one
        // declaration per coordinate/configuration so resolution accounting is
        // stable and does not depend on which sniffing strategy succeeded.
        let mut seen_dependencies = HashSet::new();
        project
            .dependencies
            .retain(|dependency| seen_dependencies.insert(dependency.clone()));

        // Add default source sets if Java plugin is detected
        if project.is_java_project() && project.source_sets.is_empty() {
            project.source_sets.push(SourceSet::main(&project.root_dir));
            project.source_sets.push(SourceSet::test(&project.root_dir));
        }

        // Include annotation processor generated sources if the directory exists.
        // Gradle's annotationProcessor configuration outputs to this standard path
        // (e.g. Hibernate metamodel generator, Dagger, MapStruct, Lombok).
        if project.is_java_project() {
            let ap_dir = project
                .root_dir
                .join("build/generated/sources/annotationProcessor/java/main");
            if ap_dir.is_dir() {
                if let Some(main) = project.source_sets.iter_mut().find(|s| s.name == "main") {
                    if !main.java_dirs.contains(&ap_dir) {
                        debug!(
                            "Adding annotation processor generated source dir: {}",
                            ap_dir.display()
                        );
                        main.java_dirs.push(ap_dir);
                    }
                }
            }
        }

        // Add Maven Central by default if no repositories configured
        if project.repositories.is_empty() {
            project.repositories.push(GradleRepository::maven_central());
        }

        if emit_report_log {
            info!("Gradle sniff summary: {}", report.summary());
            debug!("Gradle sniff details:\n{}", report.debug_details());
        }

        Ok((project, Some(report)))
    }

    fn parse_buildsrc_script_plugins(project: &mut GradleProject) {
        let Some(script_root) = Self::find_buildsrc_script_root(&project.root_dir) else {
            return;
        };

        for plugin in project.plugins.clone() {
            let script_path = script_root.join(format!("{}.gradle", plugin.id));
            if !script_path.exists() {
                continue;
            }
            match read_utf8_file_bounded(&script_path, MAX_BUILD_FILE_BYTES) {
                Ok(content) => {
                    Self::parse_compile_thrift_output_dir(&content, project);
                }
                Err(e) => {
                    debug!(
                        "Failed to read buildSrc script plugin {}: {}",
                        script_path.display(),
                        e
                    );
                }
            }
        }
    }

    fn find_buildsrc_script_root(start: &Path) -> Option<PathBuf> {
        for ancestor in start.ancestors() {
            let candidate = ancestor.join("buildSrc/src/main/groovy");
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn parse_compile_thrift_output_dir(content: &str, project: &mut GradleProject) {
        let content = strip_gradle_comments(content);
        let mut remainder = content.as_str();
        while let Some(block) = find_named_block(remainder, "compileThrift") {
            for cap in THRIFT_OUTPUT_DIR_RE.captures_iter(block) {
                let dir = cap[1].trim();
                if !dir.is_empty() {
                    Self::insert_source_set_dir(project, "main", dir);
                }
            }
            let end = block.as_ptr() as usize - remainder.as_ptr() as usize + block.len() + 1;
            remainder = &remainder[end..];
        }
    }

    fn sniff_composite_internal_plugins(
        project: &mut GradleProject,
        report: &mut GradleSniffReport,
    ) {
        let included_roots = report.include_build_roots.clone();
        let mut subprojects: Vec<String> = project
            .subprojects
            .iter()
            .filter(|p| !p.starts_with("@build:"))
            .cloned()
            .collect();
        subprojects.sort_unstable();
        subprojects.dedup();

        let version_catalog_versions = project
            .version_catalog
            .as_ref()
            .map(|c| c.versions.clone())
            .unwrap_or_default();

        let executor = GradlePluginExecutor::with_policy(SecurityPolicy::strict());
        let mut vm_total = VmExecutionSummary::empty();
        let mut vm_duration_total = Duration::from_millis(0);

        for plugin in project.plugins.clone() {
            if !plugin.id.contains('.') {
                continue;
            }

            let mut resolved = None;
            for included_root in &included_roots {
                match super::composite_plugin::resolve_plugin_descriptor(included_root, &plugin.id)
                {
                    Ok(Some(descriptor)) => {
                        resolved = Some((descriptor, included_root.clone()));
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        report.add_note(format!(
                            "plugin descriptor scan failed for {} in {}: {}",
                            plugin.id,
                            included_root.display(),
                            e
                        ));
                    }
                }
            }

            match resolved {
                Some((descriptor, classpath_hint)) => {
                    let implementation_class = descriptor.implementation_class.clone();
                    let outcome = match super::plugin_bytecode::locate_plugin_bytecode(
                        &classpath_hint,
                        &implementation_class,
                    ) {
                        Ok(bytecode) => match bytecode.load_snapshot(&implementation_class) {
                            Ok(loaded) => {
                                let fingerprint = sniff_cache::fingerprint_loaded_inputs(
                                    &loaded,
                                    &project.root_dir,
                                    &subprojects,
                                    &version_catalog_versions,
                                );
                                let cache_key = sniff_cache::InternalPluginCacheKey::new(
                                    &project.root_dir,
                                    &classpath_hint,
                                    plugin.id.clone(),
                                    implementation_class.clone(),
                                );

                                if let Some(config) = sniff_cache::get(&cache_key, fingerprint) {
                                    report.add_note(format!(
                                        "internal plugin cache hit (execution skipped): {}",
                                        plugin.id
                                    ));
                                    Self::apply_bytecode_project_properties(&config, project);
                                    Self::apply_bytecode_plugins(&config, project);
                                    Self::apply_bytecode_repositories(&config, project);
                                    Self::apply_bytecode_dependencies(&config, project);
                                    Self::apply_bytecode_source_sets(&config, project);
                                    SniffOutcome::Executed
                                } else {
                                    report.add_note(format!(
                                        "internal plugin cache miss: {}",
                                        plugin.id
                                    ));
                                    match executor.execute_apply_loaded(
                                        &implementation_class,
                                        loaded,
                                        &project.root_dir,
                                        subprojects.clone(),
                                        version_catalog_versions.clone(),
                                    ) {
                                        Ok((config, stats)) => {
                                            vm_duration_total += stats.duration;
                                            vm_total.instruction_limit_hit |=
                                                stats.instruction_limit_hit;
                                            vm_total.timed_out |= stats.timed_out;
                                            vm_total.memory_limit_hit |= stats.memory_limit_hit;
                                            vm_total.stack_limit_hit |= stats.stack_limit_hit;

                                            report.add_note(format!(
                                                "internal plugin executed: {} ({} deps)",
                                                plugin.id,
                                                config.dependency_list.len()
                                            ));

                                            Self::apply_bytecode_project_properties(
                                                &config, project,
                                            );
                                            Self::apply_bytecode_plugins(&config, project);
                                            Self::apply_bytecode_repositories(&config, project);
                                            Self::apply_bytecode_dependencies(&config, project);
                                            Self::apply_bytecode_source_sets(&config, project);

                                            sniff_cache::put(cache_key, fingerprint, config);
                                            SniffOutcome::Executed
                                        }
                                        Err(e) => {
                                            vm_total.aborted = true;
                                            report.add_note(format!(
                                                "internal plugin execution failed: {} -> {}",
                                                plugin.id, e
                                            ));
                                            SniffOutcome::Skipped {
                                                reason: SniffSkipReason::ExecutionFailed {
                                                    message: exec_error_summary(&e),
                                                },
                                            }
                                        }
                                    }
                                }
                            }
                            Err(super::plugin_bytecode::PluginBytecodeError::Missing {
                                remediation,
                                ..
                            }) => SniffOutcome::Skipped {
                                reason: SniffSkipReason::MissingBytecode { remediation },
                            },
                            Err(super::plugin_bytecode::PluginBytecodeError::Snapshot(error)) => {
                                vm_total.aborted = true;
                                report.add_note(format!(
                                    "internal plugin classpath snapshot failed: {} -> {}",
                                    plugin.id, error
                                ));
                                SniffOutcome::Skipped {
                                    reason: SniffSkipReason::ExecutionFailed {
                                        message: exec_error_summary(&error),
                                    },
                                }
                            }
                            Err(error) => {
                                vm_total.aborted = true;
                                report.add_note(format!(
                                    "internal plugin classpath snapshot failed: {} -> {}",
                                    plugin.id, error
                                ));
                                SniffOutcome::Skipped {
                                    reason: SniffSkipReason::ExecutionFailed {
                                        message: error.to_string(),
                                    },
                                }
                            }
                        },
                        Err(super::plugin_bytecode::PluginBytecodeError::Missing {
                            remediation,
                            ..
                        }) => SniffOutcome::Skipped {
                            reason: SniffSkipReason::MissingBytecode { remediation },
                        },
                        Err(e) => {
                            report.add_note(format!(
                                "internal plugin bytecode probe failed for {}: {}",
                                plugin.id, e
                            ));
                            SniffOutcome::Skipped {
                                reason: SniffSkipReason::Unsupported,
                            }
                        }
                    };

                    report.plugin_resolutions.push(GradlePluginResolution {
                        plugin_id: plugin.id.clone(),
                        outcome,
                        implementation_class: Some(implementation_class),
                        classpath_hint: Some(classpath_hint),
                    });
                }
                None => report.plugin_resolutions.push(GradlePluginResolution {
                    plugin_id: plugin.id.clone(),
                    outcome: SniffOutcome::Skipped {
                        reason: SniffSkipReason::Unsupported,
                    },
                    implementation_class: None,
                    classpath_hint: None,
                }),
            }
        }

        if !report.plugin_resolutions.is_empty() {
            vm_total.duration = Some(vm_duration_total);
            report.vm_summary = Some(vm_total);
        }
    }

    /// Normalize a version catalog alias to match Gradle's internal format
    ///
    /// Gradle normalizes catalog aliases:
    /// - `spring-boot-starter` → `spring.boot.starter` (dashes to dots)
    /// - `springBootStarter` → `spring.boot.starter` (camelCase to dots)
    /// - Mixed: `my-library.core-utils` → `my.library.core.utils`
    fn normalize_catalog_alias(alias: &str) -> String {
        let mut result = String::with_capacity(alias.len() + 4);
        let mut prev_was_lower = false;

        for ch in alias.chars() {
            if ch == '-' || ch == '_' {
                // Convert dashes and underscores to dots
                if !result.ends_with('.') {
                    result.push('.');
                }
                prev_was_lower = false;
            } else if ch.is_ascii_uppercase() {
                // Insert dot before uppercase if previous was lowercase (camelCase boundary)
                if prev_was_lower && !result.ends_with('.') {
                    result.push('.');
                }
                result.push(ch.to_ascii_lowercase());
                prev_was_lower = false;
            } else {
                result.push(ch);
                prev_was_lower = ch.is_ascii_lowercase();
            }
        }

        result
    }

    fn register_catalog_library(catalog: &mut VersionCatalog, alias: &str, coordinates: String) {
        let normalized = Self::normalize_catalog_alias(alias);
        catalog
            .libraries
            .insert(normalized.clone(), coordinates.clone());
        if alias != normalized {
            catalog.libraries.insert(alias.to_string(), coordinates);
        }
    }

    fn register_catalog_bundle(catalog: &mut VersionCatalog, alias: &str, members: Vec<String>) {
        let normalized = Self::normalize_catalog_alias(alias);
        catalog.bundles.insert(normalized.clone(), members.clone());
        if alias != normalized {
            catalog.bundles.insert(alias.to_string(), members);
        }
    }

    fn resolve_catalog_library<'a>(catalog: &'a VersionCatalog, alias: &str) -> Option<&'a String> {
        let normalized = Self::normalize_catalog_alias(alias);
        catalog
            .libraries
            .get(&normalized)
            .or_else(|| catalog.libraries.get(alias))
    }

    fn resolve_catalog_bundle<'a>(
        catalog: &'a VersionCatalog,
        alias: &str,
    ) -> Option<&'a Vec<String>> {
        let normalized = Self::normalize_catalog_alias(alias);
        catalog
            .bundles
            .get(&normalized)
            .or_else(|| catalog.bundles.get(alias))
    }

    fn to_camel_case(alias: &str) -> String {
        let mut parts = alias.split('.').filter(|part| !part.is_empty());
        let mut result = String::new();
        if let Some(first) = parts.next() {
            result.push_str(first);
        }
        for part in parts {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.push(first.to_ascii_uppercase());
                result.extend(chars.map(|c| c.to_ascii_lowercase()));
            }
        }
        result
    }

    fn is_groovy_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        let first = match chars.next() {
            Some(ch) => ch,
            None => return false,
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            return false;
        }
        chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }

    fn groovy_escape(value: &str) -> String {
        value.replace('\\', "\\\\").replace('\'', "\\'")
    }

    fn groovy_key(key: &str) -> String {
        if Self::is_groovy_identifier(key) {
            key.to_string()
        } else {
            format!("'{}'", Self::groovy_escape(key))
        }
    }

    fn groovy_literal(value: &GroovyValue) -> String {
        match value {
            GroovyValue::Str(value) => format!("'{}'", Self::groovy_escape(value)),
            GroovyValue::List(items) => {
                let rendered: Vec<String> = items.iter().map(Self::groovy_literal).collect();
                format!("[{}]", rendered.join(", "))
            }
            GroovyValue::Map(map) => {
                let rendered: Vec<String> = map
                    .iter()
                    .map(|(key, value)| {
                        format!("{}: {}", Self::groovy_key(key), Self::groovy_literal(value))
                    })
                    .collect();
                format!("[{}]", rendered.join(", "))
            }
        }
    }

    fn insert_nested_map(
        map: &mut BTreeMap<String, GroovyValue>,
        path: &[String],
        value: GroovyValue,
    ) {
        if let Some((head, rest)) = path.split_first() {
            if rest.is_empty() {
                map.entry(head.clone()).or_insert(value);
                return;
            }

            let entry = map
                .entry(head.clone())
                .or_insert_with(|| GroovyValue::Map(BTreeMap::new()));
            if let GroovyValue::Map(ref mut nested) = entry {
                Self::insert_nested_map(nested, rest, value);
            }
        }
    }

    fn build_version_catalog_prelude(catalog: &VersionCatalog) -> String {
        let mut libs_map = BTreeMap::new();
        let mut bundles_map = BTreeMap::new();
        let mut seen = HashMap::new();

        for (alias, coords) in &catalog.libraries {
            let normalized = Self::normalize_catalog_alias(alias);
            let entry = seen
                .entry(normalized.clone())
                .or_insert_with(|| coords.clone());
            if entry != coords {
                continue;
            }

            let path: Vec<String> = normalized.split('.').map(|s| s.to_string()).collect();
            if !path.is_empty() {
                Self::insert_nested_map(&mut libs_map, &path, GroovyValue::Str(coords.clone()));
            }

            let camel = Self::to_camel_case(&normalized);
            if !camel.is_empty() {
                libs_map
                    .entry(camel)
                    .or_insert_with(|| GroovyValue::Str(coords.clone()));
            }
        }

        for (alias, members) in &catalog.bundles {
            let normalized = Self::normalize_catalog_alias(alias);
            let path: Vec<String> = normalized.split('.').map(|s| s.to_string()).collect();
            let mut list_items = Vec::new();

            for member in members {
                if let Some(coords) = Self::resolve_catalog_library(catalog, member) {
                    list_items.push(GroovyValue::Str(coords.clone()));
                }
            }

            if !list_items.is_empty() && !path.is_empty() {
                Self::insert_nested_map(&mut bundles_map, &path, GroovyValue::List(list_items));
            }
        }

        if !bundles_map.is_empty() {
            libs_map.insert("bundles".to_string(), GroovyValue::Map(bundles_map));
        }

        if libs_map.is_empty() {
            return String::new();
        }

        let libs_literal = Self::groovy_literal(&GroovyValue::Map(libs_map));
        format!("def libs = {}\n", libs_literal)
    }

    fn apply_bytecode_dependencies(
        config: &hyperion_bytecode::gradle::GradleBuildConfig,
        project: &mut GradleProject,
    ) {
        for dep in &config.dependency_list {
            let config_kind = DependencyConfiguration::from_config_str(&dep.configuration);

            if dep.is_project {
                Self::add_project_dependency(project, config_kind, &dep.artifact);
                continue;
            }

            let coords = if dep.version.is_empty() {
                format!("{}:{}", dep.group, dep.artifact)
            } else {
                format!("{}:{}:{}", dep.group, dep.artifact, dep.version)
            };

            let is_platform = dep.extension.as_deref() == Some("pom");
            let dep_opt = if is_platform {
                GradleDependency::from_coordinates_with_platform(&coords, config_kind, true)
            } else {
                GradleDependency::from_coordinates(&coords, config_kind)
            };

            if let Some(mut gradle_dep) = dep_opt {
                // An enforced platform's versions override the rest of the graph, which
                // is what a forced declaration does here.
                gradle_dep.forced |= dep.enforced;
                gradle_dep.classifier = dep.classifier.clone();
                if dep.extension.is_some() {
                    gradle_dep.packaging = dep.extension.clone();
                }
                project.dependencies.push(gradle_dep);
            }
        }
    }

    fn add_project_dependency(
        project: &mut GradleProject,
        configuration: DependencyConfiguration,
        module: &str,
    ) {
        let module = module.trim_start_matches(':');
        if module.is_empty() {
            return;
        }
        let dependencies = project
            .project_dependencies
            .entry(configuration)
            .or_default();
        if !dependencies.iter().any(|dependency| dependency == module) {
            dependencies.push(module.to_string());
        }
    }

    fn apply_bytecode_project_properties(
        config: &hyperion_bytecode::gradle::GradleBuildConfig,
        project: &mut GradleProject,
    ) {
        if let Some(group) = &config.group {
            project.group = Some(group.clone());
        }
        if let Some(version) = &config.version {
            project.version = Some(version.clone());
        }
    }

    fn apply_bytecode_plugins(
        config: &hyperion_bytecode::gradle::GradleBuildConfig,
        project: &mut GradleProject,
    ) {
        for plugin in config
            .plugin_definitions
            .iter()
            .filter(|plugin| plugin.apply)
        {
            if project.plugins.iter().any(|p| p.id == plugin.id) {
                continue;
            }
            project.plugins.push(GradlePlugin {
                id: plugin.id.clone(),
                version: plugin.version.clone(),
            });
        }
    }

    fn apply_bytecode_repositories(
        config: &hyperion_bytecode::gradle::GradleBuildConfig,
        project: &mut GradleProject,
    ) {
        for repo in &config.repository_list {
            let gradle_repo = GradleRepository {
                name: repo.name.clone(),
                url: repo.url.clone(),
            };
            if !project.repositories.contains(&gradle_repo) {
                project.repositories.push(gradle_repo);
            }
        }
    }

    fn apply_bytecode_source_sets(
        config: &hyperion_bytecode::gradle::GradleBuildConfig,
        project: &mut GradleProject,
    ) {
        for (key, value) in &config.properties {
            if let Some(source_set) = Self::source_set_name_from_property(key) {
                let dirs = Self::split_paths(value);
                for dir in dirs {
                    Self::insert_source_set_dir(project, &source_set, &dir);
                }
            }

            if key == "extension.compileThrift.outputDir" && !value.is_empty() {
                for dir in Self::split_paths(value) {
                    Self::insert_source_set_dir(project, "main", &dir);
                }
            }
        }
    }

    fn source_set_name_from_property(key: &str) -> Option<String> {
        let trimmed = key.strip_prefix("sourceSet.")?.strip_suffix(".srcDirs")?;
        if let Some(rest) = trimmed.strip_prefix("gradle/SourceDirectorySet/") {
            return rest.split('/').next().map(|name| name.to_string());
        }
        Some(trimmed.to_string())
    }

    fn split_paths(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect()
    }

    fn insert_source_set_dir(project: &mut GradleProject, name: &str, dir: &str) {
        let project_dir = project.root_dir.to_string_lossy();
        let expanded = dir
            .replace("${projectDir}", &project_dir)
            .replace("$projectDir", &project_dir);
        let mut path = PathBuf::from(expanded);
        if !path.is_absolute() {
            path = project.root_dir.join(path);
        }
        let index = match project.source_sets.iter().position(|set| set.name == name) {
            Some(index) => index,
            None => {
                let set = if name == "main" {
                    SourceSet::main(&project.root_dir)
                } else if name == "test" {
                    SourceSet::test(&project.root_dir)
                } else {
                    SourceSet::new(name.to_string())
                };
                project.source_sets.push(set);
                project.source_sets.len() - 1
            }
        };
        let source_set = &mut project.source_sets[index];
        if !source_set.java_dirs.contains(&path) {
            source_set.java_dirs.push(path);
        }
    }

    /// Parse version catalog (gradle/libs.versions.toml)
    fn parse_version_catalog(path: &Path) -> Result<VersionCatalog> {
        let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)?;
        let toml_value: toml::Value = content
            .parse()
            .map_err(|e| GradleParserError::ParseError(format!("Failed to parse TOML: {}", e)))?;

        let mut catalog = VersionCatalog::default();

        // Parse [versions] section
        if let Some(versions) = toml_value.get("versions").and_then(|v| v.as_table()) {
            for (key, value) in versions {
                if let Some(version_str) = value.as_str() {
                    catalog
                        .versions
                        .insert(key.clone(), version_str.to_string());
                }
            }
        }

        // Parse [libraries] section
        if let Some(libraries) = toml_value.get("libraries").and_then(|v| v.as_table()) {
            for (alias, lib_def) in libraries {
                if let Some(table) = lib_def.as_table() {
                    // Handle: { module = "group:artifact", version = "1.0" }
                    // or: { module = "group:artifact", version.ref = "version-key" }
                    if let Some(module) = table.get("module").and_then(|m| m.as_str()) {
                        let version = if let Some(version_str) =
                            table.get("version").and_then(|v| v.as_str())
                        {
                            // Direct version string
                            Some(version_str.to_string())
                        } else if let Some(version_table) =
                            table.get("version").and_then(|v| v.as_table())
                        {
                            // Version reference: version.ref = "..."
                            version_table
                                .get("ref")
                                .and_then(|r| r.as_str())
                                .and_then(|ref_key| catalog.versions.get(ref_key))
                                .cloned()
                        } else {
                            None
                        };

                        // Build coordinates: group:artifact:version
                        let coordinates = if let Some(v) = version {
                            format!("{}:{}", module, v)
                        } else {
                            module.to_string()
                        };

                        Self::register_catalog_library(&mut catalog, alias, coordinates);

                        debug!("Catalog library: {} -> {}", alias, module);
                    }
                } else if let Some(string_def) = lib_def.as_str() {
                    // Handle shorthand: library = "group:artifact:version"
                    Self::register_catalog_library(&mut catalog, alias, string_def.to_string());
                    debug!("Catalog library (shorthand): {} -> {}", alias, string_def);
                }
            }
        }

        // Parse [bundles] section
        if let Some(bundles) = toml_value.get("bundles").and_then(|v| v.as_table()) {
            for (alias, bundle_def) in bundles {
                if let Some(array) = bundle_def.as_array() {
                    let mut members = Vec::new();
                    for item in array {
                        if let Some(member) = item.as_str() {
                            members.push(member.to_string());
                        }
                    }
                    if !members.is_empty() {
                        Self::register_catalog_bundle(&mut catalog, alias, members);
                    }
                }
            }
        }

        Ok(catalog)
    }

    /// Load legacy `versions += [...]` / `libs += [...]` maps from scripts
    /// referenced by `apply from:`. The scan is intentionally conservative:
    /// only regular files below the project root are read, and recursion and
    /// file size are capped.
    fn merge_applied_dependency_catalogs(
        build_file: &Path,
        project: &mut GradleProject,
    ) -> Result<()> {
        let workspace_root =
            fs::canonicalize(&project.root_dir).unwrap_or_else(|_| project.root_dir.clone());
        let mut pending = vec![build_file.to_path_buf()];
        let mut visited = HashSet::new();
        let mut merged = VersionCatalog::default();

        while let Some(script) = pending.pop() {
            if visited.len() >= MAX_APPLIED_CATALOG_FILES || !visited.insert(script.clone()) {
                continue;
            }

            let maximum = if script == build_file {
                MAX_BUILD_FILE_BYTES
            } else {
                MAX_APPLIED_CATALOG_BYTES as usize
            };
            let content = read_utf8_file_bounded(&script, maximum)?;
            for cap in APPLY_FROM_RE.captures_iter(&content) {
                let Some(raw_match) = cap.get(1) else {
                    continue;
                };
                let raw = raw_match.as_str();
                let Some(candidate) = Self::resolve_applied_script_path(raw, &project.root_dir)
                else {
                    debug!("Skipping dynamic applied Gradle script path: {}", raw);
                    continue;
                };
                let canonical = match fs::canonicalize(&candidate) {
                    Ok(path) => path,
                    Err(_) => {
                        debug!("Applied Gradle script not found: {}", candidate.display());
                        continue;
                    }
                };
                if !canonical.starts_with(&workspace_root) {
                    warn!(
                        "Skipping applied Gradle script outside workspace: {}",
                        canonical.display()
                    );
                    continue;
                }
                let metadata = fs::metadata(&canonical)?;
                if !metadata.is_file() || metadata.len() > MAX_APPLIED_CATALOG_BYTES {
                    warn!(
                        "Skipping applied Gradle script {} (not a file or exceeds {} bytes)",
                        canonical.display(),
                        MAX_APPLIED_CATALOG_BYTES
                    );
                    continue;
                }
                if !visited.contains(&canonical) {
                    pending.push(canonical);
                }
            }

            if script != build_file {
                Self::parse_legacy_dependency_catalog(&content, &mut merged);
            }
        }

        if merged.libraries.is_empty() && merged.versions.is_empty() {
            return Ok(());
        }

        let catalog = project
            .version_catalog
            .get_or_insert_with(VersionCatalog::default);
        let versions_before = catalog.versions.len();
        let libraries_before = catalog.libraries.len();
        for (key, value) in merged.versions {
            catalog.versions.entry(key).or_insert(value);
        }
        for (key, value) in merged.libraries {
            catalog.libraries.entry(key).or_insert(value);
        }
        info!(
            "Loaded applied Gradle dependency catalog: {} versions, {} libraries",
            catalog.versions.len() - versions_before,
            catalog.libraries.len() - libraries_before
        );
        Ok(())
    }

    fn resolve_applied_script_path(raw: &str, project_root: &Path) -> Option<PathBuf> {
        let root = project_root.to_string_lossy();
        let expanded = raw
            .replace("${rootDir}", &root)
            .replace("$rootDir", &root)
            .replace("${rootProject.projectDir}", &root)
            .replace("$rootProject.projectDir", &root);
        if expanded.contains('$') {
            return None;
        }
        let path = PathBuf::from(expanded);
        Some(if path.is_absolute() {
            path
        } else {
            project_root.join(path)
        })
    }

    fn parse_legacy_dependency_catalog(content: &str, catalog: &mut VersionCatalog) {
        let content = strip_gradle_comments(content);
        let mut variables = HashMap::new();
        for cap in LEGACY_LITERAL_VAR_RE.captures_iter(&content) {
            variables
                .entry(cap[1].to_string())
                .or_insert_with(|| cap[2].to_string());
        }

        for block in Self::legacy_map_blocks(&content, "versions") {
            for (key, value) in Self::legacy_map_entries(block) {
                catalog.versions.entry(key).or_insert(value);
            }
        }
        for cap in LEGACY_VERSION_ASSIGN_RE.captures_iter(&content) {
            let Some(key) = cap.get(1).or_else(|| cap.get(2)).map(|m| m.as_str()) else {
                continue;
            };
            let value = cap
                .get(3)
                .map(|m| m.as_str().to_string())
                .or_else(|| cap.get(4).and_then(|m| variables.get(m.as_str()).cloned()));
            if let Some(value) = value {
                // A conditional script may contain alternatives. Keeping the
                // first statically-known value is deterministic; the resolver
                // reports any cache miss if the active build selected another.
                catalog.versions.entry(key.to_string()).or_insert(value);
            }
        }
        if let Some(scala) = catalog.versions.get("scala").cloned() {
            if let Some((base, _)) = scala.rsplit_once('.') {
                catalog
                    .versions
                    .entry("baseScala".to_string())
                    .or_insert_with(|| base.to_string());
            }
        }

        for block in Self::legacy_map_blocks(&content, "libs") {
            for (alias, raw_coordinates) in Self::legacy_map_entries(block) {
                let coordinates =
                    Self::interpolate_legacy_value(&raw_coordinates, &catalog.versions, &variables);
                let parts: Vec<&str> = coordinates.split(':').collect();
                if parts.len() < 2 || parts[0].contains('$') || parts[1].contains('$') {
                    debug!(
                        "Skipping dynamic legacy dependency alias {} -> {}",
                        alias, raw_coordinates
                    );
                    continue;
                }
                // If only the version is dynamic, retain group/artifact. BOM or
                // local-cache selection may still resolve it, with explicit
                // fallback accounting in the resolver.
                let usable = if parts.get(2).is_some_and(|version| version.contains('$')) {
                    format!("{}:{}", parts[0], parts[1])
                } else {
                    coordinates
                };
                Self::register_catalog_library(catalog, &alias, usable);
            }
        }
    }

    fn legacy_map_blocks<'a>(content: &'a str, name: &str) -> Vec<&'a str> {
        let marker = format!("{} +=", name);
        let mut blocks = Vec::new();
        let mut search_from = 0;
        while let Some(relative) = content[search_from..].find(&marker) {
            let marker_start = search_from + relative;
            let Some(open_relative) = content[marker_start + marker.len()..].find('[') else {
                break;
            };
            let open = marker_start + marker.len() + open_relative;
            let mut depth = 0usize;
            let mut quote = None;
            let mut escaped = false;
            for (offset, ch) in content[open..].char_indices() {
                if let Some(active_quote) = quote {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == active_quote {
                        quote = None;
                    }
                    continue;
                }
                if ch == '\'' || ch == '"' {
                    quote = Some(ch);
                } else if ch == '[' {
                    depth += 1;
                } else if ch == ']' {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        blocks.push(&content[open + 1..open + offset]);
                        search_from = open + offset + 1;
                        break;
                    }
                }
            }
            if search_from <= marker_start {
                break;
            }
        }
        blocks
    }

    fn legacy_map_entries(block: &str) -> Vec<(String, String)> {
        LEGACY_MAP_ENTRY_RE
            .captures_iter(block)
            .filter_map(|cap| {
                let key = cap.get(1).or_else(|| cap.get(2))?.as_str().to_string();
                Some((key, cap.get(3)?.as_str().to_string()))
            })
            .collect()
    }

    fn interpolate_legacy_value(
        value: &str,
        versions: &HashMap<String, String>,
        variables: &HashMap<String, String>,
    ) -> String {
        LEGACY_INTERPOLATION_RE
            .replace_all(value, |cap: &regex::Captures<'_>| {
                let name = &cap[1];
                name.strip_prefix("versions.")
                    .and_then(|key| versions.get(key))
                    .or_else(|| variables.get(name))
                    .cloned()
                    .unwrap_or_else(|| cap[0].to_string())
            })
            .into_owned()
    }

    /// Parse a Groovy DSL build file (build.gradle)
    async fn parse_groovy_build_file(
        path: &Path,
        project: &mut GradleProject,
        mut report: Option<&mut GradleSniffReport>,
    ) -> Result<()> {
        let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)?;
        let fallback_content = strip_gradle_comments(&content);

        let prelude = project
            .version_catalog
            .as_ref()
            .map(Self::build_version_catalog_prelude)
            .unwrap_or_default();

        let source = if prelude.is_empty() {
            content
        } else {
            format!("{}\n{}", prelude, content)
        };

        let mut executor = GradleExecutor::new();
        executor.set_recording_proxy(true);
        match executor.execute_source_bytecode_only_with_project(&source, &project.root_dir) {
            Ok(config) => {
                if let Some(report) = report.as_deref_mut() {
                    report.add_note(format!(
                        "build.gradle bytecode execution: executed ({} deps, {} plugins)",
                        config.dependency_list.len(),
                        config.plugin_definitions.len()
                    ));
                }
                Self::apply_bytecode_project_properties(&config, project);
                Self::apply_bytecode_plugins(&config, project);
                Self::apply_bytecode_repositories(&config, project);
                Self::apply_bytecode_dependencies(&config, project);
                Self::apply_bytecode_source_sets(&config, project);
            }
            Err(e) => {
                warn!(
                    "Bytecode Gradle execution failed for {}: {} (falling back to regex parsing)",
                    path.display(),
                    e
                );
                if let Some(report) = &mut report {
                    report.add_note(format!(
                        "build.gradle bytecode execution: failed ({}) -> regex fallback",
                        e
                    ));
                }
            }
        }

        // FALLBACK: Regex-based parsing as a secondary source of truth.
        // Bytecode extraction is best-effort and may miss patterns (especially in minimal builds).
        // Running a lightweight parse here improves robustness while avoiding hard dependency
        // on a specific sniffing strategy.
        Self::parse_project_properties(&fallback_content, project);
        Self::parse_plugins(&fallback_content, project)?;
        Self::parse_repositories(&fallback_content, project)?;
        Self::parse_dependencies_groovy(&fallback_content, project)?;
        Self::parse_compile_thrift_output_dir(&fallback_content, project);

        Ok(())
    }

    /// Parse a Kotlin DSL build file (build.gradle.kts)
    async fn parse_kotlin_build_file(path: &Path, project: &mut GradleProject) -> Result<()> {
        let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)?;
        let content = strip_gradle_comments(&content);

        // Parse group and version
        Self::parse_project_properties(&content, project);

        // Parse plugins
        Self::parse_plugins(&content, project)?;

        // Parse repositories
        Self::parse_repositories(&content, project)?;

        // Parse dependencies (Kotlin DSL uses similar syntax)
        Self::parse_dependencies_kotlin(&content, project)?;

        Ok(())
    }

    /// Parse project properties (group, version)
    fn parse_project_properties(content: &str, project: &mut GradleProject) {
        // Match: group = 'com.example' or group = "com.example"
        if let Some(cap) = GROUP_RE.captures(content) {
            project.group = Some(cap[1].to_string());
            debug!("Found group: {}", &cap[1]);
        }

        // Match: version = '1.0.0' or version = "1.0.0"
        if let Some(cap) = VERSION_RE.captures(content) {
            project.version = Some(cap[1].to_string());
            debug!("Found version: {}", &cap[1]);
        }
    }

    /// Parse plugins block
    fn parse_plugins(content: &str, project: &mut GradleProject) -> Result<()> {
        let plugins_block = find_named_block(content, "plugins").unwrap_or_default();

        // Match: id 'java' or id("java")
        for cap in PLUGIN_ID_RE.captures_iter(plugins_block) {
            Self::upsert_plugin(project, &cap[1], None);
        }

        // Match: id 'plugin' version 'x.y.z' or id("plugin") version "x.y.z"
        for cap in PLUGIN_VERSION_RE.captures_iter(plugins_block) {
            Self::upsert_plugin(project, &cap[1], Some(cap[2].to_string()));
        }

        // Kotlin's generated plugin accessor maps kotlin("jvm") to
        // org.jetbrains.kotlin.jvm (and likewise for kapt/plugin.spring/etc.).
        for cap in KOTLIN_PLUGIN_RE.captures_iter(plugins_block) {
            let plugin_id = format!("org.jetbrains.kotlin.{}", &cap[1]);
            let version = cap.get(2).map(|value| value.as_str().to_string());
            Self::upsert_plugin(project, &plugin_id, version);
        }

        // Core Kotlin DSL plugins are type-safe accessors rather than id(...) calls.
        // A token-only statement inside the plugins block is an accessor; backticks
        // preserve IDs such as `java-library` and `kotlin-dsl`.
        for statement in plugins_block.split(['\n', ';']) {
            let accessor = statement.trim().trim_matches('`');
            if !accessor.is_empty()
                && accessor
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            {
                Self::upsert_plugin(project, accessor, None);
            }
        }

        // Also check for apply plugin: syntax
        for cap in APPLY_PLUGIN_RE.captures_iter(content) {
            Self::upsert_plugin(project, &cap[1], None);
        }

        Ok(())
    }

    fn upsert_plugin(project: &mut GradleProject, plugin_id: &str, version: Option<String>) {
        if let Some(plugin) = project.plugins.iter_mut().find(|p| p.id == plugin_id) {
            if version.is_some() {
                plugin.version = version;
            }
            return;
        }

        debug!("Found plugin: {}", plugin_id);
        project.plugins.push(match version {
            Some(version) => GradlePlugin::with_version(plugin_id.to_string(), version),
            None => GradlePlugin::new(plugin_id.to_string()),
        });
    }

    /// Parse repositories block
    fn parse_repositories(content: &str, project: &mut GradleProject) -> Result<()> {
        let push_if_new = |repos: &mut Vec<GradleRepository>, repo: GradleRepository| {
            if !repos.contains(&repo) {
                debug!("Found repository: {}", repo.name);
                repos.push(repo);
            }
        };

        if content.contains("mavenCentral()") {
            push_if_new(&mut project.repositories, GradleRepository::maven_central());
        }
        if content.contains("mavenLocal()") {
            push_if_new(&mut project.repositories, GradleRepository::maven_local());
        }
        if content.contains("google()") {
            push_if_new(&mut project.repositories, GradleRepository::google());
        }
        if content.contains("jcenter()") {
            push_if_new(&mut project.repositories, GradleRepository::jcenter());
        }

        for cap in MAVEN_URL_RE.captures_iter(content) {
            let url = cap[1].to_string();
            push_if_new(
                &mut project.repositories,
                GradleRepository::custom(url.clone(), url),
            );
        }

        Ok(())
    }

    /// Parse dependencies (Groovy DSL) via bytecode execution.
    fn parse_dependencies_groovy(content: &str, project: &mut GradleProject) -> Result<()> {
        let properties = read_gradle_properties(&project.root_dir);
        let interpolated = interpolate_string_constants(content, &properties);
        let content = interpolated.as_str();
        let prelude = project
            .version_catalog
            .as_ref()
            .map(Self::build_version_catalog_prelude)
            .unwrap_or_default();

        let source = if prelude.is_empty() {
            content.to_string()
        } else {
            format!("{}\n{}", prelude, content)
        };

        let mut executor = GradleExecutor::new();
        executor.set_recording_proxy(true);
        match executor.execute_source_bytecode_only_with_project(&source, &project.root_dir) {
            Ok(config) => Self::apply_bytecode_dependencies(&config, project),
            Err(e) => {
                debug!(
                    "Groovy bytecode dependency extraction failed: {} (falling back to regex)",
                    e
                );
            }
        }

        Self::parse_dependencies_groovy_regex(content, project)?;
        apply_dependency_blocks(content, project);
        Ok(())
    }

    /// Parse dependencies in a Groovy build file using regex (offline fallback).
    ///
    /// This is intentionally conservative: it supports common patterns and
    /// version-catalog references, but does not attempt to emulate Groovy semantics.
    fn parse_dependencies_groovy_regex(content: &str, project: &mut GradleProject) -> Result<()> {
        for capture in GROOVY_PROJECT_DEP_RE.captures_iter(content) {
            let configuration = DependencyConfiguration::from_config_str(&capture[1]);
            Self::add_project_dependency(project, configuration, &capture[2]);
        }

        for cap in STRING_DEP_RE
            .captures_iter(content)
            .chain(STRING_DEP_CALL_RE.captures_iter(content))
        {
            let config_str = &cap[1];
            let coords = &cap[2];
            let config = DependencyConfiguration::from_config_str(config_str);
            if let Some(dep) = GradleDependency::from_coordinates(coords, config) {
                if !project
                    .dependencies
                    .iter()
                    .any(|d| d.same_declaration(&dep))
                {
                    project.dependencies.push(dep);
                }
            }
        }

        spring_boot_bom_platforms(content, project);

        // Match: implementation platform('g:a:v')
        for cap in PLATFORM_DEP_RE.captures_iter(content) {
            let config_str = &cap[1];
            let coords = &cap[2];
            let config = DependencyConfiguration::from_config_str(config_str);
            if let Some(dep) =
                GradleDependency::from_coordinates_with_platform(coords, config, true)
            {
                if !project
                    .dependencies
                    .iter()
                    .any(|d| d.same_declaration(&dep))
                {
                    project.dependencies.push(dep);
                }
            }
        }

        // Match: implementation(libs.xxx) and implementation libs.xxx - version catalog
        for cap in CATALOG_CALL_RE
            .captures_iter(content)
            .chain(CATALOG_SPACE_RE.captures_iter(content))
        {
            let config_str = &cap[1];
            let alias = &cap[2];
            let config = DependencyConfiguration::from_config_str(config_str);

            if let Some(catalog) = &project.version_catalog {
                if alias.starts_with("bundles.") {
                    let bundle_alias = alias.trim_start_matches("bundles.");
                    if let Some(members) = Self::resolve_catalog_bundle(catalog, bundle_alias) {
                        for member in members {
                            if let Some(coords) = Self::resolve_catalog_library(catalog, member) {
                                if let Some(dep) =
                                    GradleDependency::from_coordinates(coords, config.clone())
                                {
                                    if !project
                                        .dependencies
                                        .iter()
                                        .any(|d| d.same_declaration(&dep))
                                    {
                                        project.dependencies.push(dep);
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(coords) = Self::resolve_catalog_library(catalog, alias) {
                    if let Some(dep) = GradleDependency::from_coordinates(coords, config) {
                        if !project
                            .dependencies
                            .iter()
                            .any(|d| d.same_declaration(&dep))
                        {
                            project.dependencies.push(dep);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse dependencies (Kotlin DSL)
    fn parse_dependencies_kotlin(content: &str, project: &mut GradleProject) -> Result<()> {
        let properties = read_gradle_properties(&project.root_dir);
        let interpolated = interpolate_string_constants(content, &properties);
        let content = interpolated.as_str();
        // Match: implementation(libs.xxx) - version catalog syntax
        for cap in CATALOG_CALL_RE.captures_iter(content) {
            let config_str = &cap[1];
            let catalog_alias = &cap[2];
            if catalog_alias.starts_with("bundles.") {
                continue;
            }

            let config = DependencyConfiguration::from_config_str(config_str);

            // Try to resolve from version catalog
            if let Some(catalog) = &project.version_catalog {
                let coordinates = Self::resolve_catalog_library(catalog, catalog_alias);

                if let Some(coords) = coordinates {
                    if let Some(dep) = GradleDependency::from_coordinates(coords, config) {
                        debug!(
                            "Found catalog dependency: libs.{} -> {} ({})",
                            catalog_alias,
                            dep.coordinates(),
                            config_str
                        );
                        project.dependencies.push(dep);
                    }
                } else {
                    warn!(
                        "Version catalog reference 'libs.{}' not found in catalog",
                        catalog_alias
                    );
                }
            } else {
                warn!(
                    "Version catalog reference 'libs.{}' used but no catalog found",
                    catalog_alias
                );
            }
        }

        // Match: implementation(libs.bundles.xxx) - version catalog bundles
        // CATALOG_CALL_RE matches libs.xxx; we filter to only bundles.* and strip the prefix
        for cap in CATALOG_CALL_RE.captures_iter(content) {
            let raw_alias = &cap[2];
            let Some(bundle_alias) = raw_alias.strip_prefix("bundles.") else {
                continue;
            };
            let config_str = &cap[1];
            let config = DependencyConfiguration::from_config_str(config_str);

            if let Some(catalog) = &project.version_catalog {
                if let Some(bundle_members) = Self::resolve_catalog_bundle(catalog, bundle_alias) {
                    for member in bundle_members {
                        if let Some(coords) = Self::resolve_catalog_library(catalog, member) {
                            if let Some(dep) =
                                GradleDependency::from_coordinates(coords, config.clone())
                            {
                                debug!(
                                    "Found catalog bundle dependency (Kotlin): libs.bundles.{} -> {} ({})",
                                    bundle_alias,
                                    dep.coordinates(),
                                    config_str
                                );
                                project.dependencies.push(dep);
                            }
                        } else {
                            warn!("Bundle member 'libs.{}' not found in catalog", member);
                        }
                    }
                } else {
                    warn!(
                        "Version catalog bundle 'libs.bundles.{}' not found in catalog",
                        bundle_alias
                    );
                }
            } else {
                warn!(
                    "Version catalog bundle 'libs.bundles.{}' used but no catalog found",
                    bundle_alias
                );
            }
        }

        // Match: implementation("group:artifact:version")
        for cap in KOTLIN_DEP_RE.captures_iter(content) {
            let config_str = &cap[1];
            let coordinates = &cap[2];

            let config = DependencyConfiguration::from_config_str(config_str);

            if let Some(dep) = GradleDependency::from_coordinates(coordinates, config) {
                debug!("Found dependency: {} ({})", dep.coordinates(), config_str);
                project.dependencies.push(dep);
            } else {
                warn!("Failed to parse dependency coordinates: {}", coordinates);
            }
        }

        spring_boot_bom_platforms(content, project);

        // Match: implementation(platform("group:artifact:version")) - Kotlin DSL style
        for cap in KOTLIN_PLATFORM_RE.captures_iter(content) {
            let config_str = &cap[1];
            let coordinates = &cap[2];

            let config = DependencyConfiguration::from_config_str(config_str);

            if let Some(dep) =
                GradleDependency::from_coordinates_with_platform(coordinates, config, true)
            {
                debug!(
                    "Found Kotlin platform dependency: {} ({})",
                    dep.coordinates(),
                    config_str
                );
                project.dependencies.push(dep);
            } else {
                warn!(
                    "Failed to parse Kotlin platform dependency coordinates: {}",
                    coordinates
                );
            }
        }

        // Match: implementation(project(":module")) - Kotlin DSL style
        for cap in KOTLIN_PROJECT_DEP_RE.captures_iter(content) {
            let config_str = &cap[1];
            let module = &cap[2];

            let config = DependencyConfiguration::from_config_str(config_str);
            debug!(
                "Found Kotlin project dependency: {} ({})",
                module, config_str
            );

            Self::add_project_dependency(project, config, module);
        }

        // Also try Groovy-style patterns for compatibility
        Self::parse_dependencies_groovy_regex(content, project)?;
        apply_dependency_blocks(content, project);

        Ok(())
    }

    /// Parse settings file for subprojects
    fn parse_settings_file(
        path: &Path,
        project: &mut GradleProject,
        mut report: Option<&mut GradleSniffReport>,
    ) -> Result<()> {
        let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)?;

        // Strip comments to avoid false matches
        let content = strip_gradle_comments(&content);

        // Parse root project name
        if let Some(cap) = ROOT_NAME_RE.captures(&content) {
            project.name = cap[1].to_string();
            debug!("Found root project name: {}", &cap[1]);
        }

        // Extract all subproject names from various include patterns
        let subprojects = extract_include_modules(&content);
        for subproject in subprojects {
            if !project.subprojects.contains(&subproject) {
                project.subprojects.push(subproject.clone());
                debug!("Found subproject: {}", subproject);
            }
            project.subproject_dirs.insert(
                subproject.clone(),
                project.root_dir.join(relative_path_from_name(&subproject)),
            );
        }

        // Extract includeBuild for composite builds
        let included_builds = extract_include_builds(&content);
        for build_path in included_builds {
            let resolved = resolve_gradle_path_variables(&build_path, &project.root_dir);
            let absolute = if resolved.is_absolute() {
                resolved
            } else {
                project.root_dir.join(&resolved)
            };
            debug!("Found includeBuild: {}", absolute.display());
            if let Some(report) = report.as_deref_mut() {
                report.add_include_build_root(absolute.clone());
            }
            // Store in a way that can be used later (add to subproject_dirs with special marker)
            // For now, we'll treat the included build's name as its directory name.
            //
            // The directory basename alone is not a unique key: `../libs/core`
            // and `../apps/core` are different builds with the same name. The
            // second insert used to overwrite the first, and the duplicate name
            // then failed the `contains` check below, so one whole included
            // build — and every dependency it contributes — was dropped
            // silently. Fall back to the full path when the short name is
            // already claimed by a different directory.
            if let Some(name) = absolute.file_name().and_then(|n| n.to_str()) {
                let short_name = format!("@build:{}", name);
                let subproject_name = if project
                    .subproject_dirs
                    .get(&short_name)
                    .is_some_and(|existing| existing != &absolute)
                {
                    format!("@build:{}", absolute.display())
                } else {
                    short_name
                };
                project
                    .subproject_dirs
                    .insert(subproject_name.clone(), absolute);
                // Also add to subprojects list so it gets processed by build_multi_module
                if !project.subprojects.contains(&subproject_name) {
                    project.subprojects.push(subproject_name);
                }
            }
        }

        // Parse projectDir overrides
        for cap in PROJECT_DIR_RE.captures_iter(&content) {
            let Some(name_match) = cap.name("name") else {
                warn!("Skipping malformed projectDir mapping without module name");
                continue;
            };
            let Some(path_match) = cap.name("path") else {
                warn!(
                    "Skipping malformed projectDir mapping for '{}' without path",
                    name_match.as_str().trim()
                );
                continue;
            };

            let name = name_match.as_str().trim().to_string();
            let raw_path = path_match.as_str().trim();

            // Resolve common Gradle path variables
            let resolved_path = resolve_gradle_path_variables(raw_path, &project.root_dir);
            let absolute = if resolved_path.is_absolute() {
                resolved_path
            } else {
                project.root_dir.join(&resolved_path)
            };

            project
                .subproject_dirs
                .insert(name.clone(), absolute.clone());
            debug!(
                "Mapped subproject '{}' -> {} (raw: {})",
                name,
                absolute.display(),
                raw_path
            );
        }

        Ok(())
    }

    async fn parse_subprojects(
        project: &mut GradleProject,
        guard: &mut ProjectParseGuard,
    ) -> Result<()> {
        let mut visited = HashSet::new();
        let subprojects = project.subprojects.clone();

        for name in subprojects {
            if !visited.insert(name.clone()) {
                continue;
            }

            let subproject_path = project
                .subproject_dirs
                .get(&name)
                .cloned()
                .unwrap_or_else(|| project.root_dir.join(relative_path_from_name(&name)));

            if !subproject_path.exists() {
                warn!(
                    "Subproject '{}' directory does not exist at {}",
                    name,
                    subproject_path.display()
                );
                continue;
            }

            let Some(entered) = guard.enter(&subproject_path) else {
                continue;
            };

            let parent_catalog = project.version_catalog.as_ref();
            let parsed = Box::pin(Self::parse_project_nested_with_catalog(
                &subproject_path,
                parent_catalog,
                guard,
            ))
            .await;
            guard.leave(&entered);

            match parsed {
                Ok(sub) => {
                    for dep in sub.dependencies {
                        if !project
                            .dependencies
                            .iter()
                            .any(|d| d.same_declaration(&dep))
                        {
                            project.dependencies.push(dep);
                        }
                    }
                    for repo in sub.repositories {
                        if !project.repositories.contains(&repo) {
                            project.repositories.push(repo);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse Gradle subproject '{}' at {}: {}",
                        name,
                        subproject_path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
    }
}

/// A string constant the build file assigns: `def v = '1.0'`, `val v = "1.0"`,
/// `ext.v = '1.0'`, or `v = '1.0'` inside an `ext { }` block.
static STRING_CONSTANT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:(?:def|val|var)\s+|ext\.)?(\w+)\s*=\s*['"]([^'"$\\]*)['"]\s*$"#)
        .unwrap()
});
/// `${name}` or `$name` inside a string.
static STRING_INTERPOLATION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{(\w+)\}|\$(\w+)").unwrap());

/// The `key=value` (or `key: value`) properties in `<root>/gradle.properties`.
///
/// Gradle makes these project properties, so a build file can use them as `$key`.
/// A missing or unreadable file contributes nothing.
fn read_gradle_properties(root_dir: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(root_dir.join("gradle.properties")) else {
        return HashMap::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('!'))
        .filter_map(|line| {
            let separator = line.find(['=', ':'])?;
            let key = line[..separator].trim();
            let value = line[separator + 1..].trim();
            (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// Replace `${name}` and `$name` inside double-quoted strings with the literal the build
/// file assigns to `name`, so that a coordinate built from a constant can be resolved.
/// Single-quoted strings, comments and code outside strings are left as written, because
/// Groovy and Kotlin only interpolate double-quoted strings. A name with no literal
/// assignment is left as written.
fn interpolate_string_constants(content: &str, properties: &HashMap<String, String>) -> String {
    // The build file's own assignments take precedence over gradle.properties.
    let mut constants = properties.clone();
    for cap in STRING_CONSTANT_RE.captures_iter(content) {
        constants.insert(cap[1].to_string(), cap[2].to_string());
    }
    if constants.is_empty() {
        return content.to_string();
    }
    let interpolate = |text: &str| {
        STRING_INTERPOLATION_RE
            .replace_all(text, |cap: &regex::Captures| {
                let name = cap.get(1).or_else(|| cap.get(2)).map_or("", |m| m.as_str());
                constants
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| cap[0].to_string())
            })
            .into_owned()
    };
    let mut result = String::with_capacity(content.len());
    let mut copied_up_to = 0;
    for (start, end) in double_quoted_string_spans(content) {
        result.push_str(&content[copied_up_to..start]);
        result.push_str(&interpolate(&content[start..end]));
        copied_up_to = end;
    }
    result.push_str(&content[copied_up_to..]);
    result
}

/// The byte ranges of the double-quoted string literals (`"..."` and `"""..."""`) in a
/// Groovy or Kotlin build file. Single-quoted strings and comments are skipped, so a quote
/// inside them does not start a string.
fn double_quoted_string_spans(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(b"//") {
            index = content[index..]
                .find('\n')
                .map_or(bytes.len(), |offset| index + offset);
        } else if rest.starts_with(b"/*") {
            index = content[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
        } else if let Some(quote) = opening_quote(rest) {
            let end = string_literal_end(bytes, index, quote);
            if quote[0] == b'"' {
                spans.push((index, end));
            }
            index = end;
        } else {
            index += 1;
        }
    }
    spans
}

/// The quote that opens a string literal at the start of `rest`, if any.
fn opening_quote(rest: &[u8]) -> Option<&'static [u8]> {
    [b"\"\"\"" as &[u8], b"\"", b"'''", b"'"]
        .into_iter()
        .find(|quote| rest.starts_with(quote))
}

/// The end of a string literal opened at `start` with `quote`: just past its closing quote,
/// or the end of the line (end of input for a triple quote) when it is unterminated.
/// In a double-quoted literal, a `${ ... }` template expression may hold quotes of its own.
fn string_literal_end(bytes: &[u8], start: usize, quote: &[u8]) -> usize {
    let multiline = quote.len() == 3;
    let template = quote[0] == b'"';
    let mut position = start + quote.len();
    while position < bytes.len() {
        if bytes[position] == b'\\' {
            position += 2;
        } else if bytes[position..].starts_with(quote) {
            return position + quote.len();
        } else if bytes[position] == b'\n' && !multiline {
            return position;
        } else if template && bytes[position..].starts_with(b"${") {
            position = template_expression_end(bytes, position + 2, multiline);
        } else {
            position += 1;
        }
    }
    bytes.len()
}

/// The end of a `${ ... }` template expression whose code starts at `start`: just past its
/// closing brace. Inside a single-line literal, an unclosed expression ends with its line.
fn template_expression_end(bytes: &[u8], start: usize, multiline: bool) -> usize {
    let mut depth = 0_u32;
    let mut position = start;
    while position < bytes.len() {
        match bytes[position] {
            b'{' => depth += 1,
            b'}' if depth == 0 => return position + 1,
            b'}' => depth -= 1,
            b'\n' if !multiline => return position,
            _ => {
                if let Some(quote) = opening_quote(&bytes[position..]) {
                    position = string_literal_end(bytes, position, quote);
                    continue;
                }
            }
        }
        position += 1;
    }
    bytes.len()
}

/// A dependency declaration with a literal coordinate string, followed by a block:
/// `implementation 'g:a:v' {` or `implementation("g:a:v") {`.
static DEPENDENCY_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(\w+)\s*\(?\s*['"]([^'"\s:]+:[^'"\s]+)['"]\s*\)?\s*\{"#).unwrap()
});
/// A dependency declared through the version catalog, followed by a block:
/// `implementation(libs.guava) {`.
static CATALOG_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(\w+)\s*\(?\s*libs\.([a-zA-Z0-9._-]+)\s*\)?\s*\{").unwrap()
});
/// One `exclude` call inside a configuration block, with its arguments on the same line.
static EXCLUDE_RULE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bexclude\s*\(?([^)\n]*)").unwrap());
/// `group: 'g'` or `module = "m"`, in either Groovy or Kotlin syntax.
static EXCLUDE_ARGUMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(group|module)\s*[:=]\s*['"]([^'"]+)['"]"#).unwrap());
/// `transitive = false` (Groovy) or `isTransitive = false` (Kotlin).
static NON_TRANSITIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:is)?[tT]ransitive\s*=\s*false\b").unwrap());
/// `force = true` (Groovy) or `isForce = true` (Kotlin).
static FORCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:is)?[fF]orce\s*=\s*true\b").unwrap());

/// Apply the `exclude`, `transitive` and `force` settings of configuration blocks
/// to the dependencies they configure.
///
/// A block can name its coordinates as a literal string or as a version-catalog
/// alias. Any other form, such as a computed notation, cannot be resolved from the
/// build text and is not applied.
fn apply_dependency_blocks(content: &str, project: &mut GradleProject) {
    for cap in DEPENDENCY_BLOCK_RE.captures_iter(content) {
        let (Some(whole), Some(name), Some(coordinates)) = (cap.get(0), cap.get(1), cap.get(2))
        else {
            continue;
        };
        apply_dependency_block(
            content,
            whole.end() - 1,
            name.as_str(),
            coordinates.as_str(),
            project,
        );
    }
    for cap in CATALOG_BLOCK_RE.captures_iter(content) {
        let (Some(whole), Some(name), Some(alias)) = (cap.get(0), cap.get(1), cap.get(2)) else {
            continue;
        };
        // A bundle header configures each of its members. A library alias configures one.
        let Some(catalog) = project.version_catalog.as_ref() else {
            continue;
        };
        let coordinates: Vec<String> = match alias.as_str().strip_prefix("bundles.") {
            Some(bundle) => GradleParser::resolve_catalog_bundle(catalog, bundle)
                .into_iter()
                .flatten()
                .filter_map(|member| GradleParser::resolve_catalog_library(catalog, member))
                .cloned()
                .collect(),
            None => GradleParser::resolve_catalog_library(catalog, alias.as_str())
                .cloned()
                .into_iter()
                .collect(),
        };
        for coordinate in coordinates {
            apply_dependency_block(
                content,
                whole.end() - 1,
                name.as_str(),
                &coordinate,
                project,
            );
        }
    }
}

/// Apply one configuration block whose `{` is at byte offset `open`.
fn apply_dependency_block(
    content: &str,
    open: usize,
    name: &str,
    coordinates: &str,
    project: &mut GradleProject,
) {
    if !is_dependency_configuration_name(name) {
        return;
    }
    let Some(block) = block_body(content, open) else {
        return;
    };
    let Some(declared) = GradleDependency::from_coordinates(
        coordinates,
        DependencyConfiguration::from_config_str(name),
    ) else {
        return;
    };
    let exclusions = exclusion_rules(block);
    let transitive = !NON_TRANSITIVE_RE.is_match(block);
    let forced = FORCE_RE.is_match(block);
    if exclusions.is_empty() && transitive && !forced {
        return;
    }
    for dep in project
        .dependencies
        .iter_mut()
        .filter(|dep| dep.same_declaration(&declared))
    {
        for rule in &exclusions {
            if !dep.exclusions.contains(rule) {
                dep.exclusions.push(rule.clone());
            }
        }
        dep.transitive &= transitive;
        dep.forced |= forced;
    }
}

/// The text between the `{` at byte offset `open` and its matching `}`.
fn block_body(content: &str, open: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (offset, ch) in content[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[open + 1..open + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `(group, module)` pairs named by `exclude` calls in a configuration block.
///
/// A missing argument becomes `*`. An `exclude` call with neither argument is ignored.
fn exclusion_rules(block: &str) -> Vec<(String, String)> {
    let mut rules = Vec::new();
    for rule in EXCLUDE_RULE_RE.captures_iter(block) {
        let mut group = None;
        let mut module = None;
        for argument in EXCLUDE_ARGUMENT_RE.captures_iter(&rule[1]) {
            match &argument[1] {
                "group" => group = Some(argument[2].to_string()),
                "module" => module = Some(argument[2].to_string()),
                _ => {}
            }
        }
        if group.is_none() && module.is_none() {
            continue;
        }
        rules.push((
            group.unwrap_or_else(|| "*".to_string()),
            module.unwrap_or_else(|| "*".to_string()),
        ));
    }
    rules
}

fn is_dependency_configuration_name(name: &str) -> bool {
    if matches!(
        name,
        "implementation"
            | "api"
            | "compile"
            | "compileOnly"
            | "runtimeOnly"
            | "annotationProcessor"
            | "testImplementation"
            | "testCompileOnly"
            | "testRuntimeOnly"
            | "developmentOnly"
            | "classpath"
            | "kapt"
            | "provided"
    ) {
        return true;
    }

    // Plugin-defined source-set configurations follow these suffixes. The
    // static parser does not know their semantics, so recognizing them here is
    // deliberately broader than GRADLE_CONFIGS and fails closed.
    let lowercase = name.to_ascii_lowercase();
    [
        "implementation",
        "compile",
        "compileonly",
        "runtime",
        "runtimeonly",
        "classpath",
        "processor",
        "provided",
        "kapt",
    ]
    .iter()
    .any(|suffix| lowercase.ends_with(suffix))
}

fn is_modeled_core_plugin(plugin_id: &str) -> bool {
    matches!(plugin_id, "base" | "java" | "java-library" | "application")
}

/// Plugins whose effect on dependency resolution is either nil or reproduced
/// by the resolver: Spring Boot's dependency management (BOM), the Kotlin JVM
/// plugin's implicit `kotlin-stdlib`, Kotlin compiler plugins, and reporting or
/// packaging plugins that add no dependencies to any resolvable classpath.
fn is_modeled_dependency_neutral_plugin(plugin_id: &str) -> bool {
    matches!(
        plugin_id,
        "org.springframework.boot"
            | "io.spring.dependency-management"
            | "org.jetbrains.kotlin.jvm"
            | "org.jetbrains.kotlin.plugin.spring"
            | "org.jetbrains.kotlin.plugin.jpa"
            | "org.jetbrains.kotlin.plugin.allopen"
            | "org.jetbrains.kotlin.plugin.noarg"
            | "war"
            | "maven-publish"
            | "jacoco"
            | "idea"
            | "eclipse"
            | "checkstyle"
            | "pmd"
            | "signing"
    )
}

fn is_modeled_core_plugin_statement(statement: &str) -> bool {
    if let Some(capture) = KOTLIN_PLUGIN_RE.captures(statement) {
        if let Some(whole) = capture.get(0) {
            if whole.start() == 0 {
                let plugin_id = format!("org.jetbrains.kotlin.{}", &capture[1]);
                return is_modeled_dependency_neutral_plugin(&plugin_id)
                    && STATIC_KNOWN_PLUGIN_SUFFIX_RE.is_match(&statement[whole.end()..]);
            }
        }
    }
    if let Some(capture) = PLUGIN_ID_RE.captures(statement) {
        let Some(whole) = capture.get(0) else {
            return false;
        };
        return whole.start() == 0
            && (is_modeled_core_plugin(&capture[1])
                && STATIC_CORE_PLUGIN_SUFFIX_RE.is_match(&statement[whole.end()..])
                || is_modeled_dependency_neutral_plugin(&capture[1])
                    && STATIC_KNOWN_PLUGIN_SUFFIX_RE.is_match(&statement[whole.end()..]));
    }

    let token_end = statement
        .find(char::is_whitespace)
        .unwrap_or(statement.len());
    let plugin_id = statement[..token_end].trim_matches('`');
    is_modeled_core_plugin(plugin_id)
        && STATIC_CORE_PLUGIN_SUFFIX_RE.is_match(&statement[token_end..])
        || matches!(
            plugin_id,
            "war" | "maven-publish" | "jacoco" | "idea" | "eclipse"
        ) && STATIC_KNOWN_PLUGIN_SUFFIX_RE.is_match(&statement[token_end..])
}

/// Cycle and work guard for the recursive descent through included projects.
///
/// Gradle lets a build include another build that includes it back
/// (`includeBuild` composite builds are documented as mutually includable),
/// and a `project(':x').projectDir` override can point at an ancestor
/// directory. Both make the subproject walk re-enter a directory it is already
/// parsing. Without a guard that recursion never terminates: it overflows the
/// stack and aborts the process, which surfaces as a language server wedged on
/// "Resolving Build System".
///
/// `ancestors` holds the directories on the current recursion path only, so a
/// project reachable from two different parents is still parsed for each of
/// them; only genuine back-edges are cut. Paths are canonicalized because the
/// same directory is reachable under different spellings (`a/../b`) and
/// through symlinks.
struct ProjectParseGuard {
    ancestors: HashSet<PathBuf>,
    depth: usize,
    /// Remaining nested parses, so a pathological include graph cannot fan out
    /// without bound even when every individual path is acyclic.
    budget: usize,
}

/// Deep enough for any real module nesting, shallow enough to keep the async
/// recursion far away from the stack limit.
const MAX_PROJECT_NESTING_DEPTH: usize = 32;

/// Generous next to the largest real multi-module builds, and a hard stop for
/// include graphs that fan out combinatorially.
const MAX_NESTED_PROJECT_PARSES: usize = 4096;

impl ProjectParseGuard {
    fn rooted_at(root: &Path) -> Self {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Self {
            ancestors: HashSet::from([canonical]),
            depth: 0,
            budget: MAX_NESTED_PROJECT_PARSES,
        }
    }

    /// Claim `dir` for the current recursion path.
    ///
    /// Returns the canonical path to hand back to [`Self::leave`], or `None`
    /// when descending would revisit an ancestor or exhaust a limit.
    fn enter(&mut self, dir: &Path) -> Option<PathBuf> {
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

        if self.ancestors.contains(&canonical) {
            warn!(
                "Skipping Gradle project at {}: it is already being parsed further up the include chain (cyclic includeBuild or projectDir override)",
                canonical.display()
            );
            return None;
        }
        if self.depth >= MAX_PROJECT_NESTING_DEPTH {
            warn!(
                "Skipping Gradle project at {}: include nesting deeper than {} levels",
                canonical.display(),
                MAX_PROJECT_NESTING_DEPTH
            );
            return None;
        }
        if self.budget == 0 {
            warn!(
                "Skipping Gradle project at {}: exceeded the {} nested-project parse limit",
                canonical.display(),
                MAX_NESTED_PROJECT_PARSES
            );
            return None;
        }

        self.budget -= 1;
        self.depth += 1;
        self.ancestors.insert(canonical.clone());
        Some(canonical)
    }

    fn leave(&mut self, canonical: &Path) {
        self.ancestors.remove(canonical);
        self.depth = self.depth.saturating_sub(1);
    }
}

fn relative_path_from_name(name: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for segment in name.split(':') {
        if segment.is_empty() {
            continue;
        }
        path.push(segment);
    }
    path
}

/// Extract module names from various include patterns in settings.gradle.
///
/// Handles:
/// - `include 'module'` - single module
/// - `include('module')` - function call syntax
/// - `include 'a', 'b', 'c'` - comma-separated
/// - `include('a', 'b', 'c')` - comma-separated in parens
/// - Multi-line includes with various formatting
fn extract_include_modules(content: &str) -> Vec<String> {
    let mut modules = Vec::new();

    // Pattern 1: include followed by comma-separated quoted strings (with or without parens)
    // Matches: include 'a', 'b', 'c' OR include('a', 'b', 'c')
    for cap in INCLUDE_BLOCK_RE.captures_iter(content) {
        let modules_str = &cap[1];
        // Extract individual quoted strings
        for module_cap in MODULE_RE.captures_iter(modules_str) {
            let module = module_cap[1].trim_start_matches(':').to_string();
            if module.is_empty() {
                continue;
            }
            if !modules.contains(&module) {
                modules.push(module);
            }
        }
    }

    modules
}

/// Extract paths from includeBuild statements for composite builds.
///
/// Handles:
/// - `includeBuild 'path'`
/// - `includeBuild('path')`
/// - `includeBuild("${rootDir}/path")`
fn extract_include_builds(content: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Match includeBuild with or without parentheses
    for cap in INCLUDE_BUILD_RE.captures_iter(content) {
        paths.push(cap[1].to_string());
    }

    paths
}

/// Check that a static settings regex consumed the complete construct rather
/// than a literal prefix of a dynamic argument list.
fn static_settings_match_is_complete(
    content: &str,
    start: usize,
    end: usize,
    keyword: &str,
) -> bool {
    let matched = &content[start..end];
    let arguments = matched[keyword.len()..].trim_start();
    let next_non_whitespace = content[end..]
        .chars()
        .find(|character| !character.is_whitespace());
    if arguments.starts_with('(') {
        return matched.trim_end().ends_with(')') && next_non_whitespace != Some('{');
    }

    let Some(last_quote) = matched.rfind(['\'', '"']) else {
        return false;
    };
    let trailing = &matched[last_quote + 1..];
    // A trailing comma can continue with a computed argument which the static
    // regex did not consume. Reject it even across a newline.
    if trailing.trim_end().ends_with(',') {
        return false;
    }
    trailing.contains('\n') || matches!(next_non_whitespace, None | Some(';') | Some('}'))
}

/// Hide string contents before looking for unmodeled settings method calls.
/// The input is already comment-stripped and remains the same byte length, so
/// regex byte offsets and diagnostics stay stable.
fn mask_quoted_gradle_text(bytes: &mut [u8]) {
    let mut quote = None;
    let mut escaped = false;
    for byte in bytes {
        if let Some(delimiter) = quote {
            let current = *byte;
            *byte = b' ';
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == delimiter {
                quote = None;
            }
        } else if matches!(*byte, b'\'' | b'"') {
            quote = Some(*byte);
            *byte = b' ';
        }
    }
}

/// Resolve common Gradle path variables in projectDir assignments.
///
/// Handles patterns like:
/// - `${rootDir}/path` → root_dir/path
/// - `${rootProject.projectDir}/path` → root_dir/path
/// - `$rootDir/path` → root_dir/path
/// - Plain relative paths → returned as-is
fn resolve_gradle_path_variables(path: &str, root_dir: &Path) -> PathBuf {
    let mut result = path.to_string();

    // Replace ${rootDir} and ${rootProject.projectDir} with root_dir
    let root_dir_str = root_dir.to_string_lossy();

    // Handle ${rootDir}
    if result.contains("${rootDir}") {
        result = result.replace("${rootDir}", &root_dir_str);
    }

    // Handle $rootDir (without braces)
    if result.contains("$rootDir") {
        result = result.replace("$rootDir", &root_dir_str);
    }

    // Handle ${rootProject.projectDir}
    if result.contains("${rootProject.projectDir}") {
        result = result.replace("${rootProject.projectDir}", &root_dir_str);
    }

    // Handle ${project.rootDir}
    if result.contains("${project.rootDir}") {
        result = result.replace("${project.rootDir}", &root_dir_str);
    }

    PathBuf::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_build_file(content: &str) -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let build_file = temp.path().join("build.gradle");
        fs::write(&build_file, content).unwrap();
        (temp, build_file)
    }

    #[tokio::test]
    async fn test_parse_simple_groovy_build() {
        let content = r#"
            plugins {
                id 'java'
            }

            group = 'com.example'
            version = '1.0.0'

            repositories {
                mavenCentral()
            }

            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
                testImplementation 'org.junit.jupiter:junit-jupiter-api:5.10.0'
            }
        "#;

        let (temp, _) = create_test_build_file(content);
        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert_eq!(project.group, Some("com.example".to_string()));
        assert_eq!(project.version, Some("1.0.0".to_string()));
        assert_eq!(project.plugins.len(), 1);
        assert_eq!(project.plugins[0].id, "java");
        assert_eq!(project.dependencies.len(), 2);
        assert!(!project.repositories.is_empty()); // At least Maven Central
    }

    #[tokio::test]
    async fn spring_boot_bom_coordinates_constant_is_a_platform_dependency() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                import org.springframework.boot.gradle.plugin.SpringBootPlugin
                plugins { java; id("org.springframework.boot") version "3.5.6" }
                dependencies {
                    implementation(platform(SpringBootPlugin.BOM_COORDINATES))
                    implementation("org.springframework.boot:spring-boot-starter-web")
                }
            "#,
        )
        .unwrap();
        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        assert!(
            project
                .dependencies
                .iter()
                .any(|dependency| dependency.is_platform
                    && dependency.group == "org.springframework.boot"
                    && dependency.artifact == "spring-boot-dependencies"),
            "{:?}",
            project.dependencies
        );
    }

    #[tokio::test]
    async fn idiomatic_kotlin_dsl_build_produces_the_static_project_model() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                plugins {
                    java
                    application
                    `java-library`
                    id("org.springframework.boot") version "3.4.1"
                    kotlin("jvm") version "2.1.0"
                    // id("commented.out")
                    /* groovy */
                }

                group = "com.example"
                version = "1.0.0"

                repositories { mavenCentral() }
                dependencies {
                    implementation("com.google.guava:guava:33.4.0-jre")
                    implementation(platform("org.springframework.boot:spring-boot-dependencies:3.4.1"))
                    testImplementation(project(":fixtures"))
                }
            "#,
        )
        .unwrap();

        let (project, report) = GradleParser::parse_project_with_report(temp.path())
            .await
            .unwrap();

        assert_eq!(project.group.as_deref(), Some("com.example"));
        assert_eq!(project.version.as_deref(), Some("1.0.0"));
        assert!(project.is_java_project());
        assert_eq!(project.source_sets.len(), 2);
        assert!(project.has_plugin("java"));
        assert!(project.has_plugin("application"));
        assert!(project.has_plugin("java-library"));
        assert!(!project.has_plugin("commented.out"));
        assert!(!project.has_plugin("groovy"));

        let boot = project
            .plugins
            .iter()
            .find(|plugin| plugin.id == "org.springframework.boot")
            .unwrap();
        assert_eq!(boot.version.as_deref(), Some("3.4.1"));
        let kotlin = project
            .plugins
            .iter()
            .find(|plugin| plugin.id == "org.jetbrains.kotlin.jvm")
            .unwrap();
        assert_eq!(kotlin.version.as_deref(), Some("2.1.0"));

        assert!(project.dependencies.iter().any(|dependency| {
            dependency.coordinates() == "com.google.guava:guava:33.4.0-jre"
                && !dependency.is_platform
        }));
        assert!(project.dependencies.iter().any(|dependency| {
            dependency.coordinates() == "org.springframework.boot:spring-boot-dependencies:3.4.1"
                && dependency.is_platform
        }));
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::TestImplementation],
            ["fixtures"]
        );
        assert!(matches!(
            report.build_file,
            Some((_, GradleDslKind::Kotlin))
        ));
    }

    #[tokio::test]
    async fn test_parse_dependencies() {
        let content = r#"
            dependencies {
                implementation 'org.springframework:spring-core:6.0.11'
                api 'com.google.guava:guava:32.1.2-jre'
                compileOnly 'org.projectlombok:lombok:1.18.30'
                testImplementation 'org.junit.jupiter:junit-jupiter:5.10.0'
            }
        "#;

        let (_temp, build_file) = create_test_build_file(content);
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));

        GradleParser::parse_dependencies_groovy(
            &fs::read_to_string(&build_file).unwrap(),
            &mut project,
        )
        .unwrap();

        assert_eq!(project.dependencies.len(), 4);

        let impl_deps = project.get_dependencies(DependencyConfiguration::Implementation);
        assert_eq!(impl_deps.len(), 1);
        assert_eq!(impl_deps[0].group, "org.springframework");
    }

    #[tokio::test]
    async fn groovy_project_dependencies_preserve_configuration_path_and_order() {
        let content = r#"
            dependencies {
                implementation project(':core')
                implementation project(':core')
                implementation project(':utilities')
                api project(':services:auth')
                compileOnly project(':implementation')
                runtimeOnly project(':implementation')
                testImplementation project(':fixtures')
            }
        "#;
        let (temp, _) = create_test_build_file(content);

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert_eq!(project.project_dependencies.len(), 5);
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::Implementation],
            ["core", "utilities"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::Api],
            ["services:auth"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::CompileOnly],
            ["implementation"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::RuntimeOnly],
            ["implementation"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::TestImplementation],
            ["fixtures"]
        );
    }

    #[tokio::test]
    async fn groovy_configuration_blocks_set_exclusions_and_transitivity() {
        let content = r"
            dependencies {
                implementation('com.example:lib:1.0') {
                    exclude group: 'com.noisy', module: 'noise'
                    exclude group: 'org.slf4j'
                }
                implementation 'com.example:plain:2.0'
                api 'com.example:leaf:3.0' {
                    transitive = false
                }
            }
        ";
        let (temp, _) = create_test_build_file(content);

        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        let find = |artifact: &str| {
            project
                .dependencies
                .iter()
                .filter(|dep| dep.artifact == artifact)
                .collect::<Vec<_>>()
        };

        let lib = find("lib");
        assert_eq!(lib.len(), 1, "a block must not duplicate its declaration");
        assert_eq!(
            lib[0].exclusions,
            [
                ("com.noisy".to_string(), "noise".to_string()),
                ("org.slf4j".to_string(), "*".to_string()),
            ]
        );
        assert!(lib[0].transitive);

        let plain = find("plain");
        assert!(plain[0].exclusions.is_empty());
        assert!(plain[0].transitive);

        let leaf = find("leaf");
        assert!(!leaf[0].transitive);
        assert!(leaf[0].exclusions.is_empty());
    }

    #[tokio::test]
    async fn kotlin_configuration_blocks_set_exclusions_and_transitivity() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                dependencies {
                    implementation("com.example:lib:1.0") {
                        exclude(group = "com.noisy", module = "noise")
                    }
                    api("com.example:leaf:3.0") {
                        isTransitive = false
                    }
                }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        let lib = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "lib")
            .expect("lib is declared");
        assert_eq!(
            lib.exclusions,
            [("com.noisy".to_string(), "noise".to_string())]
        );
        let leaf = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "leaf")
            .expect("leaf is declared");
        assert!(!leaf.transitive);
    }

    #[tokio::test]
    async fn configuration_blocks_apply_to_version_catalog_aliases_and_force() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("gradle")).unwrap();
        fs::write(
            temp.path().join("gradle/libs.versions.toml"),
            r#"
[libraries]
guava = { module = "com.google.guava:guava", version = "32.1.2-jre" }
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                dependencies {
                    implementation(libs.guava) {
                        exclude(group = "com.noisy", module = "noise")
                    }
                    api("com.example:pinned:1.0") {
                        isForce = true
                    }
                }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        let guava = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "guava")
            .expect("guava comes from the catalog");
        assert_eq!(
            guava.exclusions,
            [("com.noisy".to_string(), "noise".to_string())]
        );
        let pinned = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "pinned")
            .expect("pinned is declared");
        assert!(pinned.forced);
        assert!(!guava.forced);
    }

    #[tokio::test]
    async fn configuration_block_on_a_bundle_applies_to_each_member() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("gradle")).unwrap();
        fs::write(
            temp.path().join("gradle/libs.versions.toml"),
            r#"
[libraries]
guava = { module = "com.google.guava:guava", version = "32.1.2-jre" }
junit = { module = "org.junit.jupiter:junit-jupiter", version = "5.10.0" }

[bundles]
testing = ["guava", "junit"]
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                dependencies {
                    implementation(libs.bundles.testing) {
                        exclude(group = "com.noisy", module = "noise")
                    }
                }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        for artifact in ["guava", "junit-jupiter"] {
            let dep = project
                .dependencies
                .iter()
                .find(|dep| dep.artifact == artifact)
                .unwrap_or_else(|| panic!("{artifact} comes from the bundle"));
            assert_eq!(
                dep.exclusions,
                [("com.noisy".to_string(), "noise".to_string())],
                "the bundle's block applies to {artifact}"
            );
        }
    }

    #[tokio::test]
    async fn groovy_dependency_coordinates_built_from_constants_resolve() {
        let content = r#"
            def guavaVersion = '32.1.2-jre'
            ext.junitVersion = '5.10.0'
            dependencies {
                implementation "com.google.guava:guava:${guavaVersion}"
                testImplementation "org.junit.jupiter:junit-jupiter:$junitVersion"
            }
        "#;
        let (temp, _) = create_test_build_file(content);

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        let guava = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "guava")
            .expect("guava is declared");
        assert_eq!(guava.version.as_deref(), Some("32.1.2-jre"));
        let junit = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "junit-jupiter")
            .expect("junit is declared");
        assert_eq!(junit.version.as_deref(), Some("5.10.0"));
    }

    fn double_quoted_literals(content: &str) -> Vec<&str> {
        double_quoted_string_spans(content)
            .into_iter()
            .map(|(start, end)| &content[start..end])
            .collect()
    }

    #[test]
    fn a_quote_inside_a_template_expression_does_not_end_the_string() {
        assert_eq!(
            double_quoted_literals(r#"a = "x:${f("a")}:$v"; b = "$w""#),
            [r#""x:${f("a")}:$v""#, r#""$w""#]
        );
        assert_eq!(
            double_quoted_literals(r#"a = "${m["k"].let { "{$it}" }}:$v" + "$w""#),
            [r#""${m["k"].let { "{$it}" }}:$v""#, r#""$w""#]
        );
        assert_eq!(
            double_quoted_literals(r#"a = "${f("it's")}:$v" + "$w""#),
            [r#""${f("it's")}:$v""#, r#""$w""#]
        );
        assert_eq!(
            double_quoted_literals("a = \"\"\"${f(\"a\")}\n$v\"\"\" + \"$w\""),
            ["\"\"\"${f(\"a\")}\n$v\"\"\"", "\"$w\""]
        );
    }

    #[test]
    fn an_unclosed_template_expression_ends_with_its_line() {
        assert_eq!(
            double_quoted_literals("a = \"${f(\nb = \"$w\""),
            ["\"${f(", "\"$w\""]
        );
    }

    #[tokio::test]
    async fn groovy_single_quoted_strings_are_not_interpolated() {
        let content = r#"
            ext.junitVersion = '5.10.0'
            dependencies {
                testImplementation 'org.junit.jupiter:junit-jupiter:$junitVersion'
            }
        "#;
        let (temp, _) = create_test_build_file(content);

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        let junit_versions: Vec<_> = project
            .dependencies
            .iter()
            .filter(|dep| dep.artifact == "junit-jupiter")
            .map(|dep| dep.version.clone())
            .collect();
        assert_eq!(junit_versions, [Some("$junitVersion".to_string())]);
    }

    #[tokio::test]
    async fn gradle_properties_supply_constants_that_the_build_file_can_override() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("gradle.properties"),
            "guavaVersion=32.1.2-jre\njunitVersion=5.10.0\n# junitVersion=0.0.0\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("build.gradle"),
            r#"
            def junitVersion = '5.11.0'
            dependencies {
                implementation "com.google.guava:guava:$guavaVersion"
                testImplementation "org.junit.jupiter:junit-jupiter:${junitVersion}"
            }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        let guava = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "guava")
            .expect("guava is declared");
        assert_eq!(guava.version.as_deref(), Some("32.1.2-jre"));
        let junit = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "junit-jupiter")
            .expect("junit is declared");
        // The build file's own assignment wins over the property of the same name.
        assert_eq!(junit.version.as_deref(), Some("5.11.0"));
    }

    #[tokio::test]
    async fn kotlin_dependency_coordinates_built_from_constants_resolve() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                val guavaVersion = "32.1.2-jre"
                dependencies {
                    implementation("com.google.guava:guava:$guavaVersion")
                    implementation("com.example:unset:${undefinedVersion}")
                }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        let guava = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "guava")
            .expect("guava is declared");
        assert_eq!(guava.version.as_deref(), Some("32.1.2-jre"));
        // A name with no literal assignment is left as written, so it stays unresolved.
        let unset = project
            .dependencies
            .iter()
            .find(|dep| dep.artifact == "unset")
            .expect("unset is declared");
        assert_eq!(unset.version.as_deref(), Some("${undefinedVersion}"));
    }

    #[tokio::test]
    async fn kotlin_project_dependencies_preserve_all_supported_configurations() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle.kts"),
            r#"
                dependencies {
                    implementation(project(":core"))
                    annotationProcessor(project(":processor"))
                    developmentOnly(project(":dev-tools"))
                }
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert_eq!(project.project_dependencies.len(), 3);
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::Implementation],
            ["core"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::AnnotationProcessor],
            ["processor"]
        );
        assert_eq!(
            project.project_dependencies[&DependencyConfiguration::DevelopmentOnly],
            ["dev-tools"]
        );
    }

    #[tokio::test]
    async fn test_parse_plugins() {
        let content = r#"
            plugins {
                id 'java'
                id 'org.springframework.boot' version '3.1.3'
            }
        "#;

        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        GradleParser::parse_plugins(content, &mut project).unwrap();

        assert_eq!(project.plugins.len(), 2);
        assert!(project.has_plugin("java"));
        assert!(project.has_plugin("org.springframework.boot"));

        let boot_plugin = project
            .plugins
            .iter()
            .find(|p| p.id == "org.springframework.boot")
            .unwrap();
        assert_eq!(boot_plugin.version, Some("3.1.3".to_string()));
    }

    #[tokio::test]
    async fn test_parse_repositories() {
        let content = r#"
            repositories {
                mavenCentral()
                mavenLocal()
                google()
            }
        "#;

        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));
        GradleParser::parse_repositories(content, &mut project).unwrap();

        assert!(project.repositories.len() >= 3);
        assert!(project
            .repositories
            .iter()
            .any(|r| r.name == "MavenCentral"));
        assert!(project.repositories.iter().any(|r| r.name == "MavenLocal"));
        assert!(project.repositories.iter().any(|r| r.name == "Google"));
    }

    #[test]
    fn test_resolve_gradle_path_variables() {
        let root = PathBuf::from("/home/user/project");

        // ${rootDir} should be replaced
        assert_eq!(
            resolve_gradle_path_variables("${rootDir}/services/auth", &root),
            PathBuf::from("/home/user/project/services/auth")
        );

        // $rootDir (no braces) should be replaced
        assert_eq!(
            resolve_gradle_path_variables("$rootDir/libs/core", &root),
            PathBuf::from("/home/user/project/libs/core")
        );

        // ${rootProject.projectDir} should be replaced
        assert_eq!(
            resolve_gradle_path_variables("${rootProject.projectDir}/shared", &root),
            PathBuf::from("/home/user/project/shared")
        );

        // ${project.rootDir} should be replaced
        assert_eq!(
            resolve_gradle_path_variables("${project.rootDir}/modules/api", &root),
            PathBuf::from("/home/user/project/modules/api")
        );

        // Plain relative path should pass through
        assert_eq!(
            resolve_gradle_path_variables("services/auth", &root),
            PathBuf::from("services/auth")
        );

        // Absolute path should pass through
        assert_eq!(
            resolve_gradle_path_variables("/absolute/path", &root),
            PathBuf::from("/absolute/path")
        );
    }

    #[tokio::test]
    async fn test_settings_with_gstring_project_dir() {
        let temp = TempDir::new().unwrap();

        // Create settings.gradle with GString projectDir
        let settings_content = r#"
            rootProject.name = 'test-project'
            include 'auth'
            project(':auth').projectDir = file("${rootDir}/services/authentication")
        "#;
        fs::write(temp.path().join("settings.gradle"), settings_content).unwrap();

        // Create the build.gradle files
        fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

        // Create the actual directory structure
        let auth_dir = temp.path().join("services").join("authentication");
        fs::create_dir_all(&auth_dir).unwrap();
        fs::write(auth_dir.join("build.gradle"), "plugins { id 'java' }").unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        // Verify the subproject was found at the correct path
        assert!(project.subprojects.contains(&"auth".to_string()));
        let mapped_path = project.subproject_dirs.get("auth");
        assert!(mapped_path.is_some(), "auth subproject should be mapped");
        assert!(
            mapped_path.unwrap().ends_with("services/authentication"),
            "Path should end with services/authentication, got: {:?}",
            mapped_path
        );
    }

    #[tokio::test]
    async fn settings_graph_audit_rejects_dynamic_or_partially_static_includes() {
        for (name, settings) in [
            (
                "computed-module",
                "def moduleName = 'api'\ninclude(moduleName)\n",
            ),
            (
                "literal-prefix",
                "def moduleName = 'api'\ninclude('core', moduleName)\n",
            ),
            ("computed-build", "includeBuild(file('../shared'))\n"),
            ("flat-module", "includeFlat 'sibling'\n"),
            (
                "computed-project-dir",
                "include 'api'\nproject(':api').projectDir = rootDir.resolve(modulePath)\n",
            ),
        ] {
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();
            fs::write(temp.path().join("settings.gradle"), settings).unwrap();

            let project = GradleParser::parse_project(temp.path()).await.unwrap();
            let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();
            assert!(
                unaccounted
                    .iter()
                    .any(|entry| entry.contains("settings project-graph construct")),
                "{name} settings must fail completeness closed: {unaccounted:?}"
            );
        }
    }

    #[tokio::test]
    async fn settings_graph_audit_accepts_only_fully_modeled_literal_forms() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();
        fs::write(
            temp.path().join("settings.gradle"),
            r#"
                rootProject.name = "include-is-only-text-here"
                include('core', 'api')
                includeBuild("${rootDir}/shared")
                project(':api').projectDir = file("${rootDir}/modules/api")
            "#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();
        let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();
        assert!(
            unaccounted.is_empty(),
            "literal settings model should be complete: {unaccounted:?}"
        );
    }

    // ============================================================
    // Tests for edge cases in settings.gradle parsing
    // ============================================================

    #[test]
    fn test_strip_gradle_comments() {
        // Single-line comment
        assert_eq!(
            strip_gradle_comments("include 'a' // this is a comment\ninclude 'b'"),
            "include 'a' \ninclude 'b'"
        );

        // Multi-line comment
        assert_eq!(
            strip_gradle_comments("include 'a' /* comment */ include 'b'"),
            "include 'a'  include 'b'"
        );

        // Comment should not strip inside strings
        assert_eq!(
            strip_gradle_comments("include '// not a comment'"),
            "include '// not a comment'"
        );

        // Multi-line comment spanning lines
        assert_eq!(
            strip_gradle_comments("include 'a'\n/* skip\nthis */\ninclude 'b'"),
            "include 'a'\n\ninclude 'b'"
        );
    }

    #[test]
    fn test_extract_include_modules_single() {
        // Single module with parens
        let modules = extract_include_modules("include('auth')");
        assert_eq!(modules, vec!["auth"]);

        // Single module without parens
        let modules = extract_include_modules("include 'auth'");
        assert_eq!(modules, vec!["auth"]);

        // With colon prefix
        let modules = extract_include_modules("include(':auth')");
        assert_eq!(modules, vec!["auth"]);
    }

    #[test]
    fn test_extract_include_modules_comma_separated() {
        // Comma-separated without parens
        let modules = extract_include_modules("include 'auth', 'core', 'api'");
        assert_eq!(modules, vec!["auth", "core", "api"]);

        // Comma-separated with parens
        let modules = extract_include_modules("include('auth', 'core', 'api')");
        assert_eq!(modules, vec!["auth", "core", "api"]);

        // Mixed quotes
        let modules = extract_include_modules(r#"include 'auth', "core", 'api'"#);
        assert_eq!(modules, vec!["auth", "core", "api"]);
    }

    #[test]
    fn test_extract_include_modules_multiline() {
        // Multi-line include block
        let content = r#"
            include(
                'auth',
                'core',
                'api'
            )
        "#;
        let modules = extract_include_modules(content);
        assert_eq!(modules, vec!["auth", "core", "api"]);

        // Multi-line without parens
        let content = r#"
            include 'auth',
                    'core',
                    'api'
        "#;
        let modules = extract_include_modules(content);
        assert_eq!(modules, vec!["auth", "core", "api"]);
    }

    #[test]
    fn test_extract_include_modules_nested() {
        // Nested module names
        let modules = extract_include_modules("include 'services:auth', 'services:api'");
        assert_eq!(modules, vec!["services:auth", "services:api"]);

        // With colons
        let modules = extract_include_modules("include(':services:auth')");
        assert_eq!(modules, vec!["services:auth"]);

        // Empty module should be ignored
        let modules = extract_include_modules("include(':')");
        assert!(modules.is_empty());
    }

    #[test]
    fn test_extract_include_modules_multiple_statements() {
        // Multiple include statements
        let content = r#"
            include 'auth'
            include 'core'
            include 'api', 'web'
        "#;
        let modules = extract_include_modules(content);
        assert!(modules.contains(&"auth".to_string()));
        assert!(modules.contains(&"core".to_string()));
        assert!(modules.contains(&"api".to_string()));
        assert!(modules.contains(&"web".to_string()));
        assert_eq!(modules.len(), 4);
    }

    #[test]
    fn test_extract_include_builds() {
        // Simple includeBuild
        let paths = extract_include_builds("includeBuild '../shared-lib'");
        assert_eq!(paths, vec!["../shared-lib"]);

        // With parens
        let paths = extract_include_builds("includeBuild('../shared-lib')");
        assert_eq!(paths, vec!["../shared-lib"]);

        // With GString
        let paths = extract_include_builds(r#"includeBuild("${rootDir}/libs/common")"#);
        assert_eq!(paths, vec!["${rootDir}/libs/common"]);

        // Multiple
        let content = r#"
            includeBuild '../lib1'
            includeBuild('../lib2')
        "#;
        let paths = extract_include_builds(content);
        assert_eq!(paths, vec!["../lib1", "../lib2"]);
    }

    #[test]
    fn test_extract_include_modules_with_comments() {
        // Comments should be stripped before extraction
        let content = strip_gradle_comments(
            r#"
            include 'auth' // authentication module
            include 'core' /* the core module */
            // include 'disabled'
            include 'api'
        "#,
        );
        let modules = extract_include_modules(&content);
        assert!(modules.contains(&"auth".to_string()));
        assert!(modules.contains(&"core".to_string()));
        assert!(modules.contains(&"api".to_string()));
        // 'disabled' should NOT be included (it's in a comment)
        assert!(!modules.contains(&"disabled".to_string()));
    }

    #[tokio::test]
    async fn test_settings_with_comma_separated_includes() {
        let temp = TempDir::new().unwrap();

        let settings_content = r#"
            rootProject.name = 'test-project'
            include 'auth', 'core', 'api'
        "#;
        fs::write(temp.path().join("settings.gradle"), settings_content).unwrap();
        fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

        // Create module directories
        for module in &["auth", "core", "api"] {
            let dir = temp.path().join(module);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("build.gradle"), "plugins { id 'java' }").unwrap();
        }

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert!(
            project.subprojects.contains(&"auth".to_string()),
            "Should find 'auth'"
        );
        assert!(
            project.subprojects.contains(&"core".to_string()),
            "Should find 'core'"
        );
        assert!(
            project.subprojects.contains(&"api".to_string()),
            "Should find 'api'"
        );
    }

    #[tokio::test]
    async fn test_settings_with_include_build() {
        let temp = TempDir::new().unwrap();

        let settings_content = r#"
            rootProject.name = 'test-project'
            include 'app'
            includeBuild '../shared-lib'
        "#;
        fs::write(temp.path().join("settings.gradle"), settings_content).unwrap();
        fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

        let app_dir = temp.path().join("app");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join("build.gradle"), "plugins { id 'java' }").unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert!(project.subprojects.contains(&"app".to_string()));
        // includeBuild should be stored with @build: prefix
        assert!(
            project.subproject_dirs.contains_key("@build:shared-lib"),
            "Should have includeBuild entry for shared-lib"
        );
    }

    /// Two included builds can share a directory basename
    /// (`../libs/core` and `../apps/core`). Keying the entry on the basename
    /// alone made the second insert overwrite the first and skip the duplicate
    /// name, silently dropping one whole build from the parse — and with it
    /// every dependency it contributes.
    #[tokio::test]
    async fn included_builds_sharing_a_directory_name_are_both_retained() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(&root).unwrap();

        for parent in ["libs", "apps"] {
            let build = temp.path().join(parent).join("core");
            fs::create_dir_all(&build).unwrap();
            fs::write(build.join("settings.gradle"), "rootProject.name = 'core'").unwrap();
            fs::write(build.join("build.gradle"), "plugins { id 'java' }").unwrap();
        }

        fs::write(
            root.join("settings.gradle"),
            r#"
            rootProject.name = 'test-project'
            includeBuild '../libs/core'
            includeBuild '../apps/core'
        "#,
        )
        .unwrap();
        fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();

        let project = GradleParser::parse_project(&root).await.unwrap();

        let included: Vec<_> = project
            .subproject_dirs
            .values()
            .filter(|dir| dir.ends_with("core"))
            .collect();
        assert_eq!(
            included.len(),
            2,
            "both included builds must be retained, got {:?}",
            project.subproject_dirs
        );
        assert_eq!(
            project
                .subprojects
                .iter()
                .filter(|name| name.starts_with("@build:"))
                .count(),
            2,
            "both included builds must be queued for parsing, got {:?}",
            project.subprojects
        );
    }

    #[tokio::test]
    async fn test_settings_with_multiline_includes() {
        let temp = TempDir::new().unwrap();

        let settings_content = r#"
            rootProject.name = 'test-project'
            include(
                'module-a',
                'module-b',
                'module-c'
            )
        "#;
        fs::write(temp.path().join("settings.gradle"), settings_content).unwrap();
        fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

        for module in &["module-a", "module-b", "module-c"] {
            let dir = temp.path().join(module);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("build.gradle"), "plugins { id 'java' }").unwrap();
        }

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert!(project.subprojects.contains(&"module-a".to_string()));
        assert!(project.subprojects.contains(&"module-b".to_string()));
        assert!(project.subprojects.contains(&"module-c".to_string()));
    }

    // BUG: Version catalog alias case sensitivity tests

    #[tokio::test]
    async fn test_version_catalog_camel_case_alias_resolution() {
        // BUG: Gradle normalizes kebab-case catalog entries to camelCase in build files
        // Catalog: spring-boot-starter-web
        // Build file: libs.springBootStarterWeb
        // Current code fails because:
        // - Catalog stores: "spring.boot.starter.web" (dashes → dots)
        // - Lookup tries: "springBootStarterWeb" (no normalization to dots)
        let temp = TempDir::new().unwrap();

        // Create version catalog with kebab-case entries
        let gradle_dir = temp.path().join("gradle");
        fs::create_dir_all(&gradle_dir).unwrap();
        fs::write(
            gradle_dir.join("libs.versions.toml"),
            r#"
[versions]
spring-boot = "3.2.0"

[libraries]
spring-boot-starter-web = { module = "org.springframework.boot:spring-boot-starter-web", version.ref = "spring-boot" }
"#,
        )
        .unwrap();

        // Build file uses camelCase (Gradle's normalized form)
        fs::write(
            temp.path().join("build.gradle"),
            r#"
plugins {
    id 'java'
}

dependencies {
    implementation(libs.springBootStarterWeb)
}
"#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        // BUG: This assertion will fail with current implementation
        // The dependency should be resolved from the catalog
        assert!(
            !project.dependencies.is_empty(),
            "BUG: camelCase catalog alias 'libs.springBootStarterWeb' should resolve to 'spring-boot-starter-web'"
        );
    }

    #[tokio::test]
    async fn test_version_catalog_dot_notation_works() {
        // Dot notation should work (this is what the parser currently normalizes to)
        let temp = TempDir::new().unwrap();

        let gradle_dir = temp.path().join("gradle");
        fs::create_dir_all(&gradle_dir).unwrap();
        fs::write(
            gradle_dir.join("libs.versions.toml"),
            r#"
[versions]
junit = "5.10.0"

[libraries]
junit-jupiter-api = { module = "org.junit.jupiter:junit-jupiter-api", version.ref = "junit" }
"#,
        )
        .unwrap();

        // Using dot notation directly (not typical but should work)
        fs::write(
            temp.path().join("build.gradle"),
            r#"
plugins {
    id 'java'
}

dependencies {
    testImplementation(libs.junit.jupiter.api)
}
"#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        // Dot notation should work because parser stores "junit.jupiter.api"
        assert!(
            !project.dependencies.is_empty(),
            "Dot notation 'libs.junit.jupiter.api' should work"
        );
    }

    #[tokio::test]
    async fn test_version_catalog_bundle_resolution() {
        let temp = TempDir::new().unwrap();

        let gradle_dir = temp.path().join("gradle");
        fs::create_dir_all(&gradle_dir).unwrap();
        fs::write(
            gradle_dir.join("libs.versions.toml"),
            r#"
[libraries]
slf4j-api = { module = "org.slf4j:slf4j-api", version = "1.7.36" }
logback-classic = { module = "ch.qos.logback:logback-classic", version = "1.2.13" }

[bundles]
logging = ["slf4j-api", "logback-classic"]
"#,
        )
        .unwrap();

        fs::write(
            temp.path().join("build.gradle"),
            r#"
plugins {
    id 'java'
}

dependencies {
    implementation libs.bundles.logging
}
"#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        let coords: std::collections::HashSet<_> = project
            .dependencies
            .iter()
            .map(|dep| dep.coordinates())
            .collect();

        assert!(
            coords.contains("org.slf4j:slf4j-api:1.7.36"),
            "Bundle should include slf4j-api"
        );
        assert!(
            coords.contains("ch.qos.logback:logback-classic:1.2.13"),
            "Bundle should include logback-classic"
        );
    }

    #[tokio::test]
    async fn test_version_ref_case_sensitivity() {
        // BUG: Version refs also have normalization issues
        // If version is "spring-boot" but ref uses "springBoot", it won't resolve
        let temp = TempDir::new().unwrap();

        let gradle_dir = temp.path().join("gradle");
        fs::create_dir_all(&gradle_dir).unwrap();

        // Version defined with kebab-case, referenced with kebab-case (should work)
        fs::write(
            gradle_dir.join("libs.versions.toml"),
            r#"
[versions]
spring-boot = "3.2.0"

[libraries]
spring-web = { module = "org.springframework.boot:spring-boot-starter-web", version.ref = "spring-boot" }
"#,
        )
        .unwrap();

        fs::write(
            temp.path().join("build.gradle"),
            r#"
plugins {
    id 'java'
}

dependencies {
    implementation(libs.spring.web)
}
"#,
        )
        .unwrap();

        let project = GradleParser::parse_project(temp.path()).await.unwrap();

        assert!(
            !project.dependencies.is_empty(),
            "Version ref with matching case should work"
        );
        if let Some(dep) = project.dependencies.first() {
            assert_eq!(dep.version, Some("3.2.0".to_string()));
        }
    }

    // Unit tests for normalize_catalog_alias

    #[test]
    fn test_normalize_catalog_alias_kebab_case() {
        assert_eq!(
            GradleParser::normalize_catalog_alias("spring-boot-starter"),
            "spring.boot.starter"
        );
        assert_eq!(
            GradleParser::normalize_catalog_alias("junit-jupiter-api"),
            "junit.jupiter.api"
        );
    }

    #[test]
    fn test_normalize_catalog_alias_camel_case() {
        assert_eq!(
            GradleParser::normalize_catalog_alias("springBootStarter"),
            "spring.boot.starter"
        );
        assert_eq!(
            GradleParser::normalize_catalog_alias("junitJupiterApi"),
            "junit.jupiter.api"
        );
    }

    #[test]
    fn test_normalize_catalog_alias_dot_notation() {
        // Dots should be preserved
        assert_eq!(
            GradleParser::normalize_catalog_alias("junit.jupiter.api"),
            "junit.jupiter.api"
        );
    }

    #[test]
    fn test_normalize_catalog_alias_mixed() {
        // Mixed: dashes and camelCase
        assert_eq!(
            GradleParser::normalize_catalog_alias("my-library.coreUtils"),
            "my.library.core.utils"
        );
    }

    #[test]
    fn test_normalize_catalog_alias_with_numbers() {
        assert_eq!(
            GradleParser::normalize_catalog_alias("slf4jApi"),
            "slf4j.api"
        );
        assert_eq!(GradleParser::normalize_catalog_alias("log4j2"), "log4j2");
    }

    #[test]
    fn test_normalize_catalog_alias_underscores() {
        // Underscores become dots (Gradle normalizes these)
        assert_eq!(
            GradleParser::normalize_catalog_alias("my_special_lib"),
            "my.special.lib"
        );
    }

    #[test]
    fn dependency_syntax_audit_rejects_dynamic_arguments() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle"),
            r#"
dependencies {
    implementation "org.example:literal:1.0"
    testImplementation(libs.junit.api)
    implementation coordinates.join(":")
    runtimeOnly "org.example:dynamic:" + runtimeVersion
}
"#,
        )
        .unwrap();
        let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());

        let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();
        assert_eq!(unaccounted.len(), 2);
        assert!(unaccounted
            .iter()
            .any(|declaration| declaration.contains("coordinates.join")));
        assert!(unaccounted
            .iter()
            .any(|declaration| declaration.contains("runtimeVersion")));
    }

    #[test]
    fn dependency_syntax_audit_recurses_into_applied_scripts_and_fails_closed() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle"),
            "apply from: 'gradle/dependencies.gradle'\n",
        )
        .unwrap();
        fs::create_dir(temp.path().join("gradle")).unwrap();
        fs::write(
            temp.path().join("gradle/dependencies.gradle"),
            "apply from: 'gradle/nested.gradle'\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("gradle/nested.gradle"),
            "dependencies { implementation 'org.example:from-script:1.0' }\n",
        )
        .unwrap();
        let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());

        let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();

        assert!(unaccounted
            .iter()
            .any(|declaration| declaration.contains("from-script")));
    }

    #[test]
    fn dependency_syntax_audit_rejects_dynamic_applied_script_paths() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle"),
            "apply from: dependencyScript\n",
        )
        .unwrap();
        let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());

        let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();

        assert!(unaccounted
            .iter()
            .any(|declaration| declaration.contains("dynamic applied-script path")));
    }

    #[test]
    fn dependency_syntax_audit_rejects_unknown_plugin_configurations_in_dependency_blocks() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("build.gradle"),
            "repositories { fooLibs 'not:a:dependency' }\ndependencies { fooLibs 'g:a:1' }\n",
        )
        .unwrap();
        let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());

        let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();

        assert_eq!(unaccounted.len(), 1);
        assert!(unaccounted[0].contains("fooLibs 'g:a:1'"));
    }

    #[test]
    fn plugin_audit_allows_only_static_modeled_core_plugins() {
        for (declaration, expected_complete) in [
            ("id 'java'", true),
            ("`java-library`", true),
            ("id(\"org.springframework.boot\") version \"3.5.6\"", true),
            (
                "id(\"io.spring.dependency-management\") version \"1.1.7\"",
                true,
            ),
            ("kotlin(\"jvm\") version \"2.2.20\" apply false", true),
            ("kotlin(\"jvm\")", true),
            ("kotlin(\"jvm\") version kotlinVersion", false),
            ("kotlin(\"kapt\")", false),
            ("id 'com.acme.codegen' version '1.0'", false),
            ("id(pluginId)", false),
            ("alias(libs.plugins.codegen)", false),
        ] {
            let temp = TempDir::new().unwrap();
            fs::write(
                temp.path().join("build.gradle"),
                format!("plugins {{ {declaration} }}"),
            )
            .unwrap();
            let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());

            let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();

            assert_eq!(
                unaccounted.is_empty(),
                expected_complete,
                "plugin declaration `{declaration}` produced {unaccounted:?}"
            );
        }

        for application in [
            "apply plugin: 'com.acme.codegen'",
            "apply plugin: selectedPlugin",
            "apply(plugin = \"java\")",
        ] {
            let temp = TempDir::new().unwrap();
            fs::write(temp.path().join("build.gradle"), application).unwrap();
            let project = GradleProject::new("audit".to_string(), temp.path().to_path_buf());
            let unaccounted = GradleParser::find_unaccounted_dependency_syntax(&project).unwrap();
            assert!(
                unaccounted
                    .iter()
                    .any(|entry| entry.contains("unmodeled plugin application")),
                "plugin application `{application}` must fail closed: {unaccounted:?}"
            );
        }
    }
}
