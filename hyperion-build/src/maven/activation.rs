// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven profile activation evaluation.
//!
//! This module evaluates Maven profile activation conditions to determine
//! which profiles should be active based on the current environment.

use super::model::{MavenProfile, ProfileActivation};
use crate::io::read_utf8_file_bounded;
use std::collections::HashMap;
use std::path::Path;
use tracing::debug;

const MAX_JDK_RELEASE_BYTES: usize = 1024 * 1024;

/// Context for evaluating profile activation conditions
#[derive(Debug, Clone)]
pub struct ProfileActivationContext {
    /// Java version (e.g., "17.0.8", "11", "1.8")
    pub java_version: String,

    /// Operating system name (e.g., "Linux", "Windows", "Mac OS X")
    pub os_name: String,

    /// Operating system architecture (e.g., "x86_64", "aarch64")
    pub os_arch: String,

    /// Operating system version, as the JVM reports it in `os.version`.
    /// Empty when it cannot be determined on this platform.
    pub os_version: String,

    /// System and user properties
    pub properties: HashMap<String, String>,

    /// Base directory for file existence checks
    pub base_dir: std::path::PathBuf,
}

impl ProfileActivationContext {
    /// Create a new activation context with current system information
    pub fn new(base_dir: std::path::PathBuf) -> Self {
        let java_version = detect_java_version();
        let os_name = normalize_os_name(std::env::consts::OS);
        let os_arch = std::env::consts::ARCH.to_string();
        let os_version = detect_os_version();

        Self {
            java_version,
            os_name,
            os_arch,
            os_version,
            properties: HashMap::new(),
            base_dir,
        }
    }

    /// Set a property value
    pub fn set_property(&mut self, key: String, value: String) {
        self.properties.insert(key, value);
    }

    /// Get a property value
    pub fn get_property(&self, key: &str) -> Option<&String> {
        self.properties.get(key)
    }
}

/// Detect Java version from environment
fn detect_java_version() -> String {
    // Try JAVA_HOME first
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        if let Some(version) = extract_java_version_from_home(&java_home) {
            return version;
        }
    }

    // Try java.version system property
    if let Ok(version) = std::env::var("java.version") {
        return version;
    }

    // Try JAVA_VERSION env var
    if let Ok(version) = std::env::var("JAVA_VERSION") {
        return version;
    }

    // Try running `java -version`
    if let Some(version) = detect_java_version_from_command() {
        return version;
    }

    // Default fallback
    debug!("Could not detect Java version, defaulting to 17");
    "17".to_string()
}

/// Extract Java version from JAVA_HOME/release file
fn extract_java_version_from_home(java_home: &str) -> Option<String> {
    let release_file = Path::new(java_home).join("release");

    if let Ok(content) = read_utf8_file_bounded(&release_file, MAX_JDK_RELEASE_BYTES) {
        // Parse release file for JAVA_VERSION="17.0.8"
        for line in content.lines() {
            if line.starts_with("JAVA_VERSION=") {
                let version = line
                    .trim_start_matches("JAVA_VERSION=")
                    .trim_matches('"')
                    .to_string();
                debug!("Detected Java version from JAVA_HOME/release: {}", version);
                return Some(version);
            }
        }
    }

    None
}

/// Detect Java version by running `java -version`
fn detect_java_version_from_command() -> Option<String> {
    use std::process::Command;

    if let Ok(output) = Command::new("java").arg("-version").output() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Parse output like: openjdk version "17.0.8" 2023-07-18
        for line in stderr.lines() {
            if line.contains("version") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        let version = line[start + 1..start + 1 + end].to_string();
                        debug!("Detected Java version from java -version: {}", version);
                        return Some(version);
                    }
                }
            }
        }
    }

    None
}

/// Normalize OS name to match Maven conventions
/// Kernel release, which is what the JVM reports as `os.version` on Linux.
///
/// Other platforms return an empty string. The standard library does not expose
/// their `os.version`, and an unknown version never satisfies a positive
/// `<version>` condition.
fn detect_os_version() -> String {
    if std::env::consts::OS != "linux" {
        return String::new();
    }
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|release| release.trim().to_string())
        .unwrap_or_default()
}

fn normalize_os_name(os: &str) -> String {
    match os {
        "macos" => "Mac OS X".to_string(),
        "linux" => "Linux".to_string(),
        "windows" => "Windows".to_string(),
        other => other.to_string(),
    }
}

/// Evaluates Maven profile activation conditions
pub struct ProfileEvaluator;

impl ProfileEvaluator {
    /// Evaluate if a profile should be activated based on the context
    pub fn evaluate(profile: &MavenProfile, ctx: &ProfileActivationContext) -> bool {
        Self::evaluate_activation(&profile.activation, ctx)
    }

    /// Evaluate a specific activation condition
    fn evaluate_activation(activation: &ProfileActivation, ctx: &ProfileActivationContext) -> bool {
        match activation {
            ProfileActivation::Always => true,

            ProfileActivation::ByProperty { name, value } => {
                Self::evaluate_property(name, value.as_deref(), ctx)
            }

            ProfileActivation::ByOS {
                name,
                arch,
                version,
            } => Self::evaluate_os(name.as_deref(), arch.as_deref(), version.as_deref(), ctx),

            ProfileActivation::ByJDK(version_spec) => Self::evaluate_jdk(version_spec, ctx),

            ProfileActivation::ByFile { exists, missing } => {
                Self::evaluate_file(exists.as_deref(), missing.as_deref(), ctx)
            }

            ProfileActivation::All(conditions) => conditions
                .iter()
                .all(|cond| Self::evaluate_activation(cond, ctx)),
        }
    }

    /// Evaluate property-based activation
    fn evaluate_property(name: &str, value: Option<&str>, ctx: &ProfileActivationContext) -> bool {
        match ctx.get_property(name) {
            Some(prop_value) => {
                if let Some(expected_value) = value {
                    // Property must match specific value
                    prop_value == expected_value
                } else {
                    // Property just needs to exist
                    true
                }
            }
            None => {
                // Property doesn't exist - profile does NOT activate
                false
            }
        }
    }

    /// Evaluate OS-based activation
    fn evaluate_os(
        name: Option<&str>,
        arch: Option<&str>,
        version: Option<&str>,
        ctx: &ProfileActivationContext,
    ) -> bool {
        let name_matches = if let Some(expected_name) = name {
            Self::os_name_matches(&ctx.os_name, expected_name)
        } else {
            true
        };

        let arch_matches = if let Some(expected_arch) = arch {
            Self::os_arch_matches(&ctx.os_arch, expected_arch)
        } else {
            true
        };

        let version_matches =
            version.is_none_or(|expected| Self::os_version_matches(&ctx.os_version, expected));

        name_matches && arch_matches && version_matches
    }

    /// Check an OS version the way Maven does: a literal comparison, with a leading
    /// `!` inverting the result. Maven does not treat the value as a pattern.
    ///
    /// Source: `OperatingSystemProfileActivator` in Maven 3.9.16,
    /// <https://maven.apache.org/ref/3.9.16/xref/org/apache/maven/profiles/activation/OperatingSystemProfileActivator.html>
    fn os_version_matches(actual: &str, expected: &str) -> bool {
        match expected.strip_prefix('!') {
            Some(negated) => actual != negated,
            None => !actual.is_empty() && actual == expected,
        }
    }

    /// Check if OS name matches (handles common variations)
    fn os_name_matches(actual: &str, expected: &str) -> bool {
        let actual_lower = actual.to_lowercase();
        let expected_lower = expected.to_lowercase();

        // Exact match
        if actual_lower == expected_lower {
            return true;
        }

        // Common aliases
        if (expected_lower.contains("windows") && actual_lower == "windows")
            || (expected_lower.contains("linux") && actual_lower == "linux")
            || (expected_lower.contains("mac") && actual_lower == "macos")
            || (expected_lower.contains("mac os x") && actual_lower == "macos")
        {
            return true;
        }

        false
    }

    /// Check if OS architecture matches
    fn os_arch_matches(actual: &str, expected: &str) -> bool {
        let actual_lower = actual.to_lowercase();
        let expected_lower = expected.to_lowercase();

        // Exact match
        if actual_lower == expected_lower {
            return true;
        }

        // Common aliases (amd64 = x86_64)
        if (expected_lower == "amd64" && actual_lower == "x86_64")
            || (expected_lower == "x86_64" && actual_lower == "amd64")
        {
            return true;
        }

        false
    }

    /// Evaluate JDK version-based activation
    fn evaluate_jdk(version_spec: &str, ctx: &ProfileActivationContext) -> bool {
        let version_spec = version_spec.trim();

        // Handle version ranges: [min,max), (min,max], [min,max], (min,max)
        if version_spec.starts_with('[') || version_spec.starts_with('(') {
            return Self::evaluate_version_range(version_spec, &ctx.java_version);
        }

        // Handle prefix matching (e.g., "11" matches "11.0.8")
        if version_spec.contains('.') {
            ctx.java_version.starts_with(version_spec)
        } else {
            // Simple major version match
            let actual_major = Self::extract_major_version(&ctx.java_version);
            let expected_major = version_spec.parse::<u32>().unwrap_or(0);
            actual_major == expected_major
        }
    }

    /// Extract major version from Java version string
    fn extract_major_version(version: &str) -> u32 {
        // Handle "1.8" style versions
        if version.starts_with("1.") {
            version
                .split('.')
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        } else {
            // Handle "11", "17", "21" style versions
            version
                .split('.')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        }
    }

    /// Evaluate version range (e.g., "[11,17)", "(11,)", "[11,]")
    fn evaluate_version_range(range: &str, actual_version: &str) -> bool {
        let range = range.trim();

        // Parse range boundaries
        let inclusive_min = range.starts_with('[');
        let inclusive_max = range.ends_with(']');

        let inner = &range[1..range.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();

        if parts.len() != 2 {
            debug!("Invalid version range format: {}", range);
            return false;
        }

        let min = parts[0].trim();
        let max = parts[1].trim();

        let actual_ver = Self::extract_major_version(actual_version);

        // Check minimum bound
        if !min.is_empty() {
            let min_ver = min.parse::<u32>().unwrap_or(0);
            if inclusive_min {
                if actual_ver < min_ver {
                    return false;
                }
            } else if actual_ver <= min_ver {
                return false;
            }
        }

        // Check maximum bound
        if !max.is_empty() {
            let max_ver = max.parse::<u32>().unwrap_or(u32::MAX);
            if inclusive_max {
                if actual_ver > max_ver {
                    return false;
                }
            } else if actual_ver >= max_ver {
                return false;
            }
        }

        true
    }

    /// Evaluate file-based activation
    fn evaluate_file(
        exists: Option<&Path>,
        missing: Option<&Path>,
        ctx: &ProfileActivationContext,
    ) -> bool {
        let exists_matches = if let Some(path) = exists {
            let full_path = ctx.base_dir.join(path);
            full_path.exists()
        } else {
            true
        };

        let missing_matches = if let Some(path) = missing {
            let full_path = ctx.base_dir.join(path);
            !full_path.exists()
        } else {
            true
        };

        exists_matches && missing_matches
    }
}
