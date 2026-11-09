// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Logging context detection for log injection severity adjustments.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_LOGBACK_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_BUILD_METADATA_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingSafety {
    StructuredSafe,
    StructuredUnknown,
    PlainText,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoggingContextSource {
    Override,
    Detected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggingContext {
    safety: LoggingSafety,
    source: LoggingContextSource,
}

impl LoggingContext {
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            safety: LoggingSafety::Unknown,
            source: LoggingContextSource::Unknown,
        }
    }

    #[must_use]
    pub const fn safety(&self) -> LoggingSafety {
        self.safety
    }

    #[must_use]
    pub const fn source(&self) -> LoggingContextSource {
        self.source
    }

    #[must_use]
    pub fn from_root(root: &Path) -> Self {
        if let Some(override_context) = Self::from_override_file(root) {
            return override_context;
        }

        let safety = detect_logback_context(root);
        Self {
            safety,
            source: LoggingContextSource::Detected,
        }
    }

    fn from_override_file(root: &Path) -> Option<Self> {
        let config_path = root.join(".hyperion-security.toml");

        let content =
            hyperion_workspace::read_utf8_file_bounded(&config_path, MAX_CONFIG_BYTES).ok()?;
        let parsed: OverrideConfig = toml::from_str(&content).ok()?;
        let logging = parsed.logging?;
        let mode = logging.mode?;
        let override_enabled = logging.override_mode.unwrap_or(true);
        if !override_enabled {
            return None;
        }

        let safety = match mode.as_str() {
            "structured_safe" => LoggingSafety::StructuredSafe,
            "structured_unknown" => LoggingSafety::StructuredUnknown,
            "plain_text" => LoggingSafety::PlainText,
            "unknown" => LoggingSafety::Unknown,
            _ => return None,
        };

        Some(Self {
            safety,
            source: LoggingContextSource::Override,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OverrideConfig {
    logging: Option<LoggingOverride>,
}

#[derive(Debug, Deserialize)]
struct LoggingOverride {
    mode: Option<String>,
    #[serde(rename = "override")]
    override_mode: Option<bool>,
}

fn detect_logback_context(root: &Path) -> LoggingSafety {
    let mut best_non_test = LoggingSafety::Unknown;
    let mut best_test = LoggingSafety::Unknown;
    let mut found_config = false;

    for path in find_logback_configs(root) {
        if let Ok(content) =
            hyperion_workspace::read_utf8_file_bounded(&path, MAX_LOGBACK_CONFIG_BYTES)
        {
            found_config = true;
            let safety = classify_logback_content(&content);
            if is_test_path(&path) {
                best_test = pick_best_logging(best_test, safety);
            } else {
                best_non_test = pick_best_logging(best_non_test, safety);
            }
        }
    }

    if best_non_test != LoggingSafety::Unknown {
        return best_non_test;
    }
    if best_test != LoggingSafety::Unknown {
        return best_test;
    }

    if found_config {
        return LoggingSafety::Unknown;
    }

    if has_logstash_dependency(root) {
        return LoggingSafety::StructuredSafe;
    }

    LoggingSafety::Unknown
}

fn find_logback_configs(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .max_depth(10)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            #[allow(clippy::case_sensitive_file_extension_comparisons)]
            if name.starts_with("logback") && name.ends_with(".xml") {
                Some(entry.into_path())
            } else {
                None
            }
        })
        .collect()
}

fn has_logstash_dependency(root: &Path) -> bool {
    let candidates = ["build.gradle", "build.gradle.kts", "libs.versions.toml"];

    WalkDir::new(root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| candidates.contains(&name))
        })
        .any(|entry| {
            hyperion_workspace::read_utf8_file_bounded(entry.path(), MAX_BUILD_METADATA_BYTES)
                .is_ok_and(|content| {
                    content.contains("logstash-logback-encoder")
                        || content.contains("logstashLogbackEncoder")
                })
        })
}

fn classify_logback_content(content: &str) -> LoggingSafety {
    if content.contains("<springProfile") {
        return classify_spring_profiles(content);
    }

    if content.contains("net.logstash.logback.encoder.LogstashEncoder") {
        return LoggingSafety::StructuredSafe;
    }

    if contains_pattern_layout(content) {
        return LoggingSafety::PlainText;
    }

    LoggingSafety::Unknown
}

const fn pick_best_logging(current: LoggingSafety, candidate: LoggingSafety) -> LoggingSafety {
    if rank_logging(candidate) > rank_logging(current) {
        candidate
    } else {
        current
    }
}

const fn rank_logging(safety: LoggingSafety) -> u8 {
    match safety {
        LoggingSafety::StructuredSafe => 3,
        LoggingSafety::StructuredUnknown => 2,
        LoggingSafety::PlainText => 1,
        LoggingSafety::Unknown => 0,
    }
}

fn is_test_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    path_str.contains("/src/test/")
        || path_str.contains("/src/tests/")
        || path_str.contains("/test/")
        || path_str.contains("/tests/")
}

fn classify_spring_profiles(content: &str) -> LoggingSafety {
    let mut non_local_logstash = false;
    let mut non_local_plain = false;
    let mut has_profiles = false;

    for (profile_name, block) in extract_profile_blocks(content) {
        has_profiles = true;
        match profile_kind(&profile_name) {
            ProfileKind::Local => continue,
            ProfileKind::NonLocal => {
                if block.contains("net.logstash.logback.encoder.LogstashEncoder") {
                    non_local_logstash = true;
                } else if contains_pattern_layout(&block) {
                    non_local_plain = true;
                }
            }
            ProfileKind::Unknown => {
                if block.contains("net.logstash.logback.encoder.LogstashEncoder") {
                    non_local_logstash = true;
                }
            }
        }
    }

    if !has_profiles {
        return LoggingSafety::Unknown;
    }

    if non_local_plain {
        return LoggingSafety::PlainText;
    }

    if non_local_logstash {
        return LoggingSafety::StructuredSafe;
    }

    LoggingSafety::StructuredUnknown
}

fn contains_pattern_layout(content: &str) -> bool {
    content.contains("CONSOLE_LOG_PATTERN")
        || content.contains("<pattern>")
        || content.contains("PatternLayout")
}

fn extract_profile_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut remainder = content;

    while let Some(start) = remainder.find("<springProfile") {
        let after_start = &remainder[start..];
        let end_tag = "</springProfile>";
        let end_idx = match after_start.find(end_tag) {
            Some(idx) => idx,
            None => break,
        };
        let block = &after_start[..end_idx + end_tag.len()];
        let name = extract_profile_name(block);
        blocks.push((name, block.to_string()));
        remainder = &after_start[end_idx + end_tag.len()..];
    }

    blocks
}

fn extract_profile_name(block: &str) -> String {
    let tag_end = match block.find('>') {
        Some(idx) => idx,
        None => return String::new(),
    };
    let header = &block[..tag_end];
    extract_attribute_value(header, "name").unwrap_or_default()
}

fn extract_attribute_value(header: &str, attr: &str) -> Option<String> {
    let mut search = header;
    let needle = format!("{attr}=");
    let start = search.find(&needle)?;
    search = &search[start + needle.len()..];
    let quote = search.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    search = &search[1..];
    let end = search.find(quote)?;
    Some(search[..end].trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileKind {
    Local,
    NonLocal,
    Unknown,
}

fn profile_kind(name: &str) -> ProfileKind {
    let mut has_local = false;
    let mut has_non_local = false;

    for token in name.split(|c: char| c == ',' || c == '|' || c.is_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if token.starts_with('!') {
            has_non_local = true;
            continue;
        }
        if token.eq_ignore_ascii_case("local") {
            has_local = true;
        } else {
            has_non_local = true;
        }
    }

    if has_non_local {
        ProfileKind::NonLocal
    } else if has_local {
        ProfileKind::Local
    } else {
        ProfileKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_logback_structured_safe_non_local() {
        let dir = tempdir().unwrap();
        let content = r#"
<configuration>
  <springProfile name="local">
    <property name="CONSOLE_LOG_PATTERN" value="%msg%n"/>
  </springProfile>
  <springProfile name="!local">
    <appender name="STDOUT" class="ch.qos.logback.core.ConsoleAppender">
      <encoder class="net.logstash.logback.encoder.LogstashEncoder"/>
    </appender>
  </springProfile>
</configuration>
"#;
        let path = dir.path().join("logback-spring.xml");
        fs::write(&path, content).unwrap();
        let context = LoggingContext::from_root(dir.path());
        assert_eq!(context.safety(), LoggingSafety::StructuredSafe);
    }

    #[test]
    fn test_logback_plain_text() {
        let dir = tempdir().unwrap();
        let content = r#"
<configuration>
  <appender name="CONSOLE" class="ch.qos.logback.core.ConsoleAppender">
    <encoder>
      <pattern>%msg%n</pattern>
    </encoder>
  </appender>
</configuration>
"#;
        let path = dir.path().join("logback.xml");
        fs::write(&path, content).unwrap();
        let context = LoggingContext::from_root(dir.path());
        assert_eq!(context.safety(), LoggingSafety::PlainText);
    }

    #[test]
    fn test_logback_structured_unknown() {
        let dir = tempdir().unwrap();
        let content = r#"
<configuration>
  <springProfile name="local">
    <appender name="STDOUT" class="ch.qos.logback.core.ConsoleAppender">
      <encoder class="net.logstash.logback.encoder.LogstashEncoder"/>
    </appender>
  </springProfile>
</configuration>
"#;
        let path = dir.path().join("logback-spring.xml");
        fs::write(&path, content).unwrap();
        let context = LoggingContext::from_root(dir.path());
        assert_eq!(context.safety(), LoggingSafety::StructuredUnknown);
    }

    #[test]
    fn test_override_file() {
        let dir = tempdir().unwrap();
        let content = r#"
[logging]
mode = "structured_safe"
override = true
"#;
        let path = dir.path().join(".hyperion-security.toml");
        fs::write(&path, content).unwrap();
        let context = LoggingContext::from_root(dir.path());
        assert_eq!(context.safety(), LoggingSafety::StructuredSafe);
        assert_eq!(context.source(), LoggingContextSource::Override);
    }
}
