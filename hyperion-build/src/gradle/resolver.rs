// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle dependency resolution.
//!
//! This module resolves Gradle dependencies to actual JAR files by:
//! 1. Checking local Gradle cache (~/.gradle/caches)
//! 2. Checking local Maven repository (~/.m2/repository)
//! 3. Resolving transitive dependencies (depth-limited)
//! 4. Reporting missing artifacts without performing network access

use super::model::{GradleDependency, GradleProject};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Errors that can occur during dependency resolution
#[derive(Debug, thiserror::Error)]
pub enum GradleResolverError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Dependency not found: {0}")]
    DependencyNotFound(String),

    #[error("Invalid dependency: {0}")]
    InvalidDependency(String),

    #[error("Cache error: {0}")]
    CacheError(String),
}

pub type Result<T> = std::result::Result<T, GradleResolverError>;

const DEPENDENCY_GRAPH_ROOT: &str = "<project>";

/// How a dependency entered the resolved dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyEdgeKind {
    /// A dependency declared by the project itself.
    Direct,
    /// A dependency declared by another resolved dependency's POM.
    Transitive,
}

/// A directed edge between exact `group:artifact:version` coordinates.
///
/// Direct dependencies originate at an internal project-root marker. Consumers
/// should use [`GradleResolutionReport::explain_path`] rather than depending on
/// that implementation detail.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyEdgeKind,
}

impl DependencyEdge {
    /// Record a dependency declared directly by the project.
    pub fn direct(to: impl Into<String>) -> Self {
        Self {
            from: DEPENDENCY_GRAPH_ROOT.to_owned(),
            to: to.into(),
            kind: DependencyEdgeKind::Direct,
        }
    }

    /// Record a dependency declared by another resolved dependency's POM.
    pub fn transitive(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind: DependencyEdgeKind::Transitive,
        }
    }
}

/// Honest accounting for cache-only dependency resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GradleResolutionReport {
    /// Whether dependency resolution covered every required dependency and
    /// every relevant transitive metadata edge.
    pub complete: bool,
    /// Why the offline model cannot prove completeness. Entries are sorted and unique.
    pub incomplete_reasons: Vec<GradleIncompleteReason>,
    /// Non-platform dependency declarations that should produce a JAR.
    pub declared: usize,
    /// Direct declarations resolved to a JAR.
    pub resolved: usize,
    /// Direct declarations that could not be resolved by the offline model.
    pub missing: Vec<String>,
    /// Platform/BOM declarations, which intentionally do not produce JARs.
    pub platforms: usize,
    /// Versionless declarations resolved by selecting an installed cache version.
    pub cache_selected_versions: usize,
    /// Unique transitive JARs in addition to direct JARs.
    pub transitive_jars: usize,
    /// Unique resolved direct and transitive JAR paths.
    pub jar_paths: Vec<PathBuf>,
    /// Resolved identities for optional documentation lookup without another graph walk.
    pub resolved_artifacts: Vec<(GradleDependency, PathBuf)>,
    /// Resolved main/runtime JARs in resolver order.
    pub main_jars: Vec<PathBuf>,
    /// Resolved test-runtime JARs in resolver order.
    pub test_jars: Vec<PathBuf>,
    /// Main-source-set compile classpath JARs, including compile-only and
    /// annotation-processor dependencies.
    pub main_compile_jars: Vec<PathBuf>,
    /// Main-source-set runtime classpath JARs.
    pub main_runtime_jars: Vec<PathBuf>,
    /// Test-source-set compile classpath JARs.
    pub test_compile_jars: Vec<PathBuf>,
    /// Test-source-set runtime classpath JARs.
    pub test_runtime_jars: Vec<PathBuf>,
    /// Main compile dependencies exported to downstream projects.
    pub main_api_jars: Vec<PathBuf>,
    /// Test compile dependencies exported to downstream test projects.
    pub test_api_jars: Vec<PathBuf>,
    /// Each resolved direct declaration paired with its originating declaration
    /// index and the coordinates actually used for resolution. The version can
    /// be populated from a BOM or cache for a versionless declaration, so
    /// consumers can preserve its configuration without matching another
    /// same-GA declaration by accident.
    pub direct_jars: Vec<(usize, GradleDependency, PathBuf)>,
    /// Exact coordinate relationships observed while resolving POM metadata.
    ///
    /// The graph is intentionally attached to this report so artifact
    /// accounting and provenance cannot drift into separate models.
    pub dependency_edges: Vec<DependencyEdge>,
}

/// A fact that prevents an offline Gradle resolution from being authoritative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GradleIncompleteReason {
    /// A platform/BOM could not be fully read; versions it manages may be unknown.
    BomMetadata(String),
    UnsupportedIdentity(String),
    UnprovenVersion(String),
    VersionUnavailable(String),
    ArtifactResolutionFailed {
        identity: String,
        error: String,
    },
    /// A module's POM (or an ancestor/import) is missing or unreadable, so its
    /// transitive dependencies are not fully known. The module's own JAR is unaffected.
    PomMetadata {
        module: String,
        problem: String,
    },
    /// A POM dependency whose version or artifact kind cannot be determined offline.
    UnresolvedTransitive {
        declared_by: String,
        dependency: String,
        problem: String,
    },
    ConflictingVersions(String),
    DepthLimit(String),
    ContextLimit,
}

impl std::fmt::Display for GradleIncompleteReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BomMetadata(problem) => write!(f, "BOM metadata is incomplete: {problem}"),
            Self::UnsupportedIdentity(id) => write!(f, "unsupported artifact identity {id}"),
            Self::UnprovenVersion(id) => write!(f, "selected version is unproven for {id}"),
            Self::VersionUnavailable(id) => write!(f, "no BOM or installed version for {id}"),
            Self::ArtifactResolutionFailed { identity, error } => {
                if error.starts_with("Dependency not found") {
                    write!(
                        f,
                        "{identity} is not in the local Gradle or Maven cache (build the project once to download it)"
                    )
                } else {
                    write!(f, "could not resolve {identity}: {error}")
                }
            }
            Self::PomMetadata { module, problem } => {
                write!(f, "POM metadata for {module} is incomplete: {problem}")
            }
            Self::UnresolvedTransitive {
                declared_by,
                dependency,
                problem,
            } => write!(f, "{dependency} (required by {declared_by}): {problem}"),
            Self::ConflictingVersions(id) => write!(f, "conflicting versions for {id}"),
            Self::DepthLimit(id) => write!(f, "transitive depth limit reached at {id}"),
            Self::ContextLimit => write!(f, "transitive resolution context limit reached"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DependencyScope {
    compile: bool,
    runtime: bool,
    test: bool,
    api: bool,
}

/// Default safety limit for transitive dependency traversal.
pub const DEFAULT_TRANSITIVE_DEPTH: usize = 64;
const MAX_RESOLUTION_CONTEXTS: usize = 16_384;

type Exclusions = Vec<(String, String)>;
type PendingDependency = (
    GradleDependency,
    usize,
    DependencyScope,
    Exclusions,
    Option<String>,
);

/// Mutable state shared by one breadth-first transitive dependency walk.
///
/// Own the queue, per-load caches, and accumulated graph together. Nothing in
/// this state survives a refresh or leaks between projects with different BOMs.
#[derive(Default)]
struct TransitiveResolution {
    incomplete_reasons: BTreeSet<GradleIncompleteReason>,
    resolved_jars: Vec<PathBuf>,
    resolved_jar_set: HashSet<PathBuf>,
    configured_jars: Vec<(PathBuf, DependencyScope)>,
    configured_jar_set: HashSet<(PathBuf, DependencyScope)>,
    visited_contexts: usize,
    /// Versions Gradle's conflict resolution selected (highest wins) that
    /// override whatever a declaration or POM asked for.
    forced_versions: HashMap<(String, String), String>,
    /// Modules whose POM declares `pom` packaging: no JAR is expected.
    pom_only: HashSet<String>,
    /// Every version requested for a module during this walk, before forcing.
    requested_versions: HashMap<(String, String), BTreeSet<String>>,
    dependency_edges: Vec<DependencyEdge>,
    dependency_edge_set: HashSet<DependencyEdge>,
    bom_versions: std::collections::HashMap<String, String>,
    complete: bool,
    max_depth: usize,
    pending: VecDeque<PendingDependency>,
    /// Keyed by coordinates, scope, exclusions and whether children are walked, so a
    /// non-transitive declaration never shares a context with one that is.
    scheduled: HashMap<(String, DependencyScope, Exclusions, bool), usize>,
    metadata: HashMap<String, Arc<ModuleView>>,
    /// Declarations still waiting for a version a platform may supply.
    unversioned: HashMap<(String, String), Option<String>>,
    artifacts: HashMap<String, Option<PathBuf>>,
    resolved_artifacts: Vec<(GradleDependency, PathBuf)>,
}

impl TransitiveResolution {
    fn schedule(
        &mut self,
        mut dep: GradleDependency,
        depth: usize,
        scope: DependencyScope,
        mut exclusions: Exclusions,
        parent: Option<String>,
    ) {
        exclusions.sort();
        exclusions.dedup();
        let module = (dep.group.clone(), dep.artifact.clone());
        if let Some(version) = dep.version.clone() {
            self.requested_versions
                .entry(module.clone())
                .or_default()
                .insert(version);
        }
        if let Some(forced) = self.forced_versions.get(&module) {
            dep.version = Some(forced.clone());
        }
        if dep.version.is_none() {
            // A platform elsewhere in the graph may still supply a version;
            // the caller re-walks once it is known.
            self.unversioned.entry(module).or_insert(parent);
            return;
        }
        let key = (dep.coordinates(), scope, exclusions.clone(), dep.transitive);
        if let Some(scheduled_depth) = self.scheduled.get(&key) {
            // Preserve all graph edges, while charging a shared context only
            // once. Breadth-first scheduling guarantees its shortest depth.
            if *scheduled_depth <= self.max_depth {
                if let Some(parent) = parent {
                    let edge = DependencyEdge::transitive(parent, key.0);
                    if self.dependency_edge_set.insert(edge.clone()) {
                        self.dependency_edges.push(edge);
                    }
                }
            }
            return;
        }
        if self.pending.len() >= MAX_RESOLUTION_CONTEXTS {
            self.complete = false;
            self.incomplete_reasons
                .insert(GradleIncompleteReason::ContextLimit);
            return;
        }
        self.scheduled.insert(key, depth);
        self.pending
            .push_back((dep, depth, scope, exclusions, parent));
    }
}

/// What the graph walk needs to know about one module.
#[derive(Default)]
struct ModuleView {
    children: Vec<(GradleDependency, Exclusions)>,
    /// Platform constraints (`dependencyConstraints`) declared by this module.
    constraints: Vec<(String, String, String)>,
    /// The module publishes no JAR of its own (BOM, redirect, pom packaging).
    no_artifact: bool,
    /// Metadata could not be read at all.
    unknown: bool,
    problems: Vec<String>,
    /// Dependencies of this module that could not be turned into a request.
    unresolved_children: Vec<(String, String)>,
}

/// Versions managed by platforms/BOMs.
#[derive(Default)]
struct BomVersions {
    /// `group:artifact:type:classifier` -> version.
    versions: HashMap<String, String>,
    /// Concrete reasons some BOM could not be fully read.
    problems: Vec<String>,
    /// The Spring dependency-management plugin overrides transitive versions.
    overrides_transitive: bool,
}

fn bom_dependency_key(
    group: &str,
    artifact: &str,
    type_: Option<&str>,
    classifier: Option<&str>,
) -> String {
    crate::pom::management_key(group, artifact, type_, classifier)
}

impl DependencyScope {
    fn from_configuration(configuration: &super::model::DependencyConfiguration) -> Self {
        Self {
            compile: configuration.is_compile(),
            runtime: configuration.is_runtime(),
            test: configuration.is_test(),
            api: configuration.is_api(),
        }
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            compile: self.compile && other.compile,
            runtime: self.runtime && other.runtime,
            test: self.test,
            api: self.api
                && (other.api
                    || matches!(
                        other,
                        Self {
                            compile: true,
                            runtime: true,
                            test: _,
                            api: false,
                        }
                    )),
        }
    }

    fn is_empty(self) -> bool {
        !self.compile && !self.runtime
    }
}

impl GradleResolutionReport {
    pub fn missing_count(&self) -> usize {
        self.missing.len()
    }

    /// Return the shortest known dependency path to exact coordinates.
    ///
    /// The project-root marker is omitted, so a direct dependency returns a
    /// one-element path. `None` means the report contains no complete edge path
    /// to the target; no provenance is guessed from JAR names or declarations.
    pub fn explain_path(&self, coordinates: &str) -> Option<Vec<String>> {
        let mut predecessors: HashMap<&str, Option<&str>> = HashMap::new();
        let mut queue = VecDeque::from([DEPENDENCY_GRAPH_ROOT]);
        predecessors.insert(DEPENDENCY_GRAPH_ROOT, None);

        while let Some(parent) = queue.pop_front() {
            for edge in self.dependency_edges.iter().filter(|edge| {
                edge.from == parent
                    && matches!(
                        (parent == DEPENDENCY_GRAPH_ROOT, edge.kind),
                        (true, DependencyEdgeKind::Direct)
                            | (false, DependencyEdgeKind::Transitive)
                    )
            }) {
                if predecessors.contains_key(edge.to.as_str()) {
                    continue;
                }
                predecessors.insert(edge.to.as_str(), Some(parent));
                if edge.to == coordinates {
                    let mut path = vec![edge.to.as_str()];
                    let mut current = parent;
                    while current != DEPENDENCY_GRAPH_ROOT {
                        path.push(current);
                        current = predecessors.get(current).copied().flatten()?;
                    }
                    path.reverse();
                    return Some(path.into_iter().map(str::to_owned).collect());
                }
                queue.push_back(edge.to.as_str());
            }
        }

        None
    }
}

/// Gradle dependency resolver
pub struct GradleResolver {
    /// Gradle module cache (~/.gradle/caches/modules-2/files-2.1)
    gradle_cache_dir: PathBuf,

    /// Maven local repository (~/.m2/repository)
    maven_local_dir: PathBuf,

    /// Effective-POM loader over both repositories.
    poms: crate::pom::PomModelCache,

    /// Loaders restricted to one repository. A POM must come from the
    /// repository that supplied the artifact: another repository's POM can
    /// describe different bytes with the same coordinates.
    gradle_poms: crate::pom::PomModelCache,
    maven_poms: crate::pom::PomModelCache,
}

impl GradleResolver {
    /// Create a new resolver with default cache directories
    pub fn new() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());

        let gradle_cache =
            hyperion_bytecode::gradle::gradle_user_home().join("caches/modules-2/files-2.1");

        let maven_local = PathBuf::from(&home).join(".m2").join("repository");

        Self::with_cache_dirs(gradle_cache, maven_local)
    }

    /// Create a resolver with custom cache directories
    pub fn with_cache_dirs(gradle_cache: PathBuf, maven_local: PathBuf) -> Self {
        let poms = crate::pom::PomModelCache::new(vec![
            crate::pom::PomRepository::GradleFiles(gradle_cache.clone()),
            crate::pom::PomRepository::MavenLocal(maven_local.clone()),
        ]);
        let gradle_poms =
            crate::pom::PomModelCache::new(vec![crate::pom::PomRepository::GradleFiles(
                gradle_cache.clone(),
            )]);
        let maven_poms =
            crate::pom::PomModelCache::new(vec![crate::pom::PomRepository::MavenLocal(
                maven_local.clone(),
            )]);
        Self {
            gradle_cache_dir: gradle_cache,
            maven_local_dir: maven_local,
            poms,
            gradle_poms,
            maven_poms,
        }
    }

    /// Resolve all dependencies for a project (including transitive)
    pub async fn resolve_dependencies(&self, project: &GradleProject) -> Result<Vec<PathBuf>> {
        self.resolve_dependencies_with_depth(project, DEFAULT_TRANSITIVE_DEPTH)
            .await
    }

    /// Resolve dependencies with specified transitive depth
    ///
    /// depth=0: Only direct dependencies
    /// depth=1: Direct + their dependencies
    /// depth=2: Direct + 2 levels of transitive
    pub async fn resolve_dependencies_with_depth(
        &self,
        project: &GradleProject,
        max_depth: usize,
    ) -> Result<Vec<PathBuf>> {
        let report = self
            .resolve_dependencies_with_report(project, max_depth)
            .await?;
        if !report.complete {
            return Err(GradleResolverError::CacheError(format!(
                "dependency resolution incomplete for project '{}'",
                project.name
            )));
        }
        Ok(report.jar_paths)
    }

    /// Resolve dependencies and retain exact declared/resolved/missing counts.
    ///
    /// Resolution degrades per dependency: whatever can be located is
    /// returned, and every dependency that cannot be located or fully
    /// understood is named in `incomplete_reasons` with the concrete cause.
    pub async fn resolve_dependencies_with_report(
        &self,
        project: &GradleProject,
        max_depth: usize,
    ) -> Result<GradleResolutionReport> {
        info!(
            "Resolving {} dependencies for project '{}' (depth={})",
            project.dependencies.len(),
            project.name,
            max_depth
        );

        // Every resolution rereads POMs: a refresh must see edited files.
        self.poms.clear();
        self.gradle_poms.clear();
        self.maven_poms.clear();

        // Build BOM version map from platform dependencies
        let bom = self.build_bom_version_map(project);
        let bom_versions = bom.versions;

        let mut declarations: Vec<(usize, GradleDependency, DependencyScope)> = Vec::new();
        let mut versionless_count = 0;
        let mut bom_resolved_count = 0;
        let mut direct_jars = HashSet::new();
        let mut report = GradleResolutionReport {
            complete: true,
            declared: project
                .dependencies
                .iter()
                .filter(|dependency| !dependency.is_platform)
                .count(),
            platforms: project
                .dependencies
                .iter()
                .filter(|dependency| dependency.is_platform)
                .count(),
            ..GradleResolutionReport::default()
        };
        let mut preparation_complete = true;
        let mut preparation_reasons: BTreeSet<GradleIncompleteReason> = BTreeSet::new();
        for problem in bom.problems {
            preparation_complete = false;
            preparation_reasons.insert(GradleIncompleteReason::BomMetadata(problem));
        }

        let mut project_dependencies: Vec<GradleDependency> = project.dependencies.clone();
        let mut implicit_declarations = Vec::new();
        if let Some(stdlib) = self.implicit_kotlin_stdlib(project) {
            implicit_declarations.push(stdlib);
        }
        // The Spring Boot Gradle plugin adds the JUnit Platform launcher to
        // `testRuntimeOnly` for projects that run JUnit tests.
        if project.has_plugin("org.springframework.boot")
            && project_dependencies.iter().any(|dependency| {
                dependency.configuration.is_test()
                    && (dependency.artifact == "spring-boot-starter-test"
                        || dependency.group == "org.junit.jupiter")
            })
            && !project_dependencies
                .iter()
                .any(|dependency| dependency.artifact == "junit-platform-launcher")
            && bom_versions.contains_key(&bom_dependency_key(
                "org.junit.platform",
                "junit-platform-launcher",
                None,
                None,
            ))
        {
            implicit_declarations.push(GradleDependency {
                exclusions: Vec::new(),
                transitive: true,
                forced: false,
                group: "org.junit.platform".into(),
                artifact: "junit-platform-launcher".into(),
                version: None,
                configuration: super::model::DependencyConfiguration::TestRuntimeOnly,
                classifier: None,
                packaging: None,
                is_platform: false,
            });
        }
        let first_implicit_index = project_dependencies.len();
        project_dependencies.extend(implicit_declarations);
        report.declared += project_dependencies.len() - first_implicit_index;

        for (declaration_index, dep) in project_dependencies.iter().enumerate() {
            // Skip platform dependencies themselves - they're just BOMs
            if dep.is_platform {
                debug!("Skipping platform BOM: {}", dep.coordinates());
                continue;
            }
            if Self::validate_artifact_identity(dep).is_err() {
                warn!(
                    "Unsupported direct artifact identity {}; keeping resolution incomplete",
                    dep.full_coordinates()
                );
                report.missing.push(dep.full_coordinates());
                preparation_complete = false;
                preparation_reasons.insert(GradleIncompleteReason::UnsupportedIdentity(
                    dep.full_coordinates(),
                ));
                continue;
            }

            // For versionless dependencies, try to resolve version from BOM
            let mut resolved_dep = dep.clone();
            if resolved_dep.version.is_none() {
                versionless_count += 1;
                let key = format!("{}:{}", dep.group, dep.artifact);

                if let Some(version) = self.resolve_version_from_bom(
                    &bom_dependency_key(
                        &dep.group,
                        &dep.artifact,
                        dep.packaging.as_deref(),
                        dep.classifier.as_deref(),
                    ),
                    &bom_versions,
                ) {
                    info!(
                        "Resolved versionless dependency {} from BOM to version {}",
                        key, version
                    );
                    resolved_dep.version = Some(version);
                    bom_resolved_count += 1;
                } else {
                    // A platform in the graph or, failing that, the installed
                    // versions decide; both are consulted after the first walk.
                    debug!("Deferring version of {key} until the graph is known");
                }
            }

            let scope = DependencyScope::from_configuration(&dep.configuration);
            declarations.push((declaration_index, resolved_dep, scope));
        }

        // Gradle keeps one version per module: the highest requested. A walk
        // records every requested version; when a module was requested at more
        // than one version the winner is forced and the walk repeats (each
        // repetition can only raise versions, so this converges).
        let mut forced_versions = HashMap::new();
        if bom.overrides_transitive {
            for (key, version) in &bom_versions {
                // key = group:artifact:type:classifier; only plain jars.
                let mut parts = key.split(':');
                if let (Some(group), Some(artifact), Some("jar"), Some("")) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                {
                    forced_versions
                        .insert((group.to_string(), artifact.to_string()), version.clone());
                }
            }
            // A version written at the declaration site wins over the BOM.
            for (_, dep, _) in &declarations {
                if let Some(version) = &dep.version {
                    if project.dependencies.iter().any(|declared| {
                        declared.group == dep.group
                            && declared.artifact == dep.artifact
                            && declared.version.is_some()
                    }) {
                        forced_versions
                            .insert((dep.group.clone(), dep.artifact.clone()), version.clone());
                    }
                }
            }
        }
        // A declaration marked `force` keeps its version even when conflict
        // resolution would select a different one.
        for (_, dep, _) in &declarations {
            if let (true, Some(version)) = (dep.forced, &dep.version) {
                forced_versions.insert((dep.group.clone(), dep.artifact.clone()), version.clone());
            }
        }
        let enforced: HashSet<(String, String)> = forced_versions.keys().cloned().collect();
        let mut unresolvable: HashSet<(String, String)> = HashSet::new();
        let mut transitive;
        let mut iterations = 0;
        loop {
            iterations += 1;
            transitive = TransitiveResolution {
                bom_versions: bom_versions.clone(),
                forced_versions: forced_versions.clone(),
                complete: preparation_complete,
                incomplete_reasons: preparation_reasons.clone(),
                max_depth,
                ..TransitiveResolution::default()
            };
            for (_, dep, scope) in &declarations {
                let direct_edge = DependencyEdge::direct(dep.coordinates());
                if transitive.dependency_edge_set.insert(direct_edge.clone()) {
                    transitive.dependency_edges.push(direct_edge);
                }
                transitive.schedule(dep.clone(), 0, *scope, dep.exclusions.clone(), None);
            }

            // Queue every declaration before walking children: each scope/exclusion
            // context is visited through its shortest path, independently of declaration order.
            while let Some((dep, depth, scope, exclusions, parent)) = transitive.pending.pop_front()
            {
                self.resolve_transitive_internal(
                    &dep,
                    &mut transitive,
                    depth,
                    scope,
                    exclusions,
                    parent,
                )
                .await?;
            }

            // Platforms are constraints on every module they manage that is in
            // the graph, transitive edges included: the highest request wins.
            let managed_requests: Vec<((String, String), String)> = transitive
                .requested_versions
                .keys()
                .filter_map(|(group, artifact)| {
                    bom_versions
                        .get(&bom_dependency_key(group, artifact, None, None))
                        .map(|version| ((group.clone(), artifact.clone()), version.clone()))
                })
                .collect();
            for (module, version) in managed_requests {
                transitive
                    .requested_versions
                    .entry(module)
                    .or_default()
                    .insert(version);
            }

            let mut changed = false;
            for (module, versions) in &transitive.requested_versions {
                // Versions enforced by the dependency-management plugin are final.
                if versions.len() < 2 || enforced.contains(module) {
                    continue;
                }
                let Some(highest) = versions
                    .iter()
                    .max_by(|left, right| crate::pom::compare_versions(left, right))
                else {
                    continue;
                };
                if forced_versions.get(module) != Some(highest) {
                    forced_versions.insert(module.clone(), highest.clone());
                    changed = true;
                }
            }
            if !changed {
                let waiting: Vec<_> = transitive
                    .unversioned
                    .iter()
                    .filter(|(module, _)| !forced_versions.contains_key(*module))
                    .map(|(module, parent)| (module.clone(), parent.clone()))
                    .collect();
                for (module, parent) in waiting {
                    // Another declaration or platform that names a version
                    // for the same module supplies it (Gradle merges them).
                    let constrained = transitive
                        .requested_versions
                        .get(&module)
                        .and_then(|versions| {
                            versions
                                .iter()
                                .max_by(|left, right| crate::pom::compare_versions(left, right))
                        })
                        .cloned();
                    if let Some(version) = constrained {
                        forced_versions.insert(module, version);
                        changed = true;
                        continue;
                    }
                    let probe = GradleDependency {
                        exclusions: Vec::new(),
                        transitive: true,
                        forced: false,
                        group: module.0.clone(),
                        artifact: module.1.clone(),
                        version: None,
                        configuration: super::model::DependencyConfiguration::Implementation,
                        classifier: None,
                        packaging: None,
                        is_platform: false,
                    };
                    if let Some(version) = self.select_installed_version(&probe)? {
                        info!(
                            "Selected installed version {version} for versionless dependency {}:{}",
                            module.0, module.1
                        );
                        if declarations
                            .iter()
                            .any(|(_, dep, _)| dep.group == module.0 && dep.artifact == module.1)
                        {
                            report.cache_selected_versions += 1;
                        }
                        // Installed files cannot prove which version Gradle selected.
                        preparation_complete = false;
                        preparation_reasons.insert(GradleIncompleteReason::UnprovenVersion(
                            format!("{}:{}:{version}", module.0, module.1),
                        ));
                        forced_versions.insert(module, version);
                        changed = true;
                    } else if unresolvable.insert(module.clone()) {
                        preparation_complete = false;
                        preparation_reasons.insert(GradleIncompleteReason::VersionUnavailable(
                            format!(
                                "{}:{}{}",
                                module.0,
                                module.1,
                                parent
                                    .map(|parent| format!(" (required by {parent})"))
                                    .unwrap_or_default()
                            ),
                        ));
                    }
                }
            }
            if !changed || iterations >= 12 {
                break;
            }
        }

        // Reasons found while examining the last walk's results.
        transitive
            .incomplete_reasons
            .extend(preparation_reasons.iter().cloned());
        transitive.complete &= preparation_complete;

        for (declaration_index, dep, _) in declarations {
            let mut dep = dep;
            if let Some(forced) = transitive
                .forced_versions
                .get(&(dep.group.clone(), dep.artifact.clone()))
            {
                dep.version = Some(forced.clone());
            }
            if dep.version.is_none() {
                report
                    .missing
                    .push(format!("{}:{}", dep.group, dep.artifact));
                transitive.complete = false;
            } else if let Some(Some(path)) = transitive.artifacts.get(&dep.coordinates()) {
                report.resolved += 1;
                direct_jars.insert(path.clone());
                report
                    .direct_jars
                    .push((declaration_index, dep, path.clone()));
            } else if transitive.pom_only.contains(&dep.coordinates()) {
                // A pom-packaged module has no JAR by design; its dependencies
                // are the resolution.
                report.resolved += 1;
            } else {
                report.missing.push(dep.coordinates());
                transitive.complete = false;
            }
        }

        report.transitive_jars = transitive
            .resolved_jars
            .iter()
            .filter(|path| !direct_jars.contains(*path))
            .count();
        report.jar_paths = transitive.resolved_jars;
        let paths_for = |include: fn(DependencyScope) -> bool| {
            let mut seen = HashSet::new();
            transitive
                .configured_jars
                .iter()
                .filter(|&(path, scope)| include(*scope) && seen.insert(path))
                .map(|(path, _)| path.clone())
                .collect()
        };
        report.main_compile_jars = paths_for(|scope| !scope.test && scope.compile);
        report.main_runtime_jars = paths_for(|scope| !scope.test && scope.runtime);
        report.main_api_jars = paths_for(|scope| !scope.test && scope.compile && scope.api);
        report.test_compile_jars = paths_for(|scope| scope.test && scope.compile);
        report.test_runtime_jars = paths_for(|scope| scope.test && scope.runtime);
        report.test_api_jars = paths_for(|scope| scope.test && scope.compile && scope.api);
        report.main_jars = report.main_runtime_jars.clone();
        report.test_jars = report.test_runtime_jars.clone();
        report.complete = transitive.complete && report.missing.is_empty();
        report
            .incomplete_reasons
            .extend(transitive.incomplete_reasons);
        // A missing artifact already explains a missing POM for the same module.
        let missing_artifacts: HashSet<String> = report
            .incomplete_reasons
            .iter()
            .filter_map(|reason| match reason {
                GradleIncompleteReason::ArtifactResolutionFailed { identity, .. } => {
                    Some(identity.clone())
                }
                _ => None,
            })
            .collect();
        report.incomplete_reasons.retain(|reason| {
            !matches!(
                reason,
                GradleIncompleteReason::PomMetadata { module, problem }
                    if missing_artifacts.contains(module) && problem.contains("is not in a local cache")
            )
        });
        report.incomplete_reasons.sort();
        report.incomplete_reasons.dedup();
        report.resolved_artifacts = transitive.resolved_artifacts;
        report.dependency_edges = transitive.dependency_edges;
        info!(
            "Gradle dependency resolution: declared={}, resolved={}, missing={}, platforms={}, unique_jars={}, transitive_jars={}, versionless={}, bom_versions={}, cache_selected_versions={}, walks={}",
            report.declared,
            report.resolved,
            report.missing_count(),
            report.platforms,
            report.jar_paths.len(),
            report.transitive_jars,
            versionless_count,
            bom_resolved_count,
            report.cache_selected_versions,
            iterations
        );

        Ok(report)
    }

    /// The Kotlin Gradle plugin adds `kotlin-stdlib` at the plugin's version to
    /// every JVM Kotlin project unless `kotlin.stdlib.default.dependency=false`.
    fn implicit_kotlin_stdlib(&self, project: &GradleProject) -> Option<GradleDependency> {
        let plugin = project.plugins.iter().find(|plugin| {
            matches!(
                plugin.id.as_str(),
                "org.jetbrains.kotlin.jvm" | "org.jetbrains.kotlin.android"
            )
        })?;
        if project.dependencies.iter().any(|dependency| {
            dependency.group == "org.jetbrains.kotlin" && dependency.artifact == "kotlin-stdlib"
        }) {
            return None;
        }
        let version = plugin.version.clone().or_else(|| {
            // The version is commonly declared once in the root project
            // (`apply false`); otherwise fall back to the newest installed.
            let probe = GradleDependency {
                exclusions: Vec::new(),
                transitive: true,
                forced: false,
                group: "org.jetbrains.kotlin".into(),
                artifact: "kotlin-stdlib".into(),
                version: None,
                configuration: super::model::DependencyConfiguration::Api,
                classifier: None,
                packaging: None,
                is_platform: false,
            };
            self.select_installed_version(&probe).ok().flatten()
        })?;
        Some(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.jetbrains.kotlin".into(),
            artifact: "kotlin-stdlib".into(),
            version: Some(version),
            configuration: super::model::DependencyConfiguration::Api,
            classifier: None,
            packaging: None,
            is_platform: false,
        })
    }

    /// Resolve version from BOM for a dependency key.
    ///
    /// Uses only exact entries parsed from the effective BOM metadata. A
    /// platform's own version or a same-group convention is not evidence that
    /// an arbitrary artifact is managed by that platform.
    fn resolve_version_from_bom(
        &self,
        key: &str,
        bom_versions: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        bom_versions.get(key).cloned()
    }

    /// Convert one effective-POM dependency into a resolvable Gradle
    /// dependency, or say precisely why it cannot be resolved offline.
    fn child_dependency(
        &self,
        pom_dependency: &crate::pom::PomDependency,
        bom_versions: &HashMap<String, String>,
    ) -> std::result::Result<(GradleDependency, Exclusions), String> {
        let configuration = match pom_dependency.scope.as_deref() {
            Some("runtime") => super::model::DependencyConfiguration::RuntimeOnly,
            None | Some("compile") => super::model::DependencyConfiguration::Implementation,
            Some(other) => return Err(format!("unsupported Maven scope '{other}'")),
        };
        if pom_dependency.classifier.is_some() {
            return Err("classified artifacts are not supported offline".to_string());
        }
        let packaging = match pom_dependency.type_.as_str() {
            "jar" | "bundle" | "maven-plugin" | "eclipse-plugin" => None,
            "pom" => Some("pom".to_string()),
            other => return Err(format!("unsupported artifact type '{other}'")),
        };
        let version = pom_dependency.version.clone().or_else(|| {
            self.resolve_version_from_bom(
                &crate::pom::management_key(
                    &pom_dependency.group,
                    &pom_dependency.artifact,
                    Some(&pom_dependency.type_),
                    None,
                ),
                bom_versions,
            )
        });
        let Some(version) = version else {
            return Err("no version is declared or managed".to_string());
        };
        Ok((
            GradleDependency {
                exclusions: Vec::new(),
                transitive: true,
                forced: false,
                group: pom_dependency.group.clone(),
                artifact: pom_dependency.artifact.clone(),
                version: Some(version),
                configuration,
                classifier: None,
                packaging,
                is_platform: false,
            },
            pom_dependency.exclusions.clone(),
        ))
    }

    /// Everything the graph walk needs to know about one module, from Gradle
    /// Module Metadata when the cache has it (as Gradle prefers) and from the
    /// effective POM otherwise.
    fn module_view(
        &self,
        dep: &GradleDependency,
        bom_versions: &HashMap<String, String>,
    ) -> ModuleView {
        let key = dep.coordinates();
        let mut view = ModuleView::default();
        let Some(version) = dep.version.as_deref() else {
            return view;
        };
        // Metadata comes from the repository that supplied the artifact.
        let poms = if self.find_in_gradle_cache(dep).ok().flatten().is_some() {
            &self.gradle_poms
        } else if self.find_in_maven_local(dep).ok().flatten().is_some() {
            &self.maven_poms
        } else {
            &self.poms
        };
        let pom = poms.effective(&dep.group, &dep.artifact, version);

        match poms.module_metadata(&dep.group, &dep.artifact, version) {
            Ok(Some(module)) => {
                view.constraints = module.constraints.clone();
                if let Some((group, artifact, target_version)) = &module.redirect {
                    // `available-at`: artifacts and dependencies live elsewhere.
                    view.no_artifact = true;
                    view.children.push((
                        GradleDependency {
                            exclusions: Vec::new(),
                            transitive: true,
                            forced: false,
                            group: group.clone(),
                            artifact: artifact.clone(),
                            version: Some(target_version.clone()),
                            configuration: super::model::DependencyConfiguration::Implementation,
                            classifier: None,
                            packaging: None,
                            is_platform: false,
                        },
                        Vec::new(),
                    ));
                    return view;
                }
                view.no_artifact = !module.publishes_files
                    && (!module.constraints.is_empty()
                        || pom.as_ref().is_ok_and(|pom| pom.packaging == "pom"));
                for child in &module.dependencies {
                    let configuration = match (child.api, child.runtime) {
                        (true, true) => super::model::DependencyConfiguration::Implementation,
                        (true, false) => super::model::DependencyConfiguration::CompileOnly,
                        _ => super::model::DependencyConfiguration::RuntimeOnly,
                    };
                    let version = child.version.clone().or_else(|| {
                        self.resolve_version_from_bom(
                            &bom_dependency_key(&child.group, &child.module, None, None),
                            bom_versions,
                        )
                    });
                    view.children.push((
                        GradleDependency {
                            exclusions: Vec::new(),
                            transitive: true,
                            forced: false,
                            group: child.group.clone(),
                            artifact: child.module.clone(),
                            version,
                            configuration,
                            classifier: None,
                            packaging: None,
                            is_platform: false,
                        },
                        child.exclusions.clone(),
                    ));
                }
                return view;
            }
            Ok(None) => {}
            Err(problem) => view.problems.push(problem),
        }

        match pom {
            Ok(effective) => {
                view.problems.extend(effective.problems.iter().cloned());
                view.no_artifact = effective.packaging == "pom";
                for pom_dependency in &effective.dependencies {
                    if pom_dependency.optional
                        || matches!(pom_dependency.scope.as_deref(), Some("test" | "provided"))
                    {
                        continue;
                    }
                    match self.child_dependency(pom_dependency, bom_versions) {
                        Ok(child) => view.children.push(child),
                        Err(problem) => view.unresolved_children.push((
                            format!("{}:{}", pom_dependency.group, pom_dependency.artifact),
                            problem,
                        )),
                    }
                }
            }
            Err(problem) => {
                view.problems.push(problem);
                view.unknown = true;
            }
        }
        let _ = key;
        view
    }

    /// Walk one dependency: locate its artifact, read its metadata, and
    /// schedule its transitive dependencies.
    async fn resolve_transitive_internal(
        &self,
        dep: &GradleDependency,
        state: &mut TransitiveResolution,
        current_depth: usize,
        scope: DependencyScope,
        inherited_exclusions: Vec<(String, String)>,
        parent: Option<String>,
    ) -> Result<()> {
        let key = dep.coordinates();
        if current_depth > state.max_depth {
            state.complete = false;
            state
                .incomplete_reasons
                .insert(GradleIncompleteReason::DepthLimit(key));
            return Ok(());
        }
        if let Some(parent) = &parent {
            let edge = DependencyEdge::transitive(parent.clone(), key.clone());
            if state.dependency_edge_set.insert(edge.clone()) {
                state.dependency_edges.push(edge);
            }
        }
        if state.visited_contexts >= MAX_RESOLUTION_CONTEXTS {
            state.complete = false;
            state
                .incomplete_reasons
                .insert(GradleIncompleteReason::ContextLimit);
            state.pending.clear();
            return Ok(());
        }
        state.visited_contexts += 1;

        // Cache parsed metadata only for this resolution. Scope and exclusions
        // affect traversal, not parsing; a later refresh must reread changed files.
        let view = if let Some(cached) = state.metadata.get(&key) {
            Arc::clone(cached)
        } else {
            let view = Arc::new(self.module_view(dep, &state.bom_versions));
            for problem in &view.problems {
                state.complete = false;
                state
                    .incomplete_reasons
                    .insert(GradleIncompleteReason::PomMetadata {
                        module: key.clone(),
                        problem: problem.clone(),
                    });
            }
            for (dependency, problem) in &view.unresolved_children {
                state.complete = false;
                state
                    .incomplete_reasons
                    .insert(GradleIncompleteReason::UnresolvedTransitive {
                        declared_by: key.clone(),
                        dependency: dependency.clone(),
                        problem: problem.clone(),
                    });
            }
            // Platform constraints take part in version selection without
            // adding nodes to the graph.
            for (group, artifact, version) in &view.constraints {
                state
                    .requested_versions
                    .entry((group.clone(), artifact.clone()))
                    .or_default()
                    .insert(version.clone());
            }
            state.metadata.insert(key.clone(), Arc::clone(&view));
            view
        };

        let no_artifact_by_design = dep.packaging.as_deref() == Some("pom") || view.no_artifact;
        let jar_path = if let Some(path) = state.artifacts.get(&key) {
            path.clone()
        } else if no_artifact_by_design {
            state.pom_only.insert(key.clone());
            state.artifacts.insert(key.clone(), None);
            None
        } else {
            let path = match self.resolve_dependency(dep).await {
                Ok(path) => Some(path),
                Err(error) => {
                    warn!("Failed to resolve {}: {}", dep.coordinates(), error);
                    state.incomplete_reasons.insert(
                        GradleIncompleteReason::ArtifactResolutionFailed {
                            identity: key.clone(),
                            error: error.to_string(),
                        },
                    );
                    None
                }
            };
            state.artifacts.insert(key.clone(), path.clone());
            path
        };
        if let Some(jar_path) = jar_path {
            if state.resolved_jar_set.insert(jar_path.clone()) {
                state.resolved_jars.push(jar_path.clone());
                state
                    .resolved_artifacts
                    .push((dep.clone(), jar_path.clone()));
            }
            if state.configured_jar_set.insert((jar_path.clone(), scope)) {
                state.configured_jars.push((jar_path, scope));
            }
        } else if !no_artifact_by_design {
            state.complete = false;
        }

        debug!(
            "Found {} transitive dependencies for {}",
            view.children.len(),
            dep.coordinates()
        );

        // `transitive = false` keeps this dependency's JAR (resolved above) but
        // does not walk into its children.
        if !dep.transitive {
            return Ok(());
        }

        let relevant_transitive_deps =
            view.children
                .iter()
                .filter_map(|(trans_dep, dependency_exclusions)| {
                    if inherited_exclusions.iter().any(|(group, artifact)| {
                        (group == "*" || group == &trans_dep.group)
                            && (artifact == "*" || artifact == &trans_dep.artifact)
                    }) {
                        return None;
                    }
                    let child_scope = scope.intersect(DependencyScope::from_configuration(
                        &trans_dep.configuration,
                    ));
                    if child_scope.is_empty() {
                        return None;
                    }
                    let mut child_exclusions = inherited_exclusions.clone();
                    for exclusion in dependency_exclusions {
                        if !child_exclusions.contains(exclusion) {
                            child_exclusions.push(exclusion.clone());
                        }
                    }
                    Some((trans_dep.clone(), child_scope, child_exclusions))
                });

        for (trans_dep, child_scope, child_exclusions) in relevant_transitive_deps {
            state.schedule(
                trans_dep,
                current_depth + 1,
                child_scope,
                child_exclusions,
                Some(key.clone()),
            );
        }

        Ok(())
    }

    /// Build a map of `group:artifact:type:classifier` -> version from
    /// platform/BOM dependencies.
    ///
    /// This method detects both:
    /// 1. Explicit platform() dependencies in build.gradle
    /// 2. Implicit Spring Boot BOM via `io.spring.dependency-management` plugin
    ///
    /// BOMs are read through the effective-POM model, so parents and imported
    /// BOMs (Spring Boot's BOM imports Jackson's, Netty's, JUnit's, ...) count.
    fn build_bom_version_map(&self, project: &GradleProject) -> BomVersions {
        use tracing::info;

        let mut bom = BomVersions::default();
        let mut platform_count = 0;

        let absorb = |bom: &mut BomVersions, group: &str, artifact: &str, version: &str| {
            match self.poms.effective(group, artifact, version) {
                Ok(effective) => {
                    for problem in &effective.problems {
                        bom.problems
                            .push(format!("{group}:{artifact}:{version}: {problem}"));
                    }
                    for (key, entry) in &effective.managed {
                        if let Some(managed_version) = &entry.version {
                            // Platforms are constraints: the highest version wins.
                            match bom.versions.get(key) {
                                Some(existing)
                                    if crate::pom::compare_versions(existing, managed_version)
                                        != std::cmp::Ordering::Less => {}
                                _ => {
                                    bom.versions.insert(key.clone(), managed_version.clone());
                                }
                            }
                        }
                    }
                }
                Err(error) => bom
                    .problems
                    .push(format!("{group}:{artifact}:{version}: {error}")),
            }
        };

        // Process explicit platform dependencies
        for dep in &project.dependencies {
            if dep.is_platform {
                // `platform(SpringBootPlugin.BOM_COORDINATES)` is versionless in
                // the build: the Spring Boot plugin's version is the BOM's.
                let plugin_version = (dep.group == "org.springframework.boot"
                    && dep.artifact == "spring-boot-dependencies")
                    .then(|| {
                        project
                            .plugins
                            .iter()
                            .find(|p| p.id == "org.springframework.boot")
                            .and_then(|p| p.version.as_ref())
                    })
                    .flatten();
                let Some(version) = dep.version.as_ref().or(plugin_version) else {
                    bom.problems.push(format!(
                        "platform {}:{} has no version",
                        dep.group, dep.artifact
                    ));
                    continue;
                };
                platform_count += 1;
                info!(
                    "Found platform: {}:{} version {}",
                    dep.group, dep.artifact, version
                );
                absorb(&mut bom, &dep.group, &dep.artifact, version);
            }
        }

        // Check for implicit Spring Boot BOM via io.spring.dependency-management plugin
        let has_dep_mgmt = project.has_plugin("io.spring.dependency-management");
        let spring_boot_version = project
            .plugins
            .iter()
            .find(|p| p.id == "org.springframework.boot")
            .and_then(|p| p.version.as_ref());

        if has_dep_mgmt {
            if let Some(version) = spring_boot_version {
                info!(
                    "Detected Spring Boot {} with dependency-management plugin",
                    version
                );
                // The plugin enforces the BOM's versions on the whole graph.
                bom.overrides_transitive = true;
                absorb(
                    &mut bom,
                    "org.springframework.boot",
                    "spring-boot-dependencies",
                    version,
                );
            } else {
                bom.problems.push(
                    "io.spring.dependency-management is applied but the Spring Boot plugin version is unknown"
                        .to_string(),
                );
            }
        }

        if platform_count > 0 || !bom.versions.is_empty() {
            info!(
                "Built BOM version map from {} platform dependencies, {} managed entries",
                platform_count,
                bom.versions.len()
            );
        } else {
            debug!("No platform dependencies found");
        }

        bom
    }

    /// Resolve all dependencies for a project, returning both JARs and javadoc JARs
    pub async fn resolve_dependencies_with_docs(
        &self,
        project: &GradleProject,
    ) -> Result<(Vec<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        info!(
            "Resolving {} dependencies with javadoc for project '{}'",
            project.dependencies.len(),
            project.name
        );

        let report = self
            .resolve_dependencies_with_report(project, DEFAULT_TRANSITIVE_DEPTH)
            .await?;
        if !report.complete {
            return Err(GradleResolverError::CacheError(format!(
                "dependency resolution incomplete for project '{}'",
                project.name
            )));
        }
        let resolved_jars = report.jar_paths.clone();
        let resolved_with_docs = self.resolve_artifact_docs(&report.resolved_artifacts).await;
        Ok((resolved_jars, resolved_with_docs))
    }

    /// Attach available documentation to already resolved artifacts, including
    /// transitive and subproject dependencies in a partially resolved model.
    pub async fn resolve_artifact_docs(
        &self,
        artifacts: &[(GradleDependency, PathBuf)],
    ) -> Vec<(PathBuf, PathBuf)> {
        let mut seen = HashSet::new();
        let mut docs = Vec::new();
        for (dependency, jar_path) in artifacts {
            if seen.insert(jar_path) {
                if let Ok(javadoc) = self.resolve_javadoc(dependency).await {
                    docs.push((jar_path.clone(), javadoc));
                }
            }
        }
        docs
    }

    /// Resolve a single dependency to a JAR file
    pub async fn resolve_dependency(&self, dep: &GradleDependency) -> Result<PathBuf> {
        Self::validate_artifact_identity(dep)?;
        // Ensure dependency has a version
        if dep.version.is_none() {
            return Err(GradleResolverError::InvalidDependency(format!(
                "Dependency {} has no version",
                dep.coordinates()
            )));
        }

        // Try Gradle cache first
        if let Some(jar) = self.find_in_gradle_cache(dep)? {
            return Ok(jar);
        }

        // Try Maven local repository
        if let Some(jar) = self.find_in_maven_local(dep)? {
            return Ok(jar);
        }

        Err(GradleResolverError::DependencyNotFound(dep.coordinates()))
    }

    fn validate_artifact_identity(dep: &GradleDependency) -> Result<()> {
        validate_coordinates(dep)?;
        if dep.classifier.is_some()
            || dep
                .packaging
                .as_deref()
                .is_some_and(|packaging| packaging != "jar")
        {
            return Err(GradleResolverError::InvalidDependency(format!(
                "unsupported artifact identity {}",
                dep.full_coordinates()
            )));
        }
        Ok(())
    }

    /// Select the highest installed version for a versionless declaration.
    /// This is an offline fallback for statically-known group/artifact aliases
    /// whose version is computed dynamically by a Gradle script.
    fn select_installed_version(&self, dep: &GradleDependency) -> Result<Option<String>> {
        validate_coordinates(dep)?;
        let gradle_artifact_dir = self.gradle_cache_dir.join(&dep.group).join(&dep.artifact);
        let maven_artifact_dir = self
            .maven_local_dir
            .join(dep.group.replace('.', "/"))
            .join(&dep.artifact);
        let mut candidates = Vec::new();

        for version in self.list_available_versions(&gradle_artifact_dir)? {
            if self
                .find_jar_for_version(&gradle_artifact_dir, &dep.artifact, &version)?
                .is_some()
            {
                candidates.push(version);
            }
        }
        for version in self.list_available_versions(&maven_artifact_dir)? {
            if self
                .find_jar_for_maven_version(&maven_artifact_dir, &dep.artifact, &version)?
                .is_some()
            {
                candidates.push(version);
            }
        }

        candidates.sort_by(|left, right| {
            Self::compare_versions(&Self::parse_version(left), &Self::parse_version(right))
                .then_with(|| left.cmp(right))
        });
        candidates.dedup();
        Ok(candidates.pop())
    }

    /// Resolve a javadoc JAR for a dependency
    pub async fn resolve_javadoc(&self, dep: &GradleDependency) -> Result<PathBuf> {
        Self::validate_artifact_identity(dep)?;
        // Try Gradle cache first
        if let Some(jar) = self.find_javadoc_in_gradle_cache(dep)? {
            return Ok(jar);
        }

        // Try Maven local repository
        if let Some(jar) = self.find_javadoc_in_maven_local(dep)? {
            return Ok(jar);
        }

        Err(GradleResolverError::DependencyNotFound(format!(
            "{}-javadoc",
            dep.coordinates()
        )))
    }

    /// Find dependency in Gradle cache
    fn find_in_gradle_cache(&self, dep: &GradleDependency) -> Result<Option<PathBuf>> {
        if !self.gradle_cache_dir.exists() {
            return Ok(None);
        }

        let requested_version = match &dep.version {
            Some(v) => v,
            None => return Ok(None),
        };

        // Gradle cache structure:
        // ~/.gradle/caches/modules-2/files-2.1/group/artifact/version/hash/artifact-version.jar
        let artifact_dir = self.gradle_cache_dir.join(&dep.group).join(&dep.artifact);

        if !artifact_dir.exists() {
            return Ok(None);
        }

        // First try exact version match
        if let Some(jar) =
            self.find_jar_for_version(&artifact_dir, &dep.artifact, requested_version)?
        {
            return Ok(Some(jar));
        }

        Ok(None)
    }

    /// Find JAR for a specific version in artifact directory
    fn find_jar_for_version(
        &self,
        artifact_dir: &Path,
        artifact_name: &str,
        version: &str,
    ) -> Result<Option<PathBuf>> {
        let version_dir = artifact_dir.join(version);
        if !version_dir.exists() {
            return Ok(None);
        }

        let jar_name = format!("{}-{}.jar", artifact_name, version);

        find_unique_cached_file(&version_dir, &jar_name)
    }

    /// List all available versions for an artifact
    fn list_available_versions(&self, artifact_dir: &Path) -> Result<Vec<String>> {
        let mut versions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(artifact_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(version) = path.file_name().and_then(|n| n.to_str()) {
                        // Skip non-version directories (like metadata)
                        if version
                            .chars()
                            .next()
                            .map(|c| c.is_ascii_digit())
                            .unwrap_or(false)
                        {
                            versions.push(version.to_string());
                        }
                    }
                }
            }
        }

        Ok(versions)
    }

    /// Find the best available version using Gradle-like "highest wins" strategy.
    ///
    /// Strategy:
    /// 1. Filter to versions with same major version as requested
    /// 2. Pick the highest version among those (matches Gradle's behavior)
    /// 3. If no same-major versions exist, fall back to closest by distance
    #[cfg(test)]
    fn find_closest_version(&self, requested: &str, available: &[String]) -> Option<String> {
        if available.is_empty() {
            return None;
        }

        let requested_parts = Self::parse_version(requested);
        let requested_major = requested_parts.first().copied().unwrap_or(0);

        // Filter to same major version
        let same_major: Vec<&String> = available
            .iter()
            .filter(|v| {
                let parts = Self::parse_version(v);
                parts.first().copied().unwrap_or(0) == requested_major
            })
            .collect();

        if !same_major.is_empty() {
            // Gradle strategy: highest version wins
            return same_major
                .into_iter()
                .max_by(|a, b| {
                    let a_parts = Self::parse_version(a);
                    let b_parts = Self::parse_version(b);
                    Self::compare_versions(&a_parts, &b_parts).then_with(|| a.cmp(b))
                })
                .cloned();
        }

        // No same-major version available - fall back to closest distance
        debug!(
            "No version with major={} found, falling back to closest distance",
            requested_major
        );

        let mut scored: Vec<(i64, &String)> = available
            .iter()
            .map(|v| {
                let parts = Self::parse_version(v);
                let distance = Self::version_distance(&requested_parts, &parts);
                (distance, v)
            })
            .collect();

        scored.sort_by(|(left_distance, left), (right_distance, right)| {
            left_distance
                .cmp(right_distance)
                .then_with(|| left.cmp(right))
        });
        scored.first().map(|(_, v)| (*v).clone())
    }

    /// Compare two version part vectors (for sorting)
    fn compare_versions(a: &[i64], b: &[i64]) -> std::cmp::Ordering {
        let max_len = a.len().max(b.len());
        for i in 0..max_len {
            let a_part = a.get(i).copied().unwrap_or(0);
            let b_part = b.get(i).copied().unwrap_or(0);
            match a_part.cmp(&b_part) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }
        std::cmp::Ordering::Equal
    }

    /// Parse version string into comparable parts
    fn parse_version(version: &str) -> Vec<i64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<i64>().ok())
            .collect()
    }

    /// Calculate distance between two versions (lower = closer)
    #[cfg(test)]
    fn version_distance(requested: &[i64], available: &[i64]) -> i64 {
        let max_len = requested.len().max(available.len());
        let mut distance: i64 = 0;

        for i in 0..max_len {
            let r = requested.get(i).copied().unwrap_or(0);
            let a = available.get(i).copied().unwrap_or(0);

            // Weight earlier parts more heavily (major > minor > patch)
            let weight = 1000_i64.pow((max_len - i) as u32);
            distance += (r - a).abs() * weight;
        }

        distance
    }

    /// Find dependency in Maven local repository
    fn find_in_maven_local(&self, dep: &GradleDependency) -> Result<Option<PathBuf>> {
        if !self.maven_local_dir.exists() {
            return Ok(None);
        }

        let requested_version = match &dep.version {
            Some(v) => v,
            None => return Ok(None),
        };

        // Maven local structure:
        // ~/.m2/repository/group/as/path/artifact/version/artifact-version.jar
        let group_path = dep.group.replace('.', "/");
        let artifact_dir = self.maven_local_dir.join(&group_path).join(&dep.artifact);

        if !artifact_dir.exists() {
            return Ok(None);
        }

        // First try exact version match
        if let Some(jar) =
            self.find_jar_for_maven_version(&artifact_dir, &dep.artifact, requested_version)?
        {
            return Ok(Some(jar));
        }

        Ok(None)
    }

    /// Find JAR for a specific version in Maven artifact directory
    fn find_jar_for_maven_version(
        &self,
        artifact_dir: &Path,
        artifact_name: &str,
        version: &str,
    ) -> Result<Option<PathBuf>> {
        let jar_name = format!("{}-{}.jar", artifact_name, version);
        let jar_path = artifact_dir.join(version).join(&jar_name);

        if jar_path.is_file() {
            debug!("Found in Maven local: {:?}", jar_path);
            Ok(Some(jar_path))
        } else {
            Ok(None)
        }
    }

    /// Find javadoc JAR in Gradle cache
    fn find_javadoc_in_gradle_cache(&self, dep: &GradleDependency) -> Result<Option<PathBuf>> {
        if !self.gradle_cache_dir.exists() {
            return Ok(None);
        }

        let version = match &dep.version {
            Some(v) => v,
            None => return Ok(None),
        };

        // Gradle cache structure for javadoc:
        // ~/.gradle/caches/modules-2/files-2.1/group/artifact/version/hash/artifact-version-javadoc.jar
        let dep_dir = self
            .gradle_cache_dir
            .join(&dep.group)
            .join(&dep.artifact)
            .join(version);

        if !dep_dir.exists() {
            return Ok(None);
        }

        // Look for javadoc JAR files in hash subdirectories
        let javadoc_jar_name = format!("{}-{}-javadoc.jar", dep.artifact, version);

        find_unique_cached_file(&dep_dir, &javadoc_jar_name)
    }

    /// Find javadoc JAR in Maven local repository
    fn find_javadoc_in_maven_local(&self, dep: &GradleDependency) -> Result<Option<PathBuf>> {
        if !self.maven_local_dir.exists() {
            return Ok(None);
        }

        let version = match &dep.version {
            Some(v) => v,
            None => return Ok(None),
        };

        // Maven local structure for javadoc:
        // ~/.m2/repository/group/as/path/artifact/version/artifact-version-javadoc.jar
        let group_path = dep.group.replace('.', "/");
        let javadoc_jar_name = format!("{}-{}-javadoc.jar", dep.artifact, version);

        let javadoc_jar_path = self
            .maven_local_dir
            .join(&group_path)
            .join(&dep.artifact)
            .join(version)
            .join(&javadoc_jar_name);

        if javadoc_jar_path.is_file() {
            debug!("Found javadoc in Maven local: {:?}", javadoc_jar_path);
            Ok(Some(javadoc_jar_path))
        } else {
            Ok(None)
        }
    }

    /// Check if a dependency can be resolved
    pub async fn can_resolve(&self, dep: &GradleDependency) -> bool {
        self.resolve_dependency(dep).await.is_ok()
    }

    /// Get cache directories
    pub fn cache_dirs(&self) -> (&Path, &Path) {
        (&self.gradle_cache_dir, &self.maven_local_dir)
    }
}

/// The files cache can contain multiple repository/checksum variants of one
/// coordinate. Without Gradle's selection metadata none is authoritative.
fn find_unique_cached_file(directory: &Path, name: &str) -> Result<Option<PathBuf>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut found = None;
    for entry in entries {
        let path = entry?.path().join(name);
        if path.is_file() {
            if found.is_some() {
                return Err(GradleResolverError::CacheError(format!(
                    "ambiguous cached artifact {name} in {}",
                    directory.display()
                )));
            }
            found = Some(path);
        }
    }
    Ok(found)
}

fn validate_cache_component(value: &str) -> Result<()> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\' | ':' | '?' | '#')
        })
    {
        return Err(GradleResolverError::InvalidDependency(format!(
            "invalid cache coordinate component: {value:?}"
        )));
    }
    Ok(())
}

fn validate_coordinates(dep: &GradleDependency) -> Result<()> {
    for segment in dep.group.split('.') {
        validate_cache_component(segment)?;
    }
    validate_cache_component(&dep.artifact)?;
    if let Some(version) = &dep.version {
        validate_cache_component(version)?;
    }
    Ok(())
}

impl Default for GradleResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::model::DependencyConfiguration;
    use tempfile::TempDir;

    #[test]
    fn test_resolver_creation() {
        let resolver = GradleResolver::new();
        let (gradle_cache, maven_local) = resolver.cache_dirs();

        assert!(gradle_cache.to_string_lossy().contains(".gradle"));
        assert!(maven_local.to_string_lossy().contains(".m2"));
    }

    #[test]
    fn test_custom_cache_dirs() {
        let gradle_cache = PathBuf::from("/custom/gradle");
        let maven_local = PathBuf::from("/custom/maven");

        let resolver = GradleResolver::with_cache_dirs(gradle_cache.clone(), maven_local.clone());
        let (actual_gradle, actual_maven) = resolver.cache_dirs();

        assert_eq!(actual_gradle, &gradle_cache);
        assert_eq!(actual_maven, &maven_local);
    }

    #[test]
    fn context_budget_records_a_specific_reason() {
        let mut state = TransitiveResolution {
            complete: true,
            max_depth: 0,
            ..TransitiveResolution::default()
        };
        for index in 0..=MAX_RESOLUTION_CONTEXTS {
            state.schedule(
                GradleDependency {
                    exclusions: Vec::new(),
                    transitive: true,
                    forced: false,
                    group: "org.fixture".into(),
                    artifact: format!("member{index}"),
                    version: Some("1.0".into()),
                    configuration: DependencyConfiguration::Implementation,
                    classifier: None,
                    packaging: None,
                    is_platform: false,
                },
                0,
                DependencyScope::from_configuration(&DependencyConfiguration::Implementation),
                Vec::new(),
                None,
            );
        }

        assert!(!state.complete);
        assert_eq!(
            state.incomplete_reasons,
            BTreeSet::from([GradleIncompleteReason::ContextLimit])
        );
    }

    #[test]
    fn dependency_paths_are_shortest_stable_and_never_fabricated() {
        let root = "com.example:root:1.0";
        let middle = "com.example:middle:1.0";
        let target = "com.example:target:1.0";
        let report = GradleResolutionReport {
            dependency_edges: vec![
                DependencyEdge::direct(root.to_owned()),
                DependencyEdge::transitive(root.to_owned(), middle.to_owned()),
                DependencyEdge::transitive(middle.to_owned(), target.to_owned()),
                DependencyEdge::transitive(root.to_owned(), target.to_owned()),
                DependencyEdge::transitive(target.to_owned(), root.to_owned()),
            ],
            ..GradleResolutionReport::default()
        };

        assert_eq!(report.explain_path(root), Some(vec![root.to_owned()]));
        assert_eq!(
            report.explain_path(target),
            Some(vec![root.to_owned(), target.to_owned()])
        );
        assert_eq!(report.explain_path("com.example:absent:1.0"), None);
    }

    #[test]
    fn dependency_edges_have_a_stable_serialized_contract() {
        let edge = DependencyEdge::transitive(
            "com.example:parent:1.0".to_owned(),
            "com.example:child:2.0".to_owned(),
        );

        assert_eq!(
            serde_json::to_value(edge).unwrap(),
            serde_json::json!({
                "from": "com.example:parent:1.0",
                "to": "com.example:child:2.0",
                "kind": "transitive"
            })
        );
    }

    #[tokio::test]
    async fn test_resolve_nonexistent_dependency() {
        let resolver = GradleResolver::new();
        let dep = GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "com.nonexistent".to_string(),
            artifact: "fake-artifact".to_string(),
            version: Some("999.999.999".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        };

        let result = resolver.resolve_dependency(&dep).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn report_distinguishes_unavailable_version_from_artifact_resolution_failure() {
        let temp = TempDir::new().unwrap();
        let resolver =
            GradleResolver::with_cache_dirs(temp.path().join("gradle"), temp.path().join("maven"));
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.fixture".into(),
            artifact: "unversioned".into(),
            version: None,
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.fixture".into(),
            artifact: "versioned".into(),
            version: Some("1.0".into()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });

        let report = resolver
            .resolve_dependencies_with_report(&project, 1)
            .await
            .unwrap();

        assert!(!report.complete);
        assert!(report
            .incomplete_reasons
            .contains(&GradleIncompleteReason::VersionUnavailable(
                "org.fixture:unversioned".into()
            )));
        assert!(report.incomplete_reasons.iter().any(|reason| matches!(
            reason,
            GradleIncompleteReason::ArtifactResolutionFailed { identity, .. }
                if identity == "org.fixture:versioned:1.0"
        )));
    }

    #[tokio::test]
    async fn report_keeps_pom_and_depth_reasons_distinct() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let maven = temp.path().join("maven");
        for (artifact, pom) in [
            ("parent", "<project><dependencies><dependency><groupId>org.fixture</groupId><artifactId>child</artifactId><version>1.0</version></dependency></dependencies></project>"),
            ("child", "<project><parent><groupId>org.fixture</groupId><artifactId>base</artifactId><version>1.0</version></parent></project>"),
        ] {
            let dir = cache.join(format!("org.fixture/{artifact}/1.0/hash"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{artifact}-1.0.jar")), b"fixture").unwrap();
            std::fs::write(dir.join(format!("{artifact}-1.0.pom")), pom).unwrap();
        }
        let resolver = GradleResolver::with_cache_dirs(cache, maven);
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.fixture".into(),
            artifact: "parent".into(),
            version: Some("1.0".into()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });

        let shallow = resolver
            .resolve_dependencies_with_report(&project, 0)
            .await
            .unwrap();
        assert!(shallow
            .incomplete_reasons
            .contains(&GradleIncompleteReason::DepthLimit(
                "org.fixture:child:1.0".into()
            )));
        let deep = resolver
            .resolve_dependencies_with_report(&project, 1)
            .await
            .unwrap();
        assert!(deep.incomplete_reasons.iter().any(|reason| matches!(
            reason,
            GradleIncompleteReason::PomMetadata { module, problem }
                if module == "org.fixture:child:1.0" && problem.contains("org.fixture:base:1.0")
        )));
        assert!(!deep
            .incomplete_reasons
            .iter()
            .any(|reason| matches!(reason, GradleIncompleteReason::DepthLimit(_))));
    }

    #[tokio::test]
    async fn test_pom_only_dependency_resolves_transitives() {
        let temp = TempDir::new().unwrap();
        let gradle_cache = temp.path().join("gradle-cache");
        let maven_local = temp.path().join("maven-local");
        std::fs::create_dir_all(&gradle_cache).unwrap();
        std::fs::create_dir_all(&maven_local).unwrap();

        let pom_dir = maven_local
            .join("org/junit/jupiter")
            .join("junit-jupiter")
            .join("5.10.0");
        std::fs::create_dir_all(&pom_dir).unwrap();
        std::fs::write(
            pom_dir.join("junit-jupiter-5.10.0.pom"),
            r#"
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.junit.jupiter</groupId>
  <artifactId>junit-jupiter</artifactId>
  <version>5.10.0</version>
  <dependencies>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter-api</artifactId>
      <version>5.10.0</version>
    </dependency>
  </dependencies>
</project>
"#,
        )
        .unwrap();

        let api_dir = maven_local
            .join("org/junit/jupiter")
            .join("junit-jupiter-api")
            .join("5.10.0");
        std::fs::create_dir_all(&api_dir).unwrap();
        std::fs::write(api_dir.join("junit-jupiter-api-5.10.0.jar"), b"").unwrap();
        std::fs::write(
            api_dir.join("junit-jupiter-api-5.10.0.pom"),
            r#"
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.junit.jupiter</groupId>
  <artifactId>junit-jupiter-api</artifactId>
  <version>5.10.0</version>
</project>
"#,
        )
        .unwrap();

        let resolver = GradleResolver::with_cache_dirs(gradle_cache, maven_local);
        let mut project =
            GradleProject::new("pom-only-test".to_string(), temp.path().to_path_buf());
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.junit.jupiter".to_string(),
            artifact: "junit-jupiter".to_string(),
            version: Some("5.10.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });

        let report = resolver
            .resolve_dependencies_with_report(&project, 1)
            .await
            .unwrap();

        assert!(
            !report.complete,
            "a POM-only root has incomplete direct artifact resolution"
        );

        assert!(
            report
                .jar_paths
                .iter()
                .any(|path| path.ends_with("junit-jupiter-api-5.10.0.jar")),
            "POM-only dependency should resolve transitive junit-jupiter-api"
        );
    }

    /// Write a Maven-local artifact with a JAR and a POM whose dependencies are `pom_dependencies`.
    fn write_maven_artifact(
        maven_local: &std::path::Path,
        group: &str,
        artifact: &str,
        version: &str,
        pom_dependencies: &str,
    ) {
        let dir = group
            .split('.')
            .fold(maven_local.to_path_buf(), |path, segment| {
                path.join(segment)
            })
            .join(artifact)
            .join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{artifact}-{version}.jar")), b"").unwrap();
        std::fs::write(
            dir.join(format!("{artifact}-{version}.pom")),
            format!(
                "<project><modelVersion>4.0.0</modelVersion><groupId>{group}</groupId>\
                 <artifactId>{artifact}</artifactId><version>{version}</version>\
                 <dependencies>{pom_dependencies}</dependencies></project>"
            ),
        )
        .unwrap();
    }

    /// Resolve one root declaration and return the file names of the JARs it pulls in.
    async fn resolved_jar_names(
        maven_local: &std::path::Path,
        root: GradleDependency,
    ) -> Vec<String> {
        resolved_jar_names_for(maven_local, vec![root]).await
    }

    /// Resolve the given root declarations and return the file names of the JARs they pull in.
    async fn resolved_jar_names_for(
        maven_local: &std::path::Path,
        roots: Vec<GradleDependency>,
    ) -> Vec<String> {
        let temp_root = maven_local.parent().unwrap();
        let resolver = GradleResolver::with_cache_dirs(
            temp_root.join("gradle-cache"),
            maven_local.to_path_buf(),
        );
        let mut project = GradleProject::new("dsl-walk".to_string(), temp_root.to_path_buf());
        project.dependencies.extend(roots);
        let report = resolver
            .resolve_dependencies_with_report(&project, 1)
            .await
            .unwrap();
        report
            .jar_paths
            .iter()
            .filter_map(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect()
    }

    #[tokio::test]
    async fn configuration_block_settings_limit_the_transitive_walk() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("gradle-cache")).unwrap();
        let maven_local = temp.path().join("maven-local");
        std::fs::create_dir_all(&maven_local).unwrap();
        write_maven_artifact(
            &maven_local,
            "org.example",
            "parent",
            "1.0",
            "<dependency><groupId>org.example</groupId><artifactId>child</artifactId>\
             <version>1.0</version></dependency>",
        );
        write_maven_artifact(&maven_local, "org.example", "child", "1.0", "");

        let root = |exclusions: Vec<(String, String)>, transitive: bool| GradleDependency {
            group: "org.example".to_string(),
            artifact: "parent".to_string(),
            version: Some("1.0".to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
            exclusions,
            transitive,
            forced: false,
        };
        let names = |exclusions: Vec<(String, String)>, transitive: bool| {
            resolved_jar_names(&maven_local, root(exclusions, transitive))
        };

        let default = names(Vec::new(), true).await;
        assert!(
            default.contains(&"child-1.0.jar".to_string()),
            "{default:?}"
        );

        let non_transitive = names(Vec::new(), false).await;
        assert_eq!(
            non_transitive,
            ["parent-1.0.jar"],
            "transitive = false keeps only the root JAR"
        );

        let named = names(vec![("org.example".to_string(), "child".to_string())], true).await;
        assert!(!named.contains(&"child-1.0.jar".to_string()), "{named:?}");
        assert!(named.contains(&"parent-1.0.jar".to_string()), "{named:?}");

        let wildcard = names(vec![("*".to_string(), "*".to_string())], true).await;
        assert!(
            !wildcard.contains(&"child-1.0.jar".to_string()),
            "{wildcard:?}"
        );
    }

    #[tokio::test]
    async fn forced_declaration_keeps_its_version_over_conflict_resolution() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("gradle-cache")).unwrap();
        let maven_local = temp.path().join("maven-local");
        std::fs::create_dir_all(&maven_local).unwrap();
        write_maven_artifact(
            &maven_local,
            "org.example",
            "parent",
            "1.0",
            "<dependency><groupId>org.example</groupId><artifactId>child</artifactId>\
             <version>2.0</version></dependency>",
        );
        write_maven_artifact(&maven_local, "org.example", "child", "1.0", "");
        write_maven_artifact(&maven_local, "org.example", "child", "2.0", "");

        let module = |artifact: &str, version: &str, forced: bool| GradleDependency {
            group: "org.example".to_string(),
            artifact: artifact.to_string(),
            version: Some(version.to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
            exclusions: Vec::new(),
            transitive: true,
            forced,
        };

        // Without `force`, conflict resolution keeps the higher version the POM asks for.
        let unforced = resolved_jar_names_for(
            &maven_local,
            vec![
                module("parent", "1.0", false),
                module("child", "1.0", false),
            ],
        )
        .await;
        assert!(
            unforced.contains(&"child-2.0.jar".to_string()),
            "{unforced:?}"
        );

        let forced = resolved_jar_names_for(
            &maven_local,
            vec![module("parent", "1.0", false), module("child", "1.0", true)],
        )
        .await;
        assert!(forced.contains(&"child-1.0.jar".to_string()), "{forced:?}");
        assert!(!forced.contains(&"child-2.0.jar".to_string()), "{forced:?}");
    }

    #[tokio::test]
    async fn test_versionless_dependency_selects_highest_installed_cache_version() {
        let temp = TempDir::new().unwrap();
        let gradle_cache = temp.path().join("gradle-cache");
        let maven_local = temp.path().join("maven-local");
        for version in ["1.2.0", "1.10.0"] {
            let hash_dir = gradle_cache
                .join("com.example")
                .join("dynamic")
                .join(version)
                .join("hash");
            std::fs::create_dir_all(&hash_dir).unwrap();
            std::fs::write(hash_dir.join(format!("dynamic-{version}.jar")), b"").unwrap();
        }
        std::fs::create_dir_all(&maven_local).unwrap();

        let resolver = GradleResolver::with_cache_dirs(gradle_cache, maven_local);
        let mut project = GradleProject::new("versionless".to_string(), temp.path().to_path_buf());
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "com.example".to_string(),
            artifact: "dynamic".to_string(),
            version: None,
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: false,
        });

        let report = resolver
            .resolve_dependencies_with_report(&project, 0)
            .await
            .unwrap();
        assert_eq!(report.declared, 1);
        assert_eq!(report.resolved, 1);
        assert_eq!(report.missing_count(), 0);
        assert_eq!(report.cache_selected_versions, 1);
        assert_eq!(report.jar_paths.len(), 1);
        assert!(report.jar_paths[0].ends_with("dynamic-1.10.0.jar"));
    }

    #[test]
    fn test_parse_version_simple() {
        assert_eq!(GradleResolver::parse_version("1.2.3"), vec![1, 2, 3]);
        assert_eq!(GradleResolver::parse_version("6.2.7"), vec![6, 2, 7]);
        assert_eq!(GradleResolver::parse_version("6.2.14"), vec![6, 2, 14]);
    }

    #[test]
    fn test_parse_version_with_qualifiers() {
        // Qualifiers like RELEASE, SNAPSHOT, etc. are ignored (numeric only)
        assert_eq!(
            GradleResolver::parse_version("1.0.0-RELEASE"),
            vec![1, 0, 0]
        );
        assert_eq!(
            GradleResolver::parse_version("2.3.4-SNAPSHOT"),
            vec![2, 3, 4]
        );
        assert_eq!(GradleResolver::parse_version("1.0.0.M1"), vec![1, 0, 0, 1]);
    }

    #[test]
    fn test_version_distance_exact_match() {
        let v1 = GradleResolver::parse_version("6.2.7");
        let v2 = GradleResolver::parse_version("6.2.7");
        assert_eq!(GradleResolver::version_distance(&v1, &v2), 0);
    }

    #[test]
    fn test_version_distance_patch_diff() {
        let requested = GradleResolver::parse_version("6.2.7");
        let v14 = GradleResolver::parse_version("6.2.14");
        let v0 = GradleResolver::parse_version("6.2.0");

        // 6.2.14 is 7 patches away from 6.2.7
        // 6.2.0 is also 7 patches away from 6.2.7
        let dist_to_14 = GradleResolver::version_distance(&requested, &v14);
        let dist_to_0 = GradleResolver::version_distance(&requested, &v0);
        assert_eq!(dist_to_14, dist_to_0); // Both 7 patches away
    }

    #[test]
    fn test_version_distance_minor_more_important_than_patch() {
        let requested = GradleResolver::parse_version("6.2.7");
        let v_minor_off = GradleResolver::parse_version("6.3.0");
        let v_patch_off = GradleResolver::parse_version("6.2.100");

        let dist_minor = GradleResolver::version_distance(&requested, &v_minor_off);
        let dist_patch = GradleResolver::version_distance(&requested, &v_patch_off);

        // Minor version difference should be weighted more heavily
        assert!(
            dist_minor > dist_patch,
            "minor diff {} should > patch diff {}",
            dist_minor,
            dist_patch
        );
    }

    #[test]
    fn test_find_closest_version_highest_wins() {
        let resolver = GradleResolver::new();
        let available = vec![
            "6.2.0".to_string(),
            "6.2.5".to_string(),
            "6.2.14".to_string(),
            "6.3.0".to_string(),
        ];

        // Gradle strategy: highest version wins within same major
        // Looking for 6.2.7 - picks 6.3.0 (highest with major=6)
        let closest = resolver.find_closest_version("6.2.7", &available);
        assert_eq!(closest, Some("6.3.0".to_string()));

        // Looking for 6.2.10 - picks 6.3.0 (highest with major=6)
        let closest = resolver.find_closest_version("6.2.10", &available);
        assert_eq!(closest, Some("6.3.0".to_string()));

        // Looking for 6.3.1 - picks 6.3.0 (highest with major=6)
        let closest = resolver.find_closest_version("6.3.1", &available);
        assert_eq!(closest, Some("6.3.0".to_string()));
    }

    #[test]
    fn test_find_closest_version_major_constraint() {
        let resolver = GradleResolver::new();
        let available = vec![
            "5.0.0".to_string(),
            "6.2.5".to_string(),
            "6.2.14".to_string(),
            "7.0.0".to_string(),
        ];

        // Request 6.2.7 - should pick 6.2.14 (highest with major=6), not 7.0.0
        let closest = resolver.find_closest_version("6.2.7", &available);
        assert_eq!(closest, Some("6.2.14".to_string()));

        // Request 5.1.0 - should pick 5.0.0 (only option with major=5)
        let closest = resolver.find_closest_version("5.1.0", &available);
        assert_eq!(closest, Some("5.0.0".to_string()));

        // Request 7.1.0 - should pick 7.0.0 (only option with major=7)
        let closest = resolver.find_closest_version("7.1.0", &available);
        assert_eq!(closest, Some("7.0.0".to_string()));
    }

    #[test]
    fn test_find_closest_version_fallback_to_closest() {
        let resolver = GradleResolver::new();
        let available = vec!["5.0.0".to_string(), "7.0.0".to_string()];

        // Request 6.0.0 - no major=6 available, falls back to closest distance
        // 5.0.0 is 1 major away, 7.0.0 is 1 major away - should pick one of them
        let closest = resolver.find_closest_version("6.0.0", &available);
        assert!(closest == Some("5.0.0".to_string()) || closest == Some("7.0.0".to_string()));
    }

    #[test]
    fn test_find_closest_version_empty() {
        let resolver = GradleResolver::new();
        let available: Vec<String> = vec![];

        let closest = resolver.find_closest_version("1.0.0", &available);
        assert_eq!(closest, None);
    }

    #[test]
    fn test_find_closest_version_prefers_highest_over_exact() {
        let resolver = GradleResolver::new();
        let available = vec![
            "1.0.0".to_string(),
            "1.0.5".to_string(),
            "1.1.0".to_string(),
        ];

        // Even though 1.0.5 is an exact match, "highest wins" picks 1.1.0
        let closest = resolver.find_closest_version("1.0.5", &available);
        assert_eq!(closest, Some("1.1.0".to_string()));
    }

    #[test]
    fn test_compare_versions() {
        use std::cmp::Ordering;

        assert_eq!(
            GradleResolver::compare_versions(&[1, 0, 0], &[1, 0, 0]),
            Ordering::Equal
        );
        assert_eq!(
            GradleResolver::compare_versions(&[1, 0, 0], &[2, 0, 0]),
            Ordering::Less
        );
        assert_eq!(
            GradleResolver::compare_versions(&[2, 0, 0], &[1, 0, 0]),
            Ordering::Greater
        );
        assert_eq!(
            GradleResolver::compare_versions(&[1, 2, 3], &[1, 2, 4]),
            Ordering::Less
        );
        assert_eq!(
            GradleResolver::compare_versions(&[1, 2], &[1, 2, 0]),
            Ordering::Equal
        );
    }

    fn platform_project(name: &str, group: &str, artifact: &str, version: &str) -> GradleProject {
        let mut project = GradleProject::new(name.to_string(), PathBuf::from("/test"));
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: group.to_string(),
            artifact: artifact.to_string(),
            version: Some(version.to_string()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: true,
        });
        project
    }

    fn write_maven_pom(root: &Path, group: &str, artifact: &str, version: &str, body: &str) {
        let dir = root
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{artifact}-{version}.pom")),
            format!("<project>{body}</project>"),
        )
        .unwrap();
    }

    #[test]
    fn explicit_platform_bom_versions_are_parsed_with_properties() {
        let temp = TempDir::new().unwrap();
        let maven_local = temp.path().join("maven-local");
        write_maven_pom(
            &maven_local,
            "com.example",
            "platform",
            "1.0.0",
            "<properties><managed.version>2.3.4</managed.version></properties>\
             <dependencyManagement><dependencies><dependency>\
             <groupId>com.example</groupId><artifactId>managed</artifactId>\
             <version>${managed.version}</version></dependency></dependencies></dependencyManagement>",
        );
        let resolver =
            GradleResolver::with_cache_dirs(temp.path().join("gradle-cache"), maven_local);
        let bom = resolver.build_bom_version_map(&platform_project(
            "platform",
            "com.example",
            "platform",
            "1.0.0",
        ));
        assert!(bom.problems.is_empty(), "{:?}", bom.problems);
        assert_eq!(
            bom.versions.get("com.example:managed:jar:"),
            Some(&"2.3.4".to_string())
        );
    }

    #[test]
    fn platform_bom_parents_and_imports_are_followed() {
        let temp = TempDir::new().unwrap();
        let maven_local = temp.path().join("maven-local");
        write_maven_pom(
            &maven_local,
            "com.example",
            "grandparent",
            "1",
            "<properties><v>7</v></properties><dependencyManagement><dependencies><dependency>\
             <groupId>com.example</groupId><artifactId>inherited</artifactId><version>${v}</version>\
             </dependency></dependencies></dependencyManagement>",
        );
        write_maven_pom(
            &maven_local,
            "com.example",
            "other-bom",
            "2.3.4",
            "<groupId>com.example</groupId><artifactId>other-bom</artifactId><version>2.3.4</version>\
             <dependencyManagement><dependencies><dependency>\
             <groupId>com.example</groupId><artifactId>imported</artifactId><version>${project.version}</version>\
             </dependency></dependencies></dependencyManagement>",
        );
        write_maven_pom(
            &maven_local,
            "com.example",
            "bom",
            "1.0.0",
            "<parent><groupId>com.example</groupId><artifactId>grandparent</artifactId><version>1</version></parent>\
             <groupId>com.example</groupId><artifactId>bom</artifactId><version>1.0.0</version>\
             <dependencyManagement><dependencies><dependency>\
             <groupId>com.example</groupId><artifactId>other-bom</artifactId><version>2.3.4</version>\
             <type>pom</type><scope>import</scope></dependency></dependencies></dependencyManagement>",
        );
        let resolver =
            GradleResolver::with_cache_dirs(temp.path().join("gradle-cache"), maven_local);
        let bom =
            resolver.build_bom_version_map(&platform_project("p", "com.example", "bom", "1.0.0"));
        assert!(bom.problems.is_empty(), "{:?}", bom.problems);
        assert_eq!(
            bom.versions.get("com.example:inherited:jar:"),
            Some(&"7".to_string())
        );
        assert_eq!(
            bom.versions.get("com.example:imported:jar:"),
            Some(&"2.3.4".to_string())
        );
    }

    #[test]
    fn missing_bom_is_reported_by_coordinate() {
        let temp = TempDir::new().unwrap();
        let resolver = GradleResolver::with_cache_dirs(
            temp.path().join("gradle-cache"),
            temp.path().join("maven-local"),
        );
        let bom = resolver.build_bom_version_map(&platform_project(
            "p",
            "com.example",
            "absent-bom",
            "1.0.0",
        ));
        assert!(bom.versions.is_empty());
        assert!(
            bom.problems[0].contains("com.example:absent-bom:1.0.0"),
            "{:?}",
            bom.problems
        );
    }

    #[test]
    fn platform_bom_conflicts_take_the_highest_version_and_unlisted_artifacts_are_not_guessed() {
        let temp = TempDir::new().unwrap();
        let maven_local = temp.path().join("maven-local");
        for (artifact, version) in [("first-bom", "1.0.0"), ("second-bom", "2.0.0")] {
            write_maven_pom(
                &maven_local,
                "com.example",
                artifact,
                version,
                &format!(
                    "<dependencyManagement><dependencies><dependency>\
                     <groupId>com.example</groupId><artifactId>managed</artifactId><version>{version}</version>\
                     </dependency></dependencies></dependencyManagement>"
                ),
            );
        }
        let resolver =
            GradleResolver::with_cache_dirs(temp.path().join("gradle-cache"), maven_local);
        let mut project = platform_project("p", "com.example", "second-bom", "2.0.0");
        project
            .dependencies
            .extend(platform_project("p", "com.example", "first-bom", "1.0.0").dependencies);

        let bom = resolver.build_bom_version_map(&project);
        assert_eq!(
            bom.versions.get("com.example:managed:jar:"),
            Some(&"2.0.0".to_string())
        );
        assert!(!bom.versions.contains_key("com.example:unlisted:jar:"));
    }

    #[tokio::test]
    async fn highest_requested_version_wins_and_losing_versions_are_not_reported_missing() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let maven = temp.path().join("maven");
        let add = |artifact: &str, version: &str, deps: &str, jar: bool| {
            let dir = cache.join(format!("org.fixture/{artifact}/{version}/hash"));
            std::fs::create_dir_all(&dir).unwrap();
            if jar {
                std::fs::write(dir.join(format!("{artifact}-{version}.jar")), b"x").unwrap();
            }
            std::fs::write(
                dir.join(format!("{artifact}-{version}.pom")),
                format!("<project><dependencies>{deps}</dependencies></project>"),
            )
            .unwrap();
        };
        let dependency = |artifact: &str, version: &str| {
            format!(
                "<dependency><groupId>org.fixture</groupId><artifactId>{artifact}</artifactId><version>{version}</version></dependency>"
            )
        };
        add("app-lib", "1", &dependency("shared", "1.0"), true);
        add("other-lib", "1", &dependency("shared", "2.0"), true);
        // 1.0 was never downloaded: Gradle picked 2.0.
        add("shared", "2.0", "", true);
        let resolver = GradleResolver::with_cache_dirs(cache, maven);
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        for artifact in ["app-lib", "other-lib"] {
            project.dependencies.push(GradleDependency {
                exclusions: Vec::new(),
                transitive: true,
                forced: false,
                group: "org.fixture".into(),
                artifact: artifact.into(),
                version: Some("1".into()),
                configuration: DependencyConfiguration::Implementation,
                classifier: None,
                packaging: None,
                is_platform: false,
            });
        }
        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();
        assert!(report.complete, "{:?}", report.incomplete_reasons);
        assert_eq!(report.jar_paths.len(), 3);
        assert!(report
            .jar_paths
            .iter()
            .any(|path| path.ends_with("shared-2.0.jar")));
    }

    #[tokio::test]
    async fn explicit_platform_versions_constrain_transitive_requests() {
        // `implementation(platform(BOM))`: a starter's POM asks for shared:1.0
        // but the platform manages shared:2.0 -> Gradle selects 2.0.
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let maven = temp.path().join("maven");
        let add = |artifact: &str, version: &str, deps: &str, jar: bool| {
            let dir = cache.join(format!("org.fixture/{artifact}/{version}/hash"));
            std::fs::create_dir_all(&dir).unwrap();
            if jar {
                std::fs::write(dir.join(format!("{artifact}-{version}.jar")), b"x").unwrap();
            }
            std::fs::write(
                dir.join(format!("{artifact}-{version}.pom")),
                format!("<project><dependencies>{deps}</dependencies></project>"),
            )
            .unwrap();
        };
        add(
            "starter",
            "1",
            "<dependency><groupId>org.fixture</groupId><artifactId>shared</artifactId><version>1.0</version></dependency>",
            true,
        );
        // Only the platform's version is in the cache.
        add("shared", "2.0", "", true);
        write_maven_pom(
            &maven,
            "org.fixture",
            "bom",
            "9",
            "<dependencyManagement><dependencies><dependency>\
             <groupId>org.fixture</groupId><artifactId>shared</artifactId><version>2.0</version>\
             </dependency></dependencies></dependencyManagement>",
        );
        let resolver = GradleResolver::with_cache_dirs(cache, maven);
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.dependencies.push(GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.fixture".into(),
            artifact: "bom".into(),
            version: Some("9".into()),
            configuration: DependencyConfiguration::Implementation,
            classifier: None,
            packaging: None,
            is_platform: true,
        });
        project.dependencies.push(fixture_dependency(
            "starter",
            Some("1"),
            DependencyConfiguration::Implementation,
        ));
        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();
        assert!(report.complete, "{:?}", report.incomplete_reasons);
        assert!(
            report
                .main_runtime_jars
                .iter()
                .any(|path| path.ends_with("shared-2.0.jar")),
            "{:?}",
            report.main_runtime_jars
        );
    }

    #[tokio::test]
    async fn one_unresolvable_dependency_does_not_hide_the_others() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let dir = cache.join("org.fixture/present/1/hash");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("present-1.jar"), b"x").unwrap();
        std::fs::write(dir.join("present-1.pom"), "<project/>").unwrap();
        let resolver = GradleResolver::with_cache_dirs(cache, temp.path().join("maven"));
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        for artifact in ["present", "absent"] {
            project.dependencies.push(GradleDependency {
                exclusions: Vec::new(),
                transitive: true,
                forced: false,
                group: "org.fixture".into(),
                artifact: artifact.into(),
                version: Some("1".into()),
                configuration: DependencyConfiguration::Implementation,
                classifier: None,
                packaging: None,
                is_platform: false,
            });
        }
        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();
        assert!(!report.complete);
        assert_eq!(report.jar_paths.len(), 1);
        assert_eq!(report.missing, vec!["org.fixture:absent:1".to_string()]);
    }

    fn write_module_artifact(
        cache: &Path,
        artifact: &str,
        version: &str,
        with_jar: bool,
        module_json: &str,
        pom_packaging: &str,
    ) {
        let dir = cache.join(format!("org.fixture/{artifact}/{version}/hash"));
        std::fs::create_dir_all(&dir).unwrap();
        if with_jar {
            std::fs::write(dir.join(format!("{artifact}-{version}.jar")), b"x").unwrap();
        }
        std::fs::write(
            dir.join(format!("{artifact}-{version}.pom")),
            format!("<project><packaging>{pom_packaging}</packaging></project>"),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("{artifact}-{version}.module")),
            module_json,
        )
        .unwrap();
    }

    fn fixture_dependency(
        artifact: &str,
        version: Option<&str>,
        configuration: DependencyConfiguration,
    ) -> GradleDependency {
        GradleDependency {
            exclusions: Vec::new(),
            transitive: true,
            forced: false,
            group: "org.fixture".into(),
            artifact: artifact.into(),
            version: version.map(str::to_string),
            configuration,
            classifier: None,
            packaging: None,
            is_platform: false,
        }
    }

    #[tokio::test]
    async fn gradle_module_metadata_separates_api_runtime_and_platform_constraints() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        write_module_artifact(
            &cache,
            "lib",
            "1.0",
            true,
            r#"{"formatVersion":"1.1","variants":[
              {"name":"apiElements","attributes":{"org.gradle.usage":"java-api","org.gradle.category":"library"},
               "files":[{"name":"lib-1.0.jar","url":"lib-1.0.jar"}],
               "dependencies":[{"group":"org.fixture","module":"bom","version":{"requires":"3"},"attributes":{"org.gradle.category":"platform"}},
                               {"group":"org.fixture","module":"annotations","version":{"requires":"1"}}]},
              {"name":"runtimeElements","attributes":{"org.gradle.usage":"java-runtime","org.gradle.category":"library"},
               "files":[{"name":"lib-1.0.jar","url":"lib-1.0.jar"}],
               "dependencies":[{"group":"org.fixture","module":"bom","version":{"requires":"3"}},
                               {"group":"org.fixture","module":"engine","version":{"requires":"1"}}]}]}"#,
            "jar",
        );
        write_module_artifact(
            &cache,
            "annotations",
            "1",
            true,
            r#"{"variants":[]}"#,
            "jar",
        );
        write_module_artifact(&cache, "engine", "1", true, r#"{"variants":[]}"#, "jar");
        write_module_artifact(
            &cache,
            "bom",
            "3",
            false,
            r#"{"variants":[{"name":"apiElements","attributes":{"org.gradle.usage":"java-api","org.gradle.category":"platform"},
               "dependencyConstraints":[{"group":"org.fixture","module":"launcher","version":{"requires":"7"}}]}]}"#,
            "pom",
        );
        write_module_artifact(&cache, "launcher", "7", true, r#"{"variants":[]}"#, "jar");
        let resolver = GradleResolver::with_cache_dirs(cache, temp.path().join("maven"));
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.dependencies = vec![
            fixture_dependency("lib", Some("1.0"), DependencyConfiguration::Implementation),
            // No version anywhere in the build: the platform supplies it.
            fixture_dependency("launcher", None, DependencyConfiguration::TestRuntimeOnly),
        ];

        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();

        assert!(report.complete, "{:?}", report.incomplete_reasons);
        let names = |paths: &[PathBuf]| -> Vec<String> {
            let mut names: Vec<String> = paths
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        // `annotations` is only in the api variant: compile classpath, not runtime.
        assert_eq!(
            names(&report.main_compile_jars),
            ["annotations-1.jar", "lib-1.0.jar"]
        );
        assert_eq!(
            names(&report.main_runtime_jars),
            ["engine-1.jar", "lib-1.0.jar"]
        );
        // The versionless test declaration got the platform's version.
        assert!(report
            .test_runtime_jars
            .iter()
            .any(|path| path.ends_with("launcher-7.jar")));
        assert!(!report
            .main_runtime_jars
            .iter()
            .any(|path| path.ends_with("launcher-7.jar")));
    }

    #[tokio::test]
    async fn available_at_redirects_to_the_module_that_holds_the_artifact() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        write_module_artifact(
            &cache,
            "multiplatform",
            "1",
            false,
            r#"{"variants":[
              {"name":"jvmApiElements-published","attributes":{"org.gradle.usage":"java-api","org.jetbrains.kotlin.platform.type":"jvm"},
               "available-at":{"url":"../multiplatform-jvm/1/x.module","group":"org.fixture","module":"multiplatform-jvm","version":"1"}},
              {"name":"jvmRuntimeElements-published","attributes":{"org.gradle.usage":"java-runtime","org.jetbrains.kotlin.platform.type":"jvm"},
               "available-at":{"url":"../multiplatform-jvm/1/x.module","group":"org.fixture","module":"multiplatform-jvm","version":"1"}},
              {"name":"jsApiElements-published","attributes":{"org.gradle.usage":"kotlin-api","org.jetbrains.kotlin.platform.type":"js"},"files":[{"name":"m.klib","url":"m.klib"}]}]}"#,
            "jar",
        );
        write_module_artifact(
            &cache,
            "multiplatform-jvm",
            "1",
            true,
            r#"{"variants":[]}"#,
            "jar",
        );
        let resolver = GradleResolver::with_cache_dirs(cache, temp.path().join("maven"));
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.dependencies = vec![fixture_dependency(
            "multiplatform",
            Some("1"),
            DependencyConfiguration::Implementation,
        )];

        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();

        assert!(report.complete, "{:?}", report.incomplete_reasons);
        assert_eq!(report.jar_paths.len(), 1);
        assert!(report.jar_paths[0].ends_with("multiplatform-jvm-1.jar"));
    }

    #[tokio::test]
    async fn kotlin_jvm_plugin_adds_the_stdlib_at_the_plugin_version() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let dir = cache.join("org.jetbrains.kotlin/kotlin-stdlib/2.0.0/hash");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kotlin-stdlib-2.0.0.jar"), b"x").unwrap();
        std::fs::write(dir.join("kotlin-stdlib-2.0.0.pom"), "<project/>").unwrap();
        let resolver = GradleResolver::with_cache_dirs(cache, temp.path().join("maven"));
        let mut project = GradleProject::new("fixture".into(), temp.path().into());
        project.plugins = vec![crate::gradle::model::GradlePlugin::with_version(
            "org.jetbrains.kotlin.jvm".to_string(),
            "2.0.0".to_string(),
        )];

        let report = resolver
            .resolve_dependencies_with_report(&project, 8)
            .await
            .unwrap();

        assert!(report.complete, "{:?}", report.incomplete_reasons);
        assert!(report
            .main_compile_jars
            .iter()
            .any(|path| path.ends_with("kotlin-stdlib-2.0.0.jar")));
    }

    #[test]
    fn test_build_bom_version_map_with_spring_boot_plugin() {
        use crate::gradle::model::GradleProject;

        let resolver = GradleResolver::new();

        // Create a project with Spring Boot and dependency-management plugins
        let mut project = GradleProject::new("test-project".to_string(), PathBuf::from("/test"));
        project.plugins = vec![
            crate::gradle::model::GradlePlugin::with_version(
                "org.springframework.boot".to_string(),
                "3.4.1".to_string(),
            ),
            crate::gradle::model::GradlePlugin::with_version(
                "io.spring.dependency-management".to_string(),
                "1.1.7".to_string(),
            ),
        ];

        // This test verifies the detection logic runs without panicking
        // Actual BOM parsing depends on having the BOM in Gradle cache
        let bom = resolver.build_bom_version_map(&project);
        let (bom_map, bom_complete) = (bom.versions, bom.problems.is_empty());

        // If the Spring Boot BOM is in the cache, we should have many managed dependencies
        // If not, the map will be empty and the completeness flag will be false.
        println!("BOM map has {} entries", bom_map.len());

        // If we have entries, verify some expected Spring Boot managed dependencies
        if !bom_map.is_empty() {
            // Spring Boot BOM should manage spring-boot-starter-web
            assert!(
                bom_map.contains_key("org.springframework.boot:spring-boot-starter-web:jar:"),
                "Expected spring-boot-starter-web to be managed"
            );
            // Verify the version is correct
            assert_eq!(
                bom_map.get("org.springframework.boot:spring-boot-starter-web:jar:"),
                Some(&"3.4.1".to_string())
            );
        }
        if bom_map.is_empty() {
            assert!(!bom_complete);
        }
    }

    #[test]
    fn test_build_bom_version_map_without_dependency_management_plugin() {
        use crate::gradle::model::GradleProject;

        let resolver = GradleResolver::new();

        // Create a project with only Spring Boot plugin (no dependency-management)
        let mut project = GradleProject::new("test-project".to_string(), PathBuf::from("/test"));
        project.plugins = vec![crate::gradle::model::GradlePlugin::with_version(
            "org.springframework.boot".to_string(),
            "3.4.1".to_string(),
        )];

        // Without dependency-management plugin, BOM is not implicitly applied
        let bom = resolver.build_bom_version_map(&project);
        let (bom_map, bom_complete) = (bom.versions, bom.problems.is_empty());
        assert!(bom_map.is_empty());
        assert!(bom_complete);
    }
}
