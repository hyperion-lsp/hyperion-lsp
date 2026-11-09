// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradleDslKind {
    Groovy,
    Kotlin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SniffOutcome {
    Executed,
    RegexFallback,
    Skipped { reason: SniffSkipReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SniffSkipReason {
    NotApplicable,
    Unsupported,
    MissingBytecode { remediation: String },
    ExecutionFailed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradlePluginResolution {
    pub plugin_id: String,
    pub outcome: SniffOutcome,
    pub implementation_class: Option<String>,
    pub classpath_hint: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmExecutionSummary {
    pub duration: Option<Duration>,
    pub instruction_limit_hit: bool,
    pub timed_out: bool,
    pub memory_limit_hit: bool,
    pub stack_limit_hit: bool,
    pub aborted: bool,
}

impl VmExecutionSummary {
    pub fn empty() -> Self {
        Self {
            duration: None,
            instruction_limit_hit: false,
            timed_out: false,
            memory_limit_hit: false,
            stack_limit_hit: false,
            aborted: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GradleSniffReport {
    pub root_dir: PathBuf,
    pub build_file: Option<(PathBuf, GradleDslKind)>,
    pub settings_file: Option<(PathBuf, GradleDslKind)>,
    pub subprojects: Vec<String>,
    pub include_build_roots: Vec<PathBuf>,
    pub version_catalog_path: Option<PathBuf>,
    pub plugin_resolutions: Vec<GradlePluginResolution>,
    pub vm_summary: Option<VmExecutionSummary>,
    pub notes: Vec<String>,
}

impl GradleSniffReport {
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            root_dir,
            build_file: None,
            settings_file: None,
            subprojects: Vec::new(),
            include_build_roots: Vec::new(),
            version_catalog_path: None,
            plugin_resolutions: Vec::new(),
            vm_summary: None,
            notes: Vec::new(),
        }
    }

    pub fn set_build_file(&mut self, path: impl Into<PathBuf>, kind: GradleDslKind) {
        self.build_file = Some((path.into(), kind));
    }

    pub fn set_settings_file(&mut self, path: impl Into<PathBuf>, kind: GradleDslKind) {
        self.settings_file = Some((path.into(), kind));
    }

    pub fn add_note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn add_include_build_root(&mut self, path: impl Into<PathBuf>) {
        self.include_build_roots.push(path.into());
    }

    pub fn set_version_catalog_path(&mut self, path: impl Into<PathBuf>) {
        self.version_catalog_path = Some(path.into());
    }

    pub fn summary(&self) -> String {
        let build = self
            .build_file
            .as_ref()
            .map(|(p, k)| format!("{} ({:?})", rel(self.root_dir.as_path(), p), k))
            .unwrap_or_else(|| "none".to_string());

        let settings = self
            .settings_file
            .as_ref()
            .map(|(p, k)| format!("{} ({:?})", rel(self.root_dir.as_path(), p), k))
            .unwrap_or_else(|| "none".to_string());

        let catalog = self
            .version_catalog_path
            .as_ref()
            .map(|p| rel(self.root_dir.as_path(), p))
            .unwrap_or_else(|| "none".to_string());

        let include_builds = self.include_build_roots.len();
        let subprojects = self.subprojects.len();
        let plugins_attempted = self.plugin_resolutions.len();
        let plugins_executed = self
            .plugin_resolutions
            .iter()
            .filter(|p| matches!(p.outcome, SniffOutcome::Executed))
            .count();

        format!(
            "root={} build={} settings={} subprojects={} includeBuilds={} versionCatalog={} plugins(executed/attempted)={}/{}",
            self.root_dir.display(),
            build,
            settings,
            subprojects,
            include_builds,
            catalog,
            plugins_executed,
            plugins_attempted
        )
    }

    pub fn debug_details(&self) -> String {
        format!("{:#?}", self)
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

impl fmt::Display for SniffOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SniffOutcome::Executed => write!(f, "executed"),
            SniffOutcome::RegexFallback => write!(f, "regex_fallback"),
            SniffOutcome::Skipped { reason } => write!(f, "skipped({})", reason),
        }
    }
}

impl fmt::Display for SniffSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SniffSkipReason::NotApplicable => write!(f, "not_applicable"),
            SniffSkipReason::Unsupported => write!(f, "unsupported"),
            SniffSkipReason::MissingBytecode { remediation } => {
                write!(f, "missing_bytecode: {}", remediation)
            }
            SniffSkipReason::ExecutionFailed { message } => {
                write!(f, "execution_failed: {}", message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_summary_is_readable_and_stableish() {
        let mut report = GradleSniffReport::new(PathBuf::from("/tmp/demo"));
        report.set_build_file("/tmp/demo/build.gradle", GradleDslKind::Groovy);
        report.set_settings_file("/tmp/demo/settings.gradle", GradleDslKind::Groovy);
        report.subprojects = vec!["app".to_string()];
        report.add_include_build_root("/tmp/demo/build-logic");
        report.set_version_catalog_path("/tmp/demo/gradle/libs.versions.toml");
        report.plugin_resolutions.push(GradlePluginResolution {
            plugin_id: "com.example.internal".to_string(),
            outcome: SniffOutcome::Skipped {
                reason: SniffSkipReason::Unsupported,
            },
            implementation_class: None,
            classpath_hint: None,
        });

        let s = report.summary();
        assert!(s.contains("build.gradle"));
        assert!(s.contains("settings.gradle"));
        assert!(s.contains("subprojects=1"));
        assert!(s.contains("includeBuilds=1"));
        assert!(s.contains("versionCatalog=gradle/libs.versions.toml"));
        assert!(s.contains("plugins(executed/attempted)=0/1"));
    }
}
