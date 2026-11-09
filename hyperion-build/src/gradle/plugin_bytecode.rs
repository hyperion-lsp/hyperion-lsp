// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_bytecode::classpath::MAX_CLASSPATH_ENTRIES;
use hyperion_bytecode::gradle_plugin_executor::{
    GradlePluginExecutionError, LoadedGradlePluginClasspath,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Bound directory enumeration separately from retained classpath roots. A
/// directory containing millions of non-JAR artifacts must not turn a cache
/// probe into unbounded filesystem work.
const MAX_PLUGIN_LIB_DIRECTORY_ENTRIES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginBytecodeSource {
    Jar(PathBuf),
    ClassesDir(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBytecode {
    pub sources: Vec<PluginBytecodeSource>,
}

impl PluginBytecode {
    pub fn classpath_entries(&self) -> Vec<PathBuf> {
        self.sources
            .iter()
            .map(|s| match s {
                PluginBytecodeSource::Jar(p) | PluginBytecodeSource::ClassesDir(p) => p.clone(),
            })
            .collect()
    }

    /// Load one exact, bounded classpath snapshot and validate the descriptor's
    /// implementation class against those same owned bytes.
    ///
    /// Keeping discovery, validation, fingerprinting, and execution on one
    /// snapshot avoids reopening every JAR once for probing and again for the
    /// VM, and removes the probe-to-execution TOCTOU window.
    pub fn load_snapshot(&self, implementation_class: &str) -> Result<LoadedGradlePluginClasspath> {
        let entries = self.classpath_entries();
        let loaded = LoadedGradlePluginClasspath::load(&entries)?;
        if !loaded.contains_class(implementation_class) {
            return Err(missing_bytecode_error(implementation_class, entries));
        }
        Ok(loaded)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PluginBytecodeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Plugin bytecode missing for {implementation_class}. Checked: {checked:?}. Remediation: {remediation}")]
    Missing {
        implementation_class: String,
        checked: Vec<PathBuf>,
        remediation: String,
    },
    #[error("plugin classpath discovery in {directory} exceeded {maximum} entries")]
    TooManyClasspathEntries { directory: PathBuf, maximum: usize },
    #[error("failed to load plugin classpath snapshot: {0}")]
    Snapshot(#[from] GradlePluginExecutionError),
}

pub type Result<T> = std::result::Result<T, PluginBytecodeError>;

/// Locate classpath entries for an included build plugin implementation class, offline.
///
/// Search order:
/// 1) `build/libs/*.jar`
/// 2) `build/classes/kotlin/main`
/// 3) `build/classes/java/main`
///
/// Returns the deterministic classpath roots to load. The implementation class
/// is validated by [`PluginBytecode::load_snapshot`] against the exact owned
/// bytes subsequently fingerprinted and executed.
pub fn locate_plugin_bytecode(
    included_build_root: &Path,
    implementation_class: &str,
) -> Result<PluginBytecode> {
    let mut sources = Vec::new();

    let libs_dir = included_build_root.join("build/libs");
    if libs_dir.is_dir() {
        let mut jar_paths = Vec::new();
        let mut entries_seen = 0_usize;
        for entry in fs::read_dir(&libs_dir)? {
            let entry = entry?;
            entries_seen += 1;
            if entries_seen > MAX_PLUGIN_LIB_DIRECTORY_ENTRIES {
                return Err(PluginBytecodeError::TooManyClasspathEntries {
                    directory: libs_dir,
                    maximum: MAX_PLUGIN_LIB_DIRECTORY_ENTRIES,
                });
            }
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jar") {
                if jar_paths.len() == MAX_CLASSPATH_ENTRIES {
                    return Err(PluginBytecodeError::TooManyClasspathEntries {
                        directory: libs_dir,
                        maximum: MAX_CLASSPATH_ENTRIES,
                    });
                }
                jar_paths.push(path);
            }
        }
        jar_paths.sort();
        for path in jar_paths {
            sources.push(PluginBytecodeSource::Jar(path));
        }
    }

    for dir in [
        included_build_root.join("build/classes/kotlin/main"),
        included_build_root.join("build/classes/java/main"),
    ] {
        if dir.is_dir() {
            if sources.len() == MAX_CLASSPATH_ENTRIES {
                return Err(PluginBytecodeError::TooManyClasspathEntries {
                    directory: included_build_root.to_path_buf(),
                    maximum: MAX_CLASSPATH_ENTRIES,
                });
            }
            sources.push(PluginBytecodeSource::ClassesDir(dir));
        }
    }

    if sources.is_empty() {
        return Err(missing_bytecode_error(implementation_class, Vec::new()));
    }

    Ok(PluginBytecode { sources })
}

fn missing_bytecode_error(
    implementation_class: &str,
    checked: Vec<PathBuf>,
) -> PluginBytecodeError {
    PluginBytecodeError::Missing {
        implementation_class: implementation_class.to_string(),
        checked,
        remediation: "Run Gradle once to compile the included build (e.g. `./gradlew :build-logic:build`) or enable opt-in online mode/warm cache.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn internal_plugin_class_bytes() -> Vec<u8> {
        let b64 = "yv66vgAAAD0AEQoAAgADBwAEDAAFAAYBABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWCAAIAQACaGkHAAoBABpjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbgEABENvZGUBAA9MaW5lTnVtYmVyVGFibGUBAAVoZWxsbwEAFCgpTGphdmEvbGFuZy9TdHJpbmc7AQAKU291cmNlRmlsZQEAE0ludGVybmFsUGx1Z2luLmphdmEAIQAJAAIAAAAAAAIAAQAFAAYAAQALAAAAHQABAAEAAAAFKrcAAbEAAAABAAwAAAAGAAEAAAADAAkADQAOAAEACwAAABsAAQAAAAAAAxIHsAAAAAEADAAAAAYAAQAAAAQAAQAPAAAAAgAQ";
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine as _;
        STANDARD.decode(b64).expect("base64 decode class bytes")
    }

    #[test]
    fn locator_finds_classes_dir_and_loads_class() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let class_dir = root.join("build/classes/java/main/com/example");
        fs::create_dir_all(&class_dir).unwrap();
        fs::write(
            class_dir.join("InternalPlugin.class"),
            internal_plugin_class_bytes(),
        )
        .unwrap();

        let cp = locate_plugin_bytecode(root, "com.example.InternalPlugin").unwrap();
        let snapshot = cp.load_snapshot("com.example.InternalPlugin").unwrap();
        assert!(snapshot.contains_class("com.example.InternalPlugin"));
    }

    #[test]
    fn locator_finds_jar_and_loads_class() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let libs_dir = root.join("build/libs");
        fs::create_dir_all(&libs_dir).unwrap();

        let jar_path = libs_dir.join("build-logic.jar");
        let file = fs::File::create(&jar_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("com/example/InternalPlugin.class", options)
            .unwrap();
        zip.write_all(&internal_plugin_class_bytes()).unwrap();
        zip.finish().unwrap();

        let cp = locate_plugin_bytecode(root, "com.example.InternalPlugin").unwrap();
        let snapshot = cp.load_snapshot("com.example.InternalPlugin").unwrap();
        assert!(snapshot.contains_class("com.example.InternalPlugin"));
    }

    #[test]
    fn locator_missing_bytecode_has_actionable_remediation() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let msg = match locate_plugin_bytecode(root, "com.example.InternalPlugin") {
            Ok(_) => panic!("expected missing bytecode error"),
            Err(e) => e.to_string(),
        };
        assert!(msg.contains("Run Gradle once"));
        assert!(msg.contains("online mode") || msg.contains("warm"));
    }

    #[test]
    fn locator_rejects_more_jars_than_the_executor_can_load() {
        let tmp = TempDir::new().unwrap();
        let libs_dir = tmp.path().join("build/libs");
        fs::create_dir_all(&libs_dir).unwrap();
        for index in 0..=MAX_CLASSPATH_ENTRIES {
            fs::write(libs_dir.join(format!("candidate-{index:04}.jar")), b"").unwrap();
        }

        let error = locate_plugin_bytecode(tmp.path(), "com.example.InternalPlugin").unwrap_err();
        assert!(matches!(
            error,
            PluginBytecodeError::TooManyClasspathEntries {
                maximum: MAX_CLASSPATH_ENTRIES,
                ..
            }
        ));
    }
}
