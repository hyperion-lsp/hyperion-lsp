// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle integration for Hyperion LSP.
//!
//! This module integrates Gradle support into the LSP server, enabling:
//! - Automatic Gradle project detection
//! - Dependency resolution from build.gradle files
//! - Classpath construction for accurate code navigation
//! - Support for multi-module Gradle projects

#[path = "gradle_archive_transaction.rs"]
mod archive_transaction;

use crate::semantic_host::SemanticHost;
use hyperion_build::gradle::{GradleClasspathBuilder, GradleParser, GradleProject, GradleResolver};
use hyperion_workspace::scanner::{generated_source_roots, WorkspaceScanner};
use hyperion_workspace::{PathLookupKey, WorkspaceIndex};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Default)]
struct OrderedPaths {
    paths: Vec<PathBuf>,
    seen: HashSet<PathLookupKey>,
}

impl OrderedPaths {
    fn push(&mut self, path: PathBuf) {
        let key = hyperion_workspace::path_lookup_key(&path);
        if self.seen.insert(key) {
            self.paths.push(path);
        }
    }

    fn contains(&self, path: &Path) -> bool {
        let key = hyperion_workspace::path_lookup_key(path);
        self.seen.contains(&key)
    }

    fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.paths.iter()
    }

    fn into_vec(self) -> Vec<PathBuf> {
        self.paths
    }
}

struct GradleClasspathAggregate {
    dependency_jars: OrderedPaths,
    compile_dependency_jars: OrderedPaths,
    runtime_dependency_jars: OrderedPaths,
    source_paths: OrderedPaths,
    complete: bool,
    structure_complete: bool,
    declared_dependencies: usize,
    resolved_dependencies: usize,
    missing_dependencies: Vec<String>,
    cache_selected_versions: usize,
    resolved_artifacts: Vec<(hyperion_build::gradle::GradleDependency, PathBuf)>,
    incomplete_reasons: BTreeSet<String>,
}

impl Default for GradleClasspathAggregate {
    fn default() -> Self {
        Self {
            dependency_jars: OrderedPaths::default(),
            compile_dependency_jars: OrderedPaths::default(),
            runtime_dependency_jars: OrderedPaths::default(),
            source_paths: OrderedPaths::default(),
            complete: true,
            structure_complete: true,
            declared_dependencies: 0,
            resolved_dependencies: 0,
            missing_dependencies: Vec::new(),
            cache_selected_versions: 0,
            resolved_artifacts: Vec::new(),
            incomplete_reasons: BTreeSet::new(),
        }
    }
}

impl GradleClasspathAggregate {
    fn absorb(
        &mut self,
        refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
        classpath: &hyperion_build::gradle::GradleClasspath,
    ) {
        self.complete &= classpath.complete;
        self.structure_complete &= classpath.structure_complete;
        for reason in &classpath.resolution.incomplete_reasons {
            // Reasons name the coordinates involved; the same problem reached
            // through several modules is reported once.
            self.incomplete_reasons.insert(reason.to_string());
        }
        if !classpath.complete && classpath.resolution.incomplete_reasons.is_empty() {
            self.incomplete_reasons.insert(format!(
                "{}: project classpath is incomplete",
                classpath.project_path
            ));
        }
        self.resolved_artifacts
            .extend(classpath.resolution.resolved_artifacts.iter().cloned());
        record_gradle_project_classpath(refresh, classpath);
        self.declared_dependencies += classpath.resolution.declared;
        self.resolved_dependencies += classpath.resolution.resolved;
        self.missing_dependencies
            .extend(classpath.resolution.missing.iter().cloned());
        self.cache_selected_versions += classpath.resolution.cache_selected_versions;

        append_mandatory_archive_paths(
            &mut self.dependency_jars,
            classpath.resolution.jar_paths.iter(),
        );
        append_mandatory_archive_paths(
            &mut self.compile_dependency_jars,
            classpath
                .compile_dependency_jars
                .iter()
                .chain(&classpath.test_compile_dependency_jars),
        );
        append_mandatory_archive_paths(
            &mut self.runtime_dependency_jars,
            classpath
                .dependency_jars
                .iter()
                .chain(&classpath.runtime_dependency_jars)
                .chain(&classpath.test_dependency_jars)
                .chain(&classpath.test_runtime_dependency_jars),
        );
        for dir in classpath
            .source_dirs
            .iter()
            .chain(&classpath.test_source_dirs)
        {
            if dir.exists() {
                self.source_paths.push(dir.clone());
            } else {
                debug!("Source dir does not exist: {:?}", dir);
            }
        }
    }
}

/// Replace the cache-derived JAR lists of one project with the exact
/// classpaths Gradle resolved (opt-in build-tool path).
fn apply_tool_classpath(
    classpath: &mut hyperion_build::gradle::GradleClasspath,
    tool: &crate::build_tool_classpath::ToolClasspaths,
) {
    use crate::build_tool_classpath::jar_entries;
    let Some(module) = tool.module_for(&classpath.project_root) else {
        return;
    };
    let main_compile = jar_entries(&module.main.compile);
    let main_runtime = jar_entries(&module.main.runtime);
    let test_compile = jar_entries(&module.test.compile);
    let test_runtime = jar_entries(&module.test.runtime);
    if main_compile.is_empty()
        && main_runtime.is_empty()
        && test_compile.is_empty()
        && test_runtime.is_empty()
    {
        return;
    }
    classpath.compile_dependency_jars.clone_from(&main_compile);
    classpath.api_dependency_jars.clone_from(&main_compile);
    classpath.dependency_jars.clone_from(&main_runtime);
    classpath.runtime_dependency_jars.clear();
    classpath
        .test_compile_dependency_jars
        .clone_from(&test_compile);
    classpath.test_api_dependency_jars.clear();
    classpath.test_dependency_jars.clone_from(&test_runtime);
    classpath.test_runtime_dependency_jars.clear();

    let mut all: Vec<PathBuf> = Vec::new();
    for jar in main_compile
        .iter()
        .chain(&main_runtime)
        .chain(&test_compile)
        .chain(&test_runtime)
    {
        if !all.contains(jar) {
            all.push(jar.clone());
        }
    }
    // The build tool's answer is authoritative: no cache-derived gaps remain.
    classpath.resolution = hyperion_build::gradle::GradleResolutionReport {
        complete: true,
        jar_paths: all,
        main_jars: main_runtime.clone(),
        test_jars: test_runtime.clone(),
        main_compile_jars: main_compile,
        main_runtime_jars: main_runtime,
        test_compile_jars: test_compile,
        test_runtime_jars: test_runtime,
        ..hyperion_build::gradle::GradleResolutionReport::default()
    };
    classpath.complete = classpath.structure_complete;
}

fn append_mandatory_archive_paths<'a>(
    target: &mut OrderedPaths,
    paths: impl Iterator<Item = &'a PathBuf>,
) {
    for path in paths {
        // Missing paths are intentional: exact archive staging must reject the
        // generation instead of allowing collection to truncate it silently.
        target.push(path.clone());
    }
}

const fn gradle_execution_model_is_complete(
    classpath_snapshot_ready: bool,
    every_classpath_complete: bool,
    missing_dependencies: usize,
    heuristic_model_complete: bool,
) -> bool {
    classpath_snapshot_ready
        && every_classpath_complete
        && missing_dependencies == 0
        && heuristic_model_complete
}

fn gradle_incomplete_summary(
    primary_reasons: &BTreeSet<String>,
    details: &BTreeSet<String>,
) -> String {
    const MAX_REASONS: usize = 5;
    const MAX_REASON_CHARS: usize = 160;

    let reasons: Vec<&String> = primary_reasons
        .iter()
        .chain(details.difference(primary_reasons))
        .collect();
    let mut parts: Vec<String> = reasons
        .iter()
        .take(MAX_REASONS)
        .map(|reason| {
            let mut chars = reason.chars();
            let bounded: String = chars.by_ref().take(MAX_REASON_CHARS).collect();
            if chars.next().is_some() {
                format!("{bounded}…")
            } else {
                bounded
            }
        })
        .collect();
    if reasons.len() > MAX_REASONS {
        parts.push(format!(
            "{} more causes in logs",
            reasons.len() - MAX_REASONS
        ));
    }
    parts.join("; ")
}

fn add_conventional_jvm_source_dirs(
    project_name: &str,
    project_dir: &Path,
    source_paths: &mut OrderedPaths,
) {
    for source_set in ["main", "test"] {
        for dir in hyperion_layout::existing_source_dirs(project_dir, source_set) {
            if !source_paths.contains(&dir) {
                info!(
                    "Adding standard source dir for '{}': {:?}",
                    project_name, dir
                );
                source_paths.push(dir);
            }
        }
    }
}

/// Result type for Gradle integration operations
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Guards and execution snapshot that may cross the destructive external-
/// membership boundary only after Gradle proved its dependency model complete.
struct CompleteGradlePublicationAttempt {
    source_refresh: Option<crate::source_provider::SourceProviderRefreshGuard>,
    classpath_refresh: crate::project_classpath::ProjectClasspathRefreshGuard,
    classpath_snapshot: (Vec<PathBuf>, Vec<PathBuf>),
}

fn complete_gradle_publication_attempt(
    classpath_snapshot: Option<(Vec<PathBuf>, Vec<PathBuf>)>,
    source_refresh: Option<crate::source_provider::SourceProviderRefreshGuard>,
    classpath_refresh: crate::project_classpath::ProjectClasspathRefreshGuard,
) -> Result<Option<CompleteGradlePublicationAttempt>> {
    let Some(classpath_snapshot) = classpath_snapshot else {
        // Classpath construction itself failed: nothing trustworthy exists to
        // publish. Neither guard has crossed a WorkspaceIndex commit, so
        // dropping them removes only private staging and leaves the previous
        // publication live.
        drop(source_refresh);
        drop(classpath_refresh);
        return Ok(None);
    };
    Ok(Some(CompleteGradlePublicationAttempt {
        source_refresh,
        classpath_refresh,
        classpath_snapshot,
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GradleClasspathPublication {
    Published,
    Unavailable,
}

/// Gradle project manager for LSP
pub struct GradleIntegration {
    /// Root directory of the workspace
    workspace_root: PathBuf,

    project_classpaths: crate::project_classpath::ProjectClasspathStore,

    /// Gradle project (if parsed)
    project: Option<GradleProject>,

    /// Dependency resolver
    resolver: GradleResolver,

    /// Classpath builder
    classpath_builder: GradleClasspathBuilder,

    /// Complete dependency vectors staged until the matching WorkspaceIndex
    /// classpath transaction commits.
    pending_classpath_snapshot: Option<(Vec<PathBuf>, Vec<PathBuf>)>,
    /// False when classpath construction or dependency resolution produced a
    /// known partial graph. Partial symbols remain useful for navigation, but
    /// destructive refactoring must not treat them as exhaustive.
    dependency_model_complete: bool,
    /// True when the shape of the build (modules, source roots, ownership) is
    /// fully understood. Missing external JARs do not clear this: symbols
    /// declared in workspace sources are still found exhaustively.
    refactor_ready: bool,
    /// Per-dependency problems (unresolvable coordinates, unreadable POMs),
    /// reported to the user but never blocking.
    dependency_issues: BTreeSet<String>,
    /// Number of dependency JARs handed to the index.
    dependency_jar_count: usize,
    incomplete_reasons: BTreeSet<String>,
    primary_incomplete_reasons: BTreeSet<String>,

    #[cfg(test)]
    inject_multi_module_classpath_failure: bool,
}

impl GradleIntegration {
    /// Create a new Gradle integration instance
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_project_classpaths(
            workspace_root,
            crate::project_classpath::compatibility_store(),
        )
    }

    pub(crate) fn with_project_classpaths(
        workspace_root: PathBuf,
        project_classpaths: crate::project_classpath::ProjectClasspathStore,
    ) -> Self {
        Self {
            workspace_root,
            project_classpaths,
            project: None,
            resolver: GradleResolver::new(),
            classpath_builder: GradleClasspathBuilder::new(),
            pending_classpath_snapshot: None,
            dependency_model_complete: true,
            refactor_ready: true,
            dependency_issues: BTreeSet::new(),
            dependency_jar_count: 0,
            incomplete_reasons: BTreeSet::new(),
            primary_incomplete_reasons: BTreeSet::new(),
            #[cfg(test)]
            inject_multi_module_classpath_failure: false,
        }
    }

    /// Check if this is a Gradle workspace
    pub fn is_gradle_workspace(&self) -> bool {
        [
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ]
        .iter()
        .any(|name| self.workspace_root.join(name).is_file())
    }

    async fn require_supported_workspace_layout(&self) -> Result<()> {
        if crate::handlers::build_system::has_nested_build_manifest_or_unknown(
            &self.workspace_root,
            crate::handlers::build_system::BuildManifestFamily::Gradle,
        )
        .await
        {
            return Err(format!(
                "workspace {} contains a nested or uninspectable Gradle build, but no root Gradle model",
                self.workspace_root.display()
            )
            .into());
        }
        Ok(())
    }

    /// Initialize Gradle support for the workspace
    ///
    /// This method:
    /// 1. Parses the Gradle project
    /// 2. Resolves dependencies
    /// 3. Builds classpaths
    /// 4. Returns (source paths, jar paths, javadoc jar paths) that should be indexed
    pub async fn initialize(
        &mut self,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        if !self.is_gradle_workspace() {
            self.require_supported_workspace_layout().await?;
            debug!("Not a Gradle workspace, skipping Gradle initialization");
            return Ok((Vec::new(), Vec::new(), Vec::new()));
        }
        let refresh = self
            .project_classpaths
            .begin_standalone_project_classpath_refresh_guard(&self.workspace_root);
        let result = self.initialize_for_refresh(refresh.ticket(), None).await?;
        let plan_jars = result.1.clone();
        let plan_docs = result.2.clone();
        let archives: Vec<_> = tokio::task::spawn_blocking(move || {
            crate::source_provider::capture_dependency_archive_plan(&plan_jars, &plan_docs)
                .map(|plans| plans.into_iter().map(|plan| plan.archive).collect())
        })
        .await
        .map_err(|error| format!("Gradle archive capture task failed: {error}"))??;
        if self.publish_pending_classpath_snapshot(refresh.ticket(), &archives)?
            == GradleClasspathPublication::Published
        {
            refresh.disarm();
        }
        Ok(result)
    }

    async fn initialize_for_refresh(
        &mut self,
        refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
        progress: Option<&crate::progress::ProgressReporter>,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        if !self.is_gradle_workspace() {
            self.require_supported_workspace_layout().await?;
            debug!("Not a Gradle workspace, skipping Gradle initialization");
            return Ok((Vec::new(), Vec::new(), Vec::new()));
        }

        self.pending_classpath_snapshot = None;
        self.dependency_model_complete = false;
        self.refactor_ready = false;
        self.dependency_issues.clear();
        self.dependency_jar_count = 0;
        self.incomplete_reasons.clear();
        self.primary_incomplete_reasons.clear();

        info!("Initializing Gradle support...");

        if let Some(progress) = progress {
            progress
                .report(
                    Some("Parsing Gradle projects and inspecting plugins...".into()),
                    None,
                )
                .await;
        }

        // Parse Gradle project and retain the report: partial plugin execution
        // or syntax the static model did not account for is useful for
        // navigation, but cannot prove a destructive refactoring exhaustive.
        let (project, sniff_report) =
            GradleParser::parse_project_with_report(&self.workspace_root).await?;
        let unaccounted_dependency_syntax =
            GradleParser::find_unaccounted_dependency_syntax(&project)?;
        let plugin_model_complete = sniff_report.plugin_resolutions.iter().all(|plugin| {
            matches!(
                &plugin.outcome,
                hyperion_build::gradle::sniff_report::SniffOutcome::Executed
            )
        });
        let vm_model_complete = sniff_report.vm_summary.as_ref().is_none_or(|summary| {
            !summary.instruction_limit_hit
                && !summary.timed_out
                && !summary.memory_limit_hit
                && !summary.stack_limit_hit
                && !summary.aborted
        });
        if !unaccounted_dependency_syntax.is_empty() {
            self.primary_incomplete_reasons.insert(format!(
                "{} unaccounted dependency or build statements",
                unaccounted_dependency_syntax.len()
            ));
            warn!(
                "Gradle static model left {} dependency declarations unaccounted; destructive refactoring readiness will remain false",
                unaccounted_dependency_syntax.len()
            );
            for statement in &unaccounted_dependency_syntax {
                debug!("Unaccounted Gradle dependency syntax: {statement}");
            }
        }
        if !plugin_model_complete || !vm_model_complete {
            warn!(
                "Gradle plugin model is partial; destructive refactoring readiness will remain false"
            );
        }
        for plugin in &sniff_report.plugin_resolutions {
            if !matches!(
                plugin.outcome,
                hyperion_build::gradle::sniff_report::SniffOutcome::Executed
            ) {
                self.incomplete_reasons
                    .insert(format!("plugin {}: {}", plugin.plugin_id, plugin.outcome));
            }
        }
        if !plugin_model_complete {
            self.primary_incomplete_reasons
                .insert("plugin model is incomplete".into());
        }
        if !vm_model_complete {
            if let Some(summary) = &sniff_report.vm_summary {
                for (hit, limit) in [
                    (summary.instruction_limit_hit, "instruction limit"),
                    (summary.timed_out, "time limit"),
                    (summary.memory_limit_hit, "memory limit"),
                    (summary.stack_limit_hit, "stack limit"),
                    (summary.aborted, "aborted"),
                ] {
                    if hit {
                        self.primary_incomplete_reasons
                            .insert(format!("build-model VM: {limit}"));
                    }
                }
            }
        }
        let heuristic_model_complete =
            unaccounted_dependency_syntax.is_empty() && plugin_model_complete && vm_model_complete;

        info!(
            "Parsed Gradle project '{}' with {} dependencies, {} subprojects",
            project.name,
            project.dependencies.len(),
            project.subprojects.len()
        );
        stage_gradle_project_ownership(refresh, &project);
        for subproject in &project.subprojects {
            if let Some(dir) = project.subproject_dirs.get(subproject) {
                info!("  - Subproject: {} at {:?}", subproject, dir);
            } else {
                warn!("  - Subproject: {} (no directory found!)", subproject);
            }
        }

        // Collect all paths to index
        let mut paths_to_index = Vec::new();
        let mut jar_paths = Vec::new();

        let mut source_set_dirs = Vec::new();
        for source_set in &project.source_sets {
            for java_dir in &source_set.java_dirs {
                if java_dir.exists() {
                    source_set_dirs.push(java_dir.clone());
                    debug!("Will index source directory: {:?}", java_dir);
                }
            }
        }

        if let Some(progress) = progress {
            progress
                .report(
                    Some(format!(
                        "Resolving Gradle classpaths for {} projects...",
                        project.subprojects.len() + 1
                    )),
                    None,
                )
                .await;
        }

        // Opt-in: ask Gradle itself for the exact classpaths. This executes the
        // project's build, so it only happens when the user enabled it.
        let mut tool_issues: Vec<String> = Vec::new();
        let tool_classpaths = if crate::build_tool_classpath::is_enabled() {
            if let Some(progress) = progress {
                progress
                    .report(
                        Some("Asking Gradle for the resolved classpaths...".into()),
                        None,
                    )
                    .await;
            }
            match crate::build_tool_classpath::gradle_classpaths(&self.workspace_root).await {
                Ok(tool) => {
                    tool_issues.extend(tool.problems.iter().cloned());
                    Some(tool)
                }
                Err(error) => {
                    warn!("Gradle classpath resolution failed: {error}");
                    tool_issues.push(format!("Gradle classpath resolution failed: {error}"));
                    None
                }
            }
        } else {
            None
        };

        // Build classpath and collect JAR paths
        let mut aggregate = GradleClasspathAggregate::default();
        let mut classpath_snapshot_ready = true;
        let mut structure_complete = true;

        if project.subprojects.is_empty() {
            match self.classpath_builder.build(&project).await {
                Ok(mut classpath) => {
                    if let Some(tool) = &tool_classpaths {
                        apply_tool_classpath(&mut classpath, tool);
                    }
                    if !classpath.structure_complete {
                        structure_complete = false;
                    }
                    if !classpath.complete {
                        warn!(
                            "Gradle dependency resolution is degraded; publishing the JARs that did resolve"
                        );
                    }
                    info!(
                        "Built classpath with {} main entries, {} test entries",
                        classpath.main_classpath().len(),
                        classpath.test_classpath().len()
                    );
                    stage_gradle_source_scopes(refresh, std::slice::from_ref(&classpath));
                    aggregate.absorb(refresh, &classpath);
                }
                Err(e) => {
                    warn!("Failed to build classpath: {}", e);
                    self.primary_incomplete_reasons
                        .insert(format!("root project: classpath build failed: {e}"));
                    classpath_snapshot_ready = false;
                    // Continue anyway - we can still index sources
                }
            }
        } else {
            match self.build_multi_module_classpaths(&project).await {
                Ok(mut classpaths) => {
                    if let Some(tool) = &tool_classpaths {
                        for classpath in &mut classpaths {
                            apply_tool_classpath(classpath, tool);
                        }
                    }
                    info!(
                        "Built {} classpaths for multi-module project",
                        classpaths.len()
                    );
                    let expected_classpaths = project.subprojects.len() + 1;
                    if classpaths.len() != expected_classpaths
                        || classpaths
                            .iter()
                            .any(|classpath| !classpath.structure_complete)
                    {
                        structure_complete = false;
                        self.primary_incomplete_reasons.insert(format!(
                            "built {} of {expected_classpaths} structurally complete project classpaths",
                            classpaths
                                .iter()
                                .filter(|classpath| classpath.structure_complete)
                                .count()
                        ));
                        warn!(
                            "Gradle module structure is incomplete: {} of {} project classpaths are structurally complete",
                            classpaths
                                .iter()
                                .filter(|classpath| classpath.structure_complete)
                                .count(),
                            expected_classpaths,
                        );
                    }
                    stage_gradle_source_scopes(refresh, &classpaths);
                    for classpath in classpaths {
                        info!(
                            "Classpath '{}': {} source dirs, {} test source dirs, {} main entries, {} test entries",
                            classpath.project_name,
                            classpath.source_dirs.len(),
                            classpath.test_source_dirs.len(),
                            classpath.main_classpath().len(),
                            classpath.test_classpath().len()
                        );
                        for dir in &classpath.source_dirs {
                            info!("    Source dir: {:?}", dir);
                        }
                        for dir in &classpath.test_source_dirs {
                            info!("    Test source dir: {:?}", dir);
                        }

                        aggregate.absorb(refresh, &classpath);
                    }
                }
                Err(e) => {
                    warn!("Failed to build multi-module classpath: {}", e);
                    self.primary_incomplete_reasons
                        .insert(format!("multi-module classpath build failed: {e}"));
                    classpath_snapshot_ready = false;
                }
            }
        }

        for dir in source_set_dirs {
            aggregate.source_paths.push(dir);
        }

        // Build-generated source is usually gitignored and lives below a
        // directory excluded by the root workspace scan. Add the conventional
        // parent once per Gradle module; the source scanner filters by JVM
        // source extension beneath it.
        for project_dir in
            std::iter::once(&project.root_dir).chain(project.subproject_dirs.values())
        {
            for dir in generated_source_roots(project_dir) {
                info!("Adding generated source root: {:?}", dir);
                aggregate.source_paths.push(dir);
            }
        }

        // FALLBACK: Check every conventional JVM language root even when the
        // static Gradle model or classpath construction is partial. This also
        // covers Kotlin/Groovy-only root projects, whose SourceSet model is
        // otherwise expressed in terms of `java_dirs`.
        add_conventional_jvm_source_dirs(
            &project.name,
            &project.root_dir,
            &mut aggregate.source_paths,
        );
        for (subproject_name, subproject_dir) in &project.subproject_dirs {
            add_conventional_jvm_source_dirs(
                subproject_name,
                subproject_dir,
                &mut aggregate.source_paths,
            );
        }

        // Every resolver-declared binary is mandatory. Preserve missing paths
        // so immutable archive staging fails the generation instead of
        // publishing a silently truncated index/classpath. Include project-
        // only compile/runtime entries as well as the aggregate resolution.
        jar_paths.extend(aggregate.dependency_jars.iter().cloned());
        jar_paths.extend(aggregate.compile_dependency_jars.iter().cloned());
        jar_paths.extend(aggregate.runtime_dependency_jars.iter().cloned());
        let mut seen_jars = HashSet::new();
        jar_paths.retain(|path| seen_jars.insert(hyperion_workspace::path_lookup_key(path)));
        paths_to_index.extend(aggregate.source_paths.into_vec());

        info!(
            "Found {} dependency JARs and {} source directories to index",
            jar_paths.len(),
            paths_to_index.len()
        );
        info!(
            "Gradle dependency accounting: declared={}, resolved={}, missing={}, cache_selected_versions={}, jar_paths_to_index={}",
            aggregate.declared_dependencies,
            aggregate.resolved_dependencies,
            aggregate.missing_dependencies.len(),
            aggregate.cache_selected_versions,
            jar_paths.len()
        );
        if !aggregate.missing_dependencies.is_empty() {
            debug!(
                "Missing Gradle dependencies: {:?}",
                aggregate.missing_dependencies
            );
        }

        // Try to resolve javadoc JARs for dependencies
        info!("Resolving javadoc JARs for dependencies...");
        let jars_with_docs = self
            .resolver
            .resolve_artifact_docs(&aggregate.resolved_artifacts)
            .await;
        info!(
            "Resolved {} dependencies with javadoc",
            jars_with_docs.len()
        );

        self.project = Some(project);

        info!(
            "Gradle initialization complete, {} source paths, {} JAR paths, {} with javadoc to index",
            paths_to_index.len(),
            jar_paths.len(),
            jars_with_docs.len()
        );

        self.dependency_model_complete = gradle_execution_model_is_complete(
            classpath_snapshot_ready,
            aggregate.complete,
            aggregate.missing_dependencies.len(),
            heuristic_model_complete,
        );
        self.refactor_ready =
            classpath_snapshot_ready && structure_complete && heuristic_model_complete;
        self.dependency_jar_count = jar_paths.len();
        self.dependency_issues.extend(aggregate.incomplete_reasons);
        self.dependency_issues.extend(tool_issues);
        for missing in &aggregate.missing_dependencies {
            // Skip declarations an explicit reason already names.
            if !self
                .dependency_issues
                .iter()
                .any(|issue| issue.contains(missing.as_str()))
            {
                self.dependency_issues
                    .insert(format!("unresolved dependency {missing}"));
            }
        }
        self.incomplete_reasons
            .extend(self.dependency_issues.iter().cloned());
        self.incomplete_reasons
            .extend(self.primary_incomplete_reasons.iter().cloned());
        if !self.dependency_model_complete && self.incomplete_reasons.is_empty() {
            self.incomplete_reasons
                .insert("dependency model is incomplete".into());
        }
        for reason in &self.incomplete_reasons {
            warn!("Gradle model incomplete: {reason}");
        }
        if classpath_snapshot_ready {
            // Publish whatever resolved. Each unresolved dependency is named in
            // `dependency_issues`; the rest of the classpath stays usable.
            self.pending_classpath_snapshot = Some((
                aggregate.compile_dependency_jars.into_vec(),
                aggregate.runtime_dependency_jars.into_vec(),
            ));
        } else {
            warn!("Gradle classpath snapshot unavailable: classpath construction failed");
        }
        Ok((paths_to_index, jar_paths, jars_with_docs))
    }

    fn publish_pending_classpath_snapshot(
        &mut self,
        refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
        archives: &[hyperion_workspace::ExternalSourceArchive],
    ) -> Result<GradleClasspathPublication> {
        let prepared = prepare_gradle_classpath_snapshot(
            self.pending_classpath_snapshot.take(),
            refresh,
            archives,
        )?;
        publish_prepared_gradle_classpath_snapshot(prepared, refresh)
    }

    /// Get the parsed Gradle project
    pub const fn project(&self) -> Option<&GradleProject> {
        self.project.as_ref()
    }

    /// Refresh Gradle project (call when build files change)
    pub async fn refresh(
        &mut self,
    ) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        info!("Refreshing Gradle project...");
        self.project = None;
        self.initialize().await
    }

    async fn build_multi_module_classpaths(
        &self,
        project: &GradleProject,
    ) -> std::result::Result<
        Vec<hyperion_build::gradle::GradleClasspath>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        #[cfg(test)]
        if self.inject_multi_module_classpath_failure {
            return Err("injected multi-module classpath failure".into());
        }

        self.classpath_builder.build_multi_module(project).await
    }

    #[cfg(test)]
    const fn inject_multi_module_classpath_failure(&mut self) {
        self.inject_multi_module_classpath_failure = true;
    }
}

type PreparedGradleClasspathSnapshot = (
    Vec<PathBuf>,
    Vec<PathBuf>,
    Vec<hyperion_workspace::ExternalSourceArchive>,
);

fn prepare_gradle_classpath_snapshot(
    pending_classpath_snapshot: Option<(Vec<PathBuf>, Vec<PathBuf>)>,
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
    archives: &[hyperion_workspace::ExternalSourceArchive],
) -> Result<Option<PreparedGradleClasspathSnapshot>> {
    let Some((compile_jars, runtime_jars)) = pending_classpath_snapshot else {
        return Ok(None);
    };
    crate::project_classpath::bind_staged_gradle_classpaths_to_archives_for_refresh(
        refresh,
        compile_jars,
        runtime_jars,
        archives,
    )
    .map(Some)
    .map_err(Into::into)
}

fn publish_prepared_gradle_classpath_snapshot(
    prepared: Option<PreparedGradleClasspathSnapshot>,
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
) -> Result<GradleClasspathPublication> {
    let (publication, retirement) =
        publish_prepared_gradle_classpath_snapshot_deferred(prepared, refresh)?;
    drop(retirement);
    Ok(publication)
}

fn publish_prepared_gradle_classpath_snapshot_deferred(
    prepared: Option<PreparedGradleClasspathSnapshot>,
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
) -> Result<(
    GradleClasspathPublication,
    Option<crate::project_classpath::DeferredClasspathRetirement>,
)> {
    let Some((compile_jars, runtime_jars, archives)) = prepared else {
        return Ok((GradleClasspathPublication::Unavailable, None));
    };
    let Some(retirement) = crate::project_classpath::publish_gradle_dependency_snapshot_with_archives_deferred_for_refresh(
        refresh,
        compile_jars,
        runtime_jars,
        archives,
    ) else {
        return Err("Gradle classpath refresh was superseded".into());
    };
    Ok((GradleClasspathPublication::Published, Some(retirement)))
}

fn record_gradle_project_classpath(
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
    classpath: &hyperion_build::gradle::GradleClasspath,
) {
    if classpath.project_root.as_os_str().is_empty() {
        return;
    }
    if !crate::project_classpath::record_gradle_resolution_for_refresh(
        refresh,
        &classpath.project_root,
        classpath.resolution.clone(),
    ) {
        return;
    }
    let mut main_outputs = classpath.output_dirs.clone();
    main_outputs.extend(classpath.dependency_output_dirs.iter().cloned());
    let mut main_compile_outputs = classpath.output_dirs.clone();
    main_compile_outputs.extend(classpath.compile_dependency_output_dirs.iter().cloned());
    let main_compile_jars = classpath.compile_dependency_jars.clone();
    let mut main_runtime_jars = classpath.dependency_jars.clone();
    main_runtime_jars.extend(classpath.runtime_dependency_jars.iter().cloned());
    info!(
        "Project {} classpath: {} compile JARs, {} runtime JARs, {} test compile JARs, {} test runtime JARs",
        classpath.project_path,
        main_compile_jars.len(),
        main_runtime_jars.len(),
        classpath.compile_dependency_jars.len() + classpath.test_compile_dependency_jars.len(),
        classpath.dependency_jars.len()
            + classpath.runtime_dependency_jars.len()
            + classpath.test_dependency_jars.len()
            + classpath.test_runtime_dependency_jars.len(),
    );
    debug!(
        "Project {} runtime JARs: {:?}",
        classpath.project_path,
        main_runtime_jars
            .iter()
            .filter_map(|jar| jar.file_name())
            .collect::<Vec<_>>()
    );
    if !crate::project_classpath::record_project_classpath_scoped_for_refresh(
        refresh,
        &classpath.project_root,
        "main",
        main_outputs.clone(),
        main_compile_outputs.clone(),
        main_compile_jars,
        main_runtime_jars,
    ) {
        return;
    }

    let mut test_outputs = classpath.test_output_dirs.clone();
    test_outputs.extend(classpath.test_dependency_output_dirs.iter().cloned());
    test_outputs.extend(main_outputs);
    let mut test_compile_outputs = classpath.test_output_dirs.clone();
    test_compile_outputs.extend(
        classpath
            .test_compile_dependency_output_dirs
            .iter()
            .cloned(),
    );
    test_compile_outputs.extend(main_compile_outputs);
    let mut test_compile_jars = classpath.compile_dependency_jars.clone();
    test_compile_jars.extend(classpath.test_compile_dependency_jars.iter().cloned());
    let mut test_runtime_jars = classpath.dependency_jars.clone();
    test_runtime_jars.extend(classpath.runtime_dependency_jars.iter().cloned());
    test_runtime_jars.extend(classpath.test_dependency_jars.iter().cloned());
    test_runtime_jars.extend(classpath.test_runtime_dependency_jars.iter().cloned());
    let _published = crate::project_classpath::record_project_classpath_scoped_for_refresh(
        refresh,
        &classpath.project_root,
        "test",
        test_outputs,
        test_compile_outputs,
        test_compile_jars,
        test_runtime_jars,
    );
}

/// Translate Gradle's ordered compile outputs back to their source roots.
/// Keeping this beside the classpath publication ensures a navigation request
/// never observes a different dependency graph from the indexed generation.
fn stage_gradle_source_scopes(
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
    classpaths: &[hyperion_build::gradle::GradleClasspath],
) {
    use std::collections::HashMap;

    let mut output_sources: HashMap<PathLookupKey, Vec<PathBuf>> = HashMap::new();
    for classpath in classpaths {
        let main = source_roots(&classpath.project_root, "main", &classpath.source_dirs);
        let test = source_roots(&classpath.project_root, "test", &classpath.test_source_dirs);
        for output in &classpath.output_dirs {
            output_sources.insert(hyperion_workspace::path_lookup_key(output), main.clone());
        }
        for output in &classpath.test_output_dirs {
            output_sources.insert(hyperion_workspace::path_lookup_key(output), test.clone());
        }
    }

    for classpath in classpaths {
        let main = source_roots(&classpath.project_root, "main", &classpath.source_dirs);
        let test = source_roots(&classpath.project_root, "test", &classpath.test_source_dirs);
        let mut main_visible = OrderedPaths::default();
        for root in &main {
            main_visible.push(root.clone());
        }
        for output in &classpath.compile_dependency_output_dirs {
            if let Some(roots) = output_sources.get(&hyperion_workspace::path_lookup_key(output)) {
                for root in roots {
                    main_visible.push(root.clone());
                }
            }
        }
        let main_visible = main_visible.into_vec();
        let _ = crate::project_classpath::record_source_scope_for_refresh(
            refresh,
            &classpath.project_root,
            "main",
            main,
            main_visible.clone(),
            classpath.compile_dependency_jars.clone(),
        );

        let mut test_visible = OrderedPaths::default();
        for root in &test {
            test_visible.push(root.clone());
        }
        for root in &main_visible {
            test_visible.push(root.clone());
        }
        for output in &classpath.test_compile_dependency_output_dirs {
            if let Some(roots) = output_sources.get(&hyperion_workspace::path_lookup_key(output)) {
                for root in roots {
                    test_visible.push(root.clone());
                }
            }
        }
        let _ = crate::project_classpath::record_source_scope_for_refresh(
            refresh,
            &classpath.project_root,
            "test",
            test,
            test_visible.into_vec(),
            classpath
                .compile_dependency_jars
                .iter()
                .chain(&classpath.test_compile_dependency_jars)
                .cloned()
                .collect(),
        );

        let unclassified = generated_source_roots(&classpath.project_root);
        if !unclassified.is_empty() {
            // The scanner deliberately indexes broad generated parents. Only
            // modeled or conventional source-set children below them have a
            // proven compile classpath; an unknown generator path fails closed.
            let _ = crate::project_classpath::record_source_scope_for_refresh(
                refresh,
                &classpath.project_root,
                "unclassified-generated",
                unclassified,
                Vec::new(),
                Vec::new(),
            );
        }
    }
}

fn source_roots(project_root: &Path, source_set: &str, configured: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = OrderedPaths::default();
    for root in configured {
        roots.push(root.clone());
    }
    for root in hyperion_layout::existing_source_dirs(project_root, source_set) {
        roots.push(root);
    }
    let generated = project_root.join("build/generated");
    for relative in [
        format!("sources/annotationProcessor/java/{source_set}"),
        format!("sources/{source_set}"),
        format!("source/proto/{source_set}/java"),
        format!("source/thrift/{source_set}/java"),
        format!("source/kapt/{source_set}"),
        format!("ksp/{source_set}"),
    ] {
        let root = generated.join(relative);
        if root.is_dir() {
            roots.push(root);
        }
    }
    let maven_generated = project_root.join("target").join(if source_set == "test" {
        "generated-test-sources"
    } else {
        "generated-sources"
    });
    if maven_generated.is_dir() {
        roots.push(maven_generated);
    }
    roots.into_vec()
}

fn stage_gradle_project_ownership(
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
    project: &GradleProject,
) {
    for source_set in ["main", "test"] {
        if !crate::project_classpath::stage_project_ownership_for_refresh(
            refresh,
            &project.root_dir,
            source_set,
        ) {
            return;
        }
    }
    for project_root in project.subproject_dirs.values() {
        for source_set in ["main", "test"] {
            if !crate::project_classpath::stage_project_ownership_for_refresh(
                refresh,
                project_root,
                source_set,
            ) {
                return;
            }
        }
    }
}

/// Initialize Gradle support for a workspace and update the workspace index
///
/// This is a convenience function for use in the LSP server's initialization.
///
/// # Arguments
///
/// * `workspace_root` - The root directory of the workspace
/// * `workspace_index` - The workspace index to update with classpaths
///
/// # Returns
///
/// Source paths that should be indexed, or an error
pub async fn initialize_gradle_support(
    workspace_root: &Path,
    workspace_index: Arc<WorkspaceIndex>,
) -> Result<Vec<PathBuf>> {
    initialize_gradle_support_internal(
        workspace_root,
        SemanticHost::new(workspace_index),
        None,
        None,
        crate::project_classpath::compatibility_store(),
    )
    .await
    .map(|outcome| outcome.source_paths)
}

/// Initialize Gradle support only after a caller-owned root scan reports ready.
///
/// Production startup uses the source-transaction completion passed to
/// `initialize_gradle_support_after_workspace_with_sources`; this compatibility
/// entry point retains its original readiness contract for other callers.
pub async fn initialize_gradle_support_after_workspace(
    workspace_root: &Path,
    workspace_index: Arc<WorkspaceIndex>,
    workspace_ready: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<PathBuf>> {
    while !workspace_ready.load(std::sync::atomic::Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    initialize_gradle_support_internal(
        workspace_root,
        SemanticHost::new(workspace_index),
        None,
        None,
        crate::project_classpath::compatibility_store(),
    )
    .await
    .map(|outcome| outcome.source_paths)
}

/// Initialize Gradle and register source attachments for resolved dependency JARs.
pub(crate) async fn initialize_gradle_support_after_workspace_with_sources(
    workspace_root: &Path,
    host: Arc<SemanticHost>,
    _workspace_ready: Arc<std::sync::atomic::AtomicBool>,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
    bulk_source_index: crate::server::source_indexing::BulkSourceIndexContext,
    project_classpaths: crate::project_classpath::ProjectClasspathStore,
) -> Result<GradleIndexOutcome> {
    initialize_gradle_support_internal(
        workspace_root,
        host,
        Some(source_providers),
        Some(bulk_source_index),
        project_classpaths,
    )
    .await
}

async fn initialize_gradle_support_internal(
    workspace_root: &Path,
    host: Arc<SemanticHost>,
    source_providers: Option<Arc<crate::source_provider::SourceProviderChain>>,
    bulk_source_index: Option<crate::server::source_indexing::BulkSourceIndexContext>,
    project_classpaths: crate::project_classpath::ProjectClasspathStore,
) -> Result<GradleIndexOutcome> {
    let mut gradle_integration = GradleIntegration::with_project_classpaths(
        workspace_root.to_path_buf(),
        project_classpaths.clone(),
    );

    if !gradle_integration.is_gradle_workspace() {
        crate::handlers::build_system::retire_build_system(
            crate::handlers::build_system::BuildManifestFamily::Gradle,
            workspace_root,
            &host,
            source_providers.as_ref(),
            &project_classpaths,
        )
        .await?;
        gradle_integration
            .require_supported_workspace_layout()
            .await?;
        debug!("Not a Gradle workspace");
        return Ok(GradleIndexOutcome {
            source_paths: Vec::new(),
            refactor_ready: true,
            dependency_jars_indexed: 0,
            dependency_issues: Vec::new(),
            structural_issues: Vec::new(),
        });
    }

    // Initialize Gradle and get paths to index
    let refresh_ticket = host
        .write()
        .await
        .index()
        .begin_external_source_refresh("build:gradle");
    let source_refresh = match &source_providers {
        Some(providers) => Some(
            providers
                .observe_group_refresh_guard(&refresh_ticket)
                .ok_or("Gradle source-provider refresh was superseded")?,
        ),
        None => None,
    };
    let classpath_refresh = project_classpaths
        .begin_project_classpath_refresh_guard(workspace_root, &refresh_ticket)
        .ok_or("Gradle project-classpath refresh was superseded")?;
    let progress = bulk_source_index
        .as_ref()
        .and_then(|context| context.build_progress())
        .cloned();
    let (source_paths, jar_paths, jars_with_docs) = gradle_integration
        .initialize_for_refresh(classpath_refresh.ticket(), progress.as_ref())
        .await?;
    let dependency_model_complete = gradle_integration.dependency_model_complete;
    let refactor_ready = gradle_integration.refactor_ready;
    let dependency_issues: Vec<String> = gradle_integration
        .dependency_issues
        .iter()
        .cloned()
        .collect();
    let dependency_jar_count = gradle_integration.dependency_jar_count;
    let publication_attempt = complete_gradle_publication_attempt(
        gradle_integration.pending_classpath_snapshot.take(),
        source_refresh,
        classpath_refresh,
    )?;
    let (jars_failed, jars_with_docs_failed) = if let Some(publication_attempt) =
        publication_attempt
    {
        if let Some(progress) = &progress {
            progress
                .report(
                    Some(format!(
                        "Preparing {} Gradle dependency archives and source attachments...",
                        jar_paths.len()
                    )),
                    None,
                )
                .await;
        }
        let plan_jars = jar_paths.clone();
        let plan_docs = jars_with_docs.clone();
        let materialized_sources = source_providers
            .as_ref()
            .map(|providers| providers.materialized_sources());
        let (archive_plan, staged_provider) = tokio::task::spawn_blocking(move || -> Result<_> {
            let archive_plan =
                crate::source_provider::capture_dependency_archive_plan(&plan_jars, &plan_docs)
                    .map_err(anyhow::Error::msg)?;
            let staged_provider = materialized_sources.and_then(|store| {
                let archives: Vec<_> = archive_plan
                    .iter()
                    .map(|plan| plan.archive.clone())
                    .collect();
                crate::source_provider::JarSourceProvider::from_binary_archives_with_store(
                    &archives, store,
                )
                .map(|provider| {
                    Arc::new(provider) as Arc<dyn crate::source_provider::SourceProvider>
                })
            });
            Ok((archive_plan, staged_provider))
        })
        .await
        .map_err(|error| format!("Gradle archive planning task failed: {error}"))??;
        let external_archives: Vec<_> = archive_plan
            .iter()
            .map(|plan| plan.archive.clone())
            .collect();
        let source_identities: Vec<_> = external_archives
            .iter()
            .map(|archive| archive.identity().clone())
            .collect();
        if let Some(progress) = &progress {
            progress
                .report(
                    Some(format!(
                        "Indexing {} Gradle dependency JARs and Javadocs...",
                        archive_plan.len()
                    )),
                    None,
                )
                .await;
        }
        let transaction = archive_transaction::CompleteGradleArchiveTransaction::new(
            Arc::clone(&host),
            source_providers,
            refresh_ticket,
            publication_attempt,
            archive_plan,
            staged_provider,
            external_archives,
            source_identities,
        );
        let archive_result = tokio::task::spawn_blocking(move || transaction.publish())
            .await
            .map_err(|error| format!("Gradle archive transaction panicked: {error}"))??;
        (
            archive_result.jars_failed,
            archive_result.jars_with_docs_failed,
        )
    } else {
        // Local source roots remain useful, but no external dependency state
        // from this incomplete model is authoritative.
        (0, 0)
    };

    // Index source files from Gradle module source directories
    // This is critical for multi-module builds where classes from sibling modules
    // need to be resolved (e.g., com.acme.authentication.* from shared/acme-utils)
    info!(
        "Gradle integration found {} source directories to index",
        source_paths.len()
    );
    for source_dir in &source_paths {
        info!("  - Source directory: {:?}", source_dir);
    }

    // Build-system source roots can extend outside the workspace manifest and
    // can overlap dirty editor buffers. Production callers use the shared
    // blocking transaction so disk indexing can never displace editor text.
    // It is safe to run concurrently with the root scan: both paths use exact
    // per-path/file locks and the final editor-buffer restoration wins.
    let force_reindex = std::env::var("HYPERION_REINDEX").is_ok_and(|v| v == "true" || v == "1");
    let (stats, source_scan_failures, open_documents_restored) =
        if let Some(context) = bulk_source_index {
            let outcome = crate::server::source_indexing::index_build_source_roots_off_runtime(
                context,
                source_paths.clone(),
                force_reindex,
            )
            .await
            .map_err(|error| format!("Gradle source indexing task failed: {error}"))?;
            (
                outcome.stats,
                outcome.source_scan_failures,
                outcome.open_documents_restored,
            )
        } else {
            let mut all_source_files = Vec::new();
            let mut source_scan_failures = 0_usize;
            for source_dir in &source_paths {
                match WorkspaceScanner::scan_dir_ignore_gitignore(source_dir) {
                    Ok(files) => all_source_files.extend(files),
                    Err(error) => {
                        source_scan_failures += 1;
                        warn!(
                            "Failed to scan source directory {:?}: {}",
                            source_dir, error
                        );
                    }
                }
            }
            all_source_files.sort_unstable();
            all_source_files.dedup();
            let stats = host
                .write()
                .await
                .run_blocking(move |txn| {
                    if force_reindex {
                        info!("HYPERION_REINDEX=true: forcing full reindex");
                        txn.index().index_files_parallel(&all_source_files)
                    } else {
                        txn.index()
                            .index_files_parallel_skip_cached(&all_source_files)
                    }
                })
                .await
                .map_err(|error| format!("Gradle source indexing task failed: {error}"))?;
            (stats, source_scan_failures, true)
        };
    let source_files_indexed = stats.files_indexed;
    let source_symbols_total = stats.symbols_indexed;

    if source_files_indexed > 0 {
        info!(
            "Indexed {} source files from {} Gradle module directories with {} total symbols",
            source_files_indexed,
            source_paths.len(),
            source_symbols_total
        );
    }

    let mut dependency_issues = dependency_issues;
    if jars_failed != 0 {
        dependency_issues.push(format!(
            "{jars_failed} dependency archives could not be read and were not indexed"
        ));
    }
    if jars_with_docs_failed != 0 {
        dependency_issues.push(format!(
            "{jars_with_docs_failed} dependency documentation archives could not be read"
        ));
    }
    if source_scan_failures != 0 || stats.files_failed != 0 || !open_documents_restored {
        return Err(format!(
            "Gradle source index is incomplete: {source_scan_failures} source roots failed, {} source files failed, open buffers restored={open_documents_restored}",
            stats.files_failed,
        )
        .into());
    }
    if !refactor_ready {
        warn!(
            "Gradle build structure is not fully understood ({}); destructive refactorings stay disabled",
            gradle_incomplete_summary(
                &gradle_integration.primary_incomplete_reasons,
                &gradle_integration.incomplete_reasons,
            )
        );
    }
    let _ = dependency_model_complete;

    Ok(GradleIndexOutcome {
        source_paths,
        refactor_ready,
        dependency_jars_indexed: dependency_jar_count.saturating_sub(jars_failed),
        dependency_issues,
        structural_issues: gradle_integration
            .primary_incomplete_reasons
            .iter()
            .cloned()
            .collect(),
    })
}

/// What one Gradle refresh published and what it could not.
#[derive(Debug, Clone)]
pub(crate) struct GradleIndexOutcome {
    pub(crate) source_paths: Vec<PathBuf>,
    /// The build's modules and source roots are fully understood, so
    /// workspace-wide reference scans are exhaustive for workspace symbols.
    pub(crate) refactor_ready: bool,
    pub(crate) dependency_jars_indexed: usize,
    /// Precise per-dependency problems; never blocking.
    pub(crate) dependency_issues: Vec<String>,
    /// Why the build structure is not fully understood (blocks refactoring).
    pub(crate) structural_issues: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

    #[test]
    fn resolver_declared_missing_jar_is_preserved_for_mandatory_staging() {
        let missing = PathBuf::from("/definitely/missing/resolver-declared.jar");
        let mut paths = OrderedPaths::default();

        append_mandatory_archive_paths(&mut paths, std::iter::once(&missing));

        assert_eq!(paths.into_vec(), vec![missing]);
    }

    #[test]
    fn incomplete_gradle_model_never_authorizes_an_execution_classpath() {
        assert!(!gradle_execution_model_is_complete(false, true, 0, true));
        assert!(!gradle_execution_model_is_complete(true, false, 0, true));
        assert!(!gradle_execution_model_is_complete(true, true, 1, true));
        assert!(!gradle_execution_model_is_complete(true, true, 0, false));
        assert!(gradle_execution_model_is_complete(true, true, 0, true));
    }

    #[tokio::test]
    async fn independent_syntax_and_resolution_causes_survive_partial_collection() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("project");
        let cache = temp.path().join("cache");
        let maven = temp.path().join("maven");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("build.gradle"),
            "plugins { id 'java' }\ndependencies { implementation 'org.fixture:present:1.0'; implementation 'org.fixture:absent:1.0'; customConfig 'org.fixture:dynamic:1.0' }",
        )
        .unwrap();
        let artifact_dir = cache.join("org.fixture/present/1.0/hash");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::fs::write(artifact_dir.join("present-1.0.jar"), b"fixture").unwrap();
        std::fs::write(artifact_dir.join("present-1.0.pom"), "<project/>").unwrap();

        let mut integration = GradleIntegration::new(root.clone());
        integration.classpath_builder = GradleClasspathBuilder::with_resolver(
            GradleResolver::with_cache_dirs(cache.clone(), maven.clone()),
        );
        integration.resolver = GradleResolver::with_cache_dirs(cache, maven);
        let (_, candidate_jars, _) = integration.initialize().await.unwrap();

        assert_eq!(candidate_jars.len(), 1);
        assert!(!integration.dependency_model_complete);
        // Structure (unmodeled `customConfig`) blocks refactoring; the absent
        // JAR is only a named dependency issue, and the present JAR still
        // publishes.
        assert!(!integration.refactor_ready);
        assert_eq!(
            integration
                .project_classpaths
                .known_dependency_jars(&root)
                .map(|jars| jars.len()),
            Some(1),
            "the JAR that resolved must be published despite the degraded model"
        );
        let summary = gradle_incomplete_summary(
            &integration.primary_incomplete_reasons,
            &integration.incomplete_reasons,
        );
        assert!(summary.contains("unaccounted"), "{summary}");
        assert!(integration
            .dependency_issues
            .iter()
            .any(|reason| reason.contains("org.fixture:absent:1.0")));

        std::fs::write(
            root.join("build.gradle"),
            "plugins { id 'java' }\ndependencies { implementation 'org.fixture:present:1.0' }",
        )
        .unwrap();
        integration.initialize().await.unwrap();
        assert!(integration.dependency_model_complete);
        assert!(integration.incomplete_reasons.is_empty());
    }

    #[test]
    fn editor_summary_prioritizes_causes_and_is_bounded() {
        let primary = BTreeSet::from(["unaccounted syntax".to_string()]);
        let details: BTreeSet<String> = (0..20)
            .map(|index| format!("module-{index}: {}", "x".repeat(300)))
            .collect();

        let summary = gradle_incomplete_summary(&primary, &details);

        assert!(summary.starts_with("unaccounted syntax; "));
        assert!(summary.contains("16 more causes in logs"));
        assert!(summary.len() < 900);
    }

    #[tokio::test]
    async fn subproject_transitive_javadocs_survive_partial_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let cache = temp.path().join("cache");
        let maven = temp.path().join("maven");
        std::fs::create_dir_all(root.join("app")).unwrap();
        std::fs::write(root.join("settings.gradle"), "include ':app'").unwrap();
        std::fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();
        std::fs::write(
            root.join("app/build.gradle"),
            "plugins { id 'java' }\ndependencies { implementation 'com.example:root:1.0' }",
        )
        .unwrap();
        for (artifact, dependencies) in [
            ("root", "<dependency><groupId>com.example</groupId><artifactId>leaf</artifactId><version>1.0</version></dependency><dependency><groupId>com.example</groupId><artifactId>missing</artifactId><version>1.0</version></dependency>"),
            ("leaf", ""),
        ] {
            let dir = cache.join(format!("com.example/{artifact}/1.0/hash"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{artifact}-1.0.jar")), b"fixture").unwrap();
            std::fs::write(dir.join(format!("{artifact}-1.0.pom")), format!("<project><dependencies>{dependencies}</dependencies></project>")).unwrap();
        }
        let leaf_jar = cache.join("com.example/leaf/1.0/hash/leaf-1.0.jar");
        let leaf_docs = leaf_jar.with_file_name("leaf-1.0-javadoc.jar");
        std::fs::write(&leaf_docs, b"optional docs").unwrap();
        let mut integration = GradleIntegration::new(root);
        integration.classpath_builder = GradleClasspathBuilder::with_resolver(
            GradleResolver::with_cache_dirs(cache.clone(), maven.clone()),
        );
        integration.resolver = GradleResolver::with_cache_dirs(cache, maven);
        let (_, jars, docs) = integration.initialize().await.unwrap();
        assert!(jars.contains(&leaf_jar));
        assert_eq!(docs, vec![(leaf_jar, leaf_docs)]);
        assert!(!integration.dependency_model_complete);
        assert!(integration.pending_classpath_snapshot.is_none());
    }

    #[tokio::test]
    async fn test_non_gradle_workspace() {
        let temp = TempDir::new().unwrap();
        let gradle_int = GradleIntegration::new(temp.path().to_path_buf());

        assert!(!gradle_int.is_gradle_workspace());
    }

    #[tokio::test]
    async fn removed_root_manifest_retires_gradle_state_before_nested_layout_error() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let stale_jar = root.join("stale-gradle.jar");
        let store = crate::project_classpath::ProjectClasspathStore::new();
        let index = Arc::new(WorkspaceIndex::new(root.to_path_buf()));
        let previous = index.begin_external_source_refresh("build:gradle");
        let previous_classpath = store
            .begin_project_classpath_refresh(root, &previous)
            .expect("previous Gradle classpath refresh begins");
        let _ = index
            .replace_external_sources_for_refresh(&previous, std::slice::from_ref(&stale_jar))
            .expect("previous Gradle classpath commits");
        assert!(
            crate::project_classpath::publish_gradle_dependency_snapshot_for_refresh(
                &previous_classpath,
                vec![stale_jar.clone()],
                vec![stale_jar.clone()],
            )
        );
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/build.gradle"), "plugins { id 'java' }").unwrap();

        let error = initialize_gradle_support_internal(
            root,
            SemanticHost::new(Arc::clone(&index)),
            None,
            None,
            store.clone(),
        )
        .await
        .expect_err("a nested-only Gradle build remains unsupported");

        assert!(error.to_string().contains("nested or uninspectable Gradle"));
        assert_eq!(store.known_dependency_jars(root), None);
        let maven = index.begin_external_source_refresh("build:maven");
        assert!(store
            .begin_project_classpath_refresh(root, &maven)
            .is_some());
    }

    #[tokio::test]
    async fn test_gradle_workspace_detection() {
        let temp = TempDir::new().unwrap();

        // Create a build.gradle file
        std::fs::write(temp.path().join("build.gradle"), "plugins { id 'java' }").unwrap();

        let gradle_int = GradleIntegration::new(temp.path().to_path_buf());

        assert!(gradle_int.is_gradle_workspace());
    }

    #[tokio::test]
    async fn dynamic_dependency_syntax_keeps_model_incomplete() {
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("build.gradle"),
            r#"
plugins { id 'java' }
def coordinates = ["org.example", "dynamic", "1.0"]
dependencies { implementation coordinates.join(":") }
"#,
        )
        .unwrap();
        let mut integration = GradleIntegration::new(temp.path().to_path_buf());

        integration.initialize().await.unwrap();

        assert!(!integration.dependency_model_complete);
    }

    #[test]
    fn conventional_root_sources_cover_all_supported_jvm_languages() {
        let temp = TempDir::new().unwrap();
        for language in ["java", "kotlin", "groovy", "scala"] {
            std::fs::create_dir_all(temp.path().join("src/main").join(language)).unwrap();
        }

        let mut paths = OrderedPaths::default();
        add_conventional_jvm_source_dirs("root", temp.path(), &mut paths);
        let paths = paths.into_vec();

        for language in ["java", "kotlin", "groovy", "scala"] {
            assert!(paths.contains(&temp.path().join("src/main").join(language)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn ordered_paths_deduplicates_filesystem_aliases_without_losing_display_order() {
        let temp = TempDir::new().unwrap();
        let real = temp.path().join("real");
        let alias = temp.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let mut paths = OrderedPaths::default();

        paths.push(alias.clone());
        paths.push(real.clone());

        assert!(paths.contains(&real));
        assert!(paths.contains(&alias));
        assert_eq!(paths.into_vec(), vec![alias]);
    }

    #[tokio::test]
    async fn test_initialize_keeps_staged_ownership_private_for_an_incomplete_snapshot() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let module = root.join("included");
        std::fs::write(
            root.join("build.gradle"),
            "plugins { id 'java' }\ndependencies { implementation 'com.example:missing:1.0.0' }",
        )
        .unwrap();
        std::fs::write(root.join("settings.gradle"), "include ':included'").unwrap();
        std::fs::create_dir_all(module.join("src/main/java/example")).unwrap();
        std::fs::write(
            module.join("src/main/java/example/Included.java"),
            "package example; public class Included {}",
        )
        .unwrap();

        let mut integration = GradleIntegration::new(root.clone());
        integration.inject_multi_module_classpath_failure();
        integration.initialize().await.unwrap();
        assert!(
            !integration.dependency_model_complete,
            "known incomplete classpaths must never satisfy destructive-refactoring readiness"
        );

        // Parsing discovered the included module and staged its ownership, but
        // an exact refresh must not expose any part of an incomplete snapshot.
        // Publishing this pending ownership independently would let request
        // routing observe a newer module graph alongside an older classpath.
        assert_eq!(
            crate::project_classpath::known_project_for_source_root(
                &root,
                &module.join("src/main/java"),
            ),
            None
        );
        assert!(
            !integration.project_classpaths.has_active_refresh(&root),
            "an unavailable snapshot must abort its private refresh authority"
        );
        crate::project_classpath::reset_project_classpaths(&root);
    }

    #[tokio::test]
    async fn test_single_module_source_indexing() {
        // This test verifies that source files from Gradle source directories
        // are properly indexed during initialization.

        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create a simple single-module Gradle project
        std::fs::write(
            root.join("build.gradle"),
            r"
                plugins {
                    id 'java'
                }
            ",
        )
        .unwrap();

        // Create source directory with Java files
        let src_dir = root.join("src/main/java/com/example");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            src_dir.join("MyService.java"),
            r#"
package com.example;

public class MyService {
    public String greet(String name) {
        return "Hello, " + name;
    }
}
            "#,
        )
        .unwrap();
        std::fs::write(
            src_dir.join("MyHelper.java"),
            r"
package com.example;

public class MyHelper {
    public static int add(int a, int b) {
        return a + b;
    }
}
            ",
        )
        .unwrap();

        // Create workspace index and initialize Gradle support
        let workspace_index = Arc::new(WorkspaceIndex::new(root.to_path_buf()));
        let result = initialize_gradle_support(root, workspace_index.clone()).await;

        // Should succeed
        assert!(
            result.is_ok(),
            "initialize_gradle_support failed: {:?}",
            result.err()
        );

        // Verify that classes are indexed
        let my_service = workspace_index.get_symbol("com.example.MyService");
        assert!(my_service.is_some(), "MyService class should be indexed");

        let my_helper = workspace_index.get_symbol("com.example.MyHelper");
        assert!(my_helper.is_some(), "MyHelper class should be indexed");

        // Verify method indexing too (without param signature)
        let greet_method = workspace_index.get_symbol("com.example.MyService.greet");
        assert!(
            greet_method.is_some(),
            "MyService.greet method should be indexed"
        );

        let add_method = workspace_index.get_symbol("com.example.MyHelper.add");
        assert!(
            add_method.is_some(),
            "MyHelper.add method should be indexed"
        );
    }

    #[tokio::test]
    async fn generated_gradle_source_is_discovered_and_indexed() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();

        let generated = root.join("build/generated/ksp/main/java/com/example");
        std::fs::create_dir_all(&generated).unwrap();
        std::fs::write(
            generated.join("GeneratedType.java"),
            "package com.example; public class GeneratedType {}",
        )
        .unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(root.to_path_buf()));
        let source_paths = initialize_gradle_support(root, Arc::clone(&workspace_index))
            .await
            .expect("initialize Gradle support");

        assert!(source_paths
            .iter()
            .any(|path| path == &root.join("build/generated")));
        assert!(workspace_index
            .find_symbol("com.example.GeneratedType")
            .is_some());
    }

    #[tokio::test]
    async fn thrift_script_plugin_output_is_visible_to_static_importers() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let generated = root.join("build/generated/thrift/example/Constants.java");
        let importer = root.join("src/main/java/example/Use.java");
        let plugin = root.join("buildSrc/src/main/groovy/example.codegen.gradle");
        for file in [&generated, &importer, &plugin] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        }
        std::fs::write(
            root.join("build.gradle"),
            "plugins { id 'java'; id 'example.codegen' }",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(
            &plugin,
            r#"compileThrift { outputDir = "${projectDir}/build/generated/thrift" }"#,
        )
        .unwrap();
        std::fs::write(
            &generated,
            "package example; public class Constants { public static final int VALUE = 1; }",
        )
        .unwrap();
        std::fs::write(&importer, "package example; import static example.Constants.VALUE; class Use { int value = VALUE; }").unwrap();
        let index = Arc::new(WorkspaceIndex::new(root.to_path_buf()));
        initialize_gradle_support_internal(
            root,
            SemanticHost::new(Arc::clone(&index)),
            None,
            None,
            crate::project_classpath::ProjectClasspathStore::new(),
        )
        .await
        .unwrap();
        for path in [&importer, &generated] {
            let view = index
                .scoped_view_for_file(index.get_file_id(path).unwrap())
                .expect("published Gradle source scope");
            assert!(view.source_scope_is_current());
            assert!(
                view.find_symbol("example.Constants.VALUE").is_some(),
                "generated constant invisible from {}",
                path.display()
            );
        }
    }

    #[test]
    fn configured_generated_root_keeps_its_source_set() {
        let temp = TempDir::new().unwrap();
        let project = temp.path();
        let custom = project.join("build/generated/thrift/custom-java");
        std::fs::create_dir_all(&custom).unwrap();

        let main = source_roots(project, "main", std::slice::from_ref(&custom));
        let test = source_roots(project, "test", &[]);
        assert!(main.contains(&custom));
        assert!(!test.contains(&custom));
    }

    #[tokio::test]
    async fn test_initial_gradle_source_index_waits_for_root_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        std::fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();

        let source_dir = root.join("src/main/java/com/example");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(
            source_dir.join("GradleOnly.java"),
            "package com.example; public class GradleOnly {}",
        )
        .unwrap();

        let workspace_index = Arc::new(WorkspaceIndex::new(root.clone()));
        let workspace_ready = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn({
            let workspace_index = Arc::clone(&workspace_index);
            let workspace_ready = Arc::clone(&workspace_ready);
            async move {
                initialize_gradle_support_after_workspace(&root, workspace_index, workspace_ready)
                    .await
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !task.is_finished(),
            "Gradle source indexing should wait for the root workspace index"
        );
        assert!(
            workspace_index
                .get_symbol("com.example.GradleOnly")
                .is_none(),
            "Gradle source must not be indexed before root readiness"
        );

        workspace_ready.store(true, Ordering::Release);
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("Gradle source indexing should resume after root readiness")
            .expect("Gradle indexing task should not panic")
            .expect("Gradle indexing should succeed");
        assert!(
            workspace_index
                .get_symbol("com.example.GradleOnly")
                .is_some(),
            "Gradle source should be indexed after root readiness"
        );
    }

    #[tokio::test]
    async fn test_source_dir_scanning() {
        // Test that WorkspaceScanner::scan_dir correctly finds source files
        use hyperion_workspace::scanner::WorkspaceScanner;

        let temp = TempDir::new().unwrap();
        let src_dir = temp.path().join("src/main/java/com/test");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create some Java files
        std::fs::write(src_dir.join("Foo.java"), "package com.test; class Foo {}").unwrap();
        std::fs::write(src_dir.join("Bar.java"), "package com.test; class Bar {}").unwrap();

        // Create a Kotlin file
        std::fs::write(src_dir.join("Baz.kt"), "package com.test\nclass Baz").unwrap();

        // Scan the directory (ignoring gitignore for Gradle source paths)
        let files = WorkspaceScanner::scan_dir_ignore_gitignore(temp.path()).unwrap();

        assert_eq!(files.len(), 3, "Should find 3 source files");
        assert!(files.iter().any(|p| p.ends_with("Foo.java")));
        assert!(files.iter().any(|p| p.ends_with("Bar.java")));
        assert!(files.iter().any(|p| p.ends_with("Baz.kt")));
    }

    #[tokio::test]
    async fn test_scan_dir_ignore_gitignore() {
        // Test that scan_dir_ignore_gitignore finds files even if in .gitignore
        use hyperion_workspace::scanner::WorkspaceScanner;

        let temp = TempDir::new().unwrap();
        let src_dir = temp.path().join("src/main/java/com/test");
        std::fs::create_dir_all(&src_dir).unwrap();

        // Create a .gitignore that ignores the src directory
        std::fs::write(temp.path().join(".gitignore"), "src/").unwrap();

        // Create a Java file in the ignored directory
        std::fs::write(
            src_dir.join("Ignored.java"),
            "package com.test; class Ignored {}",
        )
        .unwrap();

        // Regular scan should respect .gitignore and find nothing
        let files_respecting_gitignore = WorkspaceScanner::scan_dir(temp.path()).unwrap();
        assert_eq!(
            files_respecting_gitignore.len(),
            0,
            "Regular scan should respect .gitignore"
        );

        // Scan ignoring gitignore should find the file
        let files_ignoring_gitignore =
            WorkspaceScanner::scan_dir_ignore_gitignore(temp.path()).unwrap();
        assert_eq!(
            files_ignoring_gitignore.len(),
            1,
            "Scan ignoring gitignore should find files in ignored directories"
        );
        assert!(files_ignoring_gitignore[0].ends_with("Ignored.java"));
    }
}
