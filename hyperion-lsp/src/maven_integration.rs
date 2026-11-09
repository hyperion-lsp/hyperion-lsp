// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven integration for Hyperion LSP.
//!
//! This module integrates Maven support into the LSP server, enabling:
//! - Automatic Maven project detection
//! - Dependency resolution from pom.xml files
//! - Classpath construction for accurate code navigation
//! - Support for multi-module Maven projects

use hyperion_build::maven::{
    DependencyScope, MavenParser, MavenProject, MavenResolutionBudget, MavenResolver,
    ProfileActivationContext, ResolutionReport,
};
use hyperion_workspace::scanner::{generated_source_roots, WorkspaceScanner};
use hyperion_workspace::WorkspaceIndex;

use crate::semantic_host::SemanticHost;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Result type for Maven integration operations
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Result of Maven initialization including resolution report
pub struct MavenInitResult {
    /// Source paths to index
    pub source_paths: Vec<PathBuf>,
    /// JAR files to index
    pub jar_paths: Vec<PathBuf>,
    /// JARs with their javadoc files
    pub jars_with_docs: Vec<(PathBuf, PathBuf)>,
    /// Dependency resolution report (for diagnostics)
    pub resolution_report: ResolutionReport,
    /// False when the dependency model, a declared source root, or an archive
    /// publication was incomplete. Discovered local source roots remain useful,
    /// but an incomplete dependency set is never authoritative membership.
    pub index_complete: bool,
    /// Precise dependency problems (unresolvable coordinates, unreadable POMs).
    /// Reported to the user; never blocks publication of what did resolve.
    pub dependency_issues: Vec<String>,
    /// Per-module main/test classpaths and source-set visibility.
    pub modules: Vec<MavenModuleClasspath>,
}

/// One reactor module's classpaths, split by source set.
///
/// `main_*` is what the module's production sources compile against and run
/// with; `test_*` adds the test-scoped dependencies and the module's own main
/// output. Reactor siblings contribute their output directories and the JARs
/// they bring, like Maven's reactor resolution.
#[derive(Debug, Clone, Default)]
pub struct MavenModuleClasspath {
    pub project_root: PathBuf,
    pub main_source_roots: Vec<PathBuf>,
    pub test_source_roots: Vec<PathBuf>,
    /// Source roots visible to production sources (own + reactor dependencies).
    pub main_visible_source_roots: Vec<PathBuf>,
    /// Source roots visible to test sources (own tests + main + dependencies).
    pub test_visible_source_roots: Vec<PathBuf>,
    pub main_output_dirs: Vec<PathBuf>,
    pub main_compile_output_dirs: Vec<PathBuf>,
    pub test_output_dirs: Vec<PathBuf>,
    pub test_compile_output_dirs: Vec<PathBuf>,
    pub main_compile_jars: Vec<PathBuf>,
    pub main_runtime_jars: Vec<PathBuf>,
    pub test_compile_jars: Vec<PathBuf>,
    pub test_runtime_jars: Vec<PathBuf>,
}

fn language_source_roots(module_root: &Path, source_set: &str) -> Vec<PathBuf> {
    let mut roots = hyperion_layout::existing_source_dirs(module_root, source_set);
    roots.extend(hyperion_workspace::scanner::maven_generated_source_roots(
        module_root,
        source_set == "test",
    ));
    roots
}

/// Assemble per-module classpaths from independently resolved modules and the
/// reactor's module-to-module dependencies.
fn build_module_classpaths(
    modules: &[MavenProject],
    reports: &[ResolutionReport],
    reactor_dependencies: &[Vec<(usize, DependencyScope)>],
) -> Vec<MavenModuleClasspath> {
    let mut classpaths: Vec<MavenModuleClasspath> = modules
        .iter()
        .map(|module| MavenModuleClasspath {
            project_root: module.root_dir.clone(),
            main_source_roots: language_source_roots(&module.root_dir, "main"),
            test_source_roots: language_source_roots(&module.root_dir, "test"),
            ..MavenModuleClasspath::default()
        })
        .collect();

    // Modules reachable through compile/runtime edges (provided counts for
    // compilation only, which is close enough for visibility).
    let main_closure = |start: usize| -> Vec<usize> {
        let mut seen = vec![start];
        let mut index = 0;
        while index < seen.len() {
            for (dependency, scope) in &reactor_dependencies[seen[index]] {
                if *scope != DependencyScope::Test && !seen.contains(dependency) {
                    seen.push(*dependency);
                }
            }
            index += 1;
        }
        seen.remove(0);
        seen
    };

    for (index, module) in modules.iter().enumerate() {
        let main_deps = main_closure(index);
        let mut test_deps = main_deps.clone();
        for (dependency, scope) in &reactor_dependencies[index] {
            if *scope == DependencyScope::Test {
                for module_index in std::iter::once(*dependency).chain(main_closure(*dependency)) {
                    if module_index != index && !test_deps.contains(&module_index) {
                        test_deps.push(module_index);
                    }
                }
            }
        }
        let own_main_out = module.root_dir.join("target/classes");
        let own_test_out = module.root_dir.join("target/test-classes");
        let dep_outputs = |deps: &[usize]| -> Vec<PathBuf> {
            deps.iter()
                .map(|dependency| modules[*dependency].root_dir.join("target/classes"))
                .collect()
        };
        let dep_roots = |deps: &[usize]| -> Vec<PathBuf> {
            deps.iter()
                .flat_map(|dependency| {
                    language_source_roots(&modules[*dependency].root_dir, "main")
                })
                .collect()
        };
        let dep_jars = |deps: &[usize]| -> Vec<PathBuf> {
            deps.iter()
                .flat_map(|dependency| reports[*dependency].main_jars.iter().cloned())
                .collect()
        };

        let classpath = &mut classpaths[index];
        classpath.main_visible_source_roots = classpath
            .main_source_roots
            .iter()
            .cloned()
            .chain(dep_roots(&main_deps))
            .collect();
        classpath.test_visible_source_roots = classpath
            .test_source_roots
            .iter()
            .cloned()
            .chain(classpath.main_source_roots.iter().cloned())
            .chain(dep_roots(&test_deps))
            .collect();
        classpath.main_output_dirs = std::iter::once(own_main_out.clone())
            .chain(dep_outputs(&main_deps))
            .collect();
        classpath.main_compile_output_dirs = classpath.main_output_dirs.clone();
        classpath.test_output_dirs = std::iter::once(own_test_out.clone())
            .chain(std::iter::once(own_main_out))
            .chain(dep_outputs(&test_deps))
            .collect();
        classpath.test_compile_output_dirs = classpath.test_output_dirs.clone();
        classpath.main_compile_jars = reports[index]
            .main_jars
            .iter()
            .cloned()
            .chain(dep_jars(&main_deps))
            .collect();
        classpath.main_runtime_jars = classpath.main_compile_jars.clone();
        classpath.test_compile_jars = reports[index]
            .resolved_jars
            .iter()
            .cloned()
            .chain(dep_jars(&test_deps))
            .collect();
        classpath.test_runtime_jars = classpath.test_compile_jars.clone();
    }
    classpaths
}

/// Stage every module's project classpath and source scope for one refresh.
fn stage_maven_module_state(
    refresh: &crate::project_classpath::ProjectClasspathRefreshTicket,
    modules: &[MavenModuleClasspath],
) {
    for module in modules {
        let root = &module.project_root;
        let _ = crate::project_classpath::record_project_classpath_scoped_for_refresh(
            refresh,
            root,
            "main",
            module.main_output_dirs.clone(),
            module.main_compile_output_dirs.clone(),
            module.main_compile_jars.clone(),
            module.main_runtime_jars.clone(),
        );
        let _ = crate::project_classpath::record_project_classpath_scoped_for_refresh(
            refresh,
            root,
            "test",
            module.test_output_dirs.clone(),
            module.test_compile_output_dirs.clone(),
            module.test_compile_jars.clone(),
            module.test_runtime_jars.clone(),
        );
        let _ = crate::project_classpath::record_source_scope_for_refresh(
            refresh,
            root,
            "main",
            module.main_source_roots.clone(),
            module.main_visible_source_roots.clone(),
            module.main_compile_jars.clone(),
        );
        let _ = crate::project_classpath::record_source_scope_for_refresh(
            refresh,
            root,
            "test",
            module.test_source_roots.clone(),
            module.test_visible_source_roots.clone(),
            module.test_compile_jars.clone(),
        );
    }
}

/// Archive inputs whose dependency membership was proven complete before any
/// destructive WorkspaceIndex/provider publication is constructed.
struct CompleteMavenDependencyModel {
    jar_paths: Vec<PathBuf>,
    jars_with_docs: Vec<(PathBuf, PathBuf)>,
}

impl CompleteMavenDependencyModel {
    /// Classify the resolver/reactor result before source-root indexing folds
    /// its independent failures into `MavenInitResult::index_complete`.
    fn from_dependency_resolution(result: &MavenInitResult) -> Option<Self> {
        result.index_complete.then(|| Self {
            jar_paths: result.jar_paths.clone(),
            jars_with_docs: result.jars_with_docs.clone(),
        })
    }
}

/// Capability to construct the destructive Maven archive transaction. A
/// partial reactor or resolver result instead drops its private staging guards
/// and never reaches archive planning or membership replacement.
struct CompleteMavenPublicationAttempt {
    model: CompleteMavenDependencyModel,
    source_refresh: Option<crate::source_provider::SourceProviderRefreshGuard>,
    classpath_refresh: crate::project_classpath::ProjectClasspathRefreshGuard,
}

fn complete_maven_publication_attempt(
    result: &MavenInitResult,
    source_refresh: Option<crate::source_provider::SourceProviderRefreshGuard>,
    classpath_refresh: crate::project_classpath::ProjectClasspathRefreshGuard,
) -> Option<CompleteMavenPublicationAttempt> {
    let Some(model) = CompleteMavenDependencyModel::from_dependency_resolution(result) else {
        drop(source_refresh);
        drop(classpath_refresh);
        return None;
    };
    Some(CompleteMavenPublicationAttempt {
        model,
        source_refresh,
        classpath_refresh,
    })
}

/// Maven project manager for LSP
pub struct MavenIntegration {
    /// Root directory of the workspace
    workspace_root: PathBuf,

    /// Maven project (if parsed)
    project: Option<MavenProject>,

    /// Dependency resolver
    resolver: MavenResolver,
}

impl MavenIntegration {
    /// Create a new Maven integration instance
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            project: None,
            resolver: MavenResolver::new(),
        }
    }

    /// Check if this is a Maven workspace
    pub fn is_maven_workspace(&self) -> bool {
        self.workspace_root.join("pom.xml").is_file()
    }

    async fn require_supported_workspace_layout(&self) -> Result<()> {
        if crate::handlers::build_system::has_nested_build_manifest_or_unknown(
            &self.workspace_root,
            crate::handlers::build_system::BuildManifestFamily::Maven,
        )
        .await
        {
            return Err(format!(
                "workspace {} contains a nested or uninspectable Maven build, but no root reactor POM",
                self.workspace_root.display()
            )
            .into());
        }
        Ok(())
    }

    /// Parse every declared reactor module and build an effective dependency
    /// model for each POM. Missing, cyclic, outside-workspace, or partially
    /// resolved modules keep the returned model useful but incomplete.
    async fn load_reactor_projects(&self) -> Result<(Vec<MavenProject>, bool, Vec<String>)> {
        const MAX_REACTOR_PROJECTS: usize = 4096;
        const MAX_REACTOR_POM_BYTES: u64 = 64 * 1024 * 1024;

        let workspace_root = self
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_root.clone());
        let mut pending = VecDeque::from([self.workspace_root.clone()]);
        let mut visited = HashSet::new();
        let mut projects = Vec::new();
        let mut complete = true;
        let mut pom_bytes = 0u64;
        let mut effective_model_budget = MavenResolutionBudget::default();
        let mut model_issues: Vec<String> = Vec::new();

        while let Some(project_dir) = pending.pop_front() {
            let project_key = hyperion_workspace::path_lookup_key(&project_dir);
            if !visited.insert(project_key) {
                warn!(
                    "Maven reactor declares {} more than once or contains a module cycle",
                    project_dir.display()
                );
                complete = false;
                continue;
            }
            if projects.len() >= MAX_REACTOR_PROJECTS {
                warn!(
                    "Maven reactor exceeds the {}-project safety bound",
                    MAX_REACTOR_PROJECTS
                );
                complete = false;
                break;
            }

            let pom_path = project_dir.join("pom.xml");
            let pom_length = match std::fs::metadata(&pom_path) {
                Ok(metadata) => metadata.len(),
                Err(error) if project_dir == self.workspace_root => return Err(error.into()),
                Err(error) => {
                    warn!(
                        "Cannot size Maven module POM {}: {}",
                        pom_path.display(),
                        error
                    );
                    complete = false;
                    continue;
                }
            };
            pom_bytes = match pom_bytes.checked_add(pom_length) {
                Some(total) if total <= MAX_REACTOR_POM_BYTES => total,
                _ => {
                    warn!(
                        "Maven reactor exceeds the {}-byte aggregate POM safety bound",
                        MAX_REACTOR_POM_BYTES
                    );
                    complete = false;
                    break;
                }
            };

            let mut project = match MavenParser::parse_project(&project_dir).await {
                Ok(project) => project,
                Err(error) if project_dir == self.workspace_root => return Err(error.into()),
                Err(error) => {
                    warn!(
                        "Failed to parse declared Maven module {}: {}",
                        project_dir.display(),
                        error
                    );
                    complete = false;
                    continue;
                }
            };

            let activation_context = ProfileActivationContext::new(project_dir.clone());
            if let Err(error) = self
                .resolver
                .activate_profiles(&mut project, &activation_context)
                .await
            {
                warn!(
                    "Failed to activate Maven profiles for {}: {}",
                    project_dir.display(),
                    error
                );
                complete = false;
            }
            // The strict pipeline follows Maven's model-building rules but
            // rejects parents and BOMs it cannot fully interpret. Those
            // failures are repaired from the lenient shared POM model below;
            // only an unrepairable model makes the reactor incomplete.
            let mut model_failure = None;
            if project.parent.is_some() {
                if let Err(error) = self
                    .resolver
                    .resolve_parent_pom_with_budget(&mut project, &mut effective_model_budget)
                    .await
                {
                    warn!(
                        "Failed to resolve Maven parent for {}: {}",
                        project_dir.display(),
                        error
                    );
                    model_failure = Some(format!("parent POM: {error}"));
                }
            }
            if !project.bom_imports.is_empty() {
                if let Err(error) = self
                    .resolver
                    .resolve_bom_imports_with_budget(&mut project, &mut effective_model_budget)
                    .await
                {
                    warn!(
                        "Failed to resolve Maven BOMs for {}: {}",
                        project_dir.display(),
                        error
                    );
                    model_failure.get_or_insert(format!("imported BOM: {error}"));
                }
            }
            if !self.resolver.finalize_project_dependencies(&mut project) {
                model_failure.get_or_insert_with(|| {
                    "effective dependency coordinates remain unresolved".to_string()
                });
            }
            if let Some(failure) = model_failure {
                match self
                    .resolver
                    .repair_effective_model(&mut project, &pom_path)
                {
                    Ok(repair)
                        if repair.problems.is_empty()
                            && repair.determined
                            && project.unmodeled_features.is_empty() =>
                    {
                        info!(
                            "Rebuilt the effective Maven model of {} after: {failure}",
                            project_dir.display()
                        );
                    }
                    Ok(repair) => {
                        warn!(
                            "Maven effective model for {} is not fully determined ({failure}): {:?}",
                            project_dir.display(),
                            repair.problems
                        );
                        model_issues.push(format!(
                            "{}: effective model incomplete ({failure})",
                            pom_path.display()
                        ));
                        // Missing ancestors can also carry modules, source
                        // directories and profiles: not exhaustive.
                        if !repair.problems.is_empty() {
                            complete = false;
                        }
                    }
                    Err(error) => {
                        warn!(
                            "Maven effective model for {} cannot be built: {error} (after: {failure})",
                            project_dir.display()
                        );
                        model_issues.push(format!(
                            "{}: effective model cannot be built ({error})",
                            pom_path.display()
                        ));
                        complete = false;
                    }
                }
            }

            for module in project.modules.clone() {
                let module = project.resolve_property(&module);
                if module.contains("${") {
                    warn!(
                        "Maven module path remains dynamic in {}: {}",
                        project_dir.display(),
                        module
                    );
                    complete = false;
                    continue;
                }
                let module_dir = project_dir.join(module);
                let module_root = module_dir
                    .canonicalize()
                    .unwrap_or_else(|_| module_dir.clone());
                if !module_root.starts_with(&workspace_root) {
                    warn!(
                        "Maven module {} is outside the watched workspace {}",
                        module_root.display(),
                        workspace_root.display()
                    );
                    complete = false;
                    continue;
                }
                if !module_root.join("pom.xml").is_file() {
                    warn!(
                        "Declared Maven module has no POM: {}",
                        module_root.display()
                    );
                    complete = false;
                    continue;
                }
                pending.push_back(module_root);
            }

            projects.push(project);
        }

        Ok((projects, complete, model_issues))
    }

    /// Initialize Maven support for the workspace
    ///
    /// This method:
    /// 1. Parses the Maven project
    /// 2. Resolves dependencies
    /// 3. Returns source paths, JAR paths, javadoc JARs, and resolution report
    pub async fn initialize(&mut self) -> Result<MavenInitResult> {
        if !self.is_maven_workspace() {
            self.require_supported_workspace_layout().await?;
            debug!("Not a Maven workspace, skipping Maven initialization");
            return Ok(MavenInitResult {
                source_paths: Vec::new(),
                jar_paths: Vec::new(),
                jars_with_docs: Vec::new(),
                resolution_report: ResolutionReport {
                    resolved_jars: Vec::new(),
                    missing_dependencies: Vec::new(),
                    complete: true,
                    main_jars: Vec::new(),
                    issues: Vec::new(),
                },
                index_complete: true,
                dependency_issues: Vec::new(),
                modules: Vec::new(),
            });
        }

        info!("Initializing Maven support...");

        let (reactor_projects, reactor_complete, mut dependency_issues) =
            self.load_reactor_projects().await?;
        let mut project = reactor_projects
            .first()
            .cloned()
            .ok_or("Maven reactor did not contain its root project")?;
        info!(
            "Parsed Maven reactor '{}' with {} projects",
            project.artifact_id,
            reactor_projects.len()
        );

        let reactor_coordinates: HashSet<_> = reactor_projects
            .iter()
            .filter_map(|reactor_project| {
                Some((
                    reactor_project.group_id.clone()?,
                    reactor_project.artifact_id.clone(),
                    reactor_project.version.clone()?,
                ))
            })
            .collect();
        let mut aggregate_dependencies = Vec::new();
        for reactor_project in &reactor_projects {
            for dependency in &reactor_project.dependencies {
                let is_reactor_dependency = dependency.version.as_ref().is_some_and(|version| {
                    reactor_coordinates.contains(&(
                        dependency.group_id.clone(),
                        dependency.artifact_id.clone(),
                        version.clone(),
                    ))
                });
                if !is_reactor_dependency && !aggregate_dependencies.contains(dependency) {
                    aggregate_dependencies.push(dependency.clone());
                }
            }
        }
        project.dependencies = aggregate_dependencies;

        // Resolve every module on its own (sharing one POM cache) to learn
        // which JARs each source set can see. Reactor siblings are not in any
        // repository: they contribute source roots, outputs and their JARs.
        let module_index: std::collections::HashMap<(String, String, String), usize> =
            reactor_projects
                .iter()
                .enumerate()
                .filter_map(|(index, reactor_project)| {
                    Some((
                        (
                            reactor_project.group_id.clone()?,
                            reactor_project.artifact_id.clone(),
                            reactor_project.version.clone()?,
                        ),
                        index,
                    ))
                })
                .collect();
        let mut reactor_dependencies = Vec::new();
        let mut module_projects = Vec::new();
        for (index, reactor_project) in reactor_projects.iter().enumerate() {
            let mut own = reactor_project.clone();
            let mut edges = Vec::new();
            own.dependencies.retain(|dependency| {
                let sibling = dependency.version.as_ref().and_then(|version| {
                    module_index.get(&(
                        dependency.group_id.clone(),
                        dependency.artifact_id.clone(),
                        version.clone(),
                    ))
                });
                match sibling {
                    Some(sibling) if *sibling != index => {
                        edges.push((*sibling, dependency.scope.clone()));
                        false
                    }
                    Some(_) => false,
                    None => true,
                }
            });
            reactor_dependencies.push(edges);
            module_projects.push(own);
        }
        let mut module_reports = self
            .resolver
            .resolve_dependencies_for_modules(&module_projects, usize::MAX)
            .await;

        // Opt-in: ask Maven itself for the exact classpaths. This runs the
        // project's build plugins, so it only happens when the user enabled it.
        let mut tool_jars: Option<Vec<PathBuf>> = None;
        if crate::build_tool_classpath::is_enabled() {
            let module_dirs: Vec<PathBuf> = reactor_projects
                .iter()
                .map(|reactor_project| reactor_project.root_dir.clone())
                .collect();
            match crate::build_tool_classpath::maven_classpaths(&self.workspace_root, &module_dirs)
                .await
            {
                Ok(tool) => {
                    dependency_issues.extend(tool.problems.iter().cloned());
                    let mut all = Vec::new();
                    for (index, module) in module_projects.iter().enumerate() {
                        let Some(exact) = tool.module_for(&module.root_dir) else {
                            continue;
                        };
                        let main = crate::build_tool_classpath::jar_entries(&exact.main.compile);
                        let test = crate::build_tool_classpath::jar_entries(&exact.test.compile);
                        for jar in main.iter().chain(&test) {
                            if !all.contains(jar) {
                                all.push(jar.clone());
                            }
                        }
                        let report = &mut module_reports[index];
                        report.resolved_jars = test;
                        report.main_jars = main;
                        report.missing_dependencies.clear();
                        report.issues.clear();
                        report.complete = true;
                    }
                    tool_jars = Some(all);
                }
                Err(error) => {
                    warn!("Maven classpath resolution failed: {error}");
                    dependency_issues.push(format!("Maven classpath resolution failed: {error}"));
                }
            }
        }
        let modules =
            build_module_classpaths(&module_projects, &module_reports, &reactor_dependencies);

        let mut paths_to_index = Vec::new();
        let mut jar_paths = Vec::new();
        let mut jars_with_docs = Vec::new();

        // Add source directories for every recursively discovered module.
        // Generated roots must be explicit because they are normally
        // gitignored and sit below build-output directories.
        for module_path in reactor_projects.iter().map(|project| &project.root_dir) {
            for source_set in ["main", "test"] {
                paths_to_index.extend(hyperion_layout::existing_source_dirs(
                    module_path,
                    source_set,
                ));
            }
            paths_to_index.extend(generated_source_roots(module_path));
        }
        paths_to_index.sort_unstable();
        paths_to_index.dedup();

        info!("Found {} source paths to index", paths_to_index.len());

        // Resolve dependencies with detailed report
        let mut resolution_report = self
            .resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;
        if let Some(exact) = tool_jars {
            // Maven's own answer replaces the cache-derived aggregate.
            resolution_report.resolved_jars = exact.clone();
            resolution_report.main_jars = exact;
            resolution_report.missing_dependencies.clear();
            resolution_report.issues.clear();
            resolution_report.complete = true;
        }
        dependency_issues.extend(resolution_report.issues.iter().cloned());
        dependency_issues.extend(resolution_report.missing_dependencies.iter().map(|missing| {
            format!("{} is not in the local Maven repository (run 'mvn dependency:resolve' to download it)", missing.coordinates)
        }));

        // Add resolved JARs
        jar_paths.extend(mandatory_resolved_jars(&resolution_report));
        info!("Found {} dependency JARs to index", jar_paths.len());

        // Log missing dependencies (will be converted to diagnostics by caller)
        if !resolution_report.missing_dependencies.is_empty() {
            warn!(
                "Missing {} dependencies - these will be reported as diagnostics",
                resolution_report.missing_dependencies.len()
            );
        }

        // Try to resolve javadoc JARs
        info!("Resolving javadoc JARs for dependencies...");
        match self.resolver.resolve_dependencies_with_docs(&project).await {
            Ok(resolved_with_docs) => {
                jars_with_docs = resolved_with_docs;
                info!(
                    "Resolved {} dependencies with javadoc",
                    jars_with_docs.len()
                );
            }
            Err(e) => {
                warn!("Failed to resolve javadoc JARs: {}", e);
                // Continue without javadoc - not critical
            }
        }

        self.project = Some(project);

        info!(
            "Maven initialization complete, {} source paths, {} JAR paths, {} with javadoc to index",
            paths_to_index.len(),
            jar_paths.len(),
            jars_with_docs.len()
        );

        Ok(MavenInitResult {
            source_paths: paths_to_index,
            jar_paths,
            jars_with_docs,
            // Dependency problems degrade individual libraries; only an
            // unreadable reactor makes workspace-wide facts non-exhaustive.
            index_complete: reactor_complete,
            dependency_issues,
            modules,
            resolution_report,
        })
    }

    /// Get the parsed Maven project
    pub const fn project(&self) -> Option<&MavenProject> {
        self.project.as_ref()
    }
}

fn mandatory_resolved_jars(report: &ResolutionReport) -> Vec<PathBuf> {
    // A resolver-declared binary is mandatory even if it disappears before
    // staging. Preserve the path so the generation fails instead of being
    // published as a truncated success.
    report.resolved_jars.clone()
}

/// Initialize Maven support for a workspace and update the workspace index
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
/// Maven initialization result including source paths and resolution report, or an error
pub async fn initialize_maven_support(
    workspace_root: &Path,
    workspace_index: Arc<WorkspaceIndex>,
) -> Result<MavenInitResult> {
    initialize_maven_support_internal(
        workspace_root,
        SemanticHost::new(workspace_index),
        None,
        None,
        crate::project_classpath::compatibility_store(),
    )
    .await
}

/// Initialize Maven and coherently replace its external source provider.
pub(crate) async fn initialize_maven_support_with_sources(
    workspace_root: &Path,
    host: Arc<SemanticHost>,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
    bulk_source_index: crate::server::source_indexing::BulkSourceIndexContext,
    project_classpaths: crate::project_classpath::ProjectClasspathStore,
) -> Result<MavenInitResult> {
    initialize_maven_support_internal(
        workspace_root,
        host,
        Some(source_providers),
        Some(bulk_source_index),
        project_classpaths,
    )
    .await
}

async fn index_maven_source_roots(
    host: &Arc<SemanticHost>,
    source_paths: Vec<PathBuf>,
    bulk_source_index: Option<crate::server::source_indexing::BulkSourceIndexContext>,
) -> bool {
    if let Some(context) = bulk_source_index {
        return match crate::server::source_indexing::index_build_source_roots_off_runtime(
            context,
            source_paths,
            false,
        )
        .await
        {
            Ok(outcome) => {
                let stats = outcome.stats;
                info!(
                    "Indexed Maven sources: {} files, {} symbols",
                    stats.files_indexed, stats.symbols_indexed
                );
                outcome.source_scan_failures == 0
                    && stats.files_failed == 0
                    && outcome.open_documents_restored
            }
            Err(error) => {
                warn!("Maven source indexing task failed: {error}");
                false
            }
        };
    }

    let mut files = Vec::new();
    let mut scan_complete = true;
    for source_path in source_paths {
        match WorkspaceScanner::scan_dir_ignore_gitignore(&source_path) {
            Ok(found) => files.extend(found),
            Err(error) => {
                warn!(
                    "Maven source scan failed at {}: {error}",
                    source_path.display()
                );
                scan_complete = false;
            }
        }
    }
    files.sort_unstable();
    files.dedup();
    let stats = match host
        .write()
        .await
        .run_blocking(move |txn| txn.index().index_files_parallel_skip_cached(&files))
        .await
    {
        Ok(stats) => stats,
        Err(error) => {
            warn!("Maven source indexing task failed: {error}");
            return false;
        }
    };
    if scan_complete {
        info!(
            "Indexed Maven sources: {} files, {} symbols",
            stats.files_indexed, stats.symbols_indexed
        );
    }
    scan_complete && stats.files_failed == 0
}

async fn initialize_maven_support_internal(
    workspace_root: &Path,
    host: Arc<SemanticHost>,
    source_providers: Option<Arc<crate::source_provider::SourceProviderChain>>,
    bulk_source_index: Option<crate::server::source_indexing::BulkSourceIndexContext>,
    project_classpaths: crate::project_classpath::ProjectClasspathStore,
) -> Result<MavenInitResult> {
    let mut maven_integration = MavenIntegration::new(workspace_root.to_path_buf());
    let workspace_index = Arc::clone(host.index());

    if !maven_integration.is_maven_workspace() {
        crate::handlers::build_system::retire_build_system(
            crate::handlers::build_system::BuildManifestFamily::Maven,
            workspace_root,
            &host,
            source_providers.as_ref(),
            &project_classpaths,
        )
        .await?;
        maven_integration
            .require_supported_workspace_layout()
            .await?;
        debug!("Not a Maven workspace");
        return Ok(MavenInitResult {
            source_paths: Vec::new(),
            jar_paths: Vec::new(),
            jars_with_docs: Vec::new(),
            resolution_report: ResolutionReport {
                resolved_jars: Vec::new(),
                missing_dependencies: Vec::new(),
                complete: true,
                main_jars: Vec::new(),
                issues: Vec::new(),
            },
            index_complete: true,
            dependency_issues: Vec::new(),
            modules: Vec::new(),
        });
    }

    let refresh_ticket = host
        .write()
        .await
        .index()
        .begin_external_source_refresh("build:maven");
    let source_refresh = match &source_providers {
        Some(providers) => Some(
            providers
                .observe_group_refresh_guard(&refresh_ticket)
                .ok_or("Maven source-provider refresh was superseded")?,
        ),
        None => None,
    };
    let classpath_refresh = project_classpaths
        .begin_project_classpath_refresh_guard(workspace_root, &refresh_ticket)
        .ok_or("Maven project-classpath refresh was superseded")?;

    // Initialize Maven and get paths to index
    let mut init_result = maven_integration.initialize().await?;
    let source_paths = init_result.source_paths.clone();
    // This classification deliberately happens before source-root indexing;
    // only reactor/dependency completeness controls archive membership.
    let publication_attempt =
        complete_maven_publication_attempt(&init_result, source_refresh, classpath_refresh);
    let CompleteMavenPublicationAttempt {
        model:
            CompleteMavenDependencyModel {
                jar_paths: plan_jars,
                jars_with_docs: plan_docs,
            },
        source_refresh,
        classpath_refresh,
    } = match publication_attempt {
        Some(attempt) => attempt,
        None => {
            // Local source roots and diagnostics remain useful, but no external
            // dependency state from this incomplete model is authoritative.
            let _source_index_complete =
                index_maven_source_roots(&host, source_paths, bulk_source_index).await;
            debug_assert!(!init_result.index_complete);
            return Ok(init_result);
        }
    };
    // Stage per-module classpaths and source-set visibility under this
    // refresh's authority; they become request-visible atomically with the
    // dependency snapshot.
    stage_maven_module_state(classpath_refresh.ticket(), &init_result.modules);
    // Capture every archive identity only after the resolver proved the
    // complete membership set. The blocking transaction then uses this one
    // immutable plan for membership, classpath, extraction, and providers.
    if let Some(context) = &bulk_source_index {
        context
            .report_build_progress(format!(
                "Preparing {} Maven dependency archives and source attachments...",
                plan_jars.len()
            ))
            .await;
    }
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
            .map(|provider| Arc::new(provider) as Arc<dyn crate::source_provider::SourceProvider>)
        });
        Ok((archive_plan, staged_provider))
    })
    .await
    .map_err(|error| format!("Maven archive planning task failed: {error}"))??;
    let external_archives: Vec<_> = archive_plan
        .iter()
        .map(|plan| plan.archive.clone())
        .collect();
    let source_identities: Vec<_> = external_archives
        .iter()
        .map(|archive| archive.identity().clone())
        .collect();
    let known_jars: Vec<_> = external_archives
        .iter()
        .filter_map(|archive| archive.identity().archive_path().map(Path::to_path_buf))
        .collect();
    // Cache the fully-resolved dependency JARs (jar_paths ∪ the JAR side of
    // jars_with_docs) so the "▶ Run" project-classpath path in
    // `run_executor` can look them up later without re-running Maven --
    // this only runs for confirmed Maven workspaces (the `is_maven_workspace`
    // check above returns early otherwise), so it never clobbers a Gradle
    // workspace's cached entry for the same root with an empty list.
    // Membership, side-classpath state, archive extraction, and source-provider
    // publication form one cancellation-safe blocking transaction. Dropping the
    // async JoinHandle detaches this closure instead of dropping either refresh
    // guard halfway through a committed generation.
    if let Some(context) = &bulk_source_index {
        context
            .report_build_progress(format!(
                "Indexing {} Maven dependency JARs and Javadocs...",
                archive_plan.len()
            ))
            .await;
    }
    let transaction_index = Arc::clone(&workspace_index);
    let transaction_host = Arc::clone(&host);
    let transaction = tokio::task::spawn_blocking(
        move || -> Result<MavenArchiveTransactionResult> {
            let host = transaction_host;
            let mut source_refresh = source_refresh;
            // Each publication step below is its own short write transaction,
            // so requests and editor notifications interleave between archives
            // instead of waiting for the whole dependency set. Callback-heavy
            // provider preparation happens off-gate. Retry from current
            // fallback state if a predecessor progresses before this
            // transaction acquires the owner gate.
            let (provider_publication, external_refresh) = host.publish_blocking(|txn| {
                let (publication_guard, provider_publication) =
                    crate::source_provider::begin_coordinated_external_publication(
                        txn.index(),
                        &refresh_ticket,
                        source_providers.as_ref(),
                        &source_identities,
                        staged_provider,
                    )
                    .map_err(|error| format!("Maven archive publication failed: {error}"))?;
                let Some(external_refresh) = txn
                    .index()
                    .replace_external_archives_for_refresh(&refresh_ticket, &external_archives)
                else {
                    let retirement = source_providers.as_ref().and_then(|providers| {
                        provider_publication.as_ref().and_then(|publication| {
                            providers.abort_group_publication_deferred(publication)
                        })
                    });
                    drop(publication_guard);
                    drop(retirement);
                    return Err::<_, Box<dyn std::error::Error + Send + Sync>>(
                        "Maven refresh was superseded before archive extraction".into(),
                    );
                };
                drop(publication_guard);
                Ok((provider_publication, external_refresh))
            })?;

            // Snapshot and extract one binary/Javadoc pair at a time. Besides
            // bounding temporary memory to one pair, keeping this loop in the
            // transaction means cancellation cannot strand a pending provider
            // publication while detached archive children continue independently.
            let mut javadocs_loaded = 0usize;
            let mut javadocs_failed = 0usize;
            let mut folded_results = Vec::with_capacity(archive_plan.len());
            for plan in archive_plan {
                let source_identity = plan.archive.identity().clone();
                let jar = plan.archive.path().to_path_buf();
                let requested_javadoc = plan.javadoc.is_some();
                let Some(revision) = external_refresh.revision_for_identity(&source_identity)
                else {
                    warn!(
                        "Maven archive identity changed before publication: {}",
                        jar.display()
                    );
                    folded_results.push(Ok(Err(())));
                    continue;
                };

                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::source_provider::prepare_dependency_archive(
                        &transaction_index,
                        &plan,
                        revision,
                    )
                }));

                match result {
                    Ok(Ok(prepared)) => {
                        let mut stats = host.publish_blocking(|txn| -> Result<_> {
                            let archive_publication = txn
                                .index()
                                .begin_committed_external_source_publication(&refresh_ticket)
                                .ok_or("Maven archive publication was superseded")?;
                            let stats = txn.index().publish_prepared_jar(prepared)?;
                            if let (Some(providers), Some(publication)) =
                                (&source_providers, &provider_publication)
                            {
                                if !providers.record_group_publication_success_for_revision(
                                    publication,
                                    &source_identity,
                                    revision,
                                ) {
                                    return Err(
                                        "Maven source-provider publication was superseded".into()
                                    );
                                }
                            }
                            drop(archive_publication);
                            Ok(stats)
                        })?;
                        if requested_javadoc
                            && !stats.documentation_loaded
                            && stats.documentation_error.is_none()
                        {
                            stats.documentation_error =
                                Some("Javadoc attachment was not loaded".to_string());
                        }
                        if stats.documentation_loaded {
                            javadocs_loaded += 1;
                        } else if requested_javadoc {
                            javadocs_failed += 1;
                            warn!(
                                "Javadoc attachment for {} was unavailable: {}",
                                jar.display(),
                                stats
                                    .documentation_error
                                    .as_deref()
                                    .unwrap_or("unknown error")
                            );
                        }
                        folded_results.push(Ok(Ok(stats.classes_indexed)));
                    }
                    Ok(Err(error)) => {
                        warn!("Failed to index JAR {}: {error}", jar.display());
                        folded_results.push(Ok(Err(())));
                    }
                    Err(payload) => {
                        error!(
                            "Maven JAR indexing for {} panicked: {}",
                            jar.display(),
                            panic_payload_message(payload.as_ref())
                        );
                        folded_results.push(Err(()));
                    }
                }
            }
            let totals = fold_jar_results(folded_results);

            // Do filesystem work outside the owner gate. Version-fenced
            // classpath lookups repeat this check after releasing the store
            // lock, so a replacement racing publication still fails closed.
            let archives_current = external_archives
                .iter()
                .all(hyperion_workspace::ExternalSourceArchive::staged_version_is_current);
            if !archives_current {
                warn!("Maven dependency archives changed before classpath publication");
            }
            let archives_complete =
                maven_execution_archives_are_complete(archives_current, &totals);
            let classpath_published = host.publish_blocking(|txn| -> Result<Option<bool>> {
                let mut retirement = None;
                let publication_guard = txn
                    .index()
                    .begin_committed_external_source_publication(&refresh_ticket)
                    .ok_or("Maven publication was superseded before completion")?;
                if let (Some(providers), Some(publication)) =
                    (&source_providers, &provider_publication)
                {
                    retirement = Some(
                        providers
                            .finish_group_publication_deferred(publication)
                            .ok_or("Maven source-provider publication was superseded")?,
                    );
                }
                // Bind every staged module classpath to the canonical archive
                // generation captured for this transaction, then publish them
                // together with the aggregate dependency snapshot and the
                // source scopes that reference them.
                let prepared = if archives_complete {
                    crate::project_classpath::bind_staged_gradle_classpaths_to_archives_for_refresh(
                        classpath_refresh.ticket(),
                        known_jars.clone(),
                        known_jars.clone(),
                        &external_archives,
                    )
                    .ok()
                } else {
                    None
                };
                let classpath_retirement = prepared.and_then(|(compile, runtime, archives)| {
                    crate::project_classpath::publish_gradle_dependency_snapshot_with_archives_deferred_for_refresh(
                        classpath_refresh.ticket(),
                        compile,
                        runtime,
                        archives,
                    )
                });
                let classpath_published =
                    archives_complete.then_some(classpath_retirement.is_some());
                if classpath_published == Some(true)
                    && !txn.index().publish_source_scopes_for_refresh(
                        &refresh_ticket,
                        classpath_refresh.ticket().published_source_scopes(),
                    )
                {
                    return Err("Maven source scopes were superseded before publication".into());
                }
                drop(publication_guard);
                drop(retirement);
                drop(classpath_retirement);
                Ok(classpath_published)
            })?;
            if let Some(source_refresh) = source_refresh.take() {
                source_refresh.disarm();
            }
            if classpath_published == Some(false) {
                return Err("Maven project-classpath refresh was superseded".into());
            }
            if classpath_published == Some(true) {
                classpath_refresh.disarm();
            }

            Ok(MavenArchiveTransactionResult {
                totals,
                javadocs_loaded,
                javadocs_failed,
            })
        },
    );
    let MavenArchiveTransactionResult {
        totals,
        javadocs_loaded,
        javadocs_failed,
    } = transaction
        .await
        .map_err(|error| format!("Maven archive transaction panicked: {error}"))??;

    if totals.tasks_panicked > 0 || totals.tasks_failed > 0 {
        warn!(
            "Indexed {} JARs with {} classes; {} JARs failed to index, {} archive extractions panicked",
            totals.jars_indexed,
            totals.classes_total,
            totals.tasks_failed,
            totals.tasks_panicked
        );
    } else {
        info!(
            "Indexed {} JARs with {} classes",
            totals.jars_indexed, totals.classes_total
        );
    }
    if javadocs_loaded > 0 || javadocs_failed > 0 {
        info!(
            "Maven Javadoc attachments: {} loaded, {} unavailable",
            javadocs_loaded, javadocs_failed
        );
    }

    // The root workspace scan finds normal Maven sources, but not gitignored
    // generated output. Scan every Maven-declared source root explicitly after
    // the archive transaction is fully published, and let skip-cached indexing
    // make overlap with the root scan cheap.
    let source_index_complete =
        index_maven_source_roots(&host, source_paths, bulk_source_index).await;

    init_result.index_complete = init_result.index_complete
        && source_index_complete
        && totals.tasks_failed == 0
        && totals.tasks_panicked == 0;

    Ok(init_result)
}

/// Tallies for the Maven JAR indexing result-fold.
///
/// `tasks_panicked` preserves the historical public log terminology, but now
/// counts archive-local unwinds caught inside the single blocking transaction.
#[derive(Debug, Default, PartialEq, Eq)]
struct JarIndexTotals {
    jars_indexed: usize,
    classes_total: usize,
    tasks_failed: usize,
    tasks_panicked: usize,
}

const fn maven_execution_archives_are_complete(
    archives_current: bool,
    totals: &JarIndexTotals,
) -> bool {
    archives_current && totals.tasks_failed == 0 && totals.tasks_panicked == 0
}

#[derive(Debug)]
struct MavenArchiveTransactionResult {
    totals: JarIndexTotals,
    javadocs_loaded: usize,
    javadocs_failed: usize,
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "non-string panic payload"
    }
}

/// Fold the per-archive results into success/failure totals.
///
/// Pulled out into a standalone helper so the success, ordinary failure, and
/// caught-panic classifications can be tested without real archives.
///
/// Note: we have to use `std::result::Result` here explicitly because this
/// module declares its own `pub type Result<T> = ...` alias.
fn fold_jar_results<I>(iter: I) -> JarIndexTotals
where
    I: IntoIterator<Item = std::result::Result<std::result::Result<usize, ()>, ()>>,
{
    let mut totals = JarIndexTotals::default();
    for result in iter {
        match result {
            Ok(Ok(classes)) => {
                totals.jars_indexed += 1;
                totals.classes_total += classes;
            }
            Ok(Err(())) => {
                totals.tasks_failed += 1;
            }
            Err(()) => {
                totals.tasks_panicked += 1;
            }
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use crate::semantic_host::SemanticHost;
    use hyperion_build::maven::{DependencyScope, MavenProject, ResolutionReport};

    use super::{
        build_module_classpaths, complete_maven_publication_attempt, fold_jar_results,
        initialize_maven_support, initialize_maven_support_internal, mandatory_resolved_jars,
        maven_execution_archives_are_complete, JarIndexTotals, MavenInitResult,
    };

    fn report_with_jars(main: &[&str], all: &[&str]) -> ResolutionReport {
        ResolutionReport {
            resolved_jars: all.iter().map(PathBuf::from).collect(),
            missing_dependencies: Vec::new(),
            complete: true,
            main_jars: main.iter().map(PathBuf::from).collect(),
            issues: Vec::new(),
        }
    }

    #[test]
    fn generated_sources_are_visible_without_exposing_dependency_test_sources() {
        let temp = TempDir::new().unwrap();
        let roots = [temp.path().join("library"), temp.path().join("app")];
        for root in &roots {
            for source in [
                "src/main/java",
                "src/test/java",
                "target/generated-sources/demo",
                "target/generated-test-sources/demo",
                "apt_generated",
            ] {
                std::fs::create_dir_all(root.join(source)).unwrap();
            }
        }
        let modules = roots
            .iter()
            .enumerate()
            .map(|(i, root)| MavenProject::new(format!("module{i}"), root.clone()))
            .collect::<Vec<_>>();
        let reports = vec![report_with_jars(&[], &[]), report_with_jars(&[], &[])];
        let classpaths = build_module_classpaths(
            &modules,
            &reports,
            &[vec![], vec![(0, DependencyScope::Compile)]],
        );
        let app = &classpaths[1];
        for root in &roots {
            for source in ["target/generated-sources", "apt_generated"] {
                assert!(
                    app.main_visible_source_roots.contains(&root.join(source)),
                    "missing production root: {}",
                    root.join(source).display()
                );
                assert!(app.test_visible_source_roots.contains(&root.join(source)));
            }
            assert!(!app
                .main_visible_source_roots
                .contains(&root.join("target/generated-test-sources")));
        }
        assert!(app
            .test_source_roots
            .contains(&roots[1].join("target/generated-test-sources")));
        assert!(app
            .test_visible_source_roots
            .contains(&roots[1].join("target/generated-test-sources")));
        assert!(!app
            .test_visible_source_roots
            .contains(&roots[0].join("target/generated-test-sources")));
    }

    #[test]
    fn module_classpaths_separate_test_jars_and_include_reactor_dependencies() {
        let temp = TempDir::new().expect("temporary reactor");
        let core_root = temp.path().join("core");
        let app_root = temp.path().join("app");
        for root in [&core_root, &app_root] {
            std::fs::create_dir_all(root.join("src/main/java")).unwrap();
            std::fs::create_dir_all(root.join("src/test/java")).unwrap();
        }
        let modules = vec![
            MavenProject::new("core".to_string(), core_root.clone()),
            MavenProject::new("app".to_string(), app_root.clone()),
        ];
        let reports = vec![
            report_with_jars(&["/m2/guava.jar"], &["/m2/guava.jar"]),
            report_with_jars(&["/m2/jackson.jar"], &["/m2/jackson.jar", "/m2/junit.jar"]),
        ];
        // app -> core (compile)
        let edges = vec![Vec::new(), vec![(0, DependencyScope::Compile)]];

        let classpaths = build_module_classpaths(&modules, &reports, &edges);

        let app = &classpaths[1];
        // Production sources see core's sources and JARs, never junit.
        assert_eq!(
            app.main_visible_source_roots,
            vec![
                app_root.join("src/main/java"),
                core_root.join("src/main/java")
            ]
        );
        assert_eq!(
            app.main_compile_jars,
            vec![
                PathBuf::from("/m2/jackson.jar"),
                PathBuf::from("/m2/guava.jar")
            ]
        );
        // Tests add junit and the module's own main output.
        assert!(app
            .test_compile_jars
            .contains(&PathBuf::from("/m2/junit.jar")));
        assert!(app
            .test_output_dirs
            .contains(&app_root.join("target/classes")));
        assert!(app
            .main_compile_output_dirs
            .contains(&core_root.join("target/classes")));
        assert_eq!(
            classpaths[0].main_compile_jars,
            vec![PathBuf::from("/m2/guava.jar")]
        );
    }
    use hyperion_workspace::{ExternalSourceIdentity, ExternalSourceRevisionToken, WorkspaceIndex};
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn compile_dependency_jar(root: &std::path::Path) -> PathBuf {
        let source_dir = root.join("src/dep");
        let classes_dir = root.join("classes");
        std::fs::create_dir_all(&source_dir).expect("create Java source directory");
        std::fs::create_dir_all(&classes_dir).expect("create Java classes directory");
        let source_path = source_dir.join("Type.java");
        let source = b"package dep; public class Type {}";
        std::fs::write(&source_path, source).expect("write Java source fixture");
        let output = Command::new("javac")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&source_path)
            .output()
            .expect("run javac for dependency fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let jar_path = root.join("dependency.jar");
        let mut jar = zip::ZipWriter::new(
            std::fs::File::create(&jar_path).expect("create binary dependency JAR"),
        );
        jar.start_file("dep/Type.class", zip::write::SimpleFileOptions::default())
            .expect("start binary class entry");
        jar.write_all(
            &std::fs::read(classes_dir.join("dep/Type.class")).expect("read compiled class"),
        )
        .expect("write binary class entry");
        jar.finish().expect("finish binary dependency JAR");

        let mut sources = zip::ZipWriter::new(
            std::fs::File::create(root.join("dependency-sources.jar"))
                .expect("create source dependency JAR"),
        );
        sources
            .start_file("dep/Type.java", zip::write::SimpleFileOptions::default())
            .expect("start source entry");
        sources.write_all(source).expect("write source entry");
        sources.finish().expect("finish source dependency JAR");
        jar_path
    }

    fn publish_complete_maven_r1(
        root: &std::path::Path,
        jar: &std::path::Path,
        index: &Arc<WorkspaceIndex>,
        providers: &Arc<crate::source_provider::SourceProviderChain>,
        project_classpaths: &crate::project_classpath::ProjectClasspathStore,
    ) -> (ExternalSourceIdentity, ExternalSourceRevisionToken) {
        let refresh_ticket = index.begin_external_source_refresh("build:maven");
        let source_refresh = providers
            .observe_group_refresh_guard(&refresh_ticket)
            .expect("begin Maven provider refresh");
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(root, &refresh_ticket)
            .expect("begin Maven classpath refresh");
        let archive_plan =
            crate::source_provider::capture_dependency_archive_plan(&[jar.to_path_buf()], &[])
                .expect("capture complete Maven archive");
        let external_archives: Vec<_> = archive_plan
            .iter()
            .map(|plan| plan.archive.clone())
            .collect();
        let identity = external_archives[0].identity().clone();
        let staged_provider =
            crate::source_provider::JarSourceProvider::from_binary_archives_with_store(
                &external_archives,
                providers.materialized_sources(),
            )
            .map(|provider| Arc::new(provider) as Arc<dyn crate::source_provider::SourceProvider>);
        let (publication_guard, provider_publication) =
            crate::source_provider::begin_coordinated_external_publication(
                index,
                &refresh_ticket,
                Some(providers),
                std::slice::from_ref(&identity),
                staged_provider,
            )
            .expect("begin complete Maven publication");
        let external_refresh = index
            .replace_external_archives_for_refresh(&refresh_ticket, &external_archives)
            .expect("commit complete Maven membership");
        drop(publication_guard);
        let revision = external_refresh
            .revision_for_identity(&identity)
            .expect("exact Maven archive revision");
        let prepared =
            crate::source_provider::prepare_dependency_archive(index, &archive_plan[0], revision)
                .expect("prepare complete Maven archive");
        let archive_guard = index
            .begin_committed_external_source_publication(&refresh_ticket)
            .expect("publish complete Maven archive");
        index
            .publish_prepared_jar(prepared)
            .expect("install complete Maven symbols");
        assert!(providers.record_group_publication_success_for_revision(
            provider_publication
                .as_ref()
                .expect("Maven provider publication"),
            &identity,
            revision,
        ));
        drop(archive_guard);

        let publication_guard = index
            .begin_committed_external_source_publication(&refresh_ticket)
            .expect("finalize complete Maven publication");
        let provider_retirement = providers
            .finish_group_publication_deferred(
                provider_publication
                    .as_ref()
                    .expect("Maven provider publication"),
            )
            .expect("finish complete Maven provider");
        let classpath_retirement = crate::project_classpath::publish_maven_dependency_snapshot_with_archives_deferred_for_refresh(
            classpath_refresh.ticket(),
            vec![identity
                .archive_path()
                .expect("dependency identity is an archive")
                .to_path_buf()],
            external_archives,
        )
        .expect("publish complete Maven classpath");
        drop(publication_guard);
        drop(provider_retirement);
        drop(classpath_retirement);
        source_refresh.disarm();
        classpath_refresh.disarm();
        (identity, revision)
    }

    #[test]
    fn resolver_declared_missing_jar_is_preserved_for_mandatory_staging() {
        let missing = PathBuf::from("/definitely/missing/resolver-declared.jar");
        let report = hyperion_build::maven::ResolutionReport {
            resolved_jars: vec![missing.clone()],
            missing_dependencies: Vec::new(),
            complete: true,
            main_jars: Vec::new(),
            issues: Vec::new(),
        };

        assert_eq!(mandatory_resolved_jars(&report), vec![missing]);
    }

    #[test]
    fn failed_maven_archives_never_authorize_an_execution_classpath() {
        let clean = JarIndexTotals::default();
        assert!(!maven_execution_archives_are_complete(false, &clean));
        assert!(!maven_execution_archives_are_complete(
            true,
            &JarIndexTotals {
                tasks_failed: 1,
                ..JarIndexTotals::default()
            },
        ));
        assert!(maven_execution_archives_are_complete(true, &clean));
    }

    #[test]
    fn incomplete_maven_refresh_preserves_exact_last_good_generation() {
        let temp = TempDir::new().expect("temporary Maven workspace");
        let jar = compile_dependency_jar(temp.path());
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let project_classpaths = crate::project_classpath::ProjectClasspathStore::new();
        let (identity, r1_revision) =
            publish_complete_maven_r1(temp.path(), &jar, &index, &providers, &project_classpaths);
        let expected_jar = identity
            .archive_path()
            .expect("dependency identity is an archive")
            .to_path_buf();
        let r1_location = index.find_symbol("dep.Type").expect("R1 symbol");
        assert_eq!(
            index.external_source_revision(&r1_location),
            Some(r1_revision)
        );
        assert!(providers
            .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
            .is_some());
        assert_eq!(
            project_classpaths.known_dependency_jars(temp.path()),
            Some(vec![expected_jar.clone()])
        );

        let r2 = index.begin_external_source_refresh("build:maven");
        let source_refresh = Some(
            providers
                .observe_group_refresh_guard(&r2)
                .expect("observe incomplete R2"),
        );
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(temp.path(), &r2)
            .expect("stage incomplete R2 classpath");
        let incomplete = MavenInitResult {
            source_paths: Vec::new(),
            jar_paths: Vec::new(),
            jars_with_docs: Vec::new(),
            resolution_report: hyperion_build::maven::ResolutionReport {
                resolved_jars: Vec::new(),
                missing_dependencies: Vec::new(),
                complete: false,
                main_jars: Vec::new(),
                issues: Vec::new(),
            },
            index_complete: false,
            dependency_issues: Vec::new(),
            modules: Vec::new(),
        };
        assert!(
            complete_maven_publication_attempt(&incomplete, source_refresh, classpath_refresh,)
                .is_none()
        );

        let retained = index.find_symbol("dep.Type").expect("retained R1 symbol");
        assert_eq!(index.external_source_revision(&retained), Some(r1_revision));
        assert!(providers
            .get_source_exact_for_revision(&identity, r1_revision, "dep.Type")
            .is_some());
        assert_eq!(
            project_classpaths.known_dependency_jars(temp.path()),
            Some(vec![expected_jar])
        );
    }

    #[test]
    fn first_incomplete_maven_refresh_publishes_no_dependency_generation() {
        let temp = TempDir::new().expect("temporary Maven workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let project_classpaths = crate::project_classpath::ProjectClasspathStore::new();
        let r1 = index.begin_external_source_refresh("build:maven");
        let source_refresh = Some(
            providers
                .observe_group_refresh_guard(&r1)
                .expect("observe incomplete R1"),
        );
        let classpath_refresh = project_classpaths
            .begin_project_classpath_refresh_guard(temp.path(), &r1)
            .expect("stage incomplete R1 classpath");
        let incomplete = MavenInitResult {
            source_paths: Vec::new(),
            jar_paths: vec![temp.path().join("partial.jar")],
            jars_with_docs: Vec::new(),
            resolution_report: hyperion_build::maven::ResolutionReport {
                resolved_jars: Vec::new(),
                missing_dependencies: Vec::new(),
                complete: false,
                main_jars: Vec::new(),
                issues: Vec::new(),
            },
            index_complete: false,
            dependency_issues: Vec::new(),
            modules: Vec::new(),
        };

        assert!(
            complete_maven_publication_attempt(&incomplete, source_refresh, classpath_refresh,)
                .is_none()
        );
        assert!(index.get_external_symbols().is_empty());
        assert_eq!(project_classpaths.known_dependency_jars(temp.path()), None);
        assert!(!providers.can_provide_exact(
            &ExternalSourceIdentity::for_archive_path(&temp.path().join("partial.jar")),
            "dep.Type",
        ));
    }

    #[tokio::test]
    async fn removed_root_manifest_retires_maven_state_before_nested_layout_error() {
        let temp = TempDir::new().expect("temp nested-only Maven workspace");
        let root = temp.path();
        let stale_jar = root.join("stale-maven.jar");
        let store = crate::project_classpath::ProjectClasspathStore::new();
        let index = Arc::new(WorkspaceIndex::new(root.to_path_buf()));
        let previous = index.begin_external_source_refresh("build:maven");
        let previous_classpath = store
            .begin_project_classpath_refresh(root, &previous)
            .expect("previous Maven classpath refresh begins");
        let _ = index
            .replace_external_sources_for_refresh(&previous, std::slice::from_ref(&stale_jar))
            .expect("previous Maven classpath commits");
        assert!(
            crate::project_classpath::publish_maven_dependency_snapshot_for_refresh(
                &previous_classpath,
                vec![stale_jar],
            )
        );
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/pom.xml"), "<project />").unwrap();

        let result = initialize_maven_support_internal(
            root,
            SemanticHost::new(Arc::clone(&index)),
            None,
            None,
            store.clone(),
        )
        .await;
        let error = result
            .err()
            .expect("a nested-only Maven build remains unsupported");

        assert!(error.to_string().contains("nested or uninspectable Maven"));
        assert_eq!(store.known_dependency_jars(root), None);
        let gradle = index.begin_external_source_refresh("build:gradle");
        assert!(store
            .begin_project_classpath_refresh(root, &gradle)
            .is_some());
    }

    #[tokio::test]
    async fn maven_generated_source_is_discovered_and_indexed() {
        let temp = TempDir::new().expect("temp Maven workspace");
        std::fs::write(
            temp.path().join("pom.xml"),
            r"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>generated-source-test</artifactId>
  <version>1.0.0</version>
</project>",
        )
        .expect("write pom.xml");

        let generated_root = temp.path().join("target/generated-sources/annotations");
        std::fs::create_dir_all(generated_root.join("com/example"))
            .expect("create generated package");
        std::fs::write(
            generated_root.join("com/example/GeneratedType.java"),
            "package com.example; public class GeneratedType {}",
        )
        .expect("write generated Java source");

        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let result = initialize_maven_support(temp.path(), Arc::clone(&index))
            .await
            .expect("initialize Maven support");

        assert!(result
            .source_paths
            .iter()
            .any(|path| path == &temp.path().join("target/generated-sources")));
        assert!(index.find_symbol("com.example.GeneratedType").is_some());
    }

    #[tokio::test]
    async fn missing_parent_metadata_keeps_maven_index_incomplete() {
        let temp = TempDir::new().expect("temp Maven workspace");
        std::fs::write(
            temp.path().join("pom.xml"),
            r"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.hyperion.fixture</groupId>
    <artifactId>definitely-missing-parent</artifactId>
    <version>0.0.0-hyperion-test</version>
  </parent>
  <artifactId>child</artifactId>
</project>",
        )
        .expect("write pom.xml");
        for language in ["kotlin", "groovy", "scala"] {
            std::fs::create_dir_all(temp.path().join("src/main").join(language))
                .expect("create language source root");
        }

        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let result = initialize_maven_support(temp.path(), index)
            .await
            .expect("partial Maven metadata remains useful");

        assert!(!result.index_complete);
        for language in ["kotlin", "groovy", "scala"] {
            assert!(result
                .source_paths
                .contains(&temp.path().join("src/main").join(language)));
        }
    }

    #[tokio::test]
    async fn recursively_declared_reactor_modules_are_indexed() {
        let temp = TempDir::new().expect("temp Maven reactor");
        std::fs::create_dir_all(temp.path().join("module-a/module-b/src/main/java/example"))
            .unwrap();
        std::fs::write(
            temp.path().join("pom.xml"),
            r"<project><modelVersion>4.0.0</modelVersion><groupId>example</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>module-a</module></modules></project>",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("module-a/pom.xml"),
            r"<project><modelVersion>4.0.0</modelVersion><parent><groupId>example</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>module-a</artifactId><packaging>pom</packaging><modules><module>module-b</module></modules></project>",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("module-a/module-b/pom.xml"),
            r"<project><modelVersion>4.0.0</modelVersion><parent><groupId>example</groupId><artifactId>module-a</artifactId><version>1</version></parent><artifactId>module-b</artifactId></project>",
        )
        .unwrap();
        std::fs::write(
            temp.path()
                .join("module-a/module-b/src/main/java/example/Nested.java"),
            "package example; public class Nested {}",
        )
        .unwrap();

        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let result = initialize_maven_support(temp.path(), Arc::clone(&index))
            .await
            .expect("initialize recursive reactor");

        assert!(result.index_complete);
        assert!(index.find_symbol("example.Nested").is_some());
    }

    #[test]
    fn fold_counts_successes_failures_and_panics() {
        // Mix of success, indexing failure, and a caught archive-local panic.
        let results: Vec<std::result::Result<std::result::Result<usize, ()>, ()>> = vec![
            Ok(Ok(10)),
            Ok(Ok(25)),
            Ok(Err(())), // Archive indexing failed.
            Err(()),     // Archive extraction panicked (caller already logged).
            Ok(Ok(5)),
        ];

        let totals = fold_jar_results(results);
        assert_eq!(
            totals,
            JarIndexTotals {
                jars_indexed: 3,
                classes_total: 40,
                tasks_failed: 1,
                tasks_panicked: 1,
            }
        );
    }

    #[test]
    fn fold_does_not_silently_swallow_failures() {
        // The bug we fixed: `flatten().flatten()` would have made tasks_failed=0
        // even though a JAR failed. Verify the new fold preserves the count.
        let results: Vec<std::result::Result<std::result::Result<usize, ()>, ()>> =
            vec![Ok(Err(())), Ok(Err(()))];
        let totals = fold_jar_results(results);
        assert_eq!(totals.jars_indexed, 0);
        assert_eq!(totals.tasks_failed, 2);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn documented_jar_task_preserves_non_utf8_javadoc_path() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let temp = tempfile::tempdir().unwrap();
        let jar = temp.path().join("dependency.jar");
        let javadoc = temp.path().join(std::ffi::OsString::from_vec(
            b"dependency-\x80-javadoc.jar".to_vec(),
        ));
        std::fs::write(&jar, b"binary").unwrap();
        std::fs::write(&javadoc, b"documentation").unwrap();

        let plans = crate::source_provider::capture_dependency_archive_plan(
            &[],
            &[(jar.clone(), javadoc.clone())],
        )
        .expect("dependency archive plan");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].archive.path(), jar);
        let owned_javadoc = plans[0]
            .javadoc
            .as_ref()
            .expect("captured production Javadoc attachment")
            .path();
        assert_eq!(
            owned_javadoc.as_os_str().as_bytes(),
            javadoc.as_os_str().as_bytes()
        );
        let lossy = std::path::PathBuf::from(javadoc.to_string_lossy().into_owned());
        assert_ne!(
            owned_javadoc, lossy,
            "the former display/string round-trip must remain observably lossy"
        );
    }
}
