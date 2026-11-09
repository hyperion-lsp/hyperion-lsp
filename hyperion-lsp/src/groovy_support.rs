// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle DSL support for Hyperion LSP
//!
//! This module integrates hyperion-groovy with the LSP server to provide
//! Gradle build file analysis including hover, goto-definition, and completion.

use hyperion_groovy::{GradleSymbol, SymbolExtractor, GRADLE_API};
use lsp_types::{Position, Range};
use tracing::{debug, trace};

/// Detects if a file is a Gradle build file
pub fn is_gradle_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    // Exclude files in src/ directories - they're source code, not build scripts
    if path_lower.contains("/src/") || path_lower.starts_with("src/") {
        return false;
    }
    path_lower.ends_with("build.gradle")
        || path_lower.ends_with("settings.gradle")
        || path_lower.ends_with(".gradle")
}

/// Gradle symbol information for LSP operations
#[derive(Clone, Debug)]
pub struct GradleSymbolInfo {
    /// The underlying Gradle symbol
    pub symbol: GradleSymbol,
    /// LSP-friendly range for this symbol
    pub range: Range,
}

/// Result of extracting Gradle symbols from content
#[derive(Clone, Debug)]
pub struct GradleExtractionResult {
    /// Extracted symbols with LSP ranges
    pub symbols: Vec<GradleSymbolInfo>,
    /// Errors encountered during extraction
    pub errors: Vec<String>,
}

/// Extracts Gradle symbols from file content
pub struct GradleSymbolExtractor {
    extractor: SymbolExtractor,
}

impl GradleSymbolExtractor {
    /// Create a new Gradle symbol extractor
    pub fn new() -> Self {
        Self {
            extractor: SymbolExtractor::new(),
        }
    }

    /// Extract symbols from Gradle source code
    pub fn extract(&self, content: &str) -> GradleExtractionResult {
        trace!("Extracting Gradle symbols from {} bytes", content.len());

        let result = self.extractor.extract(content);

        let symbols: Vec<GradleSymbolInfo> = result
            .symbols
            .into_iter()
            .map(|symbol| {
                // LSP `character` is in UTF-16 code units, not UTF-8 bytes.
                // `symbol.name.len()` would over-count for any non-ASCII identifier
                // and produce wrong end columns.
                let name_utf16 = symbol.name.encode_utf16().count();
                let range = Range {
                    start: Position {
                        line: (symbol.line.saturating_sub(1)) as u32,
                        character: symbol.column as u32,
                    },
                    end: Position {
                        line: (symbol.line.saturating_sub(1)) as u32,
                        character: (symbol.column + name_utf16) as u32,
                    },
                };

                GradleSymbolInfo { symbol, range }
            })
            .collect();

        debug!("Extracted {} Gradle symbols", symbols.len());

        GradleExtractionResult {
            symbols,
            errors: result.errors,
        }
    }

    /// Find symbol at a specific position
    pub fn symbol_at(
        &self,
        symbols: &[GradleSymbolInfo],
        position: Position,
    ) -> Option<GradleSymbolInfo> {
        symbols
            .iter()
            .find(|sym| {
                let start_line = sym.range.start.line;
                let end_line = sym.range.end.line;
                let start_char = sym.range.start.character;
                let end_char = sym.range.end.character;

                if position.line < start_line || position.line > end_line {
                    return false;
                }

                if position.line == start_line && position.line == end_line {
                    return position.character >= start_char && position.character <= end_char;
                }

                if position.line == start_line {
                    return position.character >= start_char;
                }

                if position.line == end_line {
                    return position.character <= end_char;
                }

                true
            })
            .cloned()
    }
}

impl Default for GradleSymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Format hover information for a Gradle method
pub fn format_hover_text(sym_info: &GradleSymbolInfo) -> String {
    match &sym_info.symbol.gradle_method {
        None => format!("```gradle\n{}\n```", sym_info.symbol.name),
        Some(method) => {
            let params = if method.signature.params.is_empty() {
                "(no parameters)".to_string()
            } else {
                method.signature.params.join(", ")
            };

            let return_type = method
                .signature
                .return_type
                .as_ref()
                .map(|t| format!(" → {t}"))
                .unwrap_or_default();

            format!(
                "```gradle\n{}({}){}\n```\n\n{}\n\n**Block:** {}",
                method.signature.name, params, return_type, method.description, method.block,
            )
        }
    }
}

/// Check if a method is a dependency method
pub fn is_dependency_method(name: &str) -> bool {
    matches!(
        name,
        "implementation"
            | "testImplementation"
            | "api"
            | "compileOnly"
            | "runtimeOnly"
            | "annotationProcessor"
    )
}

/// Check if a method is a repository method
pub fn is_repository_method(name: &str) -> bool {
    matches!(name, "mavenCentral" | "mavenLocal" | "maven")
}

/// Get Gradle methods for a specific block
pub fn methods_for_block(block: &str) -> Vec<String> {
    GRADLE_API
        .iter()
        .filter(|(_, method)| method.block == block || method.block == "root")
        .map(|(name, _)| name.clone())
        .collect()
}

/// Create a completion item for a Gradle method
pub fn method_to_completion(name: &str) -> Option<(String, String)> {
    GRADLE_API.get(name).map(|method| {
        let label = format!("{} - {}", name, method.block);
        let detail = method.description.clone();
        (label, detail)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gradle_files() {
        assert!(is_gradle_file("build.gradle"));
        assert!(is_gradle_file("/project/build.gradle"));
        assert!(is_gradle_file("settings.gradle"));
        assert!(is_gradle_file("custom.gradle"));
        assert!(!is_gradle_file("build.xml"));
        assert!(!is_gradle_file("src/main/java/build.gradle")); // Common false positive
    }

    #[test]
    fn extract_and_find_symbols() {
        let extractor = GradleSymbolExtractor::new();
        let content = r"
dependencies {
    implementation 'junit:junit:4.13'
    testImplementation 'org.mockito:mockito-core:4.0'
}

repositories {
    mavenCentral()
}
";

        let result = extractor.extract(content);
        assert!(result.symbols.len() >= 3);

        // Find implementation method
        let impl_sym = result
            .symbols
            .iter()
            .find(|s| s.symbol.name == "implementation");
        assert!(impl_sym.is_some());
    }

    #[test]
    fn symbol_at_position() {
        let extractor = GradleSymbolExtractor::new();
        let content = "implementation 'junit:junit:4.13'";

        let result = extractor.extract(content);
        let position = Position {
            line: 0,
            character: 5, // Somewhere in "implementation"
        };

        let found = extractor.symbol_at(&result.symbols, position);
        assert!(found.is_some());
        assert_eq!(found.unwrap().symbol.name, "implementation");
    }

    #[test]
    fn format_gradle_hover() {
        let extractor = GradleSymbolExtractor::new();
        let result = extractor.extract("implementation 'junit:junit:4.13'");

        if let Some(sym) = result.symbols.first() {
            let hover = format_hover_text(sym);
            assert!(hover.contains("implementation"));
            assert!(hover.contains("Block"));
        }
    }

    #[test]
    fn dependency_method_detection() {
        assert!(is_dependency_method("implementation"));
        assert!(is_dependency_method("testImplementation"));
        assert!(is_dependency_method("api"));
        assert!(is_dependency_method("compileOnly"));
        assert!(!is_dependency_method("mavenCentral"));
    }

    #[test]
    fn repository_method_detection() {
        assert!(is_repository_method("mavenCentral"));
        assert!(is_repository_method("mavenLocal"));
        assert!(!is_repository_method("implementation"));
    }

    #[test]
    fn methods_for_dependencies_block() {
        let methods = methods_for_block("dependencies");
        assert!(methods.contains(&"implementation".to_string()));
        assert!(methods.contains(&"testImplementation".to_string()));
    }

    #[test]
    fn extract_range_end_invariant_is_utf16_units() {
        // Invariant: the range end for an extracted symbol is `column + utf16_units(name)`.
        // The previous implementation used `column + name.len()` (UTF-8 bytes), which would
        // drift on any non-ASCII identifier. ASCII-only Gradle DSL doesn't trigger drift,
        // but the invariant must hold regardless. Verify against every symbol the extractor
        // produces; the extractor uses the same UTF-16 helper internally.
        let extractor = GradleSymbolExtractor::new();
        let content =
            "dependencies {\n    implementation 'a:b:1'\n    testImplementation 'a:b:1'\n}\n";
        let result = extractor.extract(content);

        assert!(!result.symbols.is_empty(), "expected some symbols");
        for sym in &result.symbols {
            let width = sym.range.end.character - sym.range.start.character;
            let expected = sym.symbol.name.encode_utf16().count() as u32;
            assert_eq!(
                width, expected,
                "symbol '{}': end - start ({}) must equal UTF-16 unit count ({})",
                sym.symbol.name, width, expected
            );
        }
    }
}
