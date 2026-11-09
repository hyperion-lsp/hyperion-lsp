// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded, deterministic loading of JVM classes from directories and JARs.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Maximum uncompressed size accepted for one class file.
pub const MAX_CLASS_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Maximum uncompressed class data retained for one classpath.
pub const MAX_CLASSPATH_CLASS_BYTES: usize = 512 * 1024 * 1024;

/// Maximum number of roots accepted by one classpath load.
///
/// Byte limits alone do not bound filesystem work: an attacker can provide an
/// arbitrary number of empty directories or JARs. Callers that discover roots
/// dynamically should enforce the same bound before retaining them.
pub const MAX_CLASSPATH_ENTRIES: usize = 256;

/// Default bound on filesystem and ZIP entries inspected across one load.
pub const MAX_CLASSPATH_DISCOVERY_ENTRIES: usize = 1_000_000;

/// Default recursion bound for directory classpath roots.
pub const MAX_CLASSPATH_DIRECTORY_DEPTH: usize = 128;

/// Resource limits for one classpath observation.
///
/// The public wrapper uses these conservative compatibility limits. Callers
/// loading a narrower domain, such as one Gradle plugin, can select a smaller
/// retained-byte and discovery budget without changing classpath semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClasspathLoadLimits {
    pub maximum_retained_class_bytes: usize,
    pub maximum_discovered_entries: usize,
    pub maximum_directory_depth: usize,
}

impl Default for ClasspathLoadLimits {
    fn default() -> Self {
        Self {
            maximum_retained_class_bytes: MAX_CLASSPATH_CLASS_BYTES,
            maximum_discovered_entries: MAX_CLASSPATH_DISCOVERY_ENTRIES,
            maximum_directory_depth: MAX_CLASSPATH_DIRECTORY_DEPTH,
        }
    }
}

/// A failure while loading classes from a classpath.
#[derive(Debug, Error)]
pub enum ClasspathLoadError {
    #[error("failed to {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read {description}: {source}")]
    ReadClass {
        description: String,
        #[source]
        source: io::Error,
    },

    #[error("invalid JAR file {path}: {source}")]
    InvalidJar {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("failed to read entry {index} of {path}: {source}")]
    JarEntry {
        path: PathBuf,
        index: usize,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("{description} is too large: {size} bytes (maximum {maximum})")]
    ClassTooLarge {
        description: String,
        size: u64,
        maximum: usize,
    },

    #[error("classpath class data exceeds {maximum} bytes")]
    ClasspathTooLarge { maximum: usize },

    #[error("classpath has {count} entries (maximum {maximum})")]
    TooManyClasspathEntries { count: usize, maximum: usize },

    #[error("classpath discovery exceeded {maximum} filesystem/ZIP entries")]
    TooManyDiscoveredEntries { maximum: usize },

    #[error("classpath directory nesting at {path} exceeds maximum depth {maximum}")]
    DirectoryTooDeep { path: PathBuf, maximum: usize },
}

/// Load every class reachable from directory and JAR classpath entries.
///
/// Canonical class names use JVM `/` separators and omit the `.class`
/// suffix. Earlier entries take precedence on collisions, and the returned
/// classes are sorted by name for deterministic VM loading.
pub fn load_classpath_classes(
    classpath: &[PathBuf],
) -> Result<Vec<(String, Vec<u8>)>, ClasspathLoadError> {
    load_classpath_classes_with_limits(classpath, ClasspathLoadLimits::default())
}

/// Load a classpath with caller-selected aggregate resource limits.
pub fn load_classpath_classes_with_limits(
    classpath: &[PathBuf],
    limits: ClasspathLoadLimits,
) -> Result<Vec<(String, Vec<u8>)>, ClasspathLoadError> {
    if classpath.len() > MAX_CLASSPATH_ENTRIES {
        return Err(ClasspathLoadError::TooManyClasspathEntries {
            count: classpath.len(),
            maximum: MAX_CLASSPATH_ENTRIES,
        });
    }
    let mut collector = ClassCollector::new(limits.maximum_retained_class_bytes);
    let mut discovery = DiscoveryBudget::new(limits.maximum_discovered_entries);
    for entry in classpath {
        if entry.is_dir() {
            // Directory identity is scoped to one classpath root. The same
            // physical directory can produce different canonical names when
            // it is later loaded relative to a parent root.
            let mut visited_dirs = HashSet::new();
            collect_directory(
                entry,
                entry,
                0,
                limits.maximum_directory_depth,
                &mut visited_dirs,
                &mut discovery,
                &mut collector,
            )?;
        } else if entry
            .extension()
            .is_some_and(|extension| extension == "jar")
        {
            collect_jar(entry, &mut discovery, &mut collector)?;
        }
    }
    collector
        .classes
        .sort_by(|left, right| left.0.cmp(&right.0));
    Ok(collector.classes)
}

struct ClassCollector {
    classes: Vec<(String, Vec<u8>)>,
    seen: HashSet<String>,
    total_bytes: usize,
    maximum_bytes: usize,
}

impl ClassCollector {
    fn new(maximum_bytes: usize) -> Self {
        Self {
            classes: Vec::new(),
            seen: HashSet::new(),
            total_bytes: 0,
            maximum_bytes,
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    fn read(
        &mut self,
        name: String,
        reader: impl Read,
        declared_size: u64,
        description: String,
    ) -> Result<(), ClasspathLoadError> {
        if self.contains(&name) {
            return Ok(());
        }
        let remaining = self.maximum_bytes.saturating_sub(self.total_bytes);
        if declared_size > remaining as u64 {
            return Err(ClasspathLoadError::ClasspathTooLarge {
                maximum: self.maximum_bytes,
            });
        }
        let bytes = read_class_bytes(reader, declared_size, description)?;

        let total_bytes = self
            .total_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= self.maximum_bytes)
            .ok_or(ClasspathLoadError::ClasspathTooLarge {
                maximum: self.maximum_bytes,
            })?;
        self.seen.insert(name.clone());
        self.classes.push((name, bytes));
        self.total_bytes = total_bytes;
        Ok(())
    }
}

impl Default for ClassCollector {
    fn default() -> Self {
        Self::new(MAX_CLASSPATH_CLASS_BYTES)
    }
}

struct DiscoveryBudget {
    seen: usize,
    maximum: usize,
}

impl DiscoveryBudget {
    fn new(maximum: usize) -> Self {
        Self { seen: 0, maximum }
    }

    fn record(&mut self, count: usize) -> Result<(), ClasspathLoadError> {
        self.seen = self
            .seen
            .checked_add(count)
            .filter(|seen| *seen <= self.maximum)
            .ok_or(ClasspathLoadError::TooManyDiscoveredEntries {
                maximum: self.maximum,
            })?;
        Ok(())
    }
}

/// Read one class without trusting its declared or eventual size.
pub(crate) fn read_class_bytes(
    reader: impl Read,
    declared_size: u64,
    description: impl Into<String>,
) -> Result<Vec<u8>, ClasspathLoadError> {
    let description = description.into();
    if declared_size > MAX_CLASS_FILE_BYTES as u64 {
        return Err(ClasspathLoadError::ClassTooLarge {
            description,
            size: declared_size,
            maximum: MAX_CLASS_FILE_BYTES,
        });
    }

    let mut bytes = Vec::with_capacity(declared_size as usize);
    reader
        .take(MAX_CLASS_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| ClasspathLoadError::ReadClass {
            description: description.clone(),
            source,
        })?;
    if bytes.len() > MAX_CLASS_FILE_BYTES {
        return Err(ClasspathLoadError::ClassTooLarge {
            description,
            size: bytes.len() as u64,
            maximum: MAX_CLASS_FILE_BYTES,
        });
    }
    Ok(bytes)
}

/// Convert a Java or JVM class name to a contained class-file path.
pub(crate) fn class_entry_path(class_name: &str) -> Option<PathBuf> {
    if class_name.is_empty()
        || class_name.starts_with(['/', '.'])
        || class_name.ends_with(['/', '.'])
        || class_name.contains(['\\', ':'])
        || class_name.chars().any(char::is_control)
        || class_name
            .split(['.', '/'])
            .any(|component| component.is_empty())
    {
        return None;
    }
    Some(PathBuf::from(format!(
        "{}.class",
        class_name.replace('.', "/")
    )))
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    maximum_depth: usize,
    visited_dirs: &mut HashSet<PathBuf>,
    discovery: &mut DiscoveryBudget,
    collector: &mut ClassCollector,
) -> Result<(), ClasspathLoadError> {
    if depth > maximum_depth {
        return Err(ClasspathLoadError::DirectoryTooDeep {
            path: directory.to_path_buf(),
            maximum: maximum_depth,
        });
    }
    let canonical = fs::canonicalize(directory).map_err(|source| ClasspathLoadError::Io {
        action: "resolve class directory",
        path: directory.to_path_buf(),
        source,
    })?;
    if !visited_dirs.insert(canonical) {
        return Ok(());
    }

    let entries = fs::read_dir(directory).map_err(|source| ClasspathLoadError::Io {
        action: "read classpath directory",
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ClasspathLoadError::Io {
            action: "read directory entry in",
            path: directory.to_path_buf(),
            source,
        })?;
        discovery.record(1)?;
        let path = entry.path();
        if path.is_dir() {
            collect_directory(
                root,
                &path,
                depth + 1,
                maximum_depth,
                visited_dirs,
                discovery,
                collector,
            )?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "class")
        {
            let Some(relative) = path.strip_prefix(root).ok() else {
                continue;
            };
            if relative
                .file_stem()
                .is_some_and(|stem| stem == "module-info")
            {
                continue;
            }
            let name = relative
                .with_extension("")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if collector.contains(&name) {
                continue;
            }
            let file = fs::File::open(&path).map_err(|source| ClasspathLoadError::Io {
                action: "open class file",
                path: path.clone(),
                source,
            })?;
            let declared_size = file
                .metadata()
                .map_err(|source| ClasspathLoadError::Io {
                    action: "inspect class file",
                    path: path.clone(),
                    source,
                })?
                .len();
            collector.read(
                name,
                file,
                declared_size,
                format!("class file {}", path.display()),
            )?;
        }
    }
    Ok(())
}

fn collect_jar(
    jar_path: &Path,
    discovery: &mut DiscoveryBudget,
    collector: &mut ClassCollector,
) -> Result<(), ClasspathLoadError> {
    let file = fs::File::open(jar_path).map_err(|source| ClasspathLoadError::Io {
        action: "open JAR",
        path: jar_path.to_path_buf(),
        source,
    })?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|source| ClasspathLoadError::InvalidJar {
            path: jar_path.to_path_buf(),
            source,
        })?;

    // Charge the full central-directory inventory before opening individual
    // entries. Non-class and duplicate entries still consume discovery work.
    discovery.record(archive.len())?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|source| ClasspathLoadError::JarEntry {
                path: jar_path.to_path_buf(),
                index,
                source,
            })?;
        let name = entry
            .name()
            .map_err(|source| ClasspathLoadError::JarEntry {
                path: jar_path.to_path_buf(),
                index,
                source,
            })?
            .into_owned();
        if !name.ends_with(".class")
            || name == "module-info.class"
            || name.ends_with("/module-info.class")
        {
            continue;
        }
        let class_name = name.strip_suffix(".class").unwrap_or(&name).to_string();
        if collector.contains(&class_name) {
            continue;
        }
        let declared_size = entry.size();
        collector.read(
            class_name.clone(),
            &mut entry,
            declared_size,
            format!("class {class_name} from JAR {}", jar_path.display()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn class_entry_paths_reject_traversal_components() {
        assert!(class_entry_path("java/lang/String").is_some());
        assert!(class_entry_path("java.lang.String").is_some());
        assert!(class_entry_path("java/../outside").is_none());
        assert!(class_entry_path("java..outside").is_none());
        assert!(class_entry_path("../outside").is_none());
    }

    #[test]
    fn earlier_classpath_entries_take_precedence() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join("Example.class"), b"first").unwrap();
        fs::write(second.path().join("Example.class"), b"second").unwrap();

        let classes =
            load_classpath_classes(&[first.path().to_path_buf(), second.path().to_path_buf()])
                .unwrap();

        assert_eq!(classes, vec![("Example".to_string(), b"first".to_vec())]);
    }

    #[test]
    fn top_level_classpath_entry_overflow_is_rejected_before_io() {
        let entries = (0..=MAX_CLASSPATH_ENTRIES)
            .map(|index| PathBuf::from(format!("/definitely-missing/classpath-{index}")))
            .collect::<Vec<_>>();

        let error = load_classpath_classes(&entries).unwrap_err();
        assert!(matches!(
            error,
            ClasspathLoadError::TooManyClasspathEntries {
                count,
                maximum: MAX_CLASSPATH_ENTRIES,
            } if count == MAX_CLASSPATH_ENTRIES + 1
        ));
    }

    #[test]
    fn directory_discovery_budget_counts_non_class_entries() {
        let root = tempfile::tempdir().unwrap();
        for name in ["one.txt", "two.txt", "three.txt"] {
            fs::write(root.path().join(name), b"").unwrap();
        }
        let limits = ClasspathLoadLimits {
            maximum_discovered_entries: 2,
            ..ClasspathLoadLimits::default()
        };

        let error =
            load_classpath_classes_with_limits(&[root.path().to_path_buf()], limits).unwrap_err();

        assert!(matches!(
            error,
            ClasspathLoadError::TooManyDiscoveredEntries { maximum: 2 }
        ));
    }

    #[test]
    fn jar_discovery_budget_counts_non_class_entries() {
        let root = tempfile::tempdir().unwrap();
        let jar_path = root.path().join("inventory.jar");
        let file = fs::File::create(&jar_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for name in ["one.txt", "two.txt", "three.txt"] {
            archive.start_file(name, options).unwrap();
        }
        archive.finish().unwrap();
        let limits = ClasspathLoadLimits {
            maximum_discovered_entries: 2,
            ..ClasspathLoadLimits::default()
        };

        let error = load_classpath_classes_with_limits(&[jar_path], limits).unwrap_err();

        assert!(matches!(
            error,
            ClasspathLoadError::TooManyDiscoveredEntries { maximum: 2 }
        ));
    }

    #[test]
    fn directory_depth_is_bounded_before_recursing_further() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("one/two")).unwrap();
        let limits = ClasspathLoadLimits {
            maximum_directory_depth: 1,
            ..ClasspathLoadLimits::default()
        };

        let error =
            load_classpath_classes_with_limits(&[root.path().to_path_buf()], limits).unwrap_err();

        assert!(matches!(
            error,
            ClasspathLoadError::DirectoryTooDeep { maximum: 1, .. }
        ));
    }

    #[test]
    fn caller_byte_budget_is_enforced_while_reading() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Example.class"), b"four").unwrap();
        let limits = ClasspathLoadLimits {
            maximum_retained_class_bytes: 3,
            ..ClasspathLoadLimits::default()
        };

        let error =
            load_classpath_classes_with_limits(&[root.path().to_path_buf()], limits).unwrap_err();

        assert!(matches!(
            error,
            ClasspathLoadError::ClasspathTooLarge { maximum: 3 }
        ));
    }

    #[test]
    fn directory_identity_is_scoped_to_each_classpath_root() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("pkg");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("Example.class"), b"fixture").unwrap();

        let classes = load_classpath_classes(&[package, root.path().to_path_buf()]).unwrap();
        let names = classes
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();

        assert_eq!(names, ["Example", "pkg/Example"]);
    }

    #[cfg(unix)]
    #[test]
    fn directory_walk_terminates_on_symlink_cycle() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(root.path().join("Example.class"), b"fixture").unwrap();
        std::os::unix::fs::symlink(root.path(), nested.join("back")).unwrap();

        let classes = load_classpath_classes(&[root.path().to_path_buf()]).unwrap();

        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].0, "Example");
    }

    #[test]
    fn jar_classes_are_loaded_and_sorted() {
        let root = tempfile::tempdir().unwrap();
        let jar_path = root.path().join("classes.jar");
        let file = fs::File::create(&jar_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive.start_file("z/Zed.class", options).unwrap();
        archive.write_all(b"zed").unwrap();
        archive.start_file("a/Alpha.class", options).unwrap();
        archive.write_all(b"alpha").unwrap();
        archive.finish().unwrap();

        let classes = load_classpath_classes(&[jar_path]).unwrap();

        assert_eq!(classes[0], ("a/Alpha".to_string(), b"alpha".to_vec()));
        assert_eq!(classes[1], ("z/Zed".to_string(), b"zed".to_vec()));
    }

    #[test]
    fn aggregate_limit_failure_does_not_partially_add_class() {
        let mut collector = ClassCollector {
            total_bytes: MAX_CLASSPATH_CLASS_BYTES,
            ..ClassCollector::default()
        };

        let error = collector
            .read("Example".to_string(), &b"x"[..], 1, "fixture".to_string())
            .unwrap_err();

        assert!(matches!(
            error,
            ClasspathLoadError::ClasspathTooLarge { .. }
        ));
        assert!(collector.classes.is_empty());
        assert!(!collector.contains("Example"));
    }

    #[test]
    fn oversized_declared_class_is_rejected_before_reading() {
        let mut collector = ClassCollector::default();

        let error = collector
            .read(
                "Example".to_string(),
                io::empty(),
                MAX_CLASS_FILE_BYTES as u64 + 1,
                "fixture".to_string(),
            )
            .unwrap_err();

        assert!(matches!(error, ClasspathLoadError::ClassTooLarge { .. }));
        assert!(collector.classes.is_empty());
    }
}
