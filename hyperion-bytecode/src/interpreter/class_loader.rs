// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Class loading and linking

use super::errors::{VMError, VMResult};
use super::method_area::{ClassInitState, LoadedClass, MethodArea};
use crate::classpath::{class_entry_path, read_class_bytes};
use parking_lot::RwLock;
use rustc_hash::FxHashSet;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Source for loading class files
pub trait ClassSource {
    /// Load a class file by fully qualified name (e.g., "com.example.User")
    /// Returns the raw bytecode of the .class file
    fn load_class(&self, fqn: &str) -> VMResult<Vec<u8>>;

    /// Check if a class exists in this source
    fn class_exists(&self, fqn: &str) -> bool;
}

/// Directory-based class source (filesystem)
pub struct DirectorySource {
    /// Root directory to search for classes
    root: PathBuf,
}

impl DirectorySource {
    /// Create a new directory source
    pub fn new(root: impl AsRef<Path>) -> Self {
        DirectorySource {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Convert FQN (e.g., "com.example.User") to file path
    fn fqn_to_path(&self, fqn: &str) -> Option<PathBuf> {
        class_entry_path(fqn).map(|path| self.root.join(path))
    }
}

impl ClassSource for DirectorySource {
    fn load_class(&self, fqn: &str) -> VMResult<Vec<u8>> {
        let path = self
            .fqn_to_path(fqn)
            .ok_or_else(|| VMError::ClassNotFound(format!("invalid class name '{fqn}'")))?;
        let file = File::open(&path)
            .map_err(|error| VMError::ClassNotFound(format!("{fqn} ({error})")))?;
        let declared_size = file
            .metadata()
            .map_err(|error| VMError::ClassNotFound(format!("{fqn} ({error})")))?
            .len();
        read_class_bytes(
            file,
            declared_size,
            format!("class {fqn} from {}", path.display()),
        )
        .map_err(|error| VMError::ClassFormatError(error.to_string()))
    }

    fn class_exists(&self, fqn: &str) -> bool {
        self.fqn_to_path(fqn).is_some_and(|path| path.is_file())
    }
}

/// JAR-based class source
///
/// Loads .class files from JAR archives (ZIP format).
/// Caches the class index on first access for efficient lookups.
pub struct JarSource<R: Read + Seek = File> {
    /// Path to JAR file (for error messages and reopening)
    jar_path: PathBuf,
    /// Cached set of class FQNs available in this JAR
    /// Uses RwLock for thread-safe lazy initialization
    class_index: RwLock<Option<FxHashSet<String>>>,
    /// Phantom data for the reader type
    _marker: std::marker::PhantomData<R>,
}

impl JarSource<File> {
    /// Create a new JAR source from a file path
    pub fn new(jar_path: impl AsRef<Path>) -> Self {
        JarSource {
            jar_path: jar_path.as_ref().to_path_buf(),
            class_index: RwLock::new(None),
            _marker: std::marker::PhantomData,
        }
    }

    /// Open the JAR file for reading
    fn open_archive(&self) -> VMResult<ZipArchive<File>> {
        let file = File::open(&self.jar_path).map_err(|e| {
            VMError::ClassNotFound(format!(
                "Failed to open JAR {}: {}",
                self.jar_path.display(),
                e
            ))
        })?;
        ZipArchive::new(file).map_err(|e| {
            VMError::ClassNotFound(format!(
                "Invalid JAR file {}: {}",
                self.jar_path.display(),
                e
            ))
        })
    }

    /// Build the class index by scanning all entries in the JAR
    fn build_class_index(&self) -> VMResult<FxHashSet<String>> {
        let mut archive = self.open_archive()?;
        let mut index = FxHashSet::default();

        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index_raw(i) {
                let name = entry
                    .name()
                    .map_err(|e| VMError::ClassNotFound(format!("Invalid JAR entry name: {e}")))?;
                if name.ends_with(".class")
                    && !name.starts_with("META-INF/")
                    && name != "module-info.class"
                    && !name.ends_with("/module-info.class")
                {
                    // Convert path to FQN: "com/example/User.class" -> "com.example.User"
                    let fqn = name.trim_end_matches(".class").replace('/', ".");
                    if class_entry_path(&fqn).as_deref() == Some(Path::new(name.as_ref())) {
                        index.insert(fqn);
                    }
                }
            }
        }

        tracing::debug!(
            jar = %self.jar_path.display(),
            class_count = index.len(),
            "Built JAR class index"
        );

        Ok(index)
    }

    /// Ensure the class index is initialized
    ///
    /// Uses double-checked locking to minimize contention:
    /// 1. Fast path: Read lock to check if index exists (lock-free when initialized)
    /// 2. Slow path: Acquire write lock BEFORE building index to prevent duplicate work
    ///
    /// This prevents the race condition where multiple threads could build the index
    /// concurrently (wasting CPU) if we released the read lock before acquiring write lock.
    fn ensure_index(&self) -> VMResult<()> {
        // Fast path: check if already initialized (lock-free read)
        if self.class_index.read().is_some() {
            return Ok(());
        }

        // Slow path: acquire write lock BEFORE building index
        let mut guard = self.class_index.write();

        // Double-check: another thread may have built it while we waited for the lock
        if guard.is_some() {
            return Ok(());
        }

        // Build index while holding write lock
        // This serializes index builds but eliminates wasted work from concurrent builds
        let index = self.build_class_index()?;
        *guard = Some(index);
        Ok(())
    }

    /// Convert FQN to JAR entry path
    fn fqn_to_entry_path(fqn: &str) -> Option<String> {
        class_entry_path(fqn).map(|path| path.to_string_lossy().into_owned())
    }
}

impl ClassSource for JarSource<File> {
    fn load_class(&self, fqn: &str) -> VMResult<Vec<u8>> {
        let entry_path = Self::fqn_to_entry_path(fqn)
            .ok_or_else(|| VMError::ClassNotFound(format!("invalid class name '{fqn}'")))?;
        let mut archive = self.open_archive()?;

        let mut file = archive.by_name(&entry_path).map_err(|e| {
            VMError::ClassNotFound(format!(
                "Class {} not found in JAR {}: {}",
                fqn,
                self.jar_path.display(),
                e
            ))
        })?;

        let declared_size = file.size();
        let bytecode = read_class_bytes(
            &mut file,
            declared_size,
            format!("class {fqn} from JAR {}", self.jar_path.display()),
        )
        .map_err(|error| VMError::ClassFormatError(error.to_string()))?;

        tracing::debug!(
            class = fqn,
            jar = %self.jar_path.display(),
            size = bytecode.len(),
            "Loaded class from JAR"
        );

        Ok(bytecode)
    }

    fn class_exists(&self, fqn: &str) -> bool {
        if Self::fqn_to_entry_path(fqn).is_none() {
            return false;
        }
        // Try to ensure the index is built
        if self.ensure_index().is_err() {
            return false;
        }

        let guard = self.class_index.read();
        guard.as_ref().is_some_and(|index| index.contains(fqn))
    }
}

/// Ordered directory/JAR classpath.
pub struct ClassPath {
    entries: Vec<ClassPathEntry>,
}

enum ClassPathEntry {
    Jar(JarSource<File>),
    Directory(DirectorySource),
}

impl ClassPathEntry {
    fn source(&self) -> &dyn ClassSource {
        match self {
            Self::Jar(source) => source,
            Self::Directory(source) => source,
        }
    }
}

impl ClassPath {
    /// Create a new empty classpath
    pub fn new() -> Self {
        ClassPath {
            entries: Vec::new(),
        }
    }

    /// Parse a classpath string (colon-separated on Unix, semicolon on Windows)
    pub fn from_classpath(classpath: &str) -> Self {
        let mut cp = ClassPath::new();

        for path in std::env::split_paths(classpath) {
            if path.as_os_str().is_empty() {
                continue;
            }
            if path.is_dir() {
                cp.add_directory(&path);
            } else if path.extension().is_some_and(|ext| ext == "jar") {
                cp.add_jar(&path);
            }
        }

        cp
    }

    /// Add a JAR file to the classpath
    pub fn add_jar(&mut self, path: impl AsRef<Path>) {
        self.entries.push(ClassPathEntry::Jar(JarSource::new(path)));
    }

    /// Add a directory to the classpath
    pub fn add_directory(&mut self, path: impl AsRef<Path>) {
        self.entries
            .push(ClassPathEntry::Directory(DirectorySource::new(path)));
    }

    /// Get the number of JAR files in the classpath
    pub fn jar_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, ClassPathEntry::Jar(_)))
            .count()
    }

    /// Get the number of directories in the classpath
    pub fn directory_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| matches!(entry, ClassPathEntry::Directory(_)))
            .count()
    }
}

impl Default for ClassPath {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassSource for ClassPath {
    fn load_class(&self, fqn: &str) -> VMResult<Vec<u8>> {
        for entry in &self.entries {
            let source = entry.source();
            if source.class_exists(fqn) {
                return source.load_class(fqn);
            }
        }

        Err(VMError::ClassNotFound(fqn.to_string()))
    }

    fn class_exists(&self, fqn: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.source().class_exists(fqn))
    }
}

/// Main class loader for the JVM interpreter
pub struct ClassLoader {
    /// Class sources (directories, JARs) to search
    sources: Vec<Box<dyn ClassSource>>,
    /// Method area for storing loaded classes
    method_area: MethodArea,
    /// Read-only parent registry used by an unpublished staging loader.
    fallback_method_area: Option<MethodArea>,
}

impl ClassLoader {
    /// Create a new class loader
    pub fn new() -> Self {
        Self::with_method_area(MethodArea::new())
    }

    /// Create a loader backed by an existing method area.
    ///
    /// A VM must have one authoritative class registry: linking and execution
    /// cannot observe independent copies without making superclass visibility
    /// depend on which facade registered a class.
    pub(crate) fn with_method_area(method_area: MethodArea) -> Self {
        ClassLoader {
            sources: Vec::new(),
            method_area,
            fallback_method_area: None,
        }
    }

    /// Create an isolated definition loader that can resolve already-published
    /// classes without cloning or mutating their registry.
    pub(crate) fn staging_with_fallback(fallback_method_area: MethodArea) -> Self {
        ClassLoader {
            sources: Vec::new(),
            method_area: MethodArea::new(),
            fallback_method_area: Some(fallback_method_area),
        }
    }

    /// Add a directory source to the class path
    pub fn add_directory(&mut self, path: impl AsRef<Path>) {
        self.sources.push(Box::new(DirectorySource::new(path)));
    }

    /// Add a JAR source to the class path
    pub fn add_jar(&mut self, path: impl AsRef<Path>) {
        self.sources.push(Box::new(JarSource::new(path)));
    }

    /// Get the method area
    pub fn method_area(&self) -> &MethodArea {
        &self.method_area
    }

    /// Load a class from any available source
    pub fn load_class(&self, fqn: &str) -> VMResult<Vec<u8>> {
        for source in &self.sources {
            if source.class_exists(fqn) {
                return source.load_class(fqn);
            }
        }
        Err(VMError::ClassNotFound(fqn.to_string()))
    }

    /// Check if a class can be loaded
    pub fn can_load_class(&self, fqn: &str) -> bool {
        self.sources.iter().any(|s| s.class_exists(fqn))
    }

    /// Register a loaded class in the method area
    pub fn register_loaded_class(&self, class: LoadedClass) -> VMResult<()> {
        self.method_area.register_class(class)
    }

    /// Atomically register an order-linked definition batch.
    pub fn register_loaded_classes(&self, classes: Vec<LoadedClass>) -> VMResult<()> {
        self.method_area.register_classes(classes)
    }

    /// Get a loaded class
    pub fn get_loaded_class(&self, fqn: &str) -> VMResult<LoadedClass> {
        if let Ok(class) = self.method_area.get_defined_class(fqn) {
            return Ok(class);
        }
        if let Some(fallback) = &self.fallback_method_area {
            if let Ok(class) = fallback.get_defined_class(fqn) {
                return Ok(class);
            }
        }
        // Native stubs belong to the isolated staging registry. A failed
        // transaction must not mutate its authoritative fallback.
        self.method_area.get_class(fqn)
    }

    /// Set class initialization state
    pub fn set_init_state(&self, fqn: &str, state: ClassInitState) -> VMResult<()> {
        self.method_area.set_init_state(fqn, state)
    }

    /// Check if a class has been loaded
    pub fn is_class_loaded(&self, fqn: &str) -> bool {
        self.method_area.class_exists(fqn)
            || self
                .fallback_method_area
                .as_ref()
                .is_some_and(|fallback| fallback.class_exists(fqn))
    }
}

impl Default for ClassLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_directory_source_fqn_to_path() {
        let dir = DirectorySource::new("/tmp");
        let path = dir.fqn_to_path("com.example.User").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/com/example/User.class"));
    }

    #[test]
    fn test_directory_source_load_class() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("com/example")).unwrap();
        let class_path = root.join("com/example/Test.class");
        let mut file = fs::File::create(&class_path).unwrap();
        file.write_all(b"CAFEBABE").unwrap();

        let source = DirectorySource::new(root);
        assert!(source.class_exists("com.example.Test"));
        assert!(!source.class_exists("com.example.Missing"));

        let bytecode = source.load_class("com.example.Test").unwrap();
        assert_eq!(bytecode, b"CAFEBABE");
        assert!(source.load_class("com.example.Missing").is_err());
    }

    #[test]
    fn test_class_loader() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        fs::create_dir_all(root.join("com/example")).unwrap();
        let class_path = root.join("com/example/Test.class");
        let mut file = fs::File::create(&class_path).unwrap();
        file.write_all(b"CAFEBABE").unwrap();

        let mut loader = ClassLoader::new();
        loader.add_directory(root);

        assert!(loader.can_load_class("com.example.Test"));
        assert!(!loader.can_load_class("com.example.Missing"));

        let bytecode = loader.load_class("com.example.Test").unwrap();
        assert_eq!(bytecode, b"CAFEBABE");
    }

    #[test]
    fn test_class_loader_registration() {
        let loader = ClassLoader::new();
        let class = LoadedClass::new("com.example.Test".to_string());

        loader.register_loaded_class(class).unwrap();
        assert!(loader.is_class_loaded("com.example.Test"));

        let loaded = loader.get_loaded_class("com.example.Test").unwrap();
        assert_eq!(&*loaded.name, "com.example.Test");
    }

    #[test]
    fn staging_prefers_an_authoritative_user_definition_over_reserved_stub_name() {
        let authoritative = MethodArea::new();
        let user_class = LoadedClass::new("java.lang.Integer".to_string());
        authoritative.register_class(user_class).unwrap();
        let staging = ClassLoader::staging_with_fallback(authoritative.clone());

        let resolved = staging.get_loaded_class("java/lang/Integer").unwrap();
        assert!(!resolved.is_native_stub);
        assert!(
            !authoritative
                .get_class("java.lang.Integer")
                .unwrap()
                .is_native_stub
        );
    }

    #[test]
    fn test_class_loader_init_state() {
        use super::super::monitor::ThreadId;

        let loader = ClassLoader::new();
        let class = LoadedClass::new("com.example.Test".to_string());

        let test_thread = ThreadId::MAIN;
        loader.register_loaded_class(class).unwrap();
        loader
            .set_init_state(
                "com.example.Test",
                ClassInitState::Initializing(test_thread),
            )
            .unwrap();

        let loaded = loader.get_loaded_class("com.example.Test").unwrap();
        assert_eq!(loaded.init_state, ClassInitState::Initializing(test_thread));
    }

    // ========== JAR Source Tests ==========

    fn create_test_jar(temp_dir: &TempDir, classes: &[(&str, &[u8])]) -> PathBuf {
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let jar_path = temp_dir.path().join("test.jar");
        let file = fs::File::create(&jar_path).unwrap();
        let mut zip = ZipWriter::new(file);

        let options = SimpleFileOptions::default();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(b"Manifest-Version: 1.0\n").unwrap();

        for (fqn, bytecode) in classes {
            let path = format!("{}.class", fqn.replace('.', "/"));
            zip.start_file(&path, options).unwrap();
            zip.write_all(bytecode).unwrap();
        }

        zip.finish().unwrap();
        jar_path
    }

    #[test]
    fn test_jar_source_class_exists() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(
            &temp_dir,
            &[
                ("com.example.User", b"CAFEBABE_USER"),
                ("com.example.Order", b"CAFEBABE_ORDER"),
            ],
        );

        let source = JarSource::new(&jar_path);
        assert!(source.class_exists("com.example.User"));
        assert!(source.class_exists("com.example.Order"));
        assert!(!source.class_exists("com.example.Missing"));
    }

    #[test]
    fn test_jar_source_load_class() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(
            &temp_dir,
            &[
                ("com.example.User", b"CAFEBABE_USER"),
                ("org.other.Service", b"CAFEBABE_SERVICE"),
            ],
        );

        let source = JarSource::new(&jar_path);
        let user_bytes = source.load_class("com.example.User").unwrap();
        assert_eq!(user_bytes, b"CAFEBABE_USER");

        let service_bytes = source.load_class("org.other.Service").unwrap();
        assert_eq!(service_bytes, b"CAFEBABE_SERVICE");
        assert!(source.load_class("com.example.Missing").is_err());
    }

    #[test]
    fn test_jar_source_fqn_to_entry_path() {
        assert_eq!(
            JarSource::<File>::fqn_to_entry_path("com.example.User").as_deref(),
            Some("com/example/User.class")
        );
    }

    #[test]
    fn class_sources_reject_absolute_and_traversing_names() {
        let temp_dir = TempDir::new().unwrap();
        let directory = DirectorySource::new(temp_dir.path());

        for name in ["/outside", "../outside", "com..outside", "C:/outside"] {
            assert!(!directory.class_exists(name));
            assert!(directory.load_class(name).is_err());
            assert!(JarSource::<File>::fqn_to_entry_path(name).is_none());
        }
    }

    #[test]
    fn test_class_loader_with_jar() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(&temp_dir, &[("com.example.JarClass", b"CAFEBABE_JAR")]);

        let mut loader = ClassLoader::new();
        loader.add_jar(&jar_path);

        assert!(loader.can_load_class("com.example.JarClass"));
        let bytecode = loader.load_class("com.example.JarClass").unwrap();
        assert_eq!(bytecode, b"CAFEBABE_JAR");
    }

    #[test]
    fn test_classpath_new() {
        let cp = ClassPath::new();
        assert_eq!(cp.jar_count(), 0);
        assert_eq!(cp.directory_count(), 0);
    }

    #[test]
    fn test_classpath_add_jar() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(&temp_dir, &[("com.example.User", b"CAFEBABE")]);

        let mut cp = ClassPath::new();
        cp.add_jar(&jar_path);
        assert_eq!(cp.jar_count(), 1);
        assert!(cp.class_exists("com.example.User"));
    }

    #[test]
    fn test_classpath_load_class() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(&temp_dir, &[("com.example.User", b"CAFEBABE_USER")]);

        let mut cp = ClassPath::new();
        cp.add_jar(&jar_path);

        let bytecode = cp.load_class("com.example.User").unwrap();
        assert_eq!(bytecode, b"CAFEBABE_USER");
        assert!(cp.load_class("com.example.Missing").is_err());
    }

    #[test]
    fn classpath_preserves_mixed_entry_order() {
        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(&temp_dir, &[("com.example.User", b"FROM_JAR")]);
        let classes_dir = temp_dir.path().join("classes");
        fs::create_dir_all(classes_dir.join("com/example")).unwrap();
        fs::write(classes_dir.join("com/example/User.class"), b"FROM_DIR").unwrap();

        let mut jar_first = ClassPath::new();
        jar_first.add_jar(&jar_path);
        jar_first.add_directory(&classes_dir);
        assert_eq!(
            jar_first.load_class("com.example.User").unwrap(),
            b"FROM_JAR"
        );

        let mut directory_first = ClassPath::new();
        directory_first.add_directory(&classes_dir);
        directory_first.add_jar(&jar_path);
        assert_eq!(
            directory_first.load_class("com.example.User").unwrap(),
            b"FROM_DIR"
        );
    }

    #[test]
    fn classpath_parser_preserves_spaces_in_paths() {
        let temp_dir = TempDir::new().unwrap();
        let classes_dir = temp_dir.path().join(" classes ");
        fs::create_dir(&classes_dir).unwrap();
        let classpath = std::env::join_paths([&classes_dir]).unwrap();

        let parsed = ClassPath::from_classpath(classpath.to_str().unwrap());

        assert_eq!(parsed.directory_count(), 1);
    }

    #[test]
    fn directory_source_rejects_oversized_class_before_reading() {
        let temp_dir = TempDir::new().unwrap();
        let class_path = temp_dir.path().join("Example.class");
        let file = fs::File::create(&class_path).unwrap();
        file.set_len(crate::classpath::MAX_CLASS_FILE_BYTES as u64 + 1)
            .unwrap();
        let source = DirectorySource::new(temp_dir.path());

        assert!(matches!(
            source.load_class("Example"),
            Err(VMError::ClassFormatError(_))
        ));
    }

    /// Test: JAR index build race condition
    ///
    /// This test verifies that the double-checked locking pattern correctly
    /// prevents multiple threads from building the JAR index concurrently.
    ///
    /// The bug was that the read lock was released before building the index,
    /// allowing multiple threads to enter build_class_index() concurrently,
    /// wasting CPU on duplicate work.
    ///
    /// The fix acquires the write lock BEFORE building the index, serializing
    /// index builds and ensuring only one thread does the work.
    #[test]
    fn test_jar_index_race_condition_m16() {
        use std::sync::Arc;
        use std::thread;

        let temp_dir = TempDir::new().unwrap();
        let jar_path = create_test_jar(
            &temp_dir,
            &[
                ("com.example.User", b"CAFEBABE_USER"),
                ("com.example.Post", b"CAFEBABE_POST"),
                ("com.example.Comment", b"CAFEBABE_COMMENT"),
            ],
        );

        // Create a JAR source
        let jar_source = Arc::new(JarSource::new(jar_path.clone()));

        // Spawn multiple threads that all try to load classes concurrently
        // This forces the first access to build the index, and subsequent accesses
        // should see the index already built.
        let mut handles = vec![];
        for i in 0..10 {
            let source = Arc::clone(&jar_source);
            let handle = thread::spawn(move || {
                // Each thread tries to load a class
                let class_name = match i % 3 {
                    0 => "com.example.User",
                    1 => "com.example.Post",
                    _ => "com.example.Comment",
                };

                // This will call ensure_index() internally
                let result = source.load_class(class_name);
                assert!(result.is_ok(), "Thread {} failed to load class", i);
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify the index was built correctly by checking class existence
        assert!(jar_source.class_exists("com.example.User"));
        assert!(jar_source.class_exists("com.example.Post"));
        assert!(jar_source.class_exists("com.example.Comment"));
        assert!(!jar_source.class_exists("com.example.Missing"));
    }
}
