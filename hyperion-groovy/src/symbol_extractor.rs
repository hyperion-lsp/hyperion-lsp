// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lightweight lexical extraction for Gradle DSL calls.
//!
//! This compatibility API recognizes known Gradle calls line by line. It is
//! intentionally narrower than [`crate::lsp_symbol_extractor`], which traverses
//! the tree-sitter-derived AST for LSP document symbols.

use crate::ast::{Dependency, GradleBlock, MethodArgument};
use crate::gradle_api::{GradleMethod, GRADLE_API};
use std::collections::HashMap;

/// A Gradle symbol extracted from source code
#[derive(Clone, Debug)]
pub struct GradleSymbol {
    /// Symbol name (method call name)
    pub name: String,
    /// The Gradle API method this maps to (if known)
    pub gradle_method: Option<GradleMethod>,
    /// Location in source
    pub line: usize,
    pub column: usize,
    /// Arguments to the method call
    pub args: Vec<MethodArgument>,
    /// Whether this is a known Gradle API method
    pub is_gradle_api: bool,
}

/// Result of symbol extraction
#[derive(Clone, Debug)]
pub struct ExtractionResult {
    /// All method calls found in the source
    pub symbols: Vec<GradleSymbol>,
    /// Top-level Gradle blocks found (dependencies, repositories, tasks, etc.)
    pub blocks: Vec<GradleBlock>,
    /// Dependencies found (parsed from dependency() calls)
    pub dependencies: Vec<Dependency>,
    /// Errors encountered during extraction
    pub errors: Vec<String>,
}

/// Extracts Gradle symbols from source code text
pub struct SymbolExtractor {
    gradle_api: HashMap<String, GradleMethod>,
}

impl SymbolExtractor {
    /// Create a new symbol extractor
    pub fn new() -> Self {
        SymbolExtractor {
            gradle_api: GRADLE_API.clone(),
        }
    }

    /// Extract symbols from Groovy source code
    ///
    /// This lexical path recognizes calls from the shipped Gradle API catalog.
    /// Use [`crate::LspSymbolExtractor`] when CST-aware symbol extraction is
    /// required.
    pub fn extract(&self, source: &str) -> ExtractionResult {
        let mut result = ExtractionResult {
            symbols: Vec::new(),
            blocks: Vec::new(),
            dependencies: Vec::new(),
            errors: Vec::new(),
        };

        // Preserve this API's line-oriented behavior; the AST-based extractor
        // is a separate path with richer Groovy coverage.
        for (line_idx, line) in source.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }

            // Look for method calls (simplified pattern)
            self.extract_method_calls(line, line_num, &mut result);
        }

        result.symbols.sort_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then(left.column.cmp(&right.column))
                .then(left.name.cmp(&right.name))
        });

        result
    }

    /// Extract method calls from a single line
    fn extract_method_calls(&self, line: &str, line_num: usize, result: &mut ExtractionResult) {
        // Pattern: methodName(...) or methodName(arg)
        // Match catalogued method names at identifier boundaries.
        let symbols_before = result.symbols.len();

        for (method_name, gradle_method) in &self.gradle_api {
            // Check for method call with parentheses
            if let Some(pos) = find_identifier_pattern(line, &format!("{}(", method_name)) {
                let symbol = GradleSymbol {
                    name: method_name.clone(),
                    gradle_method: Some(gradle_method.clone()),
                    line: line_num,
                    column: line[..pos].encode_utf16().count(),
                    // Parenthesized argument decoding is outside this lexical API.
                    args: Vec::new(),
                    is_gradle_api: true,
                };
                result.symbols.push(symbol);
            }

            // Check for method call without parentheses (Groovy style)
            // "implementation 'com.example:lib:1.0'"
            let pattern = format!("{} ", method_name);
            if let Some(pos) = find_identifier_pattern(line, &pattern) {
                // A parenthesized call was already emitted above.
                if line[pos + method_name.len()..].starts_with('(') {
                    continue;
                }
                let rest = line[pos + pattern.len()..].trim_start();
                let args = self.parse_arguments(rest);

                let symbol = GradleSymbol {
                    name: method_name.clone(),
                    gradle_method: Some(gradle_method.clone()),
                    line: line_num,
                    column: line[..pos].encode_utf16().count(),
                    args,
                    is_gradle_api: true,
                };
                result.symbols.push(symbol);

                // If this is a dependency method, parse the dependency
                if [
                    "implementation",
                    "testImplementation",
                    "api",
                    "compileOnly",
                    "runtimeOnly",
                    "androidTestImplementation",
                    "debugImplementation",
                    "testRuntimeOnly",
                    "ksp",
                    "classpath",
                ]
                .contains(&method_name.as_str())
                {
                    if let Some(dep) = self.parse_dependency_from_line(line) {
                        result.dependencies.push(dep);
                    }
                }
            }
        }

        // Preserve unknown Groovy/third-party DSL calls for basic hover and
        // navigation. Only use this fallback when no known Gradle API symbol
        // was found on the line, and reject assignment/declaration syntax.
        if result.symbols.len() == symbols_before {
            if let Some((name, byte_column)) = leading_dynamic_call(line) {
                result.symbols.push(GradleSymbol {
                    name,
                    gradle_method: None,
                    line: line_num,
                    column: line[..byte_column].encode_utf16().count(),
                    args: Vec::new(),
                    is_gradle_api: false,
                });
            }
        }
    }

    /// Parse arguments from a method call
    fn parse_arguments(&self, arg_str: &str) -> Vec<MethodArgument> {
        let mut args = Vec::new();

        // Check for string literal (single or double quoted)
        if let Some(rest) = arg_str.strip_prefix('\'') {
            if let Some(end) = rest.find('\'') {
                let content = &rest[..end];
                args.push(MethodArgument::StringLiteral(content.to_string()));
            }
        } else if let Some(rest) = arg_str.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                let content = &rest[..end];
                if content.contains("${") {
                    args.push(MethodArgument::GString(content.to_string()));
                } else {
                    args.push(MethodArgument::StringLiteral(content.to_string()));
                }
            }
        }

        args
    }

    /// Parse dependency from a line like: implementation 'com.example:lib:1.0'
    fn parse_dependency_from_line(&self, line: &str) -> Option<Dependency> {
        // Extract quoted string
        if let Some(start) = line.find('\'') {
            if let Some(end) = line[start + 1..].find('\'') {
                let dep_str = &line[start + 1..start + 1 + end];
                return Dependency::from_string(dep_str);
            }
        } else if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                let dep_str = &line[start + 1..start + 1 + end];
                // Check if it's a GString with interpolation
                if !dep_str.contains("${") {
                    return Dependency::from_string(dep_str);
                }
            }
        }

        None
    }
}

fn find_identifier_pattern(line: &str, pattern: &str) -> Option<usize> {
    line.match_indices(pattern).find_map(|(position, _)| {
        let boundary = position == 0
            || !line[..position]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '_');
        (boundary && is_code_position(line, position)).then_some(position)
    })
}

fn is_code_position(line: &str, position: usize) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut chars = line[..position].chars().peekable();
    while let Some(character) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '/' && chars.peek() == Some(&'/') {
            return false;
        }
    }
    quote.is_none()
}

fn leading_dynamic_call(line: &str) -> Option<(String, usize)> {
    let byte_column = line.len() - line.trim_start().len();
    let trimmed = &line[byte_column..];
    let name_end = trimmed
        .char_indices()
        .take_while(|(_, character)| character.is_alphanumeric() || *character == '_')
        .map(|(offset, character)| offset + character.len_utf8())
        .last()?;
    let name = &trimmed[..name_end];
    if matches!(
        name,
        "def" | "class" | "interface" | "enum" | "if" | "for" | "while" | "return" | "import"
    ) {
        return None;
    }

    let rest = trimmed[name_end..].trim_start();
    let invocation = rest.is_empty()
        || rest.starts_with(['(', '\'', '"', '{'])
        || rest
            .chars()
            .next()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
    invocation.then(|| (name.to_string(), byte_column))
}

impl Default for SymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_simple_dependency() {
        let extractor = SymbolExtractor::new();
        let source = r#"
            dependencies {
                implementation 'junit:junit:4.13'
            }
        "#;
        let result = extractor.extract(source);
        assert!(!result.symbols.is_empty());
    }

    #[test]
    fn parse_gradle_method_with_parens() {
        let extractor = SymbolExtractor::new();
        let source = "mavenCentral()";
        let result = extractor.extract(source);
        assert!(result.symbols.iter().any(|s| s.name == "mavenCentral"));
    }

    #[test]
    fn parse_dependency_string() {
        let extractor = SymbolExtractor::new();
        let dep = extractor
            .parse_dependency_from_line("implementation 'com.google.guava:guava:31.1-jre'");
        assert!(dep.is_some());
        let dep = dep.unwrap();
        assert_eq!(dep.group, "com.google.guava");
        assert_eq!(dep.artifact, "guava");
        assert_eq!(dep.version, "31.1-jre");
    }

    #[test]
    fn recognize_gradle_api_method() {
        let extractor = SymbolExtractor::new();
        let source = "implementation 'junit:junit:4.13'";
        let result = extractor.extract(source);
        let impl_symbol = result.symbols.iter().find(|s| s.name == "implementation");
        assert!(impl_symbol.is_some());
        let impl_symbol = impl_symbol.unwrap();
        assert!(impl_symbol.is_gradle_api);
        assert!(impl_symbol.gradle_method.is_some());
    }

    #[test]
    fn preserves_indented_utf16_columns() {
        let extractor = SymbolExtractor::new();
        let result = extractor.extract("    implementation 'junit:junit:4.13'");
        let symbol = result.symbols.first().expect("implementation symbol");
        assert_eq!(symbol.column, 4);
    }

    #[test]
    fn ignores_known_gradle_names_inside_strings_and_comments() {
        let extractor = SymbolExtractor::new();
        let result = extractor
            .extract("println \"implementation fake\" // mavenCentral() and api 'fake:fake:1'");
        assert_eq!(
            result
                .symbols
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["println"]
        );
        assert!(!result.symbols[0].is_gradle_api);
    }
}
