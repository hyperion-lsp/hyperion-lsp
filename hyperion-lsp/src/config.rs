// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Configuration loading for Hyperion tooling.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::diagnostic_engine::{DiagnosticOptions, NullabilityUnknownSeverity};

const CONFIG_FILE_NAME: &str = ".hyperion.toml";
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Config file not found: {path}", path = .path.display())]
    ExplicitFileNotFound { path: PathBuf },
    #[error("Failed to inspect config {path}: {source}", path = .path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to read config {path}: {source}", path = .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse config {path}: {source}", path = .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct HyperionConfig {
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiagnosticsConfig {
    pub typecheck: Option<bool>,
    pub nullability: Option<bool>,
    pub inspections: Option<bool>,
    pub security: Option<bool>,
    /// Also run the inspections that IntelliJ's default profile leaves
    /// disabled (magic numbers, raw type usage). Defaults to false.
    pub opt_in_inspections: Option<bool>,
    /// Severity for null-dereference warnings when nullability is Unknown (unannotated types).
    /// Valid values: "warning", "info", "hint", "off". Defaults to "off";
    /// an unknown platform contract is not evidence that a value is nullable.
    pub nullability_unknown_severity: Option<String>,
}

impl HyperionConfig {
    pub fn diagnostic_options(&self) -> DiagnosticOptions {
        let mut options = DiagnosticOptions::default();
        if let Some(value) = self.diagnostics.typecheck {
            options.run_type_check = value;
        }
        if let Some(value) = self.diagnostics.nullability {
            options.run_nullability = value;
        }
        if let Some(value) = self.diagnostics.inspections {
            options.run_inspections = value;
        }
        if let Some(value) = self.diagnostics.security {
            options.run_security = value;
        }
        if let Some(value) = self.diagnostics.opt_in_inspections {
            options.opt_in_inspections = value;
        }
        if let Some(ref value) = self.diagnostics.nullability_unknown_severity {
            options.nullability_unknown_severity = NullabilityUnknownSeverity::parse(value);
        }
        options
    }
}

pub fn load_config(
    root: &Path,
    explicit_path: Option<&Path>,
) -> Result<HyperionConfig, ConfigError> {
    let config_path = resolve_config_path(root, explicit_path)?;
    let Some(config_path) = config_path else {
        return Ok(HyperionConfig::default());
    };

    let contents = hyperion_workspace::read_utf8_file_bounded(&config_path, MAX_CONFIG_BYTES)
        .map_err(|source| ConfigError::Read {
            path: config_path.clone(),
            source,
        })?;
    toml::from_str(&contents).map_err(|source| ConfigError::Parse {
        path: config_path,
        source,
    })
}

fn resolve_config_path(
    root: &Path,
    explicit_path: Option<&Path>,
) -> Result<Option<PathBuf>, ConfigError> {
    if let Some(path) = explicit_path {
        match path.try_exists() {
            Ok(true) => {}
            Ok(false) => {
                return Err(ConfigError::ExplicitFileNotFound {
                    path: path.to_path_buf(),
                });
            }
            Err(source) => {
                return Err(ConfigError::Inspect {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
        return Ok(Some(path.to_path_buf()));
    }

    let candidate = root.join(CONFIG_FILE_NAME);
    if candidate.exists() {
        Ok(Some(candidate))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_config_defaults_without_file() {
        let temp = TempDir::new().unwrap();
        let config = load_config(temp.path(), None).unwrap();
        let options = config.diagnostic_options();
        assert!(options.run_type_check);
        assert!(options.run_nullability);
        assert!(options.run_inspections);
        assert!(options.run_security);
        assert_eq!(
            options.nullability_unknown_severity,
            NullabilityUnknownSeverity::Off
        );
    }

    #[test]
    fn test_load_config_parses_diagnostics() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(".hyperion.toml");
        fs::write(
            &config_path,
            r"
[diagnostics]
typecheck = false
nullability = true
inspections = false
security = true
",
        )
        .unwrap();

        let config = load_config(temp.path(), None).unwrap();
        let options = config.diagnostic_options();
        assert!(!options.run_type_check);
        assert!(options.run_nullability);
        assert!(!options.run_inspections);
        assert!(options.run_security);
    }

    #[test]
    fn test_load_config_nullability_unknown_severity() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(".hyperion.toml");
        fs::write(
            &config_path,
            r#"
[diagnostics]
nullability_unknown_severity = "hint"
"#,
        )
        .unwrap();

        let config = load_config(temp.path(), None).unwrap();
        let options = config.diagnostic_options();
        assert_eq!(
            options.nullability_unknown_severity,
            NullabilityUnknownSeverity::Hint
        );
    }

    #[test]
    fn test_load_config_nullability_unknown_severity_off() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(".hyperion.toml");
        fs::write(
            &config_path,
            r#"
[diagnostics]
nullability_unknown_severity = "off"
"#,
        )
        .unwrap();

        let config = load_config(temp.path(), None).unwrap();
        let options = config.diagnostic_options();
        assert_eq!(
            options.nullability_unknown_severity,
            NullabilityUnknownSeverity::Off
        );
    }

    #[test]
    fn test_load_config_explicit_path_missing() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("missing.toml");
        let err = load_config(temp.path(), Some(&missing)).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::ExplicitFileNotFound { path } if path == missing
        ));
    }

    #[test]
    fn test_load_config_preserves_parse_error_and_path() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(CONFIG_FILE_NAME);
        fs::write(&config_path, "[diagnostics\ntypecheck = true").unwrap();

        let error = load_config(temp.path(), None).unwrap_err();

        assert!(matches!(
            error,
            ConfigError::Parse { path, source }
                if path == config_path && !source.message().is_empty()
        ));
    }

    #[test]
    fn test_load_config_rejects_oversized_files() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(CONFIG_FILE_NAME);
        fs::write(&config_path, vec![b'x'; MAX_CONFIG_BYTES + 1]).unwrap();

        let error = load_config(temp.path(), None).unwrap_err();

        assert!(matches!(
            error,
            ConfigError::Read { path, source }
                if path == config_path && source.to_string().contains("exceeds")
        ));
    }
}
