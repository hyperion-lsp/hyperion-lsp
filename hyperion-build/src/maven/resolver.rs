// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven dependency resolution.
//!
//! This module resolves Maven dependencies to actual JAR files by:
//! 1. Checking local Maven repository (~/.m2/repository)
//! 2. Resolving transitive dependencies (cycle-safe, optionally depth-limited)
//! 3. Reporting missing artifacts without performing network access

use super::activation::{ProfileActivationContext, ProfileEvaluator};
use super::model::{DependencyScope, ManagedDependencyKey, MavenDependency, MavenProject};
use crate::pom::{management_key, PomModelCache, PomRepository};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

const MAX_MAVEN_TRANSITIVE_DEPTH: usize = 256;
const MAX_MAVEN_DEPENDENCY_NODES: usize = 20_000;
const MAX_MAVEN_EFFECTIVE_POM_NODES: usize = 4_096;
const MAX_MAVEN_AGGREGATE_POM_BYTES: u64 = 64 * 1024 * 1024;

/// Errors that can occur during dependency resolution
#[derive(Debug, thiserror::Error)]
pub enum MavenResolverError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Dependency not found: {0}")]
    DependencyNotFound(String),

    #[error("Invalid Maven coordinates: {0}")]
    InvalidCoordinates(String),
}

pub type Result<T> = std::result::Result<T, MavenResolverError>;

/// Information about a missing dependency
#[derive(Debug, Clone)]
pub struct MissingDependency {
    /// Dependency coordinates (group:artifact:version)
    pub coordinates: String,
    /// Expected path where the JAR should be
    pub expected_path: PathBuf,
    /// Why it's missing (e.g., "Not in local repository")
    pub reason: String,
}

/// Report of dependency resolution results
#[derive(Debug, Clone)]
pub struct ResolutionReport {
    /// Successfully resolved JAR files
    pub resolved_jars: Vec<PathBuf>,
    /// Dependencies that couldn't be resolved
    pub missing_dependencies: Vec<MissingDependency>,
    /// Whether every reachable dependency and its metadata were inspected.
    ///
    /// Partial reports remain useful for navigation, but must not be used as an
    /// exhaustive proof for destructive workspace operations.
    pub complete: bool,
    /// JARs on the main (compile/runtime) classpath, in resolution order.
    /// `resolved_jars` additionally contains the test-only JARs.
    pub main_jars: Vec<PathBuf>,
    /// Precise metadata problems (unreadable or missing POMs, unmanaged
    /// versions). Every dependency that did resolve is still in the report.
    pub issues: Vec<String>,
}

/// Result of [`MavenResolver::repair_effective_model`].
#[derive(Debug, Clone)]
pub struct EffectiveModelRepair {
    /// Every dependency has fully resolved coordinates and a version.
    pub determined: bool,
    /// Ancestors or imported BOMs that could not be read; the model is then
    /// not exact even if every dependency happens to have a version.
    pub problems: Vec<String>,
}

/// Maven dependency resolver
pub struct MavenResolver {
    /// Maven local repository (~/.m2/repository)
    maven_local_dir: PathBuf,

    /// Effective-POM loader over the local repository.
    poms: PomModelCache,
}

/// Shared safety accounting for one Maven effective-model transaction.
///
/// Reusing this value across reactor modules prevents individually bounded
/// parent/BOM walks from becoming unbounded in aggregate.
#[derive(Default)]
pub struct MavenResolutionBudget {
    /// Coordinates currently being expanded through either a parent edge or a
    /// BOM-import edge. One shared stack is required: a parent importing a BOM
    /// which inherits that parent is just as cyclic as two parent edges.
    active_coordinates: HashSet<String>,
    expanded_nodes: usize,
    parsed_bytes: u64,
}

impl MavenResolutionBudget {
    fn enter(&mut self, coordinate: String) -> Result<()> {
        if !self.active_coordinates.insert(coordinate.clone()) {
            return Err(MavenResolverError::InvalidCoordinates(format!(
                "cyclic Maven parent/BOM graph at {coordinate}"
            )));
        }
        Ok(())
    }

    fn charge_pom(&mut self, path: &Path) -> Result<()> {
        if self.expanded_nodes >= MAX_MAVEN_EFFECTIVE_POM_NODES {
            return Err(MavenResolverError::InvalidCoordinates(format!(
                "Maven effective-model traversal exceeds the {MAX_MAVEN_EFFECTIVE_POM_NODES}-POM safety bound"
            )));
        }
        let length = std::fs::metadata(path)?.len();
        let Some(total) = self.parsed_bytes.checked_add(length) else {
            return Err(MavenResolverError::InvalidCoordinates(
                "Maven effective-model byte accounting overflowed".to_string(),
            ));
        };
        if total > MAX_MAVEN_AGGREGATE_POM_BYTES {
            return Err(MavenResolverError::InvalidCoordinates(format!(
                "Maven effective-model traversal exceeds the {MAX_MAVEN_AGGREGATE_POM_BYTES}-byte aggregate POM bound"
            )));
        }
        self.expanded_nodes += 1;
        self.parsed_bytes = total;
        Ok(())
    }
}

/// Mutable state shared by every branch of one transitive dependency walk.
///
/// Keeping the report, conflict mediation, and safety accounting together
/// makes it impossible for a recursive call to accidentally use fresh or
/// mismatched bookkeeping.
struct TransitiveResolutionState {
    resolved_jars: Vec<PathBuf>,
    missing_dependencies: Vec<MissingDependency>,
    issues: Vec<String>,
    seen: HashSet<String>,
    complete: bool,
    max_depth: usize,
}

impl TransitiveResolutionState {
    fn new(max_depth: usize) -> Self {
        Self {
            resolved_jars: Vec::new(),
            missing_dependencies: Vec::new(),
            issues: Vec::new(),
            seen: HashSet::new(),
            complete: true,
            max_depth: max_depth.min(MAX_MAVEN_TRANSITIVE_DEPTH),
        }
    }
}

/// A dependency waiting in the breadth-first walk (Maven's "nearest wins").
struct PendingDependency {
    dependency: MavenDependency,
    depth: usize,
    /// Exclusions declared anywhere on the path from the project.
    exclusions: Vec<(String, String)>,
}

impl MavenResolver {
    /// Create a new resolver with default repository
    pub fn new() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());

        let maven_local = PathBuf::from(&home).join(".m2").join("repository");

        Self::with_repository(maven_local)
    }

    /// Create a resolver with custom repository directory
    pub fn with_repository(maven_local: PathBuf) -> Self {
        let poms = PomModelCache::new(vec![PomRepository::MavenLocal(maven_local.clone())]);
        Self {
            maven_local_dir: maven_local,
            poms,
        }
    }

    /// Resolve parent POM and merge its properties and managed dependencies
    pub async fn resolve_parent_pom(&self, project: &mut MavenProject) -> Result<()> {
        let mut resolution = MavenResolutionBudget::default();
        self.resolve_parent_pom_inner(project, &mut resolution)
            .await
    }

    /// Resolve a parent while sharing resource accounting with the surrounding
    /// reactor/effective-model transaction.
    pub async fn resolve_parent_pom_with_budget(
        &self,
        project: &mut MavenProject,
        resolution: &mut MavenResolutionBudget,
    ) -> Result<()> {
        self.resolve_parent_pom_inner(project, resolution).await
    }

    async fn resolve_parent_pom_inner(
        &self,
        project: &mut MavenProject,
        resolution: &mut MavenResolutionBudget,
    ) -> Result<()> {
        if let Some(mut parent) = project.parent.clone() {
            parent.group_id = project.resolve_property(&parent.group_id);
            parent.artifact_id = project.resolve_property(&parent.artifact_id);
            parent.version = project.resolve_property(&parent.version);
            parent.relative_path = parent
                .relative_path
                .as_deref()
                .map(|path| project.resolve_property(path));
            if parent.group_id.contains("${")
                || parent.artifact_id.contains("${")
                || parent.version.contains("${")
            {
                return Err(MavenResolverError::InvalidCoordinates(format!(
                    "parent coordinates remain unresolved: {}:{}:{}",
                    parent.group_id, parent.artifact_id, parent.version
                )));
            }
            debug!(
                "Resolving parent POM: {}:{}:{}",
                parent.group_id, parent.artifact_id, parent.version
            );

            let parent_key = format!(
                "{}:{}:{}",
                parent.group_id, parent.artifact_id, parent.version
            );
            resolution.enter(parent_key.clone())?;

            // Maven checks the reactor-relative parent before the repository.
            // `None` means the conventional ../pom.xml; an explicit empty
            // relativePath disables that lookup.
            let relative_parent = parent.relative_path.as_deref().unwrap_or("../pom.xml");
            let reactor_parent = (!relative_parent.is_empty())
                .then(|| project.root_dir.join(relative_parent))
                .filter(|path| path.is_file());
            let parent_pom_path = if let Some(path) = reactor_parent {
                path
            } else {
                self.pom_path_for(&parent.group_id, &parent.artifact_id, &parent.version)?
            };

            if parent_pom_path.exists() {
                resolution.charge_pom(&parent_pom_path)?;
                // Parse parent POM
                let parent_dir = parent_pom_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."));
                let mut parent_project = super::model::MavenProject::new(
                    parent.artifact_id.clone(),
                    parent_dir.to_path_buf(),
                );

                super::parser::MavenParser::parse_pom(&parent_pom_path, &mut parent_project)
                    .map_err(|e| MavenResolverError::Io(std::io::Error::other(e.to_string())))?;

                // Recursively resolve parent's parent
                if parent_project.parent.is_some() {
                    Box::pin(self.resolve_parent_pom_inner(&mut parent_project, resolution))
                        .await?;
                }
                let activation_context =
                    ProfileActivationContext::new(parent_project.root_dir.clone());
                self.activate_profiles(&mut parent_project, &activation_context)
                    .await?;
                Box::pin(self.resolve_bom_imports_inner(&mut parent_project, resolution)).await?;
                if !self.finalize_dependency_model(&mut parent_project) {
                    return Err(MavenResolverError::InvalidCoordinates(format!(
                        "parent POM {} retains unresolved effective coordinates",
                        parent_pom_path.display()
                    )));
                }
                if parent_project.group_id.as_deref() != Some(parent.group_id.as_str())
                    || parent_project.artifact_id != parent.artifact_id
                    || parent_project.version.as_deref() != Some(parent.version.as_str())
                {
                    return Err(MavenResolverError::InvalidCoordinates(format!(
                        "parent POM {} does not match declared coordinates {}:{}:{}",
                        parent_pom_path.display(),
                        parent.group_id,
                        parent.artifact_id,
                        parent.version
                    )));
                }

                // Merge parent properties (child overrides parent)
                for (key, value) in &parent_project.properties {
                    project
                        .properties
                        .entry(key.clone())
                        .or_insert(value.clone());
                }

                // Supported inherited content remains useful even when a
                // parent contains unmodeled constructs. Carry their blockers
                // into the child instead of discarding all parent profiles
                // or claiming that the child is complete.
                for feature in &parent_project.unmodeled_features {
                    if !project.unmodeled_features.contains(feature) {
                        project.unmodeled_features.push(feature.clone());
                    }
                }

                // Merge parent managed dependencies (child overrides parent)
                for (key, dependency) in &parent_project.managed_dependencies {
                    project
                        .managed_dependencies
                        .entry(key.clone())
                        .or_insert(dependency.clone());
                }

                if project.group_id.is_none() {
                    project.group_id.clone_from(&parent_project.group_id);
                }
                if project.version.is_none() {
                    project.version.clone_from(&parent_project.version);
                }
                for dependency in parent_project.dependencies {
                    let inherited_key = (
                        dependency.group_id.as_str(),
                        dependency.artifact_id.as_str(),
                        dependency.packaging.as_str(),
                        dependency.classifier.as_deref(),
                    );
                    let overridden = project.dependencies.iter().any(|child| {
                        (
                            child.group_id.as_str(),
                            child.artifact_id.as_str(),
                            child.packaging.as_str(),
                            child.classifier.as_deref(),
                        ) == inherited_key
                    });
                    if !overridden {
                        project.dependencies.push(dependency);
                    }
                }

                info!(
                    "Merged parent POM {}:{} - {} properties, {} managed deps",
                    parent.group_id,
                    parent.artifact_id,
                    parent_project.properties.len(),
                    parent_project.managed_dependencies.len()
                );
                resolution.active_coordinates.remove(&parent_key);
            } else {
                return Err(MavenResolverError::DependencyNotFound(format!(
                    "parent POM {}:{}:{} (expected at {})",
                    parent.group_id,
                    parent.artifact_id,
                    parent.version,
                    parent_pom_path.display()
                )));
            }
        }
        Ok(())
    }

    /// Rebuild what the strict effective-model pipeline could not determine
    /// (a parent or imported BOM it rejects) from the lenient shared POM model.
    ///
    /// Dependencies with unresolved coordinates are replaced by their effective
    /// counterparts, missing versions and scopes are filled in, and the
    /// effective `dependencyManagement` is merged under the existing entries.
    /// The outcome names every ancestor/import that could not be read; with none
    /// missing, `determined` says whether every dependency ended up fully
    /// resolved.
    pub fn repair_effective_model(
        &self,
        project: &mut MavenProject,
        pom_path: &Path,
    ) -> std::result::Result<EffectiveModelRepair, String> {
        let effective = self.poms.effective_for_file(pom_path)?;
        let is_dynamic = |dependency: &MavenDependency| {
            dependency.group_id.contains("${")
                || dependency.artifact_id.contains("${")
                || dependency.packaging.contains("${")
        };
        project
            .dependencies
            .retain(|dependency| !is_dynamic(dependency));
        for entry in &effective.dependencies {
            let existing = project.dependencies.iter_mut().find(|dependency| {
                dependency.group_id == entry.group
                    && dependency.artifact_id == entry.artifact
                    && dependency.packaging == entry.type_
                    && dependency.classifier == entry.classifier
            });
            match existing {
                Some(dependency) => {
                    if dependency.version.is_none()
                        || dependency
                            .version
                            .as_deref()
                            .is_some_and(|version| version.contains("${"))
                    {
                        dependency.version.clone_from(&entry.version);
                    }
                    if !dependency.scope_explicit {
                        if let Some(scope) = &entry.scope {
                            dependency.scope = DependencyScope::from_scope_str(scope);
                        }
                    }
                    for exclusion in &entry.exclusions {
                        if !dependency.exclusions.contains(exclusion) {
                            dependency.exclusions.push(exclusion.clone());
                        }
                    }
                }
                None => project.dependencies.push(MavenDependency {
                    group_id: entry.group.clone(),
                    artifact_id: entry.artifact.clone(),
                    version: entry.version.clone(),
                    scope: entry
                        .scope
                        .as_deref()
                        .map_or(DependencyScope::Compile, DependencyScope::from_scope_str),
                    scope_explicit: entry.scope.is_some(),
                    classifier: entry.classifier.clone(),
                    packaging: entry.type_.clone(),
                    packaging_explicit: true,
                    system_path: None,
                    optional: entry.optional,
                    optional_explicit: false,
                    exclusions: entry.exclusions.clone(),
                }),
            }
        }
        for (key, entry) in &effective.managed {
            let Some(version) = &entry.version else {
                continue;
            };
            let mut parts = key.splitn(4, ':');
            let (Some(group), Some(artifact), Some(type_), Some(classifier)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let dependency = MavenDependency {
                group_id: group.to_string(),
                artifact_id: artifact.to_string(),
                version: Some(version.clone()),
                scope: entry
                    .scope
                    .as_deref()
                    .map_or(DependencyScope::Compile, DependencyScope::from_scope_str),
                scope_explicit: entry.scope.is_some(),
                classifier: (!classifier.is_empty()).then(|| classifier.to_string()),
                packaging: type_.to_string(),
                packaging_explicit: true,
                system_path: None,
                optional: false,
                optional_explicit: false,
                exclusions: entry.exclusions.clone(),
            };
            project
                .managed_dependencies
                .entry(super::model::ManagedDependencyKey::for_dependency(
                    &dependency,
                ))
                .or_insert(dependency);
        }
        let determined = project.dependencies.iter().all(|dependency| {
            dependency_model_is_complete(dependency) || dependency.scope == DependencyScope::System
        });
        Ok(EffectiveModelRepair {
            determined,
            problems: effective.problems.clone(),
        })
    }

    /// Apply properties and dependency management after parent/profile/BOM
    /// merging. Returns false for unresolved coordinates or unmodeled features.
    pub fn finalize_project_dependencies(&self, project: &mut MavenProject) -> bool {
        self.finalize_dependency_model(project) && project.unmodeled_features.is_empty()
    }

    /// Finalize the modeled content independently of structural completeness,
    /// so a parent's supported facts can be inherited along with its blockers.
    fn finalize_dependency_model(&self, project: &mut MavenProject) -> bool {
        let properties = project.clone();
        let raw_managed_dependencies = std::mem::take(&mut project.managed_dependencies);
        project.group_id = project
            .group_id
            .as_deref()
            .map(|group| properties.resolve_property(group));
        project.artifact_id = properties.resolve_property(&project.artifact_id);
        project.version = project
            .version
            .as_deref()
            .map(|version| properties.resolve_property(version));
        let mut complete = project
            .group_id
            .as_ref()
            .is_some_and(|group| !group.contains("${"))
            && !project.artifact_id.contains("${")
            && project
                .version
                .as_ref()
                .is_some_and(|version| !version.contains("${"));
        for (_, mut dependency) in raw_managed_dependencies {
            resolve_dependency_properties(&mut dependency, &properties);
            complete &= dependency_model_is_complete(&dependency);
            let key = super::model::ManagedDependencyKey::for_dependency(&dependency);
            if project
                .managed_dependencies
                .insert(key, dependency)
                .is_some()
            {
                complete = false;
            }
        }
        let managed_dependencies = project.managed_dependencies.clone();
        for dependency in &mut project.dependencies {
            resolve_dependency_properties(dependency, &properties);
            MavenProject::apply_managed_dependency_from(&managed_dependencies, dependency);
            // Managed attributes can introduce a system path or scope after the
            // direct declaration was expanded.
            resolve_dependency_properties(dependency, &properties);
            complete &= dependency_model_is_complete(dependency);
        }
        complete
    }

    /// Resolve BOM imports and merge their managed dependencies
    pub async fn resolve_bom_imports(&self, project: &mut MavenProject) -> Result<()> {
        let mut resolution = MavenResolutionBudget::default();
        self.resolve_bom_imports_inner(project, &mut resolution)
            .await
    }

    /// Resolve BOM imports using shared effective-model resource accounting.
    pub async fn resolve_bom_imports_with_budget(
        &self,
        project: &mut MavenProject,
        resolution: &mut MavenResolutionBudget,
    ) -> Result<()> {
        self.resolve_bom_imports_inner(project, resolution).await
    }

    async fn resolve_bom_imports_inner(
        &self,
        project: &mut MavenProject,
        resolution: &mut MavenResolutionBudget,
    ) -> Result<()> {
        if project.bom_imports.is_empty() {
            return Ok(());
        }

        info!("Resolving {} BOM imports", project.bom_imports.len());

        for bom_import in project.bom_imports.clone() {
            let group_id = project.resolve_property(&bom_import.group_id);
            let artifact_id = project.resolve_property(&bom_import.artifact_id);
            let version = match bom_import.version.as_deref() {
                Some(version) => project.resolve_property(version),
                None => {
                    return Err(MavenResolverError::InvalidCoordinates(format!(
                        "BOM import {}:{} has no version",
                        group_id, artifact_id
                    )));
                }
            };
            if group_id.contains("${") || artifact_id.contains("${") || version.contains("${") {
                return Err(MavenResolverError::InvalidCoordinates(format!(
                    "BOM import has unresolved coordinates {group_id}:{artifact_id}:{version}"
                )));
            }

            debug!(
                "Resolving BOM import: {}:{}:{}",
                group_id, artifact_id, version
            );
            let bom_key = format!("{group_id}:{artifact_id}:{version}");
            resolution.enter(bom_key.clone())?;

            // Find BOM POM in local repository
            let bom_pom_path = self.pom_path_for(&group_id, &artifact_id, &version)?;

            if bom_pom_path.exists() {
                resolution.charge_pom(&bom_pom_path)?;
                // Parse BOM POM
                let bom_dir = bom_pom_path.parent().unwrap_or(std::path::Path::new("."));
                let mut bom_project =
                    super::model::MavenProject::new(artifact_id.clone(), bom_dir.to_path_buf());

                super::parser::MavenParser::parse_pom(&bom_pom_path, &mut bom_project)
                    .map_err(|e| MavenResolverError::Io(std::io::Error::other(e.to_string())))?;

                if bom_project.parent.is_some() {
                    Box::pin(self.resolve_parent_pom_inner(&mut bom_project, resolution)).await?;
                }
                Box::pin(self.resolve_bom_imports_inner(&mut bom_project, resolution)).await?;
                if !self.finalize_project_dependencies(&mut bom_project) {
                    return Err(MavenResolverError::InvalidCoordinates(format!(
                        "BOM POM {} retains unresolved effective coordinates",
                        bom_pom_path.display()
                    )));
                }
                if bom_project.group_id.as_deref() != Some(group_id.as_str())
                    || bom_project.artifact_id != artifact_id
                    || bom_project.version.as_deref() != Some(version.as_str())
                {
                    return Err(MavenResolverError::InvalidCoordinates(format!(
                        "BOM POM {} does not match declared coordinates {}:{}:{}",
                        bom_pom_path.display(),
                        group_id,
                        artifact_id,
                        version
                    )));
                }

                // Import the BOM's managed dependencies (child overrides BOM)
                for (key, dependency) in &bom_project.managed_dependencies {
                    project
                        .managed_dependencies
                        .entry(key.clone())
                        .or_insert(dependency.clone());
                }

                info!(
                    "Imported BOM {}:{}:{} - {} managed dependencies",
                    group_id,
                    artifact_id,
                    version,
                    bom_project.managed_dependencies.len()
                );
                resolution.active_coordinates.remove(&bom_key);
            } else {
                return Err(MavenResolverError::DependencyNotFound(format!(
                    "BOM {}:{}:{} (expected at {}). Run `mvn dependency:resolve` to populate the repository",
                    group_id,
                    artifact_id,
                    version,
                    bom_pom_path.display()
                )));
            }
        }

        Ok(())
    }

    /// Activate profiles based on context and merge their dependencies/properties
    pub async fn activate_profiles(
        &self,
        project: &mut MavenProject,
        context: &ProfileActivationContext,
    ) -> Result<()> {
        if project.profiles.is_empty() {
            return Ok(());
        }

        info!(
            "Evaluating {} profiles for activation",
            project.profiles.len()
        );

        let mut activated_profiles = Vec::new();
        let mut activated_dependencies = Vec::new();
        let mut activated_properties = HashMap::new();

        for profile in ProfileEvaluator::select_active(&project.profiles, context) {
            info!("Activating profile: {}", profile.id);
            activated_profiles.push(profile.id.clone());

            // Add profile dependencies
            activated_dependencies.extend(profile.dependencies.clone());

            // Merge profile properties (profile properties override base)
            for (key, value) in &profile.properties {
                activated_properties.insert(key.clone(), value.clone());
            }
        }

        // Merge activated content into project
        for dependency in activated_dependencies {
            let key = ManagedDependencyKey::for_dependency(&dependency);
            if let Some(existing) = project
                .dependencies
                .iter_mut()
                .find(|existing| ManagedDependencyKey::for_dependency(existing) == key)
            {
                // Maven injects a profile dependency as a whole declaration.
                // Omitted attributes come from management, not the replaced
                // dependency. Later active profiles dominate earlier ones.
                *existing = dependency;
            } else {
                project.dependencies.push(dependency);
            }
        }

        for (key, value) in activated_properties {
            project.properties.insert(key, value);
        }

        info!(
            "Activated {} profiles: {:?}",
            activated_profiles.len(),
            activated_profiles
        );

        Ok(())
    }

    /// Get the expected JAR path for a dependency
    fn get_jar_path(&self, dep: &MavenDependency) -> Result<PathBuf> {
        let version = dep.version.as_ref().ok_or_else(|| {
            MavenResolverError::InvalidCoordinates("No version specified".to_string())
        })?;

        let group_path = maven_group_path(&dep.group_id)?;
        let jar_name = maven_artifact_name(dep, version)?;

        Ok(self
            .maven_local_dir
            .join(group_path)
            .join(&dep.artifact_id)
            .join(version)
            .join(jar_name))
    }

    fn pom_path_for(&self, group: &str, artifact: &str, version: &str) -> Result<PathBuf> {
        let group_path = maven_group_path(group)?;
        validate_maven_component(artifact, "artifact ID")?;
        validate_maven_component(version, "version")?;
        let pom_name = format!("{artifact}-{version}.pom");
        Ok(self
            .maven_local_dir
            .join(group_path)
            .join(artifact)
            .join(version)
            .join(pom_name))
    }

    /// Resolve all dependencies for a project (including transitive)
    pub async fn resolve_dependencies(&self, project: &MavenProject) -> Result<Vec<PathBuf>> {
        let report = self
            .resolve_dependencies_with_report(project, usize::MAX)
            .await;
        Ok(report.resolved_jars)
    }

    /// Resolve dependencies and return a detailed report.
    ///
    /// The walk is breadth-first with Maven's "nearest declaration wins"
    /// mediation, applies the project's `dependencyManagement` to transitive
    /// dependencies, and reads library metadata through the shared
    /// effective-POM model (parents, BOM imports, properties). Dependencies
    /// that cannot be located are reported individually; everything that
    /// resolved is still returned.
    pub async fn resolve_dependencies_with_report(
        &self,
        project: &MavenProject,
        max_depth: usize,
    ) -> ResolutionReport {
        info!(
            "Resolving {} dependencies for project '{}' (depth={})",
            project.dependencies.len(),
            project.artifact_id,
            max_depth
        );
        // Every resolution rereads POMs: a refresh must see edited files.
        self.poms.clear();
        self.resolve_with_loaded_poms(project, max_depth)
    }

    /// Resolve several projects (the modules of one reactor) sharing one POM
    /// cache, so a library's metadata is read once for the whole reactor.
    pub async fn resolve_dependencies_for_modules(
        &self,
        projects: &[MavenProject],
        max_depth: usize,
    ) -> Vec<ResolutionReport> {
        self.poms.clear();
        projects
            .iter()
            .map(|project| self.resolve_with_loaded_poms(project, max_depth))
            .collect()
    }

    fn resolve_with_loaded_poms(
        &self,
        project: &MavenProject,
        max_depth: usize,
    ) -> ResolutionReport {
        let managed: HashMap<String, MavenDependency> = project
            .managed_dependencies
            .values()
            .map(|dependency| {
                (
                    management_key(
                        &dependency.group_id,
                        &dependency.artifact_id,
                        Some(&dependency.packaging),
                        dependency.classifier.as_deref(),
                    ),
                    dependency.clone(),
                )
            })
            .collect();

        // The main classpath is resolved without test-scoped declarations;
        // the full graph then adds the test-only JARs, like Maven's separate
        // compile and test classpaths.
        let main_directs: Vec<&MavenDependency> = project
            .dependencies
            .iter()
            .filter(|dependency| dependency.scope != DependencyScope::Test)
            .collect();
        let all_directs: Vec<&MavenDependency> = project.dependencies.iter().collect();
        let mut main_state = TransitiveResolutionState::new(max_depth);
        self.walk(&main_directs, &managed, &mut main_state);
        let mut state = TransitiveResolutionState::new(max_depth);
        self.walk(&all_directs, &managed, &mut state);

        // Problems of the main graph are a subset of the full graph's.
        state.complete &= main_state.complete;
        let main_jars = main_state.resolved_jars;

        info!(
            "Resolved {} total JARs ({} main), {} missing, {} metadata issues",
            state.resolved_jars.len(),
            main_jars.len(),
            state.missing_dependencies.len(),
            state.issues.len()
        );
        if !state.missing_dependencies.is_empty() {
            warn!(
                "Missing {} dependencies - run 'mvn dependency:resolve' to download",
                state.missing_dependencies.len()
            );
        }

        state.issues.sort();
        state.issues.dedup();
        ResolutionReport {
            resolved_jars: state.resolved_jars,
            missing_dependencies: state.missing_dependencies,
            complete: state.complete,
            main_jars,
            issues: state.issues,
        }
    }

    /// Resolve dependencies with specified transitive depth
    ///
    /// depth=0: Only direct dependencies
    /// depth=1: Direct + their dependencies
    /// depth=2: Direct + 2 levels of transitive (recommended)
    pub async fn resolve_dependencies_with_depth(
        &self,
        project: &MavenProject,
        max_depth: usize,
    ) -> Result<Vec<PathBuf>> {
        let report = self
            .resolve_dependencies_with_report(project, max_depth)
            .await;
        Ok(report.resolved_jars)
    }

    fn walk(
        &self,
        directs: &[&MavenDependency],
        managed: &HashMap<String, MavenDependency>,
        state: &mut TransitiveResolutionState,
    ) {
        // A module declared twice in one POM: Maven keeps the last declaration.
        let identity_of = |dependency: &MavenDependency| {
            (
                dependency.group_id.clone(),
                dependency.artifact_id.clone(),
                dependency.packaging.clone(),
                dependency.classifier.clone(),
            )
        };
        let last_declarations: HashMap<_, usize> = directs
            .iter()
            .enumerate()
            .map(|(index, dependency)| (identity_of(dependency), index))
            .collect();
        let mut queue: VecDeque<PendingDependency> = directs
            .iter()
            .enumerate()
            .filter(|(index, dependency)| last_declarations[&identity_of(dependency)] == *index)
            .map(|(_, dependency)| dependency)
            .map(|dependency| PendingDependency {
                dependency: (*dependency).clone(),
                depth: 0,
                exclusions: Vec::new(),
            })
            .collect();
        while let Some(PendingDependency {
            mut dependency,
            depth,
            exclusions,
        }) = queue.pop_front()
        {
            let identity = format!(
                "{}:{}:{}:{}",
                dependency.group_id,
                dependency.artifact_id,
                dependency.packaging,
                dependency.classifier.as_deref().unwrap_or("")
            );
            if state.seen.contains(&identity) {
                // A nearer declaration already decided this module's version.
                continue;
            }
            if state.seen.len() >= MAX_MAVEN_DEPENDENCY_NODES {
                state.complete = false;
                state.issues.push(format!(
                    "dependency graph exceeds the {MAX_MAVEN_DEPENDENCY_NODES}-node safety bound"
                ));
                break;
            }
            // The project's dependencyManagement wins over transitive versions.
            if depth > 0 {
                let key = management_key(
                    &dependency.group_id,
                    &dependency.artifact_id,
                    Some(&dependency.packaging),
                    dependency.classifier.as_deref(),
                );
                if let Some(version) = managed.get(&key).and_then(|entry| entry.version.clone()) {
                    dependency.version = Some(version);
                }
            }
            state.seen.insert(identity);

            // Maven does not propagate optional or non-transitive scopes from a
            // dependency's POM into the consumer's classpath.
            if depth > 0
                && (dependency.optional
                    || matches!(
                        dependency.scope,
                        DependencyScope::Test
                            | DependencyScope::Provided
                            | DependencyScope::System
                            | DependencyScope::Import
                    ))
            {
                continue;
            }

            // System-scoped dependencies are exact, direct, and non-transitive.
            if dependency.scope == DependencyScope::System {
                match self.resolve_dependency_with_tracking(&dependency) {
                    Some(jar) => state.resolved_jars.push(jar),
                    None => {
                        state.complete = false;
                        let (expected_path, reason) = self.get_missing_dep_info(&dependency);
                        state.missing_dependencies.push(MissingDependency {
                            coordinates: dependency.coordinates(),
                            expected_path,
                            reason,
                        });
                    }
                }
                continue;
            }

            // Resolution is deliberately local-only. Hyperion never performs
            // network access; users populate Maven's repository with their
            // normal build tooling.
            let pom_packaged = dependency.packaging == "pom";
            if !pom_packaged {
                match self.resolve_dependency_with_tracking(&dependency) {
                    Some(jar) => {
                        if !state.resolved_jars.contains(&jar) {
                            state.resolved_jars.push(jar);
                        }
                    }
                    None => {
                        state.complete = false;
                        let (expected_path, reason) = self.get_missing_dep_info(&dependency);
                        state.missing_dependencies.push(MissingDependency {
                            coordinates: dependency.coordinates(),
                            expected_path,
                            reason,
                        });
                    }
                }
            }

            let Some(version) = dependency.version.clone() else {
                continue;
            };
            let effective =
                match self
                    .poms
                    .effective(&dependency.group_id, &dependency.artifact_id, &version)
                {
                    Ok(effective) => effective,
                    Err(problem) => {
                        // The JAR (if any) is kept; only its dependencies are unknown.
                        state.complete = false;
                        state.issues.push(format!(
                            "dependencies of {} are unknown: {problem}",
                            dependency.coordinates()
                        ));
                        continue;
                    }
                };
            for problem in &effective.problems {
                state.complete = false;
                state.issues.push(format!(
                    "metadata of {} is incomplete: {problem}",
                    dependency.coordinates()
                ));
            }
            for child in &effective.dependencies {
                if child.optional
                    || matches!(
                        child.scope.as_deref(),
                        Some("test" | "provided" | "system" | "import")
                    )
                {
                    continue;
                }
                if dependency
                    .exclusions
                    .iter()
                    .chain(&exclusions)
                    .any(|(group, artifact)| {
                        (group == "*" || group == &child.group)
                            && (artifact == "*" || artifact == &child.artifact)
                    })
                {
                    continue;
                }
                let mut child_dependency = MavenDependency {
                    group_id: child.group.clone(),
                    artifact_id: child.artifact.clone(),
                    version: child.version.clone(),
                    scope: child
                        .scope
                        .as_deref()
                        .map_or(DependencyScope::Compile, DependencyScope::from_scope_str),
                    scope_explicit: child.scope.is_some(),
                    classifier: child.classifier.clone(),
                    packaging: child.type_.clone(),
                    packaging_explicit: true,
                    system_path: None,
                    optional: false,
                    optional_explicit: false,
                    exclusions: child.exclusions.clone(),
                };
                if child_dependency.version.is_none() {
                    let key = management_key(
                        &child_dependency.group_id,
                        &child_dependency.artifact_id,
                        Some(&child_dependency.packaging),
                        child_dependency.classifier.as_deref(),
                    );
                    child_dependency.version =
                        managed.get(&key).and_then(|entry| entry.version.clone());
                }
                if child_dependency.version.is_none() {
                    state.complete = false;
                    state.issues.push(format!(
                        "{}:{} (required by {}) has no declared or managed version",
                        child.group,
                        child.artifact,
                        dependency.coordinates()
                    ));
                    continue;
                }
                if depth >= state.max_depth {
                    state.complete = false;
                    state.issues.push(format!(
                        "dependency traversal stopped at depth {} below {}",
                        state.max_depth,
                        dependency.coordinates()
                    ));
                    break;
                }
                let mut child_exclusions = exclusions.clone();
                for exclusion in &dependency.exclusions {
                    if !child_exclusions.contains(exclusion) {
                        child_exclusions.push(exclusion.clone());
                    }
                }
                queue.push_back(PendingDependency {
                    dependency: child_dependency,
                    depth: depth + 1,
                    exclusions: child_exclusions,
                });
            }
        }
    }

    /// Resolve a single dependency to a JAR file
    fn resolve_dependency(&self, dep: &MavenDependency) -> Result<Option<PathBuf>> {
        if dep.version.is_none() {
            warn!(
                "Skipping dependency with no version: {}:{}",
                dep.group_id, dep.artifact_id
            );
            return Ok(None);
        }

        let jar_path = if dep.scope == DependencyScope::System {
            let raw_path = dep.system_path.as_ref().ok_or_else(|| {
                MavenResolverError::InvalidCoordinates(format!(
                    "system-scoped dependency {} has no systemPath",
                    dep.coordinates()
                ))
            })?;
            let path = PathBuf::from(raw_path);
            if !path.is_absolute() {
                return Err(MavenResolverError::InvalidCoordinates(format!(
                    "systemPath for {} is not absolute: {}",
                    dep.coordinates(),
                    path.display()
                )));
            }
            path
        } else {
            self.get_jar_path(dep)?
        };

        if jar_path.is_file() {
            debug!("Resolved {}: {}", dep.coordinates(), jar_path.display());
            Ok(Some(jar_path))
        } else {
            warn!(
                "Dependency not found in local repository: {} (expected at {})",
                dep.coordinates(),
                jar_path.display()
            );
            Ok(None)
        }
    }

    /// Resolve a dependency without throwing errors (for tracking)
    fn resolve_dependency_with_tracking(&self, dep: &MavenDependency) -> Option<PathBuf> {
        match self.resolve_dependency(dep) {
            Ok(result) => result,
            Err(e) => {
                debug!("Error resolving {}: {}", dep.coordinates(), e);
                None
            }
        }
    }

    /// Get information about why a dependency is missing
    fn get_missing_dep_info(&self, dep: &MavenDependency) -> (PathBuf, String) {
        if dep.version.is_none() {
            return (PathBuf::new(), "No version specified".to_string());
        }

        if dep.scope == DependencyScope::System {
            return dep.system_path.as_ref().map_or_else(
                || {
                    (
                        PathBuf::new(),
                        "System-scoped dependency has no static absolute systemPath".to_string(),
                    )
                },
                |path| {
                    (
                        PathBuf::from(path),
                        format!("System dependency not found at exact systemPath {path}"),
                    )
                },
            );
        }

        let jar_path = match self.get_jar_path(dep) {
            Ok(path) => path,
            Err(error) => return (PathBuf::new(), error.to_string()),
        };

        (
            jar_path,
            "Not found in local Maven repository (~/.m2/repository). Run 'mvn dependency:resolve' to download.".to_string(),
        )
    }

    /// Resolve dependencies with javadoc
    pub async fn resolve_dependencies_with_docs(
        &self,
        project: &MavenProject,
    ) -> Result<Vec<(PathBuf, PathBuf)>> {
        info!(
            "Resolving {} dependencies with javadoc for project '{}'",
            project.dependencies.len(),
            project.artifact_id
        );

        let mut resolved_with_docs = Vec::new();

        for dep in &project.dependencies {
            if dep.scope == DependencyScope::System {
                continue;
            }
            if let Some(jar) = self.resolve_dependency(dep)? {
                // Try to find javadoc jar
                let javadoc_dep = MavenDependency {
                    classifier: Some("javadoc".to_string()),
                    ..dep.clone()
                };

                if let Some(javadoc_jar) = self.resolve_dependency(&javadoc_dep)? {
                    resolved_with_docs.push((jar, javadoc_jar));
                }
            }
        }

        info!(
            "Resolved {}/{} dependencies with javadoc",
            resolved_with_docs.len(),
            project.dependencies.len()
        );

        Ok(resolved_with_docs)
    }
}

impl Default for MavenResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_dependency_properties(dependency: &mut MavenDependency, properties: &MavenProject) {
    dependency.group_id = properties.resolve_property(&dependency.group_id);
    dependency.artifact_id = properties.resolve_property(&dependency.artifact_id);
    dependency.packaging = properties.resolve_property(&dependency.packaging);
    dependency.classifier = dependency
        .classifier
        .as_deref()
        .map(|classifier| properties.resolve_property(classifier));
    dependency.version = dependency
        .version
        .as_deref()
        .map(|version| properties.resolve_property(version));
    dependency.system_path = dependency
        .system_path
        .as_deref()
        .map(|path| properties.resolve_property(path));
    for (group, artifact) in &mut dependency.exclusions {
        *group = properties.resolve_property(group);
        *artifact = properties.resolve_property(artifact);
    }
}

fn dependency_model_is_complete(dependency: &MavenDependency) -> bool {
    let static_coordinates = !dependency.group_id.contains("${")
        && !dependency.artifact_id.contains("${")
        && !dependency.packaging.contains("${")
        && dependency
            .classifier
            .as_ref()
            .is_none_or(|classifier| !classifier.contains("${"))
        && dependency
            .version
            .as_ref()
            .is_some_and(|version| !version.contains("${"))
        && dependency
            .exclusions
            .iter()
            .all(|(group, artifact)| !group.contains("${") && !artifact.contains("${"));
    let exact_system_path = match dependency.scope {
        DependencyScope::System => dependency
            .system_path
            .as_ref()
            .is_some_and(|path| !path.contains("${") && std::path::Path::new(path).is_absolute()),
        _ => dependency.system_path.is_none(),
    };
    static_coordinates && exact_system_path
}

fn validate_maven_component(value: &str, description: &str) -> Result<()> {
    let invalid = value.is_empty()
        || matches!(value, "." | "..")
        || value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '/' | '\\' | ':' | '?' | '#')
        });
    if invalid {
        return Err(MavenResolverError::InvalidCoordinates(format!(
            "invalid Maven {description}: '{value}'"
        )));
    }
    Ok(())
}

fn maven_group_path(group: &str) -> Result<PathBuf> {
    let mut path = PathBuf::new();
    for segment in group.split('.') {
        validate_maven_component(segment, "group ID segment")?;
        path.push(segment);
    }
    Ok(path)
}

fn maven_artifact_name(dep: &MavenDependency, version: &str) -> Result<String> {
    validate_maven_component(&dep.artifact_id, "artifact ID")?;
    validate_maven_component(version, "version")?;
    validate_maven_component(&dep.packaging, "packaging")?;
    // Maven dependency types select artifact handlers, not literal extensions.
    // Keep the declared type intact for dependency-management matching, and
    // apply the standard handler's extension/default classifier only at lookup.
    let (extension, default_classifier) = match dep.packaging.as_str() {
        "test-jar" => ("jar", Some("tests")),
        "ejb-client" => ("jar", Some("client")),
        "java-source" => ("jar", Some("sources")),
        "javadoc" => ("jar", Some("javadoc")),
        "maven-plugin" | "ejb" => ("jar", None),
        type_ => (type_, None),
    };
    if let Some(classifier) = dep.classifier.as_deref().or(default_classifier) {
        validate_maven_component(classifier, "classifier")?;
        Ok(format!(
            "{}-{version}-{classifier}.{extension}",
            dep.artifact_id
        ))
    } else {
        Ok(format!("{}-{version}.{extension}", dep.artifact_id))
    }
}

#[cfg(test)]
mod local_repository_tests {
    use super::super::model::{DependencyScope, ParentPom};
    use super::*;

    #[test]
    fn aggregate_pom_budget_rejects_oversized_effective_model() {
        let repository = tempfile::tempdir().unwrap();
        let pom = repository.path().join("oversized.pom");
        let file = std::fs::File::create(&pom).unwrap();
        file.set_len(MAX_MAVEN_AGGREGATE_POM_BYTES + 1).unwrap();
        let mut budget = MavenResolutionBudget::default();

        assert!(matches!(
            budget.charge_pom(&pom),
            Err(MavenResolverError::InvalidCoordinates(message))
                if message.contains("aggregate POM bound")
        ));
    }

    #[tokio::test]
    async fn missing_dependency_is_reported_without_mutating_the_repository() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let dependency = MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: "missing".to_string(),
            version: Some("1.0".to_string()),
            scope: super::super::model::DependencyScope::Compile,
            scope_explicit: true,
            classifier: None,
            packaging: "jar".to_string(),
            packaging_explicit: false,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        };
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project.dependencies.push(dependency);

        let report = resolver.resolve_dependencies_with_report(&project, 2).await;

        assert!(report.resolved_jars.is_empty());
        assert!(!report.complete);
        assert_eq!(report.missing_dependencies.len(), 1);
        assert_eq!(
            report.missing_dependencies[0].coordinates,
            "org.example:missing:1.0"
        );
        assert!(report.missing_dependencies[0]
            .reason
            .contains("mvn dependency:resolve"));
        assert_eq!(std::fs::read_dir(repository.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn system_scope_uses_exact_system_path_never_same_gav_from_local_repository() {
        let fixture = tempfile::tempdir().unwrap();
        let repository = fixture.path().join("repository");
        let local_gav = repository
            .join("org/example/system-library/1.0")
            .join("system-library-1.0.jar");
        std::fs::create_dir_all(local_gav.parent().unwrap()).unwrap();
        std::fs::write(&local_gav, b"wrong local artifact").unwrap();
        let exact_system_jar = fixture.path().join("vendor/exact-system-library.jar");
        std::fs::create_dir_all(exact_system_jar.parent().unwrap()).unwrap();
        std::fs::write(&exact_system_jar, b"exact system artifact").unwrap();

        let resolver = MavenResolver::with_repository(repository);
        let mut project = MavenProject::new("fixture".to_string(), fixture.path().into());
        project.group_id = Some("org.example".to_string());
        project.version = Some("1.0".to_string());
        project.dependencies.push(MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: "system-library".to_string(),
            version: Some("1.0".to_string()),
            scope: DependencyScope::System,
            scope_explicit: true,
            classifier: None,
            packaging: "jar".to_string(),
            packaging_explicit: false,
            system_path: Some(exact_system_jar.to_string_lossy().into_owned()),
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        });

        assert!(resolver.finalize_project_dependencies(&mut project));
        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;
        assert!(
            report.complete,
            "exact system dependency should be complete"
        );
        assert_eq!(
            report.resolved_jars,
            std::slice::from_ref(&exact_system_jar)
        );
        assert_ne!(report.resolved_jars, [local_gav]);

        project.dependencies[0].system_path = None;
        assert!(!resolver.finalize_project_dependencies(&mut project));
        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;
        assert!(!report.complete);
        assert!(report.resolved_jars.is_empty());
        assert_eq!(report.missing_dependencies.len(), 1);
    }

    #[tokio::test]
    async fn missing_transitive_dependency_is_reported_and_incomplete() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let artifact_dir = repository.path().join("org/example/direct/1.0");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::fs::write(artifact_dir.join("direct-1.0.jar"), b"fixture").unwrap();
        std::fs::write(
            artifact_dir.join("direct-1.0.pom"),
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>direct</artifactId><version>1.0</version><dependencies><dependency><groupId>org.example</groupId><artifactId>missing-transitive</artifactId><version>2.0</version></dependency></dependencies></project>",
        )
        .unwrap();
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project.dependencies.push(MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: "direct".to_string(),
            version: Some("1.0".to_string()),
            scope: DependencyScope::Compile,
            scope_explicit: true,
            classifier: None,
            packaging: "jar".to_string(),
            packaging_explicit: false,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        });

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert!(!report.complete);
        assert_eq!(report.missing_dependencies.len(), 1);
        assert_eq!(
            report.missing_dependencies[0].coordinates,
            "org.example:missing-transitive:2.0"
        );
    }

    fn write_maven_artifact(repository: &Path, artifact: &str, version: &str, body: &str) {
        let dir = repository.join(format!("org/example/{artifact}/{version}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{artifact}-{version}.jar")), b"fixture").unwrap();
        std::fs::write(
            dir.join(format!("{artifact}-{version}.pom")),
            format!(
                "<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>{artifact}</artifactId><version>{version}</version>{body}</project>"
            ),
        )
        .unwrap();
    }

    fn compile_dependency(artifact: &str, version: Option<&str>) -> MavenDependency {
        MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: artifact.to_string(),
            version: version.map(str::to_string),
            scope: DependencyScope::Compile,
            scope_explicit: true,
            classifier: None,
            packaging: "jar".to_string(),
            packaging_explicit: false,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        }
    }

    #[tokio::test]
    async fn standard_maven_types_resolve_their_artifact_extensions_and_default_classifiers() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        let mut expected = Vec::new();
        for (type_, suffix) in [
            ("test-jar", "-tests.jar"),
            ("maven-plugin", ".jar"),
            ("ejb", ".jar"),
            ("ejb-client", "-client.jar"),
            ("java-source", "-sources.jar"),
            ("javadoc", "-javadoc.jar"),
            ("jar", ".jar"),
            ("war", ".war"),
            ("custom", ".custom"),
        ] {
            write_maven_artifact(repository.path(), type_, "1.0", "");
            let dir = repository.path().join(format!("org/example/{type_}/1.0"));
            // Do not leave a plain JAR that could hide selection of the wrong variant.
            std::fs::remove_file(dir.join(format!("{type_}-1.0.jar"))).unwrap();
            let artifact = dir.join(format!("{type_}-1.0{suffix}"));
            std::fs::write(&artifact, b"fixture").unwrap();
            expected.push(artifact);
            let mut dependency = compile_dependency(type_, Some("1.0"));
            dependency.packaging = type_.to_string();
            dependency.packaging_explicit = true;
            project.dependencies.push(dependency);
        }

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert!(
            report.complete,
            "{:?} {:?}",
            report.issues, report.missing_dependencies
        );
        assert_eq!(report.resolved_jars, expected);
        assert_eq!(report.main_jars, expected);
    }

    #[tokio::test]
    async fn transitive_test_jar_uses_default_classifier_and_honors_explicit_override() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        write_maven_artifact(
            repository.path(),
            "library",
            "1.0",
            "<dependencies><dependency><groupId>org.example</groupId><artifactId>support</artifactId><version>1.0</version><type>test-jar</type></dependency><dependency><groupId>org.example</groupId><artifactId>support</artifactId><version>1.0</version><type>test-jar</type><classifier>integration</classifier></dependency></dependencies>",
        );
        write_maven_artifact(repository.path(), "support", "1.0", "");
        let support = repository.path().join("org/example/support/1.0");
        std::fs::remove_file(support.join("support-1.0.jar")).unwrap();
        let tests = support.join("support-1.0-tests.jar");
        let integration = support.join("support-1.0-integration.jar");
        std::fs::write(&tests, b"fixture").unwrap();
        std::fs::write(&integration, b"fixture").unwrap();
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project
            .dependencies
            .push(compile_dependency("library", Some("1.0")));

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert!(
            report.complete,
            "{:?} {:?}",
            report.issues, report.missing_dependencies
        );
        assert_eq!(report.resolved_jars.len(), 3);
        assert!(report.resolved_jars.contains(&tests));
        assert!(report.resolved_jars.contains(&integration));
    }

    #[tokio::test]
    async fn nearest_declaration_wins_version_conflicts_like_maven() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        write_maven_artifact(repository.path(), "library", "1.0", "");
        write_maven_artifact(repository.path(), "library", "2.0", "");
        write_maven_artifact(
            repository.path(),
            "middle",
            "1.0",
            "<dependencies><dependency><groupId>org.example</groupId><artifactId>library</artifactId><version>2.0</version></dependency></dependencies>",
        );
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project.dependencies = vec![
            compile_dependency("middle", Some("1.0")),
            compile_dependency("library", Some("1.0")),
        ];

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        // The direct declaration (depth 0) beats the transitive one (depth 2).
        assert!(report.complete, "{:?}", report.issues);
        assert_eq!(report.resolved_jars.len(), 2);
        assert!(report
            .resolved_jars
            .iter()
            .any(|jar| jar.ends_with("library-1.0.jar")));
    }

    #[tokio::test]
    async fn transitive_dependencies_use_parent_properties_and_imported_bom_versions() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        write_maven_artifact(repository.path(), "leaf", "3.1", "");
        write_maven_artifact(
            repository.path(),
            "bom",
            "1.0",
            "<packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>leaf</artifactId><version>3.1</version></dependency></dependencies></dependencyManagement>",
        );
        write_maven_artifact(
            repository.path(),
            "parent",
            "1.0",
            "<packaging>pom</packaging><properties><leaf.scope>compile</leaf.scope></properties><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>bom</artifactId><version>1.0</version><type>pom</type><scope>import</scope></dependency></dependencies></dependencyManagement>",
        );
        write_maven_artifact(
            repository.path(),
            "library",
            "1.0",
            "<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version></parent><dependencies><dependency><groupId>org.example</groupId><artifactId>leaf</artifactId></dependency></dependencies>",
        );
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project.dependencies = vec![compile_dependency("library", Some("1.0"))];

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert!(
            report.complete,
            "{:?} {:?}",
            report.issues, report.missing_dependencies
        );
        assert!(report
            .resolved_jars
            .iter()
            .any(|jar| jar.ends_with("leaf-3.1.jar")));
    }

    #[tokio::test]
    async fn repairing_the_effective_model_supplies_versions_from_parent_and_imported_boms() {
        let fixture = tempfile::tempdir().unwrap();
        let repository = fixture.path().join("repository");
        write_maven_artifact(
            &repository,
            "bom",
            "1.0",
            "<packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>managed</artifactId><version>4.2</version></dependency></dependencies></dependencyManagement>",
        );
        write_maven_artifact(
            &repository,
            "parent",
            "1.0",
            "<packaging>pom</packaging><properties><other.version>9.9</other.version></properties><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>bom</artifactId><version>1.0</version><type>pom</type><scope>import</scope></dependency><dependency><groupId>org.example</groupId><artifactId>other</artifactId><version>${other.version}</version></dependency></dependencies></dependencyManagement>",
        );
        write_maven_artifact(&repository, "managed", "4.2", "");
        write_maven_artifact(&repository, "other", "9.9", "");
        let project_dir = fixture.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let pom = project_dir.join("pom.xml");
        std::fs::write(
            &pom,
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version><relativePath/></parent><artifactId>app</artifactId><dependencies><dependency><groupId>org.example</groupId><artifactId>managed</artifactId></dependency><dependency><groupId>org.example</groupId><artifactId>other</artifactId></dependency></dependencies></project>",
        )
        .unwrap();
        let resolver = MavenResolver::with_repository(repository);
        let mut project = MavenProject::new("app".to_string(), project_dir);
        super::super::parser::MavenParser::parse_pom(&pom, &mut project).unwrap();

        let repair = resolver.repair_effective_model(&mut project, &pom).unwrap();

        assert!(repair.determined, "{repair:?}");
        assert!(repair.problems.is_empty(), "{repair:?}");
        let versions: Vec<_> = project
            .dependencies
            .iter()
            .map(|dependency| dependency.version.clone().unwrap())
            .collect();
        assert_eq!(versions, ["4.2", "9.9"]);
        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;
        assert!(report.complete, "{:?}", report.issues);
        assert_eq!(report.resolved_jars.len(), 2);
    }

    #[tokio::test]
    async fn test_scoped_jars_are_not_on_the_main_classpath() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        write_maven_artifact(repository.path(), "main-lib", "1.0", "");
        write_maven_artifact(repository.path(), "test-lib", "1.0", "");
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        let mut test_dependency = compile_dependency("test-lib", Some("1.0"));
        test_dependency.scope = DependencyScope::Test;
        project.dependencies = vec![compile_dependency("main-lib", Some("1.0")), test_dependency];

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert_eq!(report.resolved_jars.len(), 2);
        assert_eq!(report.main_jars.len(), 1);
        assert!(report.main_jars[0].ends_with("main-lib-1.0.jar"));
    }

    #[tokio::test]
    async fn one_unreadable_library_pom_does_not_hide_other_dependencies() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        write_maven_artifact(repository.path(), "healthy", "1.0", "");
        let dir = repository.path().join("org/example/broken/1.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken-1.0.jar"), b"x").unwrap();
        std::fs::write(dir.join("broken-1.0.pom"), "<project><parent>").unwrap();
        let mut project = MavenProject::new("fixture".to_string(), repository.path().into());
        project.dependencies = vec![
            compile_dependency("broken", Some("1.0")),
            compile_dependency("healthy", Some("1.0")),
        ];

        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;

        assert!(!report.complete);
        assert_eq!(report.resolved_jars.len(), 2, "both JARs still resolve");
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.contains("org.example:broken:1.0")));
    }

    #[tokio::test]
    async fn missing_parent_and_bom_metadata_are_hard_completeness_failures() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());

        let mut child = MavenProject::new("child".to_string(), repository.path().into());
        child.parent = Some(ParentPom {
            group_id: "org.example".to_string(),
            artifact_id: "missing-parent".to_string(),
            version: "1.0".to_string(),
            relative_path: None,
        });
        assert!(matches!(
            resolver.resolve_parent_pom(&mut child).await,
            Err(MavenResolverError::DependencyNotFound(_))
        ));

        let mut with_bom = MavenProject::new("child".to_string(), repository.path().into());
        with_bom.bom_imports.push(MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: "missing-bom".to_string(),
            version: Some("1.0".to_string()),
            scope: DependencyScope::Import,
            scope_explicit: true,
            classifier: None,
            packaging: "pom".to_string(),
            packaging_explicit: true,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        });
        assert!(matches!(
            resolver.resolve_bom_imports(&mut with_bom).await,
            Err(MavenResolverError::DependencyNotFound(_))
        ));
    }

    #[tokio::test]
    async fn bom_identity_resolves_group_artifact_and_version_from_parent_properties() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let parent_dir = repository.path().join("org/example/parent/1.0");
        let bom_dir = repository.path().join("org/example/platform-bom/2.0");
        std::fs::create_dir_all(&parent_dir).unwrap();
        std::fs::create_dir_all(&bom_dir).unwrap();
        std::fs::write(
            parent_dir.join("parent-1.0.pom"),
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version><packaging>pom</packaging><properties><bom.group>org.example</bom.group><bom.artifact>platform-bom</bom.artifact><bom.version>2.0</bom.version></properties></project>",
        )
        .unwrap();
        std::fs::write(
            bom_dir.join("platform-bom-2.0.pom"),
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>platform-bom</artifactId><version>2.0</version><packaging>pom</packaging></project>",
        )
        .unwrap();

        let mut child = MavenProject::new("child".to_string(), repository.path().into());
        child.parent = Some(ParentPom {
            group_id: "org.example".to_string(),
            artifact_id: "parent".to_string(),
            version: "1.0".to_string(),
            relative_path: Some(String::new()),
        });
        child.bom_imports.push(MavenDependency {
            group_id: "${bom.group}".to_string(),
            artifact_id: "${bom.artifact}".to_string(),
            version: Some("${bom.version}".to_string()),
            scope: DependencyScope::Import,
            scope_explicit: true,
            classifier: None,
            packaging: "pom".to_string(),
            packaging_explicit: true,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        });

        resolver.resolve_parent_pom(&mut child).await.unwrap();
        resolver.resolve_bom_imports(&mut child).await.unwrap();
    }

    #[tokio::test]
    async fn parent_bom_cross_cycle_is_rejected_without_recursive_reset() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let parent_dir = repository.path().join("org/example/parent/1.0");
        let bom_dir = repository.path().join("org/example/bom/1.0");
        std::fs::create_dir_all(&parent_dir).unwrap();
        std::fs::create_dir_all(&bom_dir).unwrap();
        std::fs::write(
            parent_dir.join("parent-1.0.pom"),
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version><packaging>pom</packaging><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>bom</artifactId><version>1.0</version><type>pom</type><scope>import</scope></dependency></dependencies></dependencyManagement></project>",
        )
        .unwrap();
        std::fs::write(
            bom_dir.join("bom-1.0.pom"),
            r"<project><modelVersion>4.0.0</modelVersion><parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1.0</version><relativePath/></parent><artifactId>bom</artifactId><packaging>pom</packaging></project>",
        )
        .unwrap();

        let mut child = MavenProject::new("child".to_string(), repository.path().into());
        child.parent = Some(ParentPom {
            group_id: "org.example".to_string(),
            artifact_id: "parent".to_string(),
            version: "1.0".to_string(),
            relative_path: Some(String::new()),
        });

        assert!(matches!(
            resolver.resolve_parent_pom(&mut child).await,
            Err(MavenResolverError::InvalidCoordinates(message))
                if message.contains("cyclic Maven parent/BOM graph")
        ));
    }

    #[test]
    fn repository_paths_reject_traversal_in_every_coordinate_component() {
        let repository = tempfile::tempdir().unwrap();
        let resolver = MavenResolver::with_repository(repository.path().to_path_buf());
        let base = MavenDependency {
            group_id: "org.example".to_string(),
            artifact_id: "library".to_string(),
            version: Some("1.0".to_string()),
            scope: super::super::model::DependencyScope::Compile,
            scope_explicit: true,
            classifier: None,
            packaging: "jar".to_string(),
            packaging_explicit: false,
            system_path: None,
            optional: false,
            optional_explicit: false,
            exclusions: Vec::new(),
        };

        for malicious in ["..", "../outside", "folder\\outside"] {
            let mut dependency = base.clone();
            dependency.artifact_id = malicious.to_string();
            assert!(resolver.get_jar_path(&dependency).is_err());

            let mut dependency = base.clone();
            dependency.version = Some(malicious.to_string());
            assert!(resolver.get_jar_path(&dependency).is_err());

            let mut dependency = base.clone();
            dependency.classifier = Some(malicious.to_string());
            assert!(resolver.get_jar_path(&dependency).is_err());

            let mut dependency = base.clone();
            dependency.packaging = malicious.to_string();
            assert!(resolver.get_jar_path(&dependency).is_err());
        }

        let mut dependency = base;
        dependency.group_id = "org...outside".to_string();
        assert!(resolver.get_jar_path(&dependency).is_err());
    }
}
