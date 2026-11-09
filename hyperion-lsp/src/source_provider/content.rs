// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Source Content Types
//!
//! Defines different types of source content that can be provided.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tower_lsp::lsp_types::Url;

/// Immutable source text and its lazily derived Java symbol locations.
///
/// Providers and navigation responses share this object. Parsing failures are
/// cached as well, preventing a malformed attachment from being reparsed on
/// every navigation request.
#[derive(Debug)]
pub struct SourceFile {
    content: Arc<str>,
    java_symbols: OnceLock<Result<Vec<SourceSymbol>, Arc<str>>>,
    kotlin_symbols: OnceLock<Result<Vec<SourceSymbol>, Arc<str>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceLanguage {
    Java,
    Kotlin,
}

#[derive(Debug)]
pub(crate) struct SourceSymbol {
    pub(crate) fqn: Arc<str>,
    pub(crate) range: hyperion_workspace::TextRange,
}

impl SourceFile {
    pub const fn new(content: Arc<str>) -> Self {
        Self {
            content,
            java_symbols: OnceLock::new(),
            kotlin_symbols: OnceLock::new(),
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn java_symbols(&self) -> Result<&[SourceSymbol], &str> {
        match self.java_symbols.get_or_init(|| {
            hyperion_workspace::symbol_extractor::SymbolExtractor::extract(&self.content)
                .map(|extraction| {
                    extraction
                        .symbols
                        .into_iter()
                        .map(|symbol| SourceSymbol {
                            fqn: symbol.fqn,
                            range: symbol.range,
                        })
                        .collect()
                })
                .map_err(|error| Arc::from(error.to_string()))
        }) {
            Ok(symbols) => Ok(symbols),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn kotlin_symbols(&self) -> Result<&[SourceSymbol], &str> {
        match self.kotlin_symbols.get_or_init(|| {
            hyperion_kotlin::SymbolExtractor::extract(&self.content)
                .map(|extraction| {
                    extraction
                        .symbols
                        .into_iter()
                        .filter(|symbol| !symbol.is_synthetic)
                        .map(|symbol| {
                            let fqn = if matches!(
                                symbol.kind,
                                tower_lsp::lsp_types::SymbolKind::FUNCTION
                                    | tower_lsp::lsp_types::SymbolKind::METHOD
                            ) {
                                let parameters = symbol
                                    .parameters
                                    .iter()
                                    .map(|parameter| kotlin_parameter_type(parameter))
                                    .collect::<Vec<_>>()
                                    .join(",");
                                Arc::from(format!("{}({parameters})", symbol.fqn))
                            } else {
                                Arc::from(symbol.fqn)
                            };
                            SourceSymbol {
                                fqn,
                                range: hyperion_workspace::TextRange::new(
                                    symbol.range.start,
                                    symbol.range.end,
                                ),
                            }
                        })
                        .collect()
                })
                .map_err(|error| Arc::from(error.to_string()))
        }) {
            Ok(symbols) => Ok(symbols),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let retained_symbols = |result: &Result<Vec<SourceSymbol>, Arc<str>>| match result {
            Ok(symbols) => {
                symbols
                    .capacity()
                    .saturating_mul(std::mem::size_of::<SourceSymbol>())
                    + symbols.iter().map(|symbol| symbol.fqn.len()).sum::<usize>()
            }
            Err(error) => error.len(),
        };
        let derived_bytes = self
            .java_symbols
            .get()
            .map_or(0, retained_symbols)
            .saturating_add(self.kotlin_symbols.get().map_or(0, retained_symbols));
        self.content.len().saturating_add(derived_bytes)
    }
}

fn kotlin_parameter_type(parameter: &str) -> String {
    let declared = parameter
        .rsplit_once(char::is_whitespace)
        .map_or(parameter, |(declared, _name)| declared)
        .trim();
    declared
        .strip_prefix("vararg ")
        .map_or_else(|| declared.to_string(), |element| format!("{element}..."))
}

/// Source content with origin metadata
#[derive(Debug, Clone)]
pub enum SourceContent {
    /// Source extracted from archive (src.zip, -sources.jar)
    Archive {
        source: Arc<SourceFile>,
        archive_path: PathBuf,
        entry_path: String,
        /// Extracted to this temp file path
        temp_file: PathBuf,
    },
    /// Decompiled from bytecode
    Decompiled {
        source: Arc<SourceFile>,
        class_file: PathBuf,
        temp_file: PathBuf,
    },
    /// Generated stub from indexed signatures
    Stub {
        source: Arc<SourceFile>,
        temp_file: PathBuf,
    },
}

impl SourceContent {
    /// Get the source text
    pub fn content(&self) -> &str {
        match self {
            Self::Archive { source, .. }
            | Self::Decompiled { source, .. }
            | Self::Stub { source, .. } => source.content(),
        }
    }

    pub(crate) fn java_symbols(&self) -> Result<&[SourceSymbol], &str> {
        match self {
            Self::Archive { source, .. }
            | Self::Decompiled { source, .. }
            | Self::Stub { source, .. } => source.java_symbols(),
        }
    }

    pub(crate) fn symbols(&self) -> Result<&[SourceSymbol], &str> {
        match self.language() {
            SourceLanguage::Java => self.java_symbols(),
            SourceLanguage::Kotlin => match self {
                Self::Archive { source, .. }
                | Self::Decompiled { source, .. }
                | Self::Stub { source, .. } => source.kotlin_symbols(),
            },
        }
    }

    /// Language of the actual materialized source, independent of the language
    /// of the document that initiated navigation. Current archive providers
    /// produce Java; the extension check also keeps explicit Kotlin providers
    /// and test stubs on the Kotlin parser path.
    pub(crate) fn language(&self) -> SourceLanguage {
        let path = match self {
            Self::Archive { entry_path, .. } => Path::new(entry_path),
            Self::Decompiled { temp_file, .. } | Self::Stub { temp_file, .. } => temp_file,
        };
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("kt"))
        {
            SourceLanguage::Kotlin
        } else {
            SourceLanguage::Java
        }
    }

    /// Get the temp file path for this source
    pub const fn temp_file(&self) -> &PathBuf {
        match self {
            Self::Archive { temp_file, .. } => temp_file,
            Self::Decompiled { temp_file, .. } => temp_file,
            Self::Stub { temp_file, .. } => temp_file,
        }
    }

    /// Get a file:// URI for LSP
    pub fn uri(&self) -> Result<Url, String> {
        Url::from_file_path(self.temp_file()).map_err(|()| {
            format!(
                "Failed to convert path to URI: {}",
                self.temp_file().display()
            )
        })
    }

    /// Whether this is "real" source (archive) vs synthetic (decompiled/stub)
    pub const fn is_real_source(&self) -> bool {
        matches!(self, Self::Archive { .. })
    }

    /// Get a human-readable description of the source origin
    pub fn description(&self) -> String {
        match self {
            Self::Archive {
                archive_path,
                entry_path,
                ..
            } => {
                format!("from {}#{}", path_name(archive_path), entry_path)
            }
            Self::Decompiled { class_file, .. } => {
                format!("decompiled from {}", path_name(class_file))
            }
            Self::Stub { .. } => "generated stub".to_string(),
        }
    }
}

fn path_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_handle_paths_without_file_names() {
        let archive = SourceContent::Archive {
            source: Arc::new(SourceFile::new(Arc::from(""))),
            archive_path: PathBuf::from("/"),
            entry_path: "java/lang/Object.java".to_owned(),
            temp_file: PathBuf::from("/tmp/Object.java"),
        };
        let decompiled = SourceContent::Decompiled {
            source: Arc::new(SourceFile::new(Arc::from(""))),
            class_file: PathBuf::from("/"),
            temp_file: PathBuf::from("/tmp/Object.java"),
        };

        assert_eq!(archive.description(), "from /#java/lang/Object.java");
        assert_eq!(decompiled.description(), "decompiled from /");
    }

    #[test]
    fn attachment_language_comes_from_the_source_origin() {
        let source = Arc::new(SourceFile::new(Arc::from(
            "package dep\nclass Api { fun ping(value: String) {} }",
        )));
        let kotlin = SourceContent::Archive {
            source,
            archive_path: PathBuf::from("/dependencies/api-sources.jar"),
            entry_path: "dep/Api.kt".to_string(),
            temp_file: PathBuf::from("/tmp/content-addressed-file"),
        };
        assert_eq!(kotlin.language(), SourceLanguage::Kotlin);
        let symbols = kotlin.symbols().expect("Kotlin symbols");
        assert!(symbols
            .iter()
            .any(|symbol| symbol.fqn.as_ref() == "dep.Api.ping(String)"));

        let java = SourceContent::Stub {
            source: Arc::new(SourceFile::new(Arc::from(
                "package dep; class Api { void ping() {} }",
            ))),
            temp_file: PathBuf::from("/tmp/Api.java"),
        };
        assert_eq!(java.language(), SourceLanguage::Java);
        assert!(java
            .symbols()
            .expect("Java symbols")
            .iter()
            .any(|symbol| symbol.fqn.as_ref() == "dep.Api.ping()"));
    }
}
