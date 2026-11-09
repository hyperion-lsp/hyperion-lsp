// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The one place that knows where JVM build tools put things.
//!
//! Hyperion needs the same three facts in the language server, the build
//! model and the debug adapter, and used to re-derive them in seven places:
//!
//! * which module and source set owns a file ([`ModuleContext`]),
//! * which files are inputs of the build model ([`is_build_input`]),
//! * where sources and compiled classes live ([`source_dirs`],
//!   [`gradle_class_dirs`], [`maven_class_dir`]) for every JVM language in
//!   [`JVM_LANGUAGES`].
//!
//! Pure `std`, no I/O beyond `is_file`/`is_dir` probes, so every crate
//! (including `hyperion-dap`, which depends on nothing else) can use it.

use std::path::{Path, PathBuf};

/// Source languages of a JVM module (`src/<set>/<language>` directory names).
///
/// Groovy must be listed wherever Java and Kotlin are: the
/// Groovy plugin compiles to its own `build/classes/groovy/<set>`.
pub const JVM_LANGUAGES: [&str; 4] = ["java", "kotlin", "groovy", "scala"];

/// Directories never searched for sources or build files: VCS metadata, IDE
/// state and build output.
pub const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".gradle",
    ".idea",
    ".hg",
    "build",
    "target",
    "out",
    "node_modules",
    "bin",
];

/// Whether `path` is an input of the Gradle/Maven build models.
///
/// A change to one can change dependency resolution. The file watcher, the refresh trigger and
/// the build-tool classpath cache fingerprint all use this one definition.
pub fn is_build_input(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(
        file_name,
        "pom.xml" | "gradle.properties" | "gradle-wrapper.properties" | "libs.versions.toml"
    ) || file_name.ends_with(".gradle")
        || file_name.ends_with(".gradle.kts")
}

/// `<module>/src/<set>/<language>` for every language, whether or not it
/// exists.
pub fn source_dirs<'a>(
    module_dir: &'a Path,
    source_set: &'a str,
) -> impl Iterator<Item = PathBuf> + 'a {
    JVM_LANGUAGES
        .iter()
        .map(move |language| module_dir.join("src").join(source_set).join(language))
}

/// The subset of [`source_dirs`] that exists.
pub fn existing_source_dirs(module_dir: &Path, source_set: &str) -> Vec<PathBuf> {
    source_dirs(module_dir, source_set)
        .filter(|dir| dir.is_dir())
        .collect()
}

/// `<module>/build/classes/<language>/<set>`, the compile output of one
/// language's Gradle plugin.
pub fn gradle_class_dir(module_dir: &Path, language: &str, source_set: &str) -> PathBuf {
    module_dir
        .join("build/classes")
        .join(language)
        .join(source_set)
}

/// [`gradle_class_dir`] for every language, whether or not it exists.
pub fn gradle_class_dirs<'a>(
    module_dir: &'a Path,
    source_set: &'a str,
) -> impl Iterator<Item = PathBuf> + 'a {
    JVM_LANGUAGES
        .iter()
        .map(move |language| gradle_class_dir(module_dir, language, source_set))
}

/// `<module>/build/resources/<set>`.
pub fn gradle_resources_dir(module_dir: &Path, source_set: &str) -> PathBuf {
    module_dir.join("build/resources").join(source_set)
}

/// Maven writes every language of the main set to `target/classes` and of the
/// test set to `target/test-classes`.
pub fn maven_class_dir(module_dir: &Path, source_set: &str) -> PathBuf {
    module_dir.join(if source_set == "main" {
        "target/classes"
    } else {
        "target/test-classes"
    })
}

/// Source roots below `project_root` (`<module>/src` and every
/// `<module>/src/<set>/<language>`), found by a walk bounded to five levels
/// that skips [`SKIPPED_DIRS`] and dot-directories.
pub fn discover_source_roots(project_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut pending = vec![(project_root.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "src" {
                roots.push(path.clone());
                let mut sets: Vec<String> = std::fs::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|set| set.path().is_dir())
                    .map(|set| set.file_name().to_string_lossy().into_owned())
                    .collect();
                sets.sort_by_key(|set| (set != "main", set != "test", set.clone()));
                for set in sets {
                    roots.extend(existing_source_dirs(path.parent().unwrap_or(&path), &set));
                }
            } else if depth < 5 && !name.starts_with('.') && !SKIPPED_DIRS.contains(&name.as_ref())
            {
                pending.push((path, depth + 1));
            }
        }
    }
    roots
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Gradle,
    Maven,
    None,
}

impl BuildTool {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gradle => "gradle",
            Self::Maven => "maven",
            Self::None => "none",
        }
    }
}

/// The module a source file or source root belongs to.
#[derive(Debug, Clone)]
pub struct ModuleContext {
    /// Directory of the owning Gradle/Maven module (or the project directory
    /// when there is no build tool).
    pub module_dir: PathBuf,
    pub build_tool: BuildTool,
    /// Directory the build tool must be run from (Gradle settings directory,
    /// topmost Maven reactor POM).
    pub build_root: PathBuf,
    /// Gradle source set name (`main`, `test`, `integrationTest`, ...).
    pub source_set: String,
    /// Directory holding the package tree of the scanned file, when known.
    pub source_root: Option<PathBuf>,
}

impl ModuleContext {
    pub fn is_test_set(&self) -> bool {
        self.source_set != "main"
    }

    /// A module directory known up front (source set `main`).
    pub fn for_dir(dir: &Path, workspace_root: &Path) -> Self {
        let (build_tool, build_root) = detect_build_tool(dir, Some(workspace_root));
        Self {
            module_dir: dir.to_path_buf(),
            build_tool,
            build_root,
            source_set: "main".to_owned(),
            source_root: None,
        }
    }

    /// The module owning a `<module>/src/<set>/<language>` source root, or
    /// `None` when `root` does not have that shape. `workspace_root` bounds
    /// the build-tool search when known.
    pub fn from_source_root(root: &Path, workspace_root: Option<&Path>) -> Option<Self> {
        let (module_dir, source_set) = layout_from_source_root(root)?;
        let (build_tool, build_root) = detect_build_tool(&module_dir, workspace_root);
        Some(Self {
            module_dir,
            build_tool,
            build_root,
            source_set,
            source_root: Some(root.to_path_buf()),
        })
    }

    /// Find the module owning `file`. `package` is the file's declared
    /// package and is used to locate the source root.
    pub fn locate(file: &Path, package: Option<&str>, workspace_root: &Path) -> Self {
        let file_dir = file.parent().unwrap_or(workspace_root);
        let source_root = package_source_root(file_dir, package);

        // Standard layout: <module>/src/<set>/<lang>/... . Anchor on the
        // source root when the package pins it down, otherwise on the
        // nearest `src` ancestor.
        let layout = source_root
            .as_deref()
            .and_then(layout_from_source_root)
            .or_else(|| layout_from_ancestors(file));
        let (module_dir, source_set) = match layout {
            Some((module, set)) => (module, set),
            None => (
                nearest_build_dir(file_dir, workspace_root)
                    .unwrap_or_else(|| default_module_dir(file_dir, workspace_root)),
                "main".to_owned(),
            ),
        };

        let (build_tool, build_root) = detect_build_tool(&module_dir, Some(workspace_root));
        Self {
            module_dir,
            build_tool,
            build_root,
            source_set,
            source_root,
        }
    }
}

/// Strip the package path off `dir` to find the source root, provided the
/// directory actually mirrors the package.
fn package_source_root(dir: &Path, package: Option<&str>) -> Option<PathBuf> {
    let Some(package) = package.filter(|package| !package.is_empty()) else {
        return Some(dir.to_path_buf());
    };
    let mut root = dir.to_path_buf();
    for segment in package.split('.').rev() {
        if root.file_name().and_then(|name| name.to_str()) != Some(segment) {
            return None;
        }
        root = root.parent()?.to_path_buf();
    }
    Some(root)
}

/// `<module>/src/<set>/<lang>` -> (module, set).
fn layout_from_source_root(root: &Path) -> Option<(PathBuf, String)> {
    let set_dir = root.parent()?;
    let src = set_dir.parent()?;
    if src.file_name()?.to_str()? != "src" {
        return None;
    }
    Some((
        src.parent()?.to_path_buf(),
        set_dir.file_name()?.to_str()?.to_owned(),
    ))
}

fn layout_from_ancestors(file: &Path) -> Option<(PathBuf, String)> {
    for ancestor in file.ancestors().skip(1) {
        if ancestor.file_name().and_then(|name| name.to_str()) != Some("src") {
            continue;
        }
        let module = ancestor.parent()?;
        let relative = file.strip_prefix(ancestor).ok()?;
        let set = relative.components().next()?.as_os_str().to_str()?;
        // `src/Foo.java` (no set directory) is a plain layout.
        if relative.components().count() < 3 {
            return None;
        }
        return Some((module.to_path_buf(), set.to_owned()));
    }
    None
}

fn nearest_build_dir(start: &Path, workspace_root: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if has_gradle_build(ancestor)
            || gradle_settings_file(ancestor).is_some()
            || ancestor.join("pom.xml").is_file()
        {
            return Some(ancestor.to_path_buf());
        }
        if ancestor == workspace_root {
            break;
        }
    }
    None
}

fn default_module_dir(file_dir: &Path, workspace_root: &Path) -> PathBuf {
    if file_dir.starts_with(workspace_root) {
        workspace_root.to_path_buf()
    } else {
        file_dir.to_path_buf()
    }
}

pub fn has_gradle_build(dir: &Path) -> bool {
    ["build.gradle", "build.gradle.kts"]
        .iter()
        .any(|name| dir.join(name).is_file())
}

/// `settings.gradle` (Groovy wins, as in Gradle) or `settings.gradle.kts`.
pub fn gradle_settings_file(dir: &Path) -> Option<PathBuf> {
    ["settings.gradle", "settings.gradle.kts"]
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

/// Nearest ancestor of `dir` (or `dir` itself) holding a Gradle settings
/// file, looking at most `max_hops` levels up when bounded.
pub fn find_settings_root(dir: &Path, max_hops: Option<usize>) -> Option<PathBuf> {
    dir.ancestors()
        .take(max_hops.map_or(usize::MAX, |hops| hops.saturating_add(1)))
        .find(|candidate| gradle_settings_file(candidate).is_some())
        .map(Path::to_path_buf)
}

/// Which build tool owns `module_dir`, and the directory it must run from.
///
/// The module's own files decide first, then ancestors (an included Gradle
/// module may have no build file of its own), up to `stop_at` or eight levels.
pub fn detect_build_tool(module_dir: &Path, stop_at: Option<&Path>) -> (BuildTool, PathBuf) {
    let mut candidate = Some(module_dir);
    let mut hops = 0;
    while let Some(dir) = candidate {
        if has_gradle_build(dir) || gradle_settings_file(dir).is_some() {
            return (BuildTool::Gradle, gradle_root(module_dir));
        }
        if dir.join("pom.xml").is_file() {
            return (BuildTool::Maven, maven_root(dir));
        }
        hops += 1;
        if Some(dir) == stop_at || hops > 8 {
            break;
        }
        candidate = dir.parent();
    }
    (BuildTool::None, module_dir.to_path_buf())
}

/// Directory holding the Gradle settings file that includes `module_dir`,
/// else the nearest directory with a build file.
fn gradle_root(module_dir: &Path) -> PathBuf {
    if let Some(root) = find_settings_root(module_dir, Some(8)) {
        return root;
    }
    module_dir
        .ancestors()
        .take(9)
        .find(|dir| has_gradle_build(dir))
        .unwrap_or(module_dir)
        .to_path_buf()
}

/// Topmost directory of the contiguous chain of `pom.xml` files above `dir`.
fn maven_root(dir: &Path) -> PathBuf {
    let mut root = dir.to_path_buf();
    while let Some(parent) = root.parent() {
        if parent.join("pom.xml").is_file() {
            root = parent.to_path_buf();
        } else {
            break;
        }
    }
    root
}

/// Whether two paths name the same directory (after canonicalisation).
pub fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path
    }

    fn locate(root: &Path, file: &str, package: Option<&str>) -> ModuleContext {
        let file = touch(root, file);
        ModuleContext::locate(&file, package, root)
    }

    #[test]
    fn locate_flat_gradle_module() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "build.gradle");
        let ctx = locate(root, "src/main/java/a/Foo.java", Some("a"));
        assert_eq!(ctx.module_dir, root);
        assert_eq!(ctx.build_tool, BuildTool::Gradle);
        assert_eq!(ctx.build_root, root);
        assert_eq!(ctx.source_set, "main");
        assert_eq!(ctx.source_root, Some(root.join("src/main/java")));
    }

    #[test]
    fn locate_included_module_without_its_own_build_file_and_any_source_set() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "settings.gradle");
        let ctx = locate(root, "app/src/integrationTest/java/Foo.java", None);
        assert_eq!(ctx.module_dir, root.join("app"));
        assert_eq!(ctx.build_tool, BuildTool::Gradle);
        assert_eq!(ctx.build_root, root);
        assert_eq!(ctx.source_set, "integrationTest");
        assert!(ctx.is_test_set());
    }

    #[test]
    fn locate_without_a_build_file_is_a_plain_module() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let ctx = locate(root, "src/main/java/Foo.java", None);
        assert_eq!(ctx.build_tool, BuildTool::None);
        assert_eq!(ctx.module_dir, root);
        assert_eq!(ctx.build_root, root);
    }

    #[test]
    fn locate_maven_module_roots_at_the_topmost_contiguous_pom() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "pom.xml");
        touch(root, "svc/pom.xml");
        touch(root, "svc/impl/pom.xml");
        let ctx = locate(root, "svc/impl/src/test/java/FooTest.java", None);
        assert_eq!(ctx.module_dir, root.join("svc/impl"));
        assert_eq!(ctx.build_tool, BuildTool::Maven);
        assert_eq!(ctx.build_root, root);
        assert_eq!(ctx.source_set, "test");
    }

    #[test]
    fn locate_gradle_wins_over_pom_in_the_same_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "pom.xml");
        touch(root, "build.gradle.kts");
        assert_eq!(
            locate(root, "src/main/java/Foo.java", None).build_tool,
            BuildTool::Gradle
        );
    }

    #[test]
    fn package_source_root_requires_a_matching_directory() {
        assert_eq!(
            package_source_root(Path::new("/m/src/main/java/a/b"), Some("a.b")),
            Some(PathBuf::from("/m/src/main/java"))
        );
        assert_eq!(
            package_source_root(Path::new("/m/src/x"), Some("a.b")),
            None
        );
        assert_eq!(
            package_source_root(Path::new("/m/src"), None),
            Some(PathBuf::from("/m/src"))
        );
    }

    #[test]
    fn source_root_locator_accepts_any_set_and_language_and_walks_eight_levels() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "build.gradle");
        for shape in [
            "src/integrationTest/java",
            "src/main/groovy",
            "a/b/c/d/e/f/src/main/java",
        ] {
            let dir = root.join(shape);
            std::fs::create_dir_all(&dir).unwrap();
            let ctx = ModuleContext::from_source_root(&dir, None).unwrap();
            assert_eq!(ctx.build_tool, BuildTool::Gradle, "{shape}");
        }
        assert!(ModuleContext::from_source_root(&root.join("some/random/dir"), None).is_none());
        let deep = root.join("a/b/c/d/e/f/g/h/i/j/src/main/java");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            ModuleContext::from_source_root(&deep, None)
                .unwrap()
                .build_tool,
            BuildTool::None
        );
    }

    #[test]
    fn settings_root_is_bounded_only_when_asked() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "settings.gradle.kts");
        let deep = root.join("a/b/c/d/e/f/g/h/i/j");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_settings_root(&deep, None), Some(root.to_path_buf()));
        assert_eq!(find_settings_root(&deep, Some(8)), None);
        assert_eq!(find_settings_root(root, Some(0)), Some(root.to_path_buf()));
    }

    #[test]
    fn build_inputs_include_applied_scripts_and_wrapper_properties() {
        for path in [
            "pom.xml",
            "build.gradle",
            "settings.gradle.kts",
            "gradle.properties",
            "gradle/wrapper/gradle-wrapper.properties",
            "gradle/libs.versions.toml",
            "gradle/dependencies.gradle",
            "gradle/deps.gradle.kts",
        ] {
            assert!(is_build_input(Path::new(path)), "missed {path}");
        }
        assert!(!is_build_input(Path::new("README.toml")));
    }

    #[test]
    fn output_conventions_cover_every_language() {
        let module = Path::new("/m");
        let dirs: Vec<_> = gradle_class_dirs(module, "test").collect();
        assert_eq!(dirs.len(), JVM_LANGUAGES.len());
        assert!(dirs.contains(&PathBuf::from("/m/build/classes/groovy/test")));
        assert_eq!(
            maven_class_dir(module, "main"),
            Path::new("/m/target/classes")
        );
        assert_eq!(
            maven_class_dir(module, "test"),
            Path::new("/m/target/test-classes")
        );
    }

    #[test]
    fn source_roots_are_found_in_nested_modules_and_any_source_set() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for dir in [
            "app/src/main/java",
            "libs/core/src/test/groovy",
            "libs/core/src/integrationTest/java",
            "build/gen/src/main/java",
            "node_modules/x/src/main/java",
            "libs/a/b/c/d/e/f/src/main/java",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let found = discover_source_roots(root);
        assert!(found.contains(&root.join("app/src/main/java")));
        assert!(found.contains(&root.join("app/src")));
        assert!(found.contains(&root.join("libs/core/src/test/groovy")));
        assert!(found.contains(&root.join("libs/core/src/integrationTest/java")));
        assert!(!found.contains(&root.join("build/gen/src/main/java")));
        assert!(!found.contains(&root.join("node_modules/x/src/main/java")));
        assert!(!found.contains(&root.join("libs/a/b/c/d/e/f/src/main/java")));
    }
}
