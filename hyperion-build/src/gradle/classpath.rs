// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle classpath construction.
//!
//! This module builds complete classpaths for Gradle projects by aggregating:
//! - Source directories
//! - Compiled output directories
//! - Resolved dependency JARs
//! - Inter-module dependencies

use super::model::{GradleDependency, GradleProject};
use super::resolver::{GradleResolutionReport, GradleResolver, DEFAULT_TRANSITIVE_DEPTH};
use std::path::PathBuf;
use tracing::{debug, info, warn};

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::with_capacity(paths.len());
    let mut deduped = Vec::with_capacity(paths.len());
    for path in paths {
        if seen.insert(path.clone()) {
            deduped.push(path);
        }
    }
    deduped
}

/// A complete classpath for a Gradle project
#[derive(Debug, Clone)]
pub struct GradleClasspath {
    /// Project name
    pub project_name: String,

    /// Whether this classpath contains every required project/dependency
    /// entry. Fallback classpaths used for source indexing are deliberately
    /// marked incomplete and must not be published for execution.
    pub complete: bool,

    /// False when the *shape* of the build is uncertain (a module could not be
    /// parsed, a referenced project is unknown, ...), as opposed to some
    /// external JAR being unavailable in the local caches. Source roots and
    /// module ownership can only be trusted when this is true.
    pub structure_complete: bool,

    /// Canonical Gradle project path, separate from the display name.
    pub project_path: String,

    /// Project directory, used to associate this classpath with a module.
    pub project_root: PathBuf,

    /// Source directories (e.g., src/main/java)
    pub source_dirs: Vec<PathBuf>,

    /// Resource directories (e.g., src/main/resources)
    pub resource_dirs: Vec<PathBuf>,

    /// Compiled output directories (e.g., build/classes/java/main)
    pub output_dirs: Vec<PathBuf>,

    /// Compiled output directories supplied by inter-project dependencies.
    pub dependency_output_dirs: Vec<PathBuf>,

    /// Compile-only inter-project output directories.
    pub compile_dependency_output_dirs: Vec<PathBuf>,

    /// Inter-project outputs exported to downstream compile classpaths.
    pub api_dependency_output_dirs: Vec<PathBuf>,

    /// Resolved dependency JAR files
    pub dependency_jars: Vec<PathBuf>,

    /// Main compile-only and compile-visible dependency JARs.
    pub compile_dependency_jars: Vec<PathBuf>,

    /// Main compile dependencies exported to downstream projects.
    pub api_dependency_jars: Vec<PathBuf>,

    /// Main runtime-only dependency JARs.
    pub runtime_dependency_jars: Vec<PathBuf>,

    /// Test source directories
    pub test_source_dirs: Vec<PathBuf>,

    /// Test resource directories
    pub test_resource_dirs: Vec<PathBuf>,

    /// Test output directories
    pub test_output_dirs: Vec<PathBuf>,

    /// Test classpath output directories supplied by inter-project dependencies.
    pub test_dependency_output_dirs: Vec<PathBuf>,

    /// Test compile-only inter-project output directories.
    pub test_compile_dependency_output_dirs: Vec<PathBuf>,

    /// Test inter-project outputs exported to downstream compile classpaths.
    pub test_api_dependency_output_dirs: Vec<PathBuf>,

    /// Test dependency JARs
    pub test_dependency_jars: Vec<PathBuf>,

    /// Test compile-only and compile-visible dependency JARs.
    pub test_compile_dependency_jars: Vec<PathBuf>,

    /// Test compile dependencies exported to downstream test projects.
    pub test_api_dependency_jars: Vec<PathBuf>,

    /// Test runtime-only dependency JARs.
    pub test_runtime_dependency_jars: Vec<PathBuf>,

    /// Exact direct dependency resolution accounting for this classpath.
    pub resolution: GradleResolutionReport,
}

impl GradleClasspath {
    /// Create a new empty classpath
    pub fn new(project_name: String) -> Self {
        Self {
            project_path: format!(":{project_name}"),
            project_name,
            complete: true,
            structure_complete: true,
            project_root: PathBuf::new(),
            source_dirs: Vec::new(),
            resource_dirs: Vec::new(),
            output_dirs: Vec::new(),
            dependency_output_dirs: Vec::new(),
            compile_dependency_output_dirs: Vec::new(),
            api_dependency_output_dirs: Vec::new(),
            dependency_jars: Vec::new(),
            compile_dependency_jars: Vec::new(),
            api_dependency_jars: Vec::new(),
            runtime_dependency_jars: Vec::new(),
            test_source_dirs: Vec::new(),
            test_resource_dirs: Vec::new(),
            test_output_dirs: Vec::new(),
            test_dependency_output_dirs: Vec::new(),
            test_compile_dependency_output_dirs: Vec::new(),
            test_api_dependency_output_dirs: Vec::new(),
            test_dependency_jars: Vec::new(),
            test_compile_dependency_jars: Vec::new(),
            test_api_dependency_jars: Vec::new(),
            test_runtime_dependency_jars: Vec::new(),
            resolution: GradleResolutionReport::default(),
        }
    }

    /// Get all main classpath entries (output + dependencies)
    pub fn main_classpath(&self) -> Vec<PathBuf> {
        let mut classpath = Vec::new();
        classpath.extend_from_slice(&self.output_dirs);
        classpath.extend_from_slice(&self.compile_dependency_output_dirs);
        classpath.extend_from_slice(&self.dependency_output_dirs);
        classpath.extend_from_slice(&self.compile_dependency_jars);
        classpath.extend_from_slice(&self.dependency_jars);
        classpath.extend_from_slice(&self.runtime_dependency_jars);
        dedupe_paths(classpath)
    }

    /// Get all test classpath entries (including main classpath)
    pub fn test_classpath(&self) -> Vec<PathBuf> {
        let mut classpath = Vec::new();
        classpath.extend_from_slice(&self.test_output_dirs);
        classpath.extend_from_slice(&self.test_compile_dependency_output_dirs);
        classpath.extend_from_slice(&self.test_dependency_output_dirs);
        classpath.extend_from_slice(&self.test_compile_dependency_jars);
        classpath.extend_from_slice(&self.output_dirs);
        classpath.extend_from_slice(&self.compile_dependency_output_dirs);
        classpath.extend_from_slice(&self.dependency_output_dirs);
        classpath.extend_from_slice(&self.compile_dependency_jars);
        classpath.extend_from_slice(&self.dependency_jars);
        classpath.extend_from_slice(&self.runtime_dependency_jars);
        classpath.extend_from_slice(&self.test_runtime_dependency_jars);
        classpath.extend_from_slice(&self.test_dependency_jars);
        dedupe_paths(classpath)
    }

    /// Get all source directories (main + test)
    pub fn all_source_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        dirs.extend_from_slice(&self.source_dirs);
        dirs.extend_from_slice(&self.test_source_dirs);
        dirs
    }

    /// Get total number of classpath entries
    pub fn entry_count(&self) -> usize {
        self.output_dirs.len()
            + self.dependency_output_dirs.len()
            + self.compile_dependency_output_dirs.len()
            + self.api_dependency_output_dirs.len()
            + self.dependency_jars.len()
            + self.compile_dependency_jars.len()
            + self.api_dependency_jars.len()
            + self.runtime_dependency_jars.len()
            + self.test_output_dirs.len()
            + self.test_dependency_output_dirs.len()
            + self.test_compile_dependency_output_dirs.len()
            + self.test_api_dependency_output_dirs.len()
            + self.test_dependency_jars.len()
            + self.test_compile_dependency_jars.len()
            + self.test_api_dependency_jars.len()
            + self.test_runtime_dependency_jars.len()
    }
}

/// Builder for constructing Gradle classpaths
pub struct GradleClasspathBuilder {
    resolver: GradleResolver,
}

impl GradleClasspathBuilder {
    /// Create a new classpath builder
    pub fn new() -> Self {
        Self {
            resolver: GradleResolver::new(),
        }
    }

    /// Create a builder with a custom resolver
    pub fn with_resolver(resolver: GradleResolver) -> Self {
        Self { resolver }
    }

    /// Build classpath for a Gradle project
    pub async fn build(
        &self,
        project: &GradleProject,
    ) -> Result<GradleClasspath, Box<dyn std::error::Error + Send + Sync>> {
        info!("Building classpath for project '{}'", project.name);

        let mut classpath = GradleClasspath::new(project.name.clone());
        classpath.project_root = project.root_dir.clone();
        classpath.project_path = project.project_path.clone();

        // Add source directories from source sets
        for source_set in &project.source_sets {
            if source_set.name == "main" {
                classpath
                    .source_dirs
                    .extend_from_slice(&source_set.java_dirs);
                classpath
                    .resource_dirs
                    .extend_from_slice(&source_set.resource_dirs);

                if let Some(ref output_dir) = source_set.output_dir {
                    classpath.output_dirs.push(output_dir.clone());
                }
            } else if source_set.name == "test" {
                classpath
                    .test_source_dirs
                    .extend_from_slice(&source_set.java_dirs);
                classpath
                    .test_resource_dirs
                    .extend_from_slice(&source_set.resource_dirs);

                if let Some(ref output_dir) = source_set.output_dir {
                    classpath.test_output_dirs.push(output_dir.clone());
                }
            }
        }
        // Also when no source set was parsed: a buildSrc convention plugin can
        // apply the java plugin, and Gradle then compiles the conventional layout.
        append_standard_language_outputs(&mut classpath.output_dirs, &project.root_dir, "main");
        append_standard_language_outputs(
            &mut classpath.test_output_dirs,
            &project.root_dir,
            "test",
        );

        // Resolve dependencies (including transitive)
        let resolution = self
            .resolver
            .resolve_dependencies_with_report(project, DEFAULT_TRANSITIVE_DEPTH)
            .await?;
        let mut main_compile_jars = resolution.main_compile_jars.clone();
        let mut main_runtime_jars = resolution.main_runtime_jars.clone();
        let mut main_api_jars = resolution.main_api_jars.clone();
        let mut test_compile_jars = resolution.test_compile_jars.clone();
        let mut test_runtime_jars = resolution.test_runtime_jars.clone();
        let mut test_api_jars = resolution.test_api_jars.clone();
        let mut direct_jars = resolution.direct_jars.clone();
        classpath.resolution = resolution;
        classpath.complete = classpath.resolution.complete;

        // Place direct dependencies according to their declarations. Use the
        // coordinates retained by the report because a versionless dependency
        // may have been assigned a BOM/cache version during resolution.
        for (declaration_index, dep) in project.dependencies.iter().enumerate() {
            if dep.is_platform {
                continue;
            }
            if let Some(jar) = take_resolved_direct_jar(&mut direct_jars, declaration_index, dep) {
                if dep.configuration.is_test() {
                    if dep.configuration.is_compile() {
                        classpath.test_compile_dependency_jars.push(jar.clone());
                        test_compile_jars.retain(|candidate| candidate != &jar);
                    }
                    if dep.configuration.is_runtime() {
                        classpath.test_dependency_jars.push(jar.clone());
                        test_runtime_jars.retain(|candidate| candidate != &jar);
                    }
                    if dep.configuration.is_api() {
                        classpath.test_api_dependency_jars.push(jar.clone());
                        test_api_jars.retain(|candidate| candidate != &jar);
                    }
                } else {
                    if dep.configuration.is_compile() {
                        classpath.compile_dependency_jars.push(jar.clone());
                        main_compile_jars.retain(|candidate| candidate != &jar);
                    }
                    if dep.configuration.is_runtime() {
                        classpath.dependency_jars.push(jar.clone());
                        main_runtime_jars.retain(|candidate| candidate != &jar);
                    }
                    if dep.configuration.is_api() {
                        classpath.api_dependency_jars.push(jar.clone());
                        main_api_jars.retain(|candidate| candidate != &jar);
                    }
                }
            }
        }

        // Add remaining dependencies in resolver order. The legacy fields keep
        // the runtime classpath, while the explicit fields retain compile-only
        // and annotation-processor entries for source compilation/indexing.
        classpath.compile_dependency_jars.extend(main_compile_jars);
        classpath.api_dependency_jars.extend(main_api_jars);
        classpath.runtime_dependency_jars.extend(main_runtime_jars);
        classpath
            .test_compile_dependency_jars
            .extend(test_compile_jars);
        classpath.test_api_dependency_jars.extend(test_api_jars);
        classpath
            .test_runtime_dependency_jars
            .extend(test_runtime_jars);

        classpath.compile_dependency_jars = dedupe_paths(classpath.compile_dependency_jars);
        classpath.api_dependency_jars = dedupe_paths(classpath.api_dependency_jars);
        classpath.dependency_jars = dedupe_paths(classpath.dependency_jars);
        classpath
            .runtime_dependency_jars
            .retain(|jar| !classpath.dependency_jars.contains(jar));
        classpath.test_compile_dependency_jars =
            dedupe_paths(classpath.test_compile_dependency_jars);
        classpath.test_api_dependency_jars = dedupe_paths(classpath.test_api_dependency_jars);
        classpath.test_dependency_jars = dedupe_paths(classpath.test_dependency_jars);
        classpath
            .test_runtime_dependency_jars
            .retain(|jar| !classpath.test_dependency_jars.contains(jar));

        info!(
            "Built classpath with {} main entries, {} test entries",
            classpath.main_classpath().len(),
            classpath.test_classpath().len()
        );

        debug!("Main classpath: {:?}", classpath.main_classpath());
        debug!("Test classpath: {:?}", classpath.test_classpath());

        Ok(classpath)
    }

    /// Build classpaths for a multi-module project
    pub async fn build_multi_module(
        &self,
        root_project: &GradleProject,
    ) -> Result<Vec<GradleClasspath>, Box<dyn std::error::Error + Send + Sync>> {
        info!("Building multi-module classpath for {}", root_project.name);

        let mut classpaths = Vec::new();
        let mut project_definitions = std::collections::HashMap::new();
        project_definitions.insert(root_project.project_path.clone(), root_project.clone());

        // Build classpath for root project
        let mut root_classpath = self.build(root_project).await?;
        add_inter_project_dependencies(
            &mut root_classpath,
            root_project,
            &root_project.subproject_dirs,
        );
        let inherited_root_classpath = root_classpath.clone();
        classpaths.push(root_classpath);

        // Build classpaths for each subproject
        for subproject_name in &root_project.subprojects {
            debug!("Building classpath for subproject: {}", subproject_name);

            // Get the subproject directory
            let subproject_dir = match root_project.subproject_dirs.get(subproject_name) {
                Some(dir) => dir,
                None => {
                    warn!(
                        "Subproject '{}' directory not found, skipping",
                        subproject_name
                    );
                    continue;
                }
            };

            // Check if subproject has a build file
            let build_gradle = subproject_dir.join("build.gradle");
            let build_gradle_kts = subproject_dir.join("build.gradle.kts");

            if !build_gradle.exists() && !build_gradle_kts.exists() {
                debug!(
                    "Subproject '{}' has no build file; using inherited root classpath",
                    subproject_name
                );
                let mut inherited_classpath = inherited_root_classpath.clone();
                let inherited_dependencies_known =
                    inherited_root_classpath.resolution.jar_paths.is_empty()
                        && inherited_root_classpath.dependency_output_dirs.is_empty()
                        && inherited_root_classpath
                            .compile_dependency_output_dirs
                            .is_empty()
                        && inherited_root_classpath
                            .api_dependency_output_dirs
                            .is_empty()
                        && inherited_root_classpath
                            .test_dependency_output_dirs
                            .is_empty()
                        && inherited_root_classpath
                            .test_compile_dependency_output_dirs
                            .is_empty()
                        && inherited_root_classpath
                            .test_api_dependency_output_dirs
                            .is_empty();
                inherited_classpath.complete &= inherited_dependencies_known;
                inherited_classpath.structure_complete &= inherited_dependencies_known;
                inherited_classpath.project_name = subproject_name.clone();
                inherited_classpath.project_path = canonical_project_path(subproject_name);
                inherited_classpath.project_root = subproject_dir.clone();
                inherited_classpath.source_dirs.clear();
                inherited_classpath.resource_dirs.clear();
                inherited_classpath.output_dirs = standard_language_outputs(subproject_dir, "main");
                inherited_classpath.dependency_output_dirs.clear();
                inherited_classpath.compile_dependency_output_dirs.clear();
                inherited_classpath.api_dependency_output_dirs.clear();
                inherited_classpath.test_source_dirs.clear();
                inherited_classpath.test_resource_dirs.clear();
                inherited_classpath.test_output_dirs =
                    standard_language_outputs(subproject_dir, "test");
                inherited_classpath.test_dependency_output_dirs.clear();
                inherited_classpath
                    .test_compile_dependency_output_dirs
                    .clear();
                inherited_classpath.test_api_dependency_output_dirs.clear();
                inherited_classpath
                    .source_dirs
                    .extend(hyperion_layout::existing_source_dirs(
                        subproject_dir,
                        "main",
                    ));
                inherited_classpath
                    .test_source_dirs
                    .extend(hyperion_layout::existing_source_dirs(
                        subproject_dir,
                        "test",
                    ));
                let mut inherited_project =
                    GradleProject::new(subproject_name.clone(), subproject_dir.clone());
                inherited_project.project_path = canonical_project_path(subproject_name);
                project_definitions
                    .insert(inherited_project.project_path.clone(), inherited_project);
                classpaths.push(inherited_classpath);
                continue;
            }

            // Parse the subproject
            match super::parser::GradleParser::parse_project(subproject_dir).await {
                Ok(mut subproject) => {
                    subproject.project_path = canonical_project_path(subproject_name);
                    // `plugins { kotlin("jvm") version "x" apply false }` in the
                    // root declares the version subprojects apply unversioned.
                    for plugin in &mut subproject.plugins {
                        if plugin.version.is_none() {
                            plugin.version = root_project
                                .plugins
                                .iter()
                                .find(|root_plugin| root_plugin.id == plugin.id)
                                .and_then(|root_plugin| root_plugin.version.clone());
                        }
                    }
                    project_definitions.insert(subproject.project_path.clone(), subproject.clone());
                    // Build classpath for this subproject
                    match self.build(&subproject).await {
                        Ok(mut subproject_classpath) => {
                            add_inter_project_dependencies(
                                &mut subproject_classpath,
                                &subproject,
                                &root_project.subproject_dirs,
                            );

                            classpaths.push(subproject_classpath);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to build classpath for subproject '{}': {}",
                                subproject_name, e
                            );

                            // FALLBACK: Use subproject's source sets even if classpath build failed
                            let mut fallback_classpath =
                                GradleClasspath::new(subproject_name.clone());
                            fallback_classpath.complete = false;
                            fallback_classpath.structure_complete = false;
                            fallback_classpath.project_root = subproject.root_dir.clone();
                            fallback_classpath.project_path = subproject.project_path.clone();
                            for source_set in &subproject.source_sets {
                                if source_set.name == "main" {
                                    for dir in &source_set.java_dirs {
                                        if dir.exists() {
                                            fallback_classpath.source_dirs.push(dir.clone());
                                        }
                                    }
                                } else if source_set.name == "test" {
                                    for dir in &source_set.java_dirs {
                                        if dir.exists() {
                                            fallback_classpath.test_source_dirs.push(dir.clone());
                                        }
                                    }
                                }
                            }

                            if !fallback_classpath.source_dirs.is_empty()
                                || !fallback_classpath.test_source_dirs.is_empty()
                            {
                                add_inter_project_dependencies(
                                    &mut fallback_classpath,
                                    &subproject,
                                    &root_project.subproject_dirs,
                                );
                                info!(
                                    "Using fallback classpath for '{}' with {} source dirs",
                                    subproject_name,
                                    fallback_classpath.source_dirs.len()
                                );
                                classpaths.push(fallback_classpath);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse subproject '{}': {}", subproject_name, e);

                    // FALLBACK: Even if parsing fails, add default source directories
                    // This ensures we still index source files even for projects with
                    // complex/unsupported build.gradle syntax
                    let mut fallback_classpath = GradleClasspath::new(subproject_name.clone());
                    fallback_classpath.complete = false;
                    fallback_classpath.structure_complete = false;
                    fallback_classpath.project_root = subproject_dir.clone();
                    fallback_classpath.project_path = canonical_project_path(subproject_name);

                    // Add default source directories if they exist
                    let main_java = subproject_dir.join("src/main/java");
                    let main_kotlin = subproject_dir.join("src/main/kotlin");
                    let test_java = subproject_dir.join("src/test/java");
                    let test_kotlin = subproject_dir.join("src/test/kotlin");

                    if main_java.exists() {
                        info!(
                            "Adding fallback source dir for '{}': {:?}",
                            subproject_name, main_java
                        );
                        fallback_classpath.source_dirs.push(main_java);
                    }
                    if main_kotlin.exists() {
                        info!(
                            "Adding fallback Kotlin source dir for '{}': {:?}",
                            subproject_name, main_kotlin
                        );
                        fallback_classpath.source_dirs.push(main_kotlin);
                    }
                    if test_java.exists() {
                        fallback_classpath.test_source_dirs.push(test_java);
                    }
                    if test_kotlin.exists() {
                        fallback_classpath.test_source_dirs.push(test_kotlin);
                    }

                    // Only add fallback if we found some source directories
                    if !fallback_classpath.source_dirs.is_empty()
                        || !fallback_classpath.test_source_dirs.is_empty()
                    {
                        classpaths.push(fallback_classpath);
                    }
                }
            }
        }

        add_transitive_inter_project_dependencies(&mut classpaths, &project_definitions);
        add_inter_project_jars(&mut classpaths);

        info!(
            "Built classpaths for {} modules (1 root + {} subprojects)",
            classpaths.len(),
            root_project.subprojects.len()
        );

        Ok(classpaths)
    }
}

/// A consumer's classpath includes the external JARs its project dependencies
/// bring along: a module that depends on `:core` compiles against `:core`'s
/// `api` JARs and runs with `:core`'s runtime JARs, exactly like Gradle's
/// project-to-project variant selection.
fn add_inter_project_jars(classpaths: &mut [GradleClasspath]) {
    let owners: std::collections::HashMap<PathBuf, usize> = classpaths
        .iter()
        .enumerate()
        .flat_map(|(index, classpath)| {
            classpath
                .output_dirs
                .iter()
                .map(move |output| (output.clone(), index))
        })
        .collect();
    let owner_of = |outputs: &[PathBuf], consumer: usize| -> Vec<usize> {
        let mut found = Vec::new();
        for output in outputs {
            if let Some(&owner) = owners.get(output) {
                if owner != consumer && !found.contains(&owner) {
                    found.push(owner);
                }
            }
        }
        found
    };
    // Snapshot first: a consumer must only pick up each dependency's own JARs,
    // not JARs it just received from a sibling.
    let snapshot: Vec<_> = classpaths
        .iter()
        .map(|classpath| {
            (
                classpath.api_dependency_jars.clone(),
                classpath
                    .dependency_jars
                    .iter()
                    .chain(&classpath.runtime_dependency_jars)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    #[allow(clippy::needless_range_loop)] // `owners`/`snapshot` are indexed in parallel
    for consumer in 0..classpaths.len() {
        let compile_owners = owner_of(
            &classpaths[consumer]
                .compile_dependency_output_dirs
                .iter()
                .chain(&classpaths[consumer].api_dependency_output_dirs)
                .cloned()
                .collect::<Vec<_>>(),
            consumer,
        );
        let runtime_owners = owner_of(&classpaths[consumer].dependency_output_dirs, consumer);
        let test_compile_owners = owner_of(
            &classpaths[consumer]
                .test_compile_dependency_output_dirs
                .iter()
                .chain(&classpaths[consumer].test_api_dependency_output_dirs)
                .cloned()
                .collect::<Vec<_>>(),
            consumer,
        );
        let test_runtime_owners =
            owner_of(&classpaths[consumer].test_dependency_output_dirs, consumer);
        let target = &mut classpaths[consumer];
        for owner in compile_owners {
            target
                .compile_dependency_jars
                .extend(snapshot[owner].0.iter().cloned());
        }
        for owner in runtime_owners {
            target
                .runtime_dependency_jars
                .extend(snapshot[owner].1.iter().cloned());
        }
        for owner in test_compile_owners {
            target
                .test_compile_dependency_jars
                .extend(snapshot[owner].0.iter().cloned());
        }
        for owner in test_runtime_owners {
            target
                .test_runtime_dependency_jars
                .extend(snapshot[owner].1.iter().cloned());
        }
        target.compile_dependency_jars =
            dedupe_paths(std::mem::take(&mut target.compile_dependency_jars));
        target.runtime_dependency_jars =
            dedupe_paths(std::mem::take(&mut target.runtime_dependency_jars));
        let direct_runtime = target.dependency_jars.clone();
        target
            .runtime_dependency_jars
            .retain(|jar| !direct_runtime.contains(jar));
        target.test_compile_dependency_jars =
            dedupe_paths(std::mem::take(&mut target.test_compile_dependency_jars));
        target.test_runtime_dependency_jars =
            dedupe_paths(std::mem::take(&mut target.test_runtime_dependency_jars));
        let direct_test_runtime = target.test_dependency_jars.clone();
        target
            .test_runtime_dependency_jars
            .retain(|jar| !direct_test_runtime.contains(jar));
    }
}

fn append_standard_language_outputs(
    output_dirs: &mut Vec<PathBuf>,
    project_root: &std::path::Path,
    source_set: &str,
) {
    for language in hyperion_layout::JVM_LANGUAGES {
        let source_dir = project_root.join("src").join(source_set).join(language);
        let output_dir = hyperion_layout::gradle_class_dir(project_root, language, source_set);
        if (source_dir.is_dir() || output_dir.is_dir()) && !output_dirs.contains(&output_dir) {
            output_dirs.push(output_dir);
        }
    }
}

fn standard_language_outputs(project_root: &std::path::Path, source_set: &str) -> Vec<PathBuf> {
    let mut output_dirs = Vec::new();
    append_standard_language_outputs(&mut output_dirs, project_root, source_set);
    output_dirs
}

fn add_inter_project_dependencies(
    classpath: &mut GradleClasspath,
    project: &GradleProject,
    project_dirs: &std::collections::HashMap<String, PathBuf>,
) {
    for (config, referenced_project) in ordered_project_dependencies(project) {
        debug!(
            "Project '{}' depends on project '{}' (config: {:?})",
            project.name, referenced_project, config
        );
        let Some(referenced_root) = lookup_project_dir(project_dirs, referenced_project) else {
            debug!(
                "Referenced project '{}' not found while building '{}' classpath",
                referenced_project, project.name
            );
            classpath.complete = false;
            classpath.structure_complete = false;
            continue;
        };
        let referenced_outputs = standard_language_outputs(referenced_root, "main");
        let (runtime_outputs, compile_outputs, api_outputs) = if config.is_test() {
            (
                &mut classpath.test_dependency_output_dirs,
                &mut classpath.test_compile_dependency_output_dirs,
                &mut classpath.test_api_dependency_output_dirs,
            )
        } else {
            (
                &mut classpath.dependency_output_dirs,
                &mut classpath.compile_dependency_output_dirs,
                &mut classpath.api_dependency_output_dirs,
            )
        };
        for output_dir in referenced_outputs {
            if config.is_runtime() && !runtime_outputs.contains(&output_dir) {
                debug!(
                    "Adding inter-project runtime output for '{}': {:?}",
                    project.name, output_dir
                );
                runtime_outputs.push(output_dir.clone());
            }
            if config.is_compile() && !compile_outputs.contains(&output_dir) {
                debug!(
                    "Adding inter-project compile output for '{}': {:?}",
                    project.name, output_dir
                );
                compile_outputs.push(output_dir.clone());
            }
            if config.is_api() && !api_outputs.contains(&output_dir) {
                api_outputs.push(output_dir);
            }
        }
    }
}

fn canonical_project_path(project_name: &str) -> String {
    let trimmed = project_name.trim();
    if trimmed.is_empty() || trimmed == ":" {
        ":".to_string()
    } else {
        format!(":{}", trimmed.trim_start_matches(':'))
    }
}

fn lookup_project_dir<'a>(
    project_dirs: &'a std::collections::HashMap<String, PathBuf>,
    project_name: &str,
) -> Option<&'a PathBuf> {
    project_dirs
        .get(project_name)
        .or_else(|| project_dirs.get(project_name.trim_start_matches(':')))
        .or_else(|| project_dirs.get(&canonical_project_path(project_name)))
}

fn ordered_project_dependencies(
    project: &GradleProject,
) -> Vec<(&super::model::DependencyConfiguration, &str)> {
    let mut dependencies = project
        .project_dependencies
        .iter()
        .flat_map(|(configuration, projects)| {
            projects
                .iter()
                .map(move |project| (configuration, project.as_str()))
        })
        .collect::<Vec<_>>();
    dependencies.sort_by(
        |(left_config, left_project), (right_config, right_project)| {
            format!("{left_config:?}")
                .cmp(&format!("{right_config:?}"))
                .then_with(|| left_project.cmp(right_project))
        },
    );
    dependencies
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DependencyVisibility {
    compile: bool,
    runtime: bool,
    api: bool,
}

impl DependencyVisibility {
    fn from_configuration(configuration: &super::model::DependencyConfiguration) -> Self {
        Self {
            compile: configuration.is_compile(),
            runtime: configuration.is_runtime(),
            api: configuration.is_api(),
        }
    }
}

struct InterProjectTraversal<'a> {
    consumer_index: usize,
    target_is_test: bool,
    visited: std::collections::HashSet<(String, DependencyVisibility)>,
    project_definitions: &'a std::collections::HashMap<String, GradleProject>,
    classpath_indices: &'a std::collections::HashMap<String, usize>,
}

fn add_transitive_inter_project_dependencies(
    classpaths: &mut [GradleClasspath],
    project_definitions: &std::collections::HashMap<String, GradleProject>,
) {
    let classpath_indices: std::collections::HashMap<String, usize> = classpaths
        .iter()
        .enumerate()
        .map(|(index, classpath)| (classpath.project_path.clone(), index))
        .collect();

    let mut definitions = project_definitions.iter().collect::<Vec<_>>();
    definitions.sort_unstable_by_key(|(name, _)| *name);
    for (project_name, project) in definitions {
        let project_path = if project_name.starts_with(':') {
            project_name.clone()
        } else {
            project.project_path.clone()
        };
        let Some(&consumer_index) = classpath_indices.get(&project_path) else {
            continue;
        };
        for (configuration, dependency) in ordered_project_dependencies(project) {
            let visibility = DependencyVisibility::from_configuration(configuration);
            let mut traversal = InterProjectTraversal {
                consumer_index,
                target_is_test: configuration.is_test(),
                visited: std::collections::HashSet::from([(project_path.clone(), visibility)]),
                project_definitions,
                classpath_indices: &classpath_indices,
            };
            add_project_dependency(dependency, visibility, classpaths, &mut traversal);
        }
    }
}

fn add_project_dependency(
    dependency_name: &str,
    visibility: DependencyVisibility,
    classpaths: &mut [GradleClasspath],
    traversal: &mut InterProjectTraversal<'_>,
) {
    let dependency_path = canonical_project_path(dependency_name);
    if !traversal
        .visited
        .insert((dependency_path.clone(), visibility))
    {
        return;
    }
    let Some(&dependency_index) = traversal.classpath_indices.get(&dependency_path) else {
        classpaths[traversal.consumer_index].complete = false;
        classpaths[traversal.consumer_index].structure_complete = false;
        return;
    };
    let dependency_classpath = classpaths[dependency_index].clone();
    let (runtime_outputs, compile_outputs, api_outputs) = if traversal.target_is_test {
        (
            &mut classpaths[traversal.consumer_index].test_dependency_output_dirs,
            &mut classpaths[traversal.consumer_index].test_compile_dependency_output_dirs,
            &mut classpaths[traversal.consumer_index].test_api_dependency_output_dirs,
        )
    } else {
        (
            &mut classpaths[traversal.consumer_index].dependency_output_dirs,
            &mut classpaths[traversal.consumer_index].compile_dependency_output_dirs,
            &mut classpaths[traversal.consumer_index].api_dependency_output_dirs,
        )
    };
    if visibility.runtime {
        for output_dir in dependency_classpath
            .output_dirs
            .iter()
            .chain(dependency_classpath.dependency_output_dirs.iter())
        {
            if !runtime_outputs.contains(output_dir) {
                runtime_outputs.push(output_dir.clone());
            }
        }
    }
    if visibility.compile {
        for output_dir in dependency_classpath
            .output_dirs
            .iter()
            .chain(dependency_classpath.api_dependency_output_dirs.iter())
        {
            if !compile_outputs.contains(output_dir) {
                compile_outputs.push(output_dir.clone());
            }
        }
    }
    if visibility.api {
        for output_dir in dependency_classpath
            .api_dependency_output_dirs
            .iter()
            .chain(dependency_classpath.output_dirs.iter())
        {
            if !api_outputs.contains(output_dir) {
                api_outputs.push(output_dir.clone());
            }
        }
    }
    let (runtime_jars, compile_jars, api_jars) = if traversal.target_is_test {
        (
            &mut classpaths[traversal.consumer_index].test_dependency_jars,
            &mut classpaths[traversal.consumer_index].test_compile_dependency_jars,
            &mut classpaths[traversal.consumer_index].test_api_dependency_jars,
        )
    } else {
        (
            &mut classpaths[traversal.consumer_index].dependency_jars,
            &mut classpaths[traversal.consumer_index].compile_dependency_jars,
            &mut classpaths[traversal.consumer_index].api_dependency_jars,
        )
    };
    if visibility.runtime {
        for jar in dependency_classpath
            .dependency_jars
            .iter()
            .cloned()
            .chain(dependency_classpath.runtime_dependency_jars.iter().cloned())
        {
            if !runtime_jars.contains(&jar) {
                runtime_jars.push(jar);
            }
        }
    }
    if visibility.compile {
        for jar in dependency_classpath.api_dependency_jars.iter().cloned() {
            if !compile_jars.contains(&jar) {
                compile_jars.push(jar);
            }
        }
    }
    if visibility.api {
        for jar in &dependency_classpath.api_dependency_jars {
            if !api_jars.contains(jar) {
                api_jars.push(jar.clone());
            }
        }
    }

    let Some(dependency_project) = traversal
        .project_definitions
        .get(&dependency_path)
        .or_else(|| traversal.project_definitions.get(dependency_name))
    else {
        classpaths[traversal.consumer_index].complete = false;
        classpaths[traversal.consumer_index].structure_complete = false;
        return;
    };
    for (configuration, transitive_dependency) in ordered_project_dependencies(dependency_project) {
        if configuration.is_test() {
            continue;
        }
        // Only API project edges expose the producer's transitive compile
        // dependencies to this consumer. Implementation/compileOnly/
        // annotationProcessor still make the producer's own output available
        // above, but their dependencies are intentionally not exported.
        let child_visibility = DependencyVisibility {
            compile: visibility.compile && configuration.is_api(),
            runtime: visibility.runtime && configuration.is_runtime(),
            api: visibility.api && configuration.is_api(),
        };
        if !child_visibility.compile && !child_visibility.runtime {
            continue;
        }
        add_project_dependency(
            transitive_dependency,
            child_visibility,
            classpaths,
            traversal,
        );
    }
}

fn take_resolved_direct_jar(
    direct_jars: &mut Vec<(usize, GradleDependency, PathBuf)>,
    declaration_index: usize,
    dependency: &GradleDependency,
) -> Option<PathBuf> {
    let index = direct_jars.iter().position(|(index, resolved, _)| {
        *index == declaration_index
            && resolved.group == dependency.group
            && resolved.artifact == dependency.artifact
            && resolved.classifier == dependency.classifier
            && resolved.packaging == dependency.packaging
            && (dependency.version.is_none() || resolved.version == dependency.version)
    })?;
    Some(direct_jars.remove(index).2)
}

impl Default for GradleClasspathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::model::{DependencyConfiguration, SourceSet};

    #[test]
    fn test_classpath_creation() {
        let classpath = GradleClasspath::new("test-project".to_string());
        assert_eq!(classpath.project_name, "test-project");
        assert_eq!(classpath.entry_count(), 0);
    }

    #[test]
    fn test_main_classpath() {
        let mut classpath = GradleClasspath::new("test".to_string());
        classpath
            .output_dirs
            .push(PathBuf::from("/project/build/classes"));
        classpath
            .dependency_jars
            .push(PathBuf::from("/home/.m2/guava.jar"));

        let main_cp = classpath.main_classpath();
        assert_eq!(main_cp.len(), 2);
    }

    #[test]
    fn standard_outputs_include_groovy() {
        // The Groovy plugin's output directory is recognised for
        // inter-project dependencies like the Java one.
        let temp = tempfile::tempdir().unwrap();
        for language in ["java", "groovy"] {
            std::fs::create_dir_all(temp.path().join("src/main").join(language)).unwrap();
        }
        assert_eq!(
            standard_language_outputs(temp.path(), "main"),
            vec![
                temp.path().join("build/classes/java/main"),
                temp.path().join("build/classes/groovy/main"),
            ]
        );
    }

    #[test]
    fn inter_project_dependencies_keep_all_language_outputs_separate_from_jars() {
        let temp = tempfile::tempdir().unwrap();
        let core = temp.path().join("core");
        for language in ["java", "kotlin", "scala"] {
            std::fs::create_dir_all(core.join("src/main").join(language)).unwrap();
        }
        let mut app = GradleProject::new("app".to_string(), temp.path().join("app"));
        app.project_dependencies.insert(
            DependencyConfiguration::Implementation,
            vec!["core".to_string()],
        );
        let mut project_dirs = std::collections::HashMap::new();
        project_dirs.insert("core".to_string(), core.clone());
        let mut classpath = GradleClasspath::new("app".to_string());

        add_inter_project_dependencies(&mut classpath, &app, &project_dirs);

        assert_eq!(
            classpath.dependency_output_dirs,
            vec![
                core.join("build/classes/java/main"),
                core.join("build/classes/kotlin/main"),
                core.join("build/classes/scala/main"),
            ]
        );
        assert!(classpath.dependency_jars.is_empty());
    }

    #[test]
    fn missing_inter_project_dependency_marks_classpath_incomplete() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = GradleProject::new("app".to_string(), temp.path().join("app"));
        app.project_dependencies.insert(
            DependencyConfiguration::Implementation,
            vec!["missing".to_string()],
        );
        let mut classpath = GradleClasspath::new("app".to_string());

        add_inter_project_dependencies(&mut classpath, &app, &std::collections::HashMap::new());

        assert!(!classpath.complete);
    }

    #[test]
    fn inter_project_dependencies_include_transitive_outputs_and_jars() {
        let temp = tempfile::tempdir().unwrap();
        let app_root = temp.path().join("app");
        let service_root = temp.path().join("service");
        let core_root = temp.path().join("core");
        let mut app = GradleProject::new("app".to_string(), app_root.clone());
        app.project_dependencies.insert(
            DependencyConfiguration::Implementation,
            vec!["service".to_string()],
        );
        let mut service = GradleProject::new("service".to_string(), service_root.clone());
        service.project_dependencies.insert(
            DependencyConfiguration::Implementation,
            vec!["core".to_string()],
        );
        let core = GradleProject::new("core".to_string(), core_root.clone());

        let mut app_classpath = GradleClasspath::new("app".to_string());
        app_classpath.project_root = app_root;
        let mut service_classpath = GradleClasspath::new("service".to_string());
        service_classpath.project_root = service_root;
        service_classpath
            .output_dirs
            .push(PathBuf::from("/build/service/classes"));
        service_classpath
            .dependency_jars
            .push(PathBuf::from("/libs/service.jar"));
        let mut core_classpath = GradleClasspath::new("core".to_string());
        core_classpath.project_root = core_root;
        core_classpath
            .output_dirs
            .push(PathBuf::from("/build/core/classes"));
        core_classpath
            .dependency_jars
            .push(PathBuf::from("/libs/core.jar"));

        let mut classpaths = vec![app_classpath, service_classpath, core_classpath];
        let projects = [("app", app), ("service", service), ("core", core)]
            .into_iter()
            .map(|(name, project)| (name.to_string(), project))
            .collect();
        add_transitive_inter_project_dependencies(&mut classpaths, &projects);

        assert!(classpaths[0]
            .dependency_output_dirs
            .contains(&PathBuf::from("/build/core/classes")));
        assert!(!classpaths[0]
            .compile_dependency_output_dirs
            .contains(&PathBuf::from("/build/core/classes")));
        assert_eq!(
            classpaths[0].dependency_jars,
            vec![
                PathBuf::from("/libs/service.jar"),
                PathBuf::from("/libs/core.jar")
            ]
        );
    }

    #[test]
    fn api_project_dependencies_export_transitive_compile_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let app_root = temp.path().join("app");
        let service_root = temp.path().join("service");
        let core_root = temp.path().join("core");
        let mut app = GradleProject::new("app".to_string(), app_root.clone());
        app.project_dependencies.insert(
            DependencyConfiguration::Implementation,
            vec!["service".to_string()],
        );
        let mut service = GradleProject::new("service".to_string(), service_root.clone());
        service
            .project_dependencies
            .insert(DependencyConfiguration::Api, vec!["core".to_string()]);
        let core = GradleProject::new("core".to_string(), core_root.clone());

        let mut app_classpath = GradleClasspath::new("app".to_string());
        app_classpath.project_root = app_root;
        let mut service_classpath = GradleClasspath::new("service".to_string());
        service_classpath.project_root = service_root;
        service_classpath
            .output_dirs
            .push(PathBuf::from("/build/service/classes"));
        let mut core_classpath = GradleClasspath::new("core".to_string());
        core_classpath.project_root = core_root;
        core_classpath
            .output_dirs
            .push(PathBuf::from("/build/core/classes"));

        let mut classpaths = vec![app_classpath, service_classpath, core_classpath];
        let projects = [("app", app), ("service", service), ("core", core)]
            .into_iter()
            .map(|(name, project)| (name.to_string(), project))
            .collect();
        add_transitive_inter_project_dependencies(&mut classpaths, &projects);

        assert_eq!(
            classpaths[0].compile_dependency_output_dirs,
            vec![
                PathBuf::from("/build/service/classes"),
                PathBuf::from("/build/core/classes")
            ]
        );
    }

    #[test]
    fn consumers_receive_the_jars_their_project_dependencies_export() {
        let core_main = PathBuf::from("/w/core/build/classes/java/main");
        let mut core = GradleClasspath::new("core".to_string());
        core.output_dirs = vec![core_main.clone()];
        core.api_dependency_jars = vec![PathBuf::from("/c/guava.jar")];
        core.compile_dependency_jars = vec![
            PathBuf::from("/c/guava.jar"),
            PathBuf::from("/c/internal-only.jar"),
        ];
        core.dependency_jars = vec![
            PathBuf::from("/c/guava.jar"),
            PathBuf::from("/c/internal-only.jar"),
        ];
        let mut app = GradleClasspath::new("app".to_string());
        app.output_dirs = vec![PathBuf::from("/w/app/build/classes/java/main")];
        app.compile_dependency_output_dirs = vec![core_main.clone()];
        app.dependency_output_dirs = vec![core_main];
        app.dependency_jars = vec![PathBuf::from("/c/jackson.jar")];
        let mut classpaths = vec![core, app];

        add_inter_project_jars(&mut classpaths);

        let app = &classpaths[1];
        // Compile sees only what core exports through `api`.
        assert_eq!(
            app.compile_dependency_jars,
            vec![PathBuf::from("/c/guava.jar")]
        );
        // Runtime carries all of core's runtime jars.
        assert_eq!(
            app.runtime_dependency_jars,
            vec![
                PathBuf::from("/c/guava.jar"),
                PathBuf::from("/c/internal-only.jar")
            ]
        );
        assert_eq!(app.dependency_jars, vec![PathBuf::from("/c/jackson.jar")]);
    }

    #[test]
    fn test_test_classpath() {
        let mut classpath = GradleClasspath::new("test".to_string());
        classpath
            .output_dirs
            .push(PathBuf::from("/project/build/classes/main"));
        classpath
            .dependency_jars
            .push(PathBuf::from("/home/.m2/guava.jar"));
        classpath
            .test_output_dirs
            .push(PathBuf::from("/project/build/classes/test"));
        classpath
            .test_dependency_jars
            .push(PathBuf::from("/home/.m2/junit.jar"));

        let test_cp = classpath.test_classpath();
        assert_eq!(test_cp.len(), 4); // 2 main + 2 test
    }

    #[test]
    fn direct_jar_matching_respects_declaration_index() {
        let resolved = GradleDependency::from_coordinates(
            "com.example:shared:1.0.0",
            DependencyConfiguration::Implementation,
        )
        .unwrap();
        let versionless = GradleDependency::from_coordinates(
            "com.example:shared",
            DependencyConfiguration::TestImplementation,
        )
        .unwrap();
        let jar = PathBuf::from("/cache/shared-1.0.0.jar");
        let mut direct = vec![(1, resolved, jar.clone())];

        assert!(take_resolved_direct_jar(&mut direct, 0, &versionless).is_none());
        assert_eq!(
            take_resolved_direct_jar(&mut direct, 1, &versionless),
            Some(jar)
        );
    }

    #[tokio::test]
    async fn test_build_classpath() {
        let mut project = GradleProject::new("test".to_string(), PathBuf::from("/test"));

        // Add source sets
        project.source_sets.push(SourceSet::main(&project.root_dir));
        project.source_sets.push(SourceSet::test(&project.root_dir));

        let builder = GradleClasspathBuilder::new();
        let classpath = builder.build(&project).await.unwrap();

        assert_eq!(classpath.project_name, "test");
        assert!(!classpath.source_dirs.is_empty());
        assert!(!classpath.test_source_dirs.is_empty());
    }

    #[tokio::test]
    async fn conventional_outputs_without_parsed_source_sets() {
        // A project that gets the java plugin from a buildSrc convention
        // plugin has no parsed source sets. Its consumers still list its
        // conventional output, and source scopes map that output back to the
        // project's sources.
        let temp = tempfile::tempdir().unwrap();
        for source_set in ["main", "test"] {
            std::fs::create_dir_all(temp.path().join("src").join(source_set).join("java")).unwrap();
        }
        let project = GradleProject::new("utils".to_string(), temp.path().to_path_buf());

        let classpath = GradleClasspathBuilder::new().build(&project).await.unwrap();

        assert_eq!(
            classpath.output_dirs,
            vec![temp.path().join("build/classes/java/main")]
        );
        assert_eq!(
            classpath.test_output_dirs,
            vec![temp.path().join("build/classes/java/test")]
        );
    }
}
