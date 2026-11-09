// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Offline effective-POM model shared by the Gradle and Maven resolvers.
//!
//! Real POMs almost never stand alone: they inherit properties, dependency
//! management and dependencies from a `<parent>`, import BOMs through
//! `<scope>import</scope>`, and reference `${project.version}`. Gradle applies
//! exactly this model (and ignores `<profiles>`), so a resolver that treats
//! "has a parent" as "cannot be understood" cannot resolve any real project.
//!
//! [`PomModelCache`] loads POMs from a list of on-disk repositories (Gradle
//! files cache and `~/.m2`), builds the effective model, and reports precise,
//! per-coordinate problems instead of an all-or-nothing verdict.

use crate::io::{read_utf8_file_bounded, MAX_BUILD_FILE_BYTES};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

static PROPERTY_REF_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\$\{([^}]+)\}").unwrap());

const MAX_PARENT_DEPTH: usize = 24;
const MAX_IMPORT_DEPTH: usize = 24;

/// One `<dependency>` element exactly as written (nothing interpolated).
#[derive(Debug, Clone, Default)]
pub(crate) struct RawPomDependency {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub scope: Option<String>,
    pub type_: Option<String>,
    pub classifier: Option<String>,
    pub optional: Option<String>,
    pub exclusions: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct RawPom {
    group: Option<String>,
    artifact: String,
    version: Option<String>,
    packaging: Option<String>,
    parent: Option<(String, String, String)>,
    /// `<parent><relativePath>`: `None` means Maven's default `../pom.xml`.
    parent_relative_path: Option<String>,
    properties: Vec<(String, String)>,
    managed: Vec<RawPomDependency>,
    dependencies: Vec<RawPomDependency>,
}

/// A dependency after interpolation and dependency-management application.
#[derive(Debug, Clone)]
pub(crate) struct PomDependency {
    pub group: String,
    pub artifact: String,
    pub version: Option<String>,
    pub scope: Option<String>,
    pub type_: String,
    pub classifier: Option<String>,
    pub optional: bool,
    pub exclusions: Vec<(String, String)>,
}

/// An entry of the effective `<dependencyManagement>` section.
#[derive(Debug, Clone)]
pub(crate) struct ManagedEntry {
    pub version: Option<String>,
    pub scope: Option<String>,
    pub optional: Option<bool>,
    pub exclusions: Vec<(String, String)>,
}

/// `group:artifact:type:classifier`, with `jar` as the default type.
pub(crate) fn management_key(
    group: &str,
    artifact: &str,
    type_: Option<&str>,
    classifier: Option<&str>,
) -> String {
    let type_ = type_.filter(|value| !value.is_empty()).unwrap_or("jar");
    format!("{group}:{artifact}:{type_}:{}", classifier.unwrap_or(""))
}

/// The fully resolved view of one POM.
#[derive(Debug, Default)]
pub(crate) struct EffectivePom {
    pub packaging: String,
    pub managed: HashMap<String, ManagedEntry>,
    /// Compile/runtime-relevant dependencies with management applied. Entries
    /// whose coordinates cannot be determined are omitted and reported in
    /// `problems`.
    pub dependencies: Vec<PomDependency>,
    /// Facts that make this module's metadata less than exact, e.g. a parent
    /// POM that is not in any local repository.
    pub problems: Vec<String>,
}

/// Effective model of a POM's ancestry before interpolation.
struct Lineage {
    packaging: String,
    properties: HashMap<String, String>,
    managed: Vec<RawPomDependency>,
    dependencies: Vec<RawPomDependency>,
    problems: Vec<String>,
}

type EffectiveResult = Result<Arc<EffectivePom>, String>;

/// Loads and memoizes effective POMs. Cheap to clone; share one per resolution
/// run so a refresh always rereads changed files.
#[derive(Clone)]
pub(crate) struct PomModelCache {
    repositories: Arc<Vec<PomRepository>>,
    cache: Arc<Mutex<HashMap<String, EffectiveResult>>>,
    modules: Arc<Mutex<HashMap<String, Option<Arc<ModuleMetadata>>>>>,
}

#[derive(Debug, Clone)]
pub(crate) enum PomRepository {
    /// `~/.gradle/caches/modules-2/files-2.1`: `group/artifact/version/<hash>/name`.
    GradleFiles(PathBuf),
    /// `~/.m2/repository`: `group/as/dirs/artifact/version/name`.
    MavenLocal(PathBuf),
}

impl PomModelCache {
    pub(crate) fn new(repositories: Vec<PomRepository>) -> Self {
        Self {
            repositories: Arc::new(repositories),
            cache: Arc::new(Mutex::new(HashMap::new())),
            modules: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Forget every memoized model so edited POMs are reread.
    pub(crate) fn clear(&self) {
        self.cache.lock().unwrap().clear();
        self.modules.lock().unwrap().clear();
    }

    /// Path of `group:artifact:version`'s POM, preferring the first repository
    /// that has it.
    pub(crate) fn pom_path(&self, group: &str, artifact: &str, version: &str) -> Option<PathBuf> {
        let name = format!("{artifact}-{version}.pom");
        for repository in self.repositories.iter() {
            match repository {
                PomRepository::GradleFiles(root) => {
                    let directory = root.join(group).join(artifact).join(version);
                    let Ok(entries) = std::fs::read_dir(&directory) else {
                        continue;
                    };
                    let mut found: Vec<PathBuf> = entries
                        .flatten()
                        .map(|entry| entry.path().join(&name))
                        .filter(|path| path.is_file())
                        .collect();
                    found.sort();
                    if let Some(path) = found.into_iter().next() {
                        return Some(path);
                    }
                }
                PomRepository::MavenLocal(root) => {
                    let path = root
                        .join(group.replace('.', "/"))
                        .join(artifact)
                        .join(version)
                        .join(&name);
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Effective model for a coordinate, or the reason it cannot be built.
    pub(crate) fn effective(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
    ) -> Result<Arc<EffectivePom>, String> {
        self.effective_at_depth(group, artifact, version, 0)
    }

    fn effective_at_depth(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        import_depth: usize,
    ) -> Result<Arc<EffectivePom>, String> {
        let key = format!("{group}:{artifact}:{version}");
        if let Some(cached) = self.cache.lock().unwrap().get(&key) {
            return cached.clone();
        }
        if import_depth > MAX_IMPORT_DEPTH {
            return Err(format!("BOM import chain too deep at {key}"));
        }
        let result = self.build_effective(group, artifact, version, import_depth);
        self.cache.lock().unwrap().insert(key, result.clone());
        result
    }

    fn load_raw(&self, group: &str, artifact: &str, version: &str) -> Result<RawPom, String> {
        let path = self
            .pom_path(group, artifact, version)
            .ok_or_else(|| format!("POM {group}:{artifact}:{version} is not in a local cache"))?;
        parse_raw_pom(&path)
    }

    fn lineage(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        depth: usize,
    ) -> Result<Lineage, String> {
        let raw = self.load_raw(group, artifact, version)?;
        self.lineage_from_raw(raw, None, (group, artifact, version), depth)
    }

    /// Ancestry of an already-parsed POM. `directory` is set for a workspace
    /// POM, whose parent may be a sibling file found through `relativePath`
    /// before the repository is consulted (Maven's lookup order).
    fn lineage_from_raw(
        &self,
        raw: RawPom,
        directory: Option<&Path>,
        (group, artifact, version): (&str, &str, &str),
        depth: usize,
    ) -> Result<Lineage, String> {
        if depth > MAX_PARENT_DEPTH {
            return Err(format!(
                "parent chain too deep at {group}:{artifact}:{version}"
            ));
        }
        let mut parent_problems = Vec::new();
        let parent = raw.parent.as_ref().and_then(|(pg, pa, pv)| {
            let reactor_parent = directory.and_then(|directory| {
                let relative = raw.parent_relative_path.as_deref().unwrap_or("../pom.xml");
                if relative.is_empty() {
                    return None;
                }
                let mut candidate = directory.join(relative);
                if candidate.is_dir() {
                    candidate.push("pom.xml");
                }
                let parent_raw = parse_raw_pom(&candidate).ok()?;
                let declared_group = parent_raw
                    .group
                    .clone()
                    .or_else(|| parent_raw.parent.as_ref().map(|parent| parent.0.clone()))?;
                let declared_version = parent_raw
                    .version
                    .clone()
                    .or_else(|| parent_raw.parent.as_ref().map(|parent| parent.2.clone()))?;
                (declared_group == *pg && parent_raw.artifact == *pa && declared_version == *pv)
                    .then_some((parent_raw, candidate))
            });
            let result = match reactor_parent {
                Some((parent_raw, path)) => {
                    self.lineage_from_raw(parent_raw, path.parent(), (pg, pa, pv), depth + 1)
                }
                None => self.lineage(pg, pa, pv, depth + 1),
            };
            match result {
                Ok(parent) => Some(parent),
                Err(error) => {
                    parent_problems
                        .push(format!("parent of {group}:{artifact}:{version}: {error}"));
                    None
                }
            }
        });
        let mut lineage = parent.unwrap_or_else(|| Lineage {
            packaging: String::new(),
            properties: HashMap::new(),
            managed: Vec::new(),
            dependencies: Vec::new(),
            problems: Vec::new(),
        });
        lineage.problems.extend(parent_problems);

        let effective_group = raw
            .group
            .clone()
            .or_else(|| raw.parent.as_ref().map(|parent| parent.0.clone()))
            .unwrap_or_else(|| group.to_string());
        let effective_version = raw
            .version
            .clone()
            .or_else(|| raw.parent.as_ref().map(|parent| parent.2.clone()))
            .unwrap_or_else(|| version.to_string());

        // Own properties override inherited ones; built-ins describe *this*
        // POM (children re-derive their own).
        for (name, value) in &raw.properties {
            lineage.properties.insert(name.clone(), value.clone());
        }
        for prefix in ["project", "pom"] {
            lineage
                .properties
                .insert(format!("{prefix}.groupId"), effective_group.clone());
            lineage
                .properties
                .insert(format!("{prefix}.artifactId"), raw.artifact.clone());
            lineage
                .properties
                .insert(format!("{prefix}.version"), effective_version.clone());
        }
        lineage
            .properties
            .insert("version".into(), effective_version.clone());
        lineage
            .properties
            .insert("groupId".into(), effective_group.clone());
        lineage
            .properties
            .insert("artifactId".into(), raw.artifact.clone());
        if let Some((pg, pa, pv)) = &raw.parent {
            for prefix in ["project.parent", "parent", "pom.parent"] {
                lineage
                    .properties
                    .insert(format!("{prefix}.groupId"), pg.clone());
                lineage
                    .properties
                    .insert(format!("{prefix}.artifactId"), pa.clone());
                lineage
                    .properties
                    .insert(format!("{prefix}.version"), pv.clone());
            }
        }
        lineage.packaging = raw.packaging.clone().unwrap_or_else(|| "jar".to_string());
        lineage.managed.extend(raw.managed);
        lineage.dependencies.extend(raw.dependencies);
        Ok(lineage)
    }

    fn build_effective(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
        import_depth: usize,
    ) -> Result<Arc<EffectivePom>, String> {
        let lineage = self.lineage(group, artifact, version, 0)?;
        self.effective_from_lineage(lineage, import_depth)
    }

    /// Effective model of a POM file that is not in any repository (a
    /// workspace project): its parent is looked up beside it first, then in the
    /// repositories. Never memoized, so edits are always seen.
    pub(crate) fn effective_for_file(&self, path: &Path) -> Result<Arc<EffectivePom>, String> {
        let raw = parse_raw_pom(path)?;
        let coordinates = (
            raw.group
                .clone()
                .or_else(|| raw.parent.as_ref().map(|parent| parent.0.clone()))
                .unwrap_or_default(),
            raw.artifact.clone(),
            raw.version
                .clone()
                .or_else(|| raw.parent.as_ref().map(|parent| parent.2.clone()))
                .unwrap_or_default(),
        );
        let lineage = self.lineage_from_raw(
            raw,
            path.parent(),
            (&coordinates.0, &coordinates.1, &coordinates.2),
            0,
        )?;
        self.effective_from_lineage(lineage, 0)
    }

    fn effective_from_lineage(
        &self,
        lineage: Lineage,
        import_depth: usize,
    ) -> Result<Arc<EffectivePom>, String> {
        let mut problems = lineage.problems.clone();
        let props = &lineage.properties;

        // Dependency management: inherited + own entries, later entries win;
        // imported BOMs fill only what is not already managed.
        let mut managed: HashMap<String, ManagedEntry> = HashMap::new();
        let mut imports: Vec<(String, String, String)> = Vec::new();
        for entry in &lineage.managed {
            let group = interpolate(&entry.group, props);
            let artifact = interpolate(&entry.artifact, props);
            let type_ = entry
                .type_
                .as_deref()
                .map(|value| interpolate(value, props));
            let scope = entry
                .scope
                .as_deref()
                .map(|value| interpolate(value, props));
            let version = entry
                .version
                .as_deref()
                .map(|value| interpolate(value, props));
            if group.contains("${") || artifact.contains("${") {
                problems.push(format!(
                    "dependencyManagement entry {group}:{artifact} has an unresolved property"
                ));
                continue;
            }
            if scope.as_deref() == Some("import") && type_.as_deref() == Some("pom") {
                match version {
                    Some(version) if !version.contains("${") => {
                        imports.push((group, artifact, version));
                    }
                    _ => problems.push(format!(
                        "imported BOM {group}:{artifact} has no resolvable version"
                    )),
                }
                continue;
            }
            let classifier = entry
                .classifier
                .as_deref()
                .map(|value| interpolate(value, props));
            let version = version.filter(|value| !value.contains("${"));
            let exclusions = entry
                .exclusions
                .iter()
                .map(|(g, a)| (interpolate(g, props), interpolate(a, props)))
                .collect();
            managed.insert(
                management_key(&group, &artifact, type_.as_deref(), classifier.as_deref()),
                ManagedEntry {
                    version,
                    scope,
                    optional: entry
                        .optional
                        .as_deref()
                        .map(|value| interpolate(value, props).eq_ignore_ascii_case("true")),
                    exclusions,
                },
            );
        }
        for (import_group, import_artifact, import_version) in imports {
            match self.effective_at_depth(
                &import_group,
                &import_artifact,
                &import_version,
                import_depth + 1,
            ) {
                Ok(bom) => {
                    for (key, entry) in &bom.managed {
                        managed.entry(key.clone()).or_insert_with(|| entry.clone());
                    }
                    problems.extend(bom.problems.iter().cloned());
                }
                Err(error) => problems.push(format!(
                    "imported BOM {import_group}:{import_artifact}:{import_version}: {error}"
                )),
            }
        }

        let mut dependencies: Vec<PomDependency> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for entry in &lineage.dependencies {
            let group = interpolate(&entry.group, props);
            let artifact = interpolate(&entry.artifact, props);
            if group.is_empty()
                || artifact.is_empty()
                || group.contains("${")
                || artifact.contains("${")
            {
                problems.push(format!(
                    "dependency {group}:{artifact} has an unresolved property"
                ));
                continue;
            }
            let type_ = entry
                .type_
                .as_deref()
                .map(|value| interpolate(value, props));
            let classifier = entry
                .classifier
                .as_deref()
                .map(|value| interpolate(value, props))
                .filter(|value| !value.is_empty());
            let key = management_key(&group, &artifact, type_.as_deref(), classifier.as_deref());
            let managed_entry = managed.get(&key);
            let mut version = entry
                .version
                .as_deref()
                .map(|value| interpolate(value, props))
                .filter(|value| !value.is_empty());
            let mut scope = entry
                .scope
                .as_deref()
                .map(|value| interpolate(value, props))
                .filter(|value| !value.is_empty());
            let mut optional = entry
                .optional
                .as_deref()
                .map(|value| interpolate(value, props).eq_ignore_ascii_case("true"));
            let mut exclusions: Vec<(String, String)> = entry
                .exclusions
                .iter()
                .map(|(g, a)| (interpolate(g, props), interpolate(a, props)))
                .collect();
            if let Some(managed_entry) = managed_entry {
                if version.is_none() {
                    version.clone_from(&managed_entry.version);
                }
                if scope.is_none() {
                    scope.clone_from(&managed_entry.scope);
                }
                if optional.is_none() {
                    optional = managed_entry.optional;
                }
                for exclusion in &managed_entry.exclusions {
                    if !exclusions.contains(exclusion) {
                        exclusions.push(exclusion.clone());
                    }
                }
            }
            if version.as_deref().is_some_and(|value| value.contains("${")) {
                problems.push(format!(
                    "dependency {group}:{artifact} has an unresolved version property"
                ));
                continue;
            }
            let resolved = PomDependency {
                group,
                artifact,
                version,
                scope,
                type_: type_
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "jar".to_string()),
                classifier,
                optional: optional.unwrap_or(false),
                exclusions,
            };
            // Child declarations replace inherited ones with the same key.
            match seen.get(&key) {
                Some(index) => dependencies[*index] = resolved,
                None => {
                    seen.insert(key, dependencies.len());
                    dependencies.push(resolved);
                }
            }
        }

        Ok(Arc::new(EffectivePom {
            packaging: lineage.packaging,
            managed,
            dependencies,
            problems,
        }))
    }
}

/// One dependency edge from Gradle Module Metadata, merged across the
/// `java-api` and `java-runtime` variants Gradle would select.
#[derive(Debug, Clone)]
pub(crate) struct ModuleDependency {
    pub group: String,
    pub module: String,
    pub version: Option<String>,
    pub exclusions: Vec<(String, String)>,
    /// Visible on consumers' compile classpath (`java-api` variant).
    pub api: bool,
    /// Present on consumers' runtime classpath (`java-runtime` variant).
    pub runtime: bool,
}

/// The subset of a `.module` file that dependency resolution needs.
#[derive(Debug, Default)]
pub(crate) struct ModuleMetadata {
    pub dependencies: Vec<ModuleDependency>,
    /// `dependencyConstraints`: versions the graph must honour without adding nodes.
    pub constraints: Vec<(String, String, String)>,
    /// `available-at`: this module's artifacts live in another module.
    pub redirect: Option<(String, String, String)>,
    /// Some selected library variant publishes at least one file.
    pub publishes_files: bool,
}

impl PomModelCache {
    /// Gradle Module Metadata for a coordinate, if the Gradle cache holds a
    /// `.module` file for it. Gradle prefers it over the POM.
    pub(crate) fn module_metadata(
        &self,
        group: &str,
        artifact: &str,
        version: &str,
    ) -> Result<Option<Arc<ModuleMetadata>>, String> {
        let key = format!("{group}:{artifact}:{version}");
        if let Some(cached) = self.modules.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let name = format!("{artifact}-{version}.module");
        let path = self
            .repositories
            .iter()
            .find_map(|repository| match repository {
                PomRepository::GradleFiles(root) => {
                    let directory = root.join(group).join(artifact).join(version);
                    let mut found: Vec<PathBuf> = std::fs::read_dir(&directory)
                        .ok()?
                        .flatten()
                        .map(|entry| entry.path().join(&name))
                        .filter(|path| path.is_file())
                        .collect();
                    found.sort();
                    found.into_iter().next()
                }
                PomRepository::MavenLocal(_) => None,
            });
        let parsed = match path {
            Some(path) => Some(Arc::new(parse_module_metadata(&path)?)),
            None => None,
        };
        self.modules.lock().unwrap().insert(key, parsed.clone());
        Ok(parsed)
    }
}

fn parse_module_metadata(path: &Path) -> Result<ModuleMetadata, String> {
    let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    let mut metadata = ModuleMetadata::default();
    let attribute = |variant: &serde_json::Value, name: &str| -> Option<String> {
        variant.get("attributes")?.get(name).map(|value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_string)
        })
    };
    let Some(variants) = document.get("variants").and_then(|value| value.as_array()) else {
        return Ok(metadata);
    };
    for variant in variants {
        let usage = attribute(variant, "org.gradle.usage").unwrap_or_default();
        let category = attribute(variant, "org.gradle.category").unwrap_or_default();
        // Skip documentation, and every variant that is not the JVM/jar one
        // Gradle's Java plugins request (Android, JS, native, KMP metadata).
        if category == "documentation" {
            continue;
        }
        if attribute(variant, "org.gradle.jvm.environment")
            .is_some_and(|value| value != "standard-jvm")
        {
            continue;
        }
        if attribute(variant, "org.jetbrains.kotlin.platform.type")
            .is_some_and(|value| value != "jvm")
        {
            continue;
        }
        if attribute(variant, "org.gradle.libraryelements")
            .is_some_and(|value| value != "jar" && value != "classes")
        {
            continue;
        }
        let is_platform = category == "platform" || category == "enforced-platform";
        let (api, runtime) = match usage.as_str() {
            "java-api" | "kotlin-api" => (true, false),
            "java-runtime" | "kotlin-runtime" => (false, true),
            _ => continue,
        };
        if let Some(target) = variant.get("available-at") {
            let field = |name: &str| target.get(name).and_then(|value| value.as_str());
            if let (Some(group), Some(module), Some(version)) =
                (field("group"), field("module"), field("version"))
            {
                metadata.redirect = Some((group.into(), module.into(), version.into()));
            }
            continue;
        }
        if !is_platform
            && variant
                .get("files")
                .and_then(|value| value.as_array())
                .is_some_and(|files| !files.is_empty())
        {
            metadata.publishes_files = true;
        }
        let version_of = |dependency: &serde_json::Value| -> Option<String> {
            let version = dependency.get("version")?;
            ["strictly", "requires", "prefer"]
                .iter()
                .find_map(|kind| version.get(kind)?.as_str().map(str::to_string))
                .filter(|value| !value.is_empty())
        };
        for constraint in variant
            .get("dependencyConstraints")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            if let (Some(group), Some(module), Some(version)) = (
                constraint.get("group").and_then(|value| value.as_str()),
                constraint.get("module").and_then(|value| value.as_str()),
                version_of(constraint),
            ) {
                let entry = (group.to_string(), module.to_string(), version);
                if !metadata.constraints.contains(&entry) {
                    metadata.constraints.push(entry);
                }
            }
        }
        if is_platform {
            continue;
        }
        for dependency in variant
            .get("dependencies")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            let (Some(group), Some(module)) = (
                dependency.get("group").and_then(|value| value.as_str()),
                dependency.get("module").and_then(|value| value.as_str()),
            ) else {
                continue;
            };
            let version = version_of(dependency);
            let exclusions: Vec<(String, String)> = dependency
                .get("excludes")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .map(|exclude| {
                    let field = |name: &str| {
                        exclude
                            .get(name)
                            .and_then(|value| value.as_str())
                            .unwrap_or("*")
                            .to_string()
                    };
                    (field("group"), field("module"))
                })
                .collect();
            match metadata
                .dependencies
                .iter_mut()
                .find(|existing| existing.group == group && existing.module == module)
            {
                Some(existing) => {
                    existing.api |= api;
                    existing.runtime |= runtime;
                    if existing.version.is_none() {
                        existing.version = version;
                    }
                    for exclusion in exclusions {
                        if !existing.exclusions.contains(&exclusion) {
                            existing.exclusions.push(exclusion);
                        }
                    }
                }
                None => metadata.dependencies.push(ModuleDependency {
                    group: group.to_string(),
                    module: module.to_string(),
                    version,
                    exclusions,
                    api,
                    runtime,
                }),
            }
        }
    }
    Ok(metadata)
}

/// Replace `${name}` references using `properties`. References that cannot be
/// resolved are left in place so callers can detect them.
pub(crate) fn interpolate(value: &str, properties: &HashMap<String, String>) -> String {
    let mut current = value.to_string();
    // Properties can reference other properties; bound the passes so a
    // self-referential definition terminates.
    for _ in 0..16 {
        if !current.contains("${") {
            break;
        }
        let next = PROPERTY_REF_RE
            .replace_all(&current, |captures: &regex::Captures<'_>| {
                properties
                    .get(&captures[1])
                    .cloned()
                    .unwrap_or_else(|| captures[0].to_string())
            })
            .into_owned();
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn parse_raw_pom(path: &Path) -> Result<RawPom, String> {
    let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut pom = RawPom::default();
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut current: Option<RawPomDependency> = None;
    let mut exclusion: (String, String) = (String::new(), String::new());
    let mut parent = (String::new(), String::new(), String::new());
    let mut buffer = Vec::new();
    let mut saw_root = false;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).into_owned();
                if stack.is_empty() {
                    if name != "project" || saw_root {
                        return Err(format!(
                            "unexpected root element '{name}' in {}",
                            path.display()
                        ));
                    }
                    saw_root = true;
                }
                stack.push(name);
                text.clear();
                let joined = stack.join("/");
                if joined == "project/dependencies/dependency"
                    || joined == "project/dependencyManagement/dependencies/dependency"
                {
                    current = Some(RawPomDependency::default());
                } else if joined.ends_with("/exclusions/exclusion") && current.is_some() {
                    exclusion = (String::new(), String::new());
                }
            }
            Ok(Event::Empty(event)) => {
                // `<version/>` style empty elements carry an empty value.
                let name = String::from_utf8_lossy(event.local_name().as_ref()).into_owned();
                if stack.is_empty() {
                    if name != "project" {
                        return Err(format!(
                            "unexpected root element '{name}' in {}",
                            path.display()
                        ));
                    }
                    saw_root = true;
                }
            }
            Ok(Event::Text(event)) => {
                text.push_str(
                    &event
                        .unescape()
                        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?,
                );
            }
            Ok(Event::CData(event)) => {
                text.push_str(&String::from_utf8_lossy(event.as_ref()));
            }
            Ok(Event::End(_)) => {
                let joined = stack.join("/");
                let value = text.trim().to_string();
                match joined.as_str() {
                    "project/groupId" => pom.group = Some(value),
                    "project/artifactId" => pom.artifact = value,
                    "project/version" => pom.version = Some(value),
                    "project/packaging" => pom.packaging = Some(value),
                    "project/parent/groupId" => parent.0 = value,
                    "project/parent/artifactId" => parent.1 = value,
                    "project/parent/version" => parent.2 = value,
                    "project/parent/relativePath" => pom.parent_relative_path = Some(value),
                    "project/parent" => {
                        if !parent.0.is_empty() && !parent.1.is_empty() && !parent.2.is_empty() {
                            pom.parent = Some(std::mem::take(&mut parent));
                        }
                    }
                    "project/dependencies/dependency" => {
                        if let Some(dependency) = current.take() {
                            pom.dependencies.push(dependency);
                        }
                    }
                    "project/dependencyManagement/dependencies/dependency" => {
                        if let Some(dependency) = current.take() {
                            pom.managed.push(dependency);
                        }
                    }
                    other => {
                        if let Some(name) = other.strip_prefix("project/properties/") {
                            if !name.contains('/') {
                                pom.properties.push((name.to_string(), value));
                            }
                        } else if let Some(dependency) = current.as_mut() {
                            let field = other
                                .strip_prefix("project/dependencies/dependency/")
                                .or_else(|| {
                                    other.strip_prefix(
                                        "project/dependencyManagement/dependencies/dependency/",
                                    )
                                });
                            match field {
                                Some("groupId") => dependency.group = value,
                                Some("artifactId") => dependency.artifact = value,
                                Some("version") => dependency.version = Some(value),
                                Some("scope") => dependency.scope = Some(value),
                                Some("type") => dependency.type_ = Some(value),
                                Some("classifier") => dependency.classifier = Some(value),
                                Some("optional") => dependency.optional = Some(value),
                                Some("exclusions/exclusion/groupId") => exclusion.0 = value,
                                Some("exclusions/exclusion/artifactId") => exclusion.1 = value,
                                Some("exclusions/exclusion") => {
                                    dependency.exclusions.push(std::mem::take(&mut exclusion));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                stack.pop();
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(format!("cannot parse {}: {error}", path.display())),
            _ => {}
        }
        buffer.clear();
    }
    if !saw_root || !stack.is_empty() {
        return Err(format!("truncated POM {}", path.display()));
    }
    Ok(pom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pom(root: &Path, group: &str, artifact: &str, version: &str, body: &str) {
        let directory = root.join(group).join(artifact).join(version).join("hash");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{artifact}-{version}.pom")),
            format!("<project>{body}</project>"),
        )
        .unwrap();
    }

    fn cache(root: &Path) -> PomModelCache {
        PomModelCache::new(vec![PomRepository::GradleFiles(root.to_path_buf())])
    }

    #[test]
    fn parent_properties_management_and_dependencies_are_inherited() {
        let temp = tempfile::tempdir().unwrap();
        write_pom(
            temp.path(),
            "org.acme",
            "parent",
            "1",
            "<groupId>org.acme</groupId><artifactId>parent</artifactId><version>1</version>\
             <properties><lib.version>2.5</lib.version></properties>\
             <dependencyManagement><dependencies><dependency>\
             <groupId>org.lib</groupId><artifactId>lib</artifactId><version>${lib.version}</version>\
             </dependency></dependencies></dependencyManagement>",
        );
        write_pom(
            temp.path(),
            "org.acme",
            "child",
            "1",
            "<parent><groupId>org.acme</groupId><artifactId>parent</artifactId><version>1</version></parent>\
             <artifactId>child</artifactId>\
             <dependencies><dependency><groupId>org.lib</groupId><artifactId>lib</artifactId></dependency>\
             <dependency><groupId>${project.groupId}</groupId><artifactId>sibling</artifactId>\
             <version>${project.version}</version></dependency></dependencies>",
        );
        let effective = cache(temp.path())
            .effective("org.acme", "child", "1")
            .unwrap();
        assert!(effective.problems.is_empty(), "{:?}", effective.problems);
        let lib = &effective.dependencies[0];
        assert_eq!(lib.version.as_deref(), Some("2.5"));
        let sibling = &effective.dependencies[1];
        assert_eq!(sibling.group, "org.acme");
        assert_eq!(sibling.version.as_deref(), Some("1"));
    }

    #[test]
    fn imported_boms_fill_management_without_overriding_local_entries() {
        let temp = tempfile::tempdir().unwrap();
        write_pom(
            temp.path(),
            "org.bom",
            "bom",
            "3",
            "<groupId>org.bom</groupId><artifactId>bom</artifactId><version>3</version>\
             <dependencyManagement><dependencies>\
             <dependency><groupId>org.lib</groupId><artifactId>a</artifactId><version>9</version></dependency>\
             <dependency><groupId>org.lib</groupId><artifactId>b</artifactId><version>9</version></dependency>\
             </dependencies></dependencyManagement>",
        );
        write_pom(
            temp.path(),
            "org.acme",
            "app",
            "1",
            "<groupId>org.acme</groupId><artifactId>app</artifactId><version>1</version>\
             <dependencyManagement><dependencies>\
             <dependency><groupId>org.lib</groupId><artifactId>a</artifactId><version>1</version></dependency>\
             <dependency><groupId>org.bom</groupId><artifactId>bom</artifactId><version>3</version>\
             <type>pom</type><scope>import</scope></dependency>\
             </dependencies></dependencyManagement>",
        );
        let effective = cache(temp.path())
            .effective("org.acme", "app", "1")
            .unwrap();
        assert_eq!(
            effective.managed["org.lib:a:jar:"].version.as_deref(),
            Some("1")
        );
        assert_eq!(
            effective.managed["org.lib:b:jar:"].version.as_deref(),
            Some("9")
        );
    }

    #[test]
    fn missing_parent_is_a_precise_problem_not_a_failure() {
        let temp = tempfile::tempdir().unwrap();
        write_pom(
            temp.path(),
            "org.acme",
            "child",
            "1",
            "<parent><groupId>org.acme</groupId><artifactId>gone</artifactId><version>1</version></parent>\
             <artifactId>child</artifactId>\
             <dependencies><dependency><groupId>x</groupId><artifactId>y</artifactId><version>1</version></dependency></dependencies>",
        );
        let effective = cache(temp.path())
            .effective("org.acme", "child", "1")
            .unwrap();
        assert_eq!(effective.dependencies.len(), 1);
        assert!(effective.problems[0].contains("org.acme:gone:1"));
    }

    #[test]
    fn profiles_and_plugin_dependencies_are_ignored_like_gradle() {
        let temp = tempfile::tempdir().unwrap();
        write_pom(
            temp.path(),
            "org.acme",
            "p",
            "1",
            "<groupId>org.acme</groupId><artifactId>p</artifactId><version>1</version>\
             <profiles><profile><dependencies><dependency><groupId>x</groupId><artifactId>prof</artifactId><version>1</version></dependency></dependencies></profile></profiles>\
             <build><plugins><plugin><dependencies><dependency><groupId>x</groupId><artifactId>plug</artifactId><version>1</version></dependency></dependencies></plugin></plugins></build>",
        );
        let effective = cache(temp.path()).effective("org.acme", "p", "1").unwrap();
        assert!(effective.dependencies.is_empty());
        assert!(effective.problems.is_empty());
    }
}

/// Compare versions like Gradle: numeric parts numerically, known qualifiers in
/// their conventional order (`dev` < other text < `rc` < `snapshot` < release <
/// `sp`), a missing part equal to a release marker.
pub(crate) fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left = version_parts(left);
    let right = version_parts(right);
    for index in 0..left.len().max(right.len()) {
        let ordering = match (left.get(index), right.get(index)) {
            (Some(a), Some(b)) => compare_part(a, b),
            (Some(a), None) => compare_part(a, &VersionPart::Text("final".into())),
            (None, Some(b)) => compare_part(&VersionPart::Text("final".into()), b),
            (None, None) => std::cmp::Ordering::Equal,
        };
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

#[derive(Debug, PartialEq, Eq)]
enum VersionPart {
    Number(u128),
    Text(String),
}

fn version_parts(version: &str) -> Vec<VersionPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut numeric = false;
    let flush = |current: &mut String, numeric: bool, parts: &mut Vec<VersionPart>| {
        if current.is_empty() {
            return;
        }
        if numeric {
            parts.push(VersionPart::Number(current.parse().unwrap_or(u128::MAX)));
        } else {
            parts.push(VersionPart::Text(current.to_ascii_lowercase()));
        }
        current.clear();
    };
    for character in version.chars() {
        if matches!(character, '.' | '-' | '_' | '+') {
            flush(&mut current, numeric, &mut parts);
            continue;
        }
        let is_digit = character.is_ascii_digit();
        if !current.is_empty() && is_digit != numeric {
            flush(&mut current, numeric, &mut parts);
        }
        numeric = is_digit;
        current.push(character);
    }
    flush(&mut current, numeric, &mut parts);
    parts
}

fn text_rank(text: &str) -> u8 {
    match text {
        "dev" => 0,
        "rc" | "cr" => 2,
        "snapshot" => 3,
        "final" | "ga" | "release" => 4,
        "sp" => 5,
        _ => 1,
    }
}

fn compare_part(left: &VersionPart, right: &VersionPart) -> std::cmp::Ordering {
    match (left, right) {
        (VersionPart::Number(a), VersionPart::Number(b)) => a.cmp(b),
        (VersionPart::Number(_), VersionPart::Text(_)) => std::cmp::Ordering::Greater,
        (VersionPart::Text(_), VersionPart::Number(_)) => std::cmp::Ordering::Less,
        (VersionPart::Text(a), VersionPart::Text(b)) => {
            text_rank(a).cmp(&text_rank(b)).then_with(|| {
                if text_rank(a) == 1 {
                    a.cmp(b)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        }
    }
}

#[cfg(test)]
mod version_tests {
    use super::compare_versions;
    use std::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn versions_order_like_gradle() {
        assert_eq!(compare_versions("2.0.16", "2.0.17"), Less);
        assert_eq!(compare_versions("2.10", "2.9"), Greater);
        assert_eq!(compare_versions("33.0.0-jre", "33.0.0-jre"), Equal);
        assert_eq!(compare_versions("1.0-rc-1", "1.0"), Less);
        assert_eq!(compare_versions("1.0-alpha", "1.0-beta"), Less);
        assert_eq!(compare_versions("1.0", "1.0-final"), Equal);
        assert_eq!(compare_versions("1.0-sp-1", "1.0"), Greater);
        assert_eq!(compare_versions("1.0.1", "1.0"), Greater);
        assert_eq!(compare_versions("1.0-SNAPSHOT", "1.0"), Less);
    }
}
