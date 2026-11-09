// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_PLUGIN_DESCRIPTOR_BYTES: usize = 1024 * 1024;
const MAX_BUILD_SCRIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginDescriptorSource {
    Jar { jar_path: PathBuf },
    ResourcesDir { descriptor_path: PathBuf },
    HeuristicBuildScript { build_script: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub plugin_id: String,
    pub implementation_class: String,
    pub source: PluginDescriptorSource,
}

#[derive(Debug, thiserror::Error)]
pub enum CompositePluginError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid jar: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Invalid Gradle plugin ID: {0}")]
    InvalidPluginId(String),
    #[error("{description} is too large: {size} bytes (maximum {maximum})")]
    TooLarge {
        description: String,
        size: u64,
        maximum: usize,
    },
}

pub type Result<T> = std::result::Result<T, CompositePluginError>;

/// Resolve `pluginId -> implementationClass` for an included build root, offline.
///
/// Strategy order:
/// 1) Compiled resources directory: `build/resources/main/META-INF/gradle-plugins/<id>.properties`
/// 2) Built plugin JARs: `build/libs/*.jar` containing `META-INF/gradle-plugins/<id>.properties`
/// 3) Heuristic parse of `build.gradle.kts` for simple `gradlePlugin { plugins { ... } }` blocks
pub fn resolve_plugin_descriptor(
    included_build_root: &Path,
    plugin_id: &str,
) -> Result<Option<PluginDescriptor>> {
    validate_plugin_id(plugin_id)?;
    let descriptor_rel = format!("META-INF/gradle-plugins/{}.properties", plugin_id);

    // 1) build/resources/main
    let resources_descriptor = included_build_root
        .join("build/resources/main")
        .join(&descriptor_rel);
    if resources_descriptor.exists() {
        let content = read_utf8_file_bounded(
            &resources_descriptor,
            MAX_PLUGIN_DESCRIPTOR_BYTES,
            "Gradle plugin descriptor",
        )?;
        if let Some(implementation_class) = parse_implementation_class_from_properties(&content) {
            return Ok(Some(PluginDescriptor {
                plugin_id: plugin_id.to_string(),
                implementation_class,
                source: PluginDescriptorSource::ResourcesDir {
                    descriptor_path: resources_descriptor,
                },
            }));
        }
    }

    // 2) build/libs/*.jar
    let libs_dir = included_build_root.join("build/libs");
    if libs_dir.is_dir() {
        let mut jars = fs::read_dir(libs_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        jars.retain(|path| path.extension().is_some_and(|extension| extension == "jar"));
        jars.sort();
        for path in jars {
            if let Some(implementation_class) = read_descriptor_from_jar(&path, &descriptor_rel)? {
                return Ok(Some(PluginDescriptor {
                    plugin_id: plugin_id.to_string(),
                    implementation_class,
                    source: PluginDescriptorSource::Jar { jar_path: path },
                }));
            }
        }
    }

    // 3) heuristic: build.gradle.kts
    let build_kts = included_build_root.join("build.gradle.kts");
    if build_kts.exists() {
        let content =
            read_utf8_file_bounded(&build_kts, MAX_BUILD_SCRIPT_BYTES, "Gradle build script")?;
        if let Some(implementation_class) =
            heuristic_find_plugin_impl_class_kts(&content, plugin_id)
        {
            return Ok(Some(PluginDescriptor {
                plugin_id: plugin_id.to_string(),
                implementation_class,
                source: PluginDescriptorSource::HeuristicBuildScript {
                    build_script: build_kts,
                },
            }));
        }
    }

    Ok(None)
}

fn validate_plugin_id(plugin_id: &str) -> Result<()> {
    if plugin_id.is_empty()
        || plugin_id.starts_with('.')
        || plugin_id.ends_with('.')
        || plugin_id.split('.').any(str::is_empty)
        || !plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(CompositePluginError::InvalidPluginId(plugin_id.to_owned()));
    }
    Ok(())
}

fn read_utf8_file_bounded(path: &Path, maximum: usize, description: &str) -> Result<String> {
    let file = fs::File::open(path)?;
    let declared_size = file.metadata()?.len();
    if declared_size > maximum as u64 {
        return Err(CompositePluginError::TooLarge {
            description: description.to_owned(),
            size: declared_size,
            maximum,
        });
    }
    read_utf8_bounded(file, maximum, description)
}

fn read_utf8_bounded(reader: impl Read, maximum: usize, description: &str) -> Result<String> {
    let mut contents = String::new();
    reader
        .take(maximum as u64 + 1)
        .read_to_string(&mut contents)?;
    if contents.len() > maximum {
        return Err(CompositePluginError::TooLarge {
            description: description.to_owned(),
            size: contents.len() as u64,
            maximum,
        });
    }
    Ok(contents)
}

pub fn parse_implementation_class_from_properties(properties: &str) -> Option<String> {
    for raw_line in properties.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        if key.trim() == "implementation-class" {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn read_descriptor_from_jar(jar_path: &Path, entry_path: &str) -> Result<Option<String>> {
    let file = fs::File::open(jar_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut entry = match zip.by_name(entry_path) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if entry.size() > MAX_PLUGIN_DESCRIPTOR_BYTES as u64 {
        return Err(CompositePluginError::TooLarge {
            description: format!("plugin descriptor {entry_path} in {}", jar_path.display()),
            size: entry.size(),
            maximum: MAX_PLUGIN_DESCRIPTOR_BYTES,
        });
    }
    let contents = read_utf8_bounded(
        &mut entry,
        MAX_PLUGIN_DESCRIPTOR_BYTES,
        &format!("plugin descriptor {entry_path} in {}", jar_path.display()),
    )?;
    Ok(parse_implementation_class_from_properties(&contents))
}

/// Narrow, best-effort parser for Kotlin DSL plugin registration blocks.
///
/// Supports patterns like:
/// ```kotlin
/// gradlePlugin {
///   plugins {
///     create("internal") {
///       id = "com.example.internal"
///       implementationClass = "com.example.InternalPlugin"
///     }
///   }
/// }
/// ```
fn heuristic_find_plugin_impl_class_kts(build_gradle_kts: &str, plugin_id: &str) -> Option<String> {
    // Only attempt a narrow scan: look for the plugin id assignment, then scan a small window for
    // implementationClass in the same block.
    let id_pat = format!("id = \"{}\"", plugin_id);
    let id_pos = build_gradle_kts.find(&id_pat)?;
    let window = &build_gradle_kts[id_pos..build_gradle_kts.len().min(id_pos + 2000)];

    // Also allow single quotes in Kotlin DSL (rare but possible)
    let impl_key = "implementationClass";
    for line in window.lines().take(200) {
        let trimmed = line.trim();
        if !trimmed.contains(impl_key) {
            continue;
        }
        let (_, rhs) = trimmed.split_once('=')?;
        let rhs = rhs.trim();
        if let Some(stripped) = rhs.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return Some(stripped.to_string());
        }
        if let Some(stripped) = rhs.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            return Some(stripped.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    #[test]
    fn plugin_ids_cannot_escape_the_descriptor_directory() {
        let tmp = TempDir::new().unwrap();

        let error = resolve_plugin_descriptor(tmp.path(), "../../outside").unwrap_err();

        assert!(matches!(error, CompositePluginError::InvalidPluginId(_)));
    }

    #[test]
    fn bounded_text_reader_rejects_oversized_content() {
        let error = read_utf8_bounded(&b"12345"[..], 4, "fixture").unwrap_err();

        assert!(matches!(error, CompositePluginError::TooLarge { .. }));
    }

    #[test]
    fn properties_parser_extracts_implementation_class() {
        let props = r#"
# comment
implementation-class=com.example.InternalPlugin
"#;
        assert_eq!(
            parse_implementation_class_from_properties(props).as_deref(),
            Some("com.example.InternalPlugin")
        );
    }

    #[test]
    fn resolve_from_resources_dir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let descriptor = root
            .join("build/resources/main/META-INF/gradle-plugins/com.example.internal.properties");
        fs::create_dir_all(descriptor.parent().unwrap()).unwrap();
        fs::write(
            &descriptor,
            "implementation-class=com.example.InternalPlugin\n",
        )
        .unwrap();

        let resolved = resolve_plugin_descriptor(root, "com.example.internal")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.implementation_class, "com.example.InternalPlugin");
        assert!(matches!(
            resolved.source,
            PluginDescriptorSource::ResourcesDir { .. }
        ));
    }

    #[test]
    fn resolve_from_jar_descriptor() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let libs_dir = root.join("build/libs");
        fs::create_dir_all(&libs_dir).unwrap();

        let jar_path = libs_dir.join("build-logic.jar");
        let file = fs::File::create(&jar_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = SimpleFileOptions::default();
        let entry = "META-INF/gradle-plugins/com.example.internal.properties";
        zip.start_file(entry, options).unwrap();
        zip.write_all(b"implementation-class=com.example.InternalPlugin\n")
            .unwrap();
        zip.finish().unwrap();

        let resolved = resolve_plugin_descriptor(root, "com.example.internal")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.implementation_class, "com.example.InternalPlugin");
        assert!(matches!(
            resolved.source,
            PluginDescriptorSource::Jar { .. }
        ));
    }

    #[test]
    fn jar_descriptor_resolution_is_deterministic() {
        let tmp = TempDir::new().unwrap();
        let libs_dir = tmp.path().join("build/libs");
        fs::create_dir_all(&libs_dir).unwrap();
        let entry = "META-INF/gradle-plugins/com.example.internal.properties";
        for (jar_name, implementation) in [
            ("z-last.jar", "com.example.Last"),
            ("a-first.jar", "com.example.First"),
        ] {
            let file = fs::File::create(libs_dir.join(jar_name)).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(entry, SimpleFileOptions::default()).unwrap();
            writeln!(zip, "implementation-class={implementation}").unwrap();
            zip.finish().unwrap();
        }

        let resolved = resolve_plugin_descriptor(tmp.path(), "com.example.internal")
            .unwrap()
            .unwrap();

        assert_eq!(resolved.implementation_class, "com.example.First");
    }

    #[test]
    fn resolve_heuristic_from_build_gradle_kts() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("build.gradle.kts"),
            r#"
gradlePlugin {
  plugins {
    create("internal") {
      id = "com.example.internal"
      implementationClass = "com.example.InternalPlugin"
    }
  }
}
"#,
        )
        .unwrap();

        let resolved = resolve_plugin_descriptor(root, "com.example.internal")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.implementation_class, "com.example.InternalPlugin");
        assert!(matches!(
            resolved.source,
            PluginDescriptorSource::HeuristicBuildScript { .. }
        ));
    }
}
