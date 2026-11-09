// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK class discovery and extraction
//!
//! This module handles locating the JDK installation and extracting
//! .class files from the lib/modules jimage format (Java 9+).
//!
//! JDK images are extracted to private temporary directories and class bytes
//! are read lazily, avoiding a process-wide copy of the entire module.

use crate::classpath::{class_entry_path, read_class_bytes, ClasspathLoadError};
use std::env;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use thiserror::Error;
use tracing::info;

const JAVA_BASE_PATTERN: &str = "glob:/java.base/**";
const MAX_JIMAGE_ERROR_BYTES: u64 = 16 * 1024;

/// A scoped extraction of the JDK's `java.base` image.
///
/// Keeping ownership explicit is important: process-global `TempDir` values do
/// not run their destructors at process exit and therefore leak one complete
/// extraction on every invocation.
pub struct JdkImage {
    _temp_dir: tempfile::TempDir,
    java_base_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum JdkError {
    #[error("JAVA_HOME not found")]
    JavaHomeNotFound,

    #[error("Modules file not found at {0}")]
    ModulesNotFound(PathBuf),

    #[error("jimage tool not found in JDK")]
    JimageNotFound,

    #[error("Failed to execute jimage command: {0}")]
    CommandFailed(#[from] std::io::Error),

    #[error("jimage command failed: {0}")]
    ExtractionFailed(String),

    #[error("Failed to read extracted class file: {0}")]
    ReadFailed(std::io::Error),

    #[error("Invalid UTF-8 in jimage output")]
    InvalidUtf8,

    #[error("Class not found in JDK image: {0}")]
    ClassNotFound(String),

    #[error("Invalid JDK class name: {0}")]
    InvalidClassName(String),

    #[error(transparent)]
    ClassRead(#[from] ClasspathLoadError),
}

/// Find JAVA_HOME from environment or system defaults
pub fn find_java_home() -> Result<PathBuf, JdkError> {
    // Try JAVA_HOME environment variable first
    if let Ok(java_home) = env::var("JAVA_HOME") {
        let path = PathBuf::from(java_home);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try system-specific defaults
    #[cfg(target_os = "macos")]
    {
        if let Some(path) = find_macos_jdk() {
            return Ok(path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(path) = find_linux_jdk() {
            return Ok(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = find_windows_jdk() {
            return Ok(path);
        }
    }

    Err(JdkError::JavaHomeNotFound)
}

/// Find JDK on macOS
#[cfg(target_os = "macos")]
fn find_macos_jdk() -> Option<PathBuf> {
    // Try /Library/Java/JavaVirtualMachines/*/Contents/Home
    let jvm_dir = Path::new("/Library/Java/JavaVirtualMachines");
    if let Ok(entries) = std::fs::read_dir(jvm_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let home = path.join("Contents").join("Home");
                if home.exists() {
                    return Some(home);
                }
            }
        }
    }
    None
}

/// Find JDK on Linux
#[cfg(target_os = "linux")]
fn find_linux_jdk() -> Option<PathBuf> {
    // Try common Linux JDK locations
    let candidates = vec!["/usr/lib/jvm/default-java", "/usr/lib/jvm/java"];

    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    // Try /usr/lib/jvm/java-*-openjdk
    if let Ok(entries) = std::fs::read_dir("/usr/lib/jvm") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name()?.to_str()?;
                if name.starts_with("java-") && name.contains("openjdk") {
                    return Some(path);
                }
            }
        }
    }

    None
}

/// Find JDK on Windows
#[cfg(target_os = "windows")]
fn find_windows_jdk() -> Option<PathBuf> {
    // Try C:\Program Files\Java\jdk-*
    let java_dir = Path::new("C:\\Program Files\\Java");
    if let Ok(entries) = std::fs::read_dir(java_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name()?.to_str()?;
                if name.starts_with("jdk-") {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Get path to modules file (Java 9+ jimage format)
pub fn get_modules_file(java_home: &Path) -> Result<PathBuf, JdkError> {
    let modules = java_home.join("lib").join("modules");
    if modules.exists() {
        Ok(modules)
    } else {
        Err(JdkError::ModulesNotFound(modules))
    }
}

/// Get the jimage tool path from the modules file path
/// modules_file is at $JAVA_HOME/lib/modules, jimage is at $JAVA_HOME/bin/jimage
fn get_jimage_tool(modules_file: &Path) -> Result<PathBuf, JdkError> {
    // Go from lib/modules up to JAVA_HOME, then to bin/jimage
    let java_home = modules_file
        .parent() // lib
        .and_then(|p| p.parent()) // JAVA_HOME
        .ok_or(JdkError::JimageNotFound)?;

    let jimage = java_home.join("bin").join("jimage");
    if jimage.exists() {
        Ok(jimage)
    } else {
        Err(JdkError::JimageNotFound)
    }
}

/// List all classes in the java.base module
pub fn list_javabase_classes(modules_file: &Path) -> Result<Vec<String>, JdkError> {
    let jimage = get_jimage_tool(modules_file)?;
    let output = Command::new(&jimage)
        .arg("list")
        .arg("--include")
        .arg(JAVA_BASE_PATTERN)
        .arg(modules_file)
        .output()?;

    if !output.status.success() {
        return Err(jimage_failure(&output.status, &output.stderr, false));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|_| JdkError::InvalidUtf8)?;

    let mut classes = Vec::new();
    let mut in_java_base = false;

    for line in stdout.lines() {
        // Check for module headers
        if line.starts_with("Module: ") {
            in_java_base = line == "Module: java.base";
            continue;
        }

        // If we're in java.base and this is a class file
        if in_java_base && line.trim().ends_with(".class") {
            // Lines are indented with spaces
            let class_path = line.trim();

            // Remove .class extension: "java/lang/String.class" -> "java/lang/String"
            if let Some(class_name) = class_path.strip_suffix(".class") {
                classes.push(class_name.to_string());
            }
        }
    }

    Ok(classes)
}

impl JdkImage {
    /// Extract `java.base` once for callers that need to read many classes.
    pub fn open(modules_file: &Path) -> Result<Self, JdkError> {
        let modules_file = fs::canonicalize(modules_file).map_err(JdkError::ReadFailed)?;
        let temp_dir = tempfile::Builder::new()
            .prefix("hyperion-jdk-extract-")
            .tempdir()
            .map_err(JdkError::ReadFailed)?;
        extract_jimage(&modules_file, JAVA_BASE_PATTERN, temp_dir.path())?;

        let java_base_dir = temp_dir.path().join("java.base");
        if !java_base_dir.is_dir() {
            return Err(JdkError::ExtractionFailed(
                "java.base was not present in the extracted image".to_string(),
            ));
        }

        Ok(Self {
            _temp_dir: temp_dir,
            java_base_dir,
        })
    }

    /// Read one class from this extracted image.
    pub fn extract_class(&self, class_name: &str) -> Result<Vec<u8>, JdkError> {
        read_extracted_class(&self.java_base_dir, class_name)
    }
}

/// Extract a specific class file from the jimage modules file
///
/// For bulk indexing, use [`JdkImage`] so the image is extracted only once.
/// This convenience function extracts only the requested entry and always
/// removes its private temporary directory before returning.
pub fn extract_class(
    modules_file: &Path,
    class_name: &str, // e.g., "java/lang/String"
) -> Result<Vec<u8>, JdkError> {
    let relative = class_entry_path(class_name)
        .ok_or_else(|| JdkError::InvalidClassName(class_name.to_owned()))?;
    let modules_file = fs::canonicalize(modules_file).map_err(JdkError::ReadFailed)?;
    let temp_dir = tempfile::Builder::new()
        .prefix("hyperion-jdk-class-")
        .tempdir()
        .map_err(JdkError::ReadFailed)?;
    let include = format!(
        "glob:/java.base/{}",
        relative.to_string_lossy().replace('\\', "/")
    );
    extract_jimage(&modules_file, &include, temp_dir.path())?;
    read_extracted_class(&temp_dir.path().join("java.base"), class_name)
}

fn read_extracted_class(java_base_dir: &Path, class_name: &str) -> Result<Vec<u8>, JdkError> {
    let relative = class_entry_path(class_name)
        .ok_or_else(|| JdkError::InvalidClassName(class_name.to_owned()))?;
    let class_path = java_base_dir.join(relative);
    let file =
        fs::File::open(&class_path).map_err(|_| JdkError::ClassNotFound(class_name.to_owned()))?;
    let declared_size = file.metadata().map_err(JdkError::ReadFailed)?.len();
    read_class_bytes(
        file,
        declared_size,
        format!("JDK class {class_name} from {}", class_path.display()),
    )
    .map_err(JdkError::from)
}

fn extract_jimage(modules_file: &Path, include: &str, destination: &Path) -> Result<(), JdkError> {
    let jimage = get_jimage_tool(modules_file)?;
    let mut stderr = tempfile::tempfile().map_err(JdkError::ReadFailed)?;
    let stderr_sink = stderr.try_clone().map_err(JdkError::ReadFailed)?;
    info!("Extracting {include} from {}", modules_file.display());
    let status = Command::new(&jimage)
        .arg("extract")
        .arg("--dir")
        .arg(destination)
        .arg("--include")
        .arg(include)
        .arg(modules_file)
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_sink))
        .status()?;
    if !status.success() {
        let stderr_len = stderr.metadata().map_err(JdkError::ReadFailed)?.len();
        stderr
            .seek(SeekFrom::Start(0))
            .map_err(JdkError::ReadFailed)?;
        let mut detail = Vec::new();
        (&mut stderr)
            .take(MAX_JIMAGE_ERROR_BYTES)
            .read_to_end(&mut detail)
            .map_err(JdkError::ReadFailed)?;
        return Err(jimage_failure(
            &status,
            &detail,
            stderr_len > MAX_JIMAGE_ERROR_BYTES,
        ));
    }
    Ok(())
}

fn jimage_failure(status: &ExitStatus, stderr: &[u8], truncated: bool) -> JdkError {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        JdkError::ExtractionFailed(status.to_string())
    } else if truncated {
        JdkError::ExtractionFailed(format!("{detail} … (truncated)"))
    } else {
        JdkError::ExtractionFailed(detail.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_java_home() {
        let java_home = find_java_home();
        if java_home.is_err() {
            println!("JAVA_HOME not found - this is expected in CI without JDK");
            return;
        }

        let java_home = java_home.unwrap();
        println!("Found JAVA_HOME: {}", java_home.display());
        assert!(java_home.exists());
        assert!(java_home.is_dir());
    }

    #[test]
    fn test_get_modules_file() {
        let java_home = match find_java_home() {
            Ok(home) => home,
            Err(_) => {
                println!("Skipping test - JAVA_HOME not found");
                return;
            }
        };

        let modules = get_modules_file(&java_home);
        if modules.is_err() {
            println!("Modules file not found - might be Java 8 or older");
            return;
        }

        let modules = modules.unwrap();
        println!("Found modules file: {}", modules.display());
        assert!(modules.exists());
        assert!(modules.is_file());
    }

    #[test]
    fn test_list_javabase_classes() {
        let java_home = find_java_home().expect("JAVA_HOME not found");
        let modules = get_modules_file(&java_home).expect("Modules file not found");

        let classes = list_javabase_classes(&modules).expect("Failed to list classes");

        println!("Found {} classes in java.base", classes.len());
        assert!(!classes.is_empty());

        // Verify some common classes are present
        assert!(classes.iter().any(|c| c == "java/lang/String"));
        assert!(classes.iter().any(|c| c == "java/lang/Object"));
        assert!(classes.iter().any(|c| c == "java/util/List"));
    }

    #[test]
    fn test_extract_string_class() {
        let java_home = find_java_home().expect("JAVA_HOME not found");
        let modules = get_modules_file(&java_home).expect("Modules file not found");

        let bytes =
            extract_class(&modules, "java/lang/String").expect("Failed to extract String.class");

        println!("Extracted String.class: {} bytes", bytes.len());
        assert!(!bytes.is_empty());

        // Verify magic number (0xCAFEBABE)
        assert_eq!(&bytes[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn scoped_jdk_image_removes_its_extraction_on_drop() {
        let java_home = find_java_home().expect("JAVA_HOME not found");
        let modules = get_modules_file(&java_home).expect("Modules file not found");
        let image = JdkImage::open(&modules).expect("Failed to open JDK image");
        let extraction_root = image._temp_dir.path().to_path_buf();

        let bytes = image
            .extract_class("java/lang/String")
            .expect("Failed to extract String.class");
        assert_eq!(&bytes[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
        assert!(extraction_root.exists());

        drop(image);
        assert!(!extraction_root.exists());
    }

    #[test]
    fn extract_class_rejects_path_traversal_before_running_jimage() {
        let error = extract_class(Path::new("missing-modules"), "../outside").unwrap_err();
        assert!(matches!(error, JdkError::InvalidClassName(_)));

        let error = extract_class(Path::new("missing-modules"), "java/../outside").unwrap_err();
        assert!(matches!(error, JdkError::InvalidClassName(_)));
    }
}
