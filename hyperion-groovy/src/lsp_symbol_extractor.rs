// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! AST-based symbol extractor for Groovy files
//!
//! Extracts:
//! - Package declarations
//! - Import statements
//! - Classes, interfaces, traits, enums
//! - Methods and fields
//! - Top-level functions and properties

use crate::tree_sitter_wrapper::GroovyParser;
pub use hyperion_types::{
    DeclarationImports as ImportMap, DeclarationModifiers as Modifiers, TextRange,
};
use lsp_types::SymbolKind;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// A symbol extracted from Groovy source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Fully qualified name
    pub fqn: String,
    /// Simple name
    pub simple_name: String,
    /// Location in source
    pub range: TextRange,
    /// Kind of symbol
    pub kind: SymbolKind,
    /// Modifiers
    pub modifiers: Modifiers,
    /// Container FQN
    pub container: Option<String>,
    /// Return type for methods
    pub return_type: Option<String>,
    /// Parameter types for methods
    pub parameters: Vec<String>,
    /// Parent class FQN
    pub parent_class: Option<String>,
    /// Implemented interfaces
    pub interfaces: Vec<String>,
    /// Documentation
    pub documentation: Option<String>,
}

/// Result of symbol extraction
#[derive(Debug, Clone, Default)]
pub struct ExtractionResult {
    /// Package name
    pub package: Option<String>,
    /// Import map
    pub imports: ImportMap,
    /// Extracted symbols
    pub symbols: Vec<Symbol>,
}

/// Extracts LSP-compatible symbols from Groovy source
pub struct LspSymbolExtractor;

impl LspSymbolExtractor {
    /// Extract symbols from Groovy source code
    pub fn extract(source: &str) -> anyhow::Result<ExtractionResult> {
        let mut parser = GroovyParser::new().map_err(|e| anyhow::anyhow!("{}", e))?;
        let tree = parser
            .parse_tree(source)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let root = tree.root_node();

        let mut result = ExtractionResult {
            imports: Self::default_imports(),
            ..ExtractionResult::default()
        };
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            match child.kind() {
                "package_declaration" => {
                    result.package = Self::extract_package(&child, source);
                    result.imports.current_package = result.package.clone();
                }
                "import_declaration" => {
                    Self::extract_import(&child, source, &mut result.imports);
                }
                "class_declaration" => {
                    Self::extract_class(&child, source, &result.package, &mut result.symbols);
                }
                "interface_declaration" => {
                    Self::extract_interface(&child, source, &result.package, &mut result.symbols);
                }
                "enum_declaration" => {
                    Self::extract_enum(&child, source, &result.package, &mut result.symbols);
                }
                "method_declaration" | "function_definition" => {
                    // Top-level function (Groovy script)
                    Self::extract_top_level_method(
                        &child,
                        source,
                        &result.package,
                        &mut result.symbols,
                    );
                }
                _ => {}
            }
        }

        debug!(
            "Groovy extraction complete: {} symbols, package={:?}",
            result.symbols.len(),
            result.package
        );
        Ok(result)
    }

    /// Groovy injects these imports into every compilation unit. Keeping them
    /// in the canonical import map makes hierarchy, navigation, dependency
    /// invalidation, and usage extraction agree on the same name-resolution
    /// environment.
    fn default_imports() -> ImportMap {
        let mut imports = ImportMap::default();
        for package in [
            "java.io",
            "java.lang",
            "java.net",
            "java.util",
            "groovy.lang",
            "groovy.util",
        ] {
            imports.add_wildcard(package.to_string());
        }
        imports.add_import("BigDecimal".to_string(), "java.math.BigDecimal".to_string());
        imports.add_import("BigInteger".to_string(), "java.math.BigInteger".to_string());
        imports
    }

    fn extract_package(node: &tree_sitter::Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
                return Some(Self::node_text(&child, source).to_string());
            }
        }
        None
    }

    fn extract_import(node: &tree_sitter::Node, source: &str, imports: &mut ImportMap) {
        let text = Self::node_text(node, source).trim();
        let Some(body) = text.strip_prefix("import") else {
            return;
        };
        let body = body.trim();
        let static_body = body
            .strip_prefix("static")
            .filter(|remainder| remainder.chars().next().is_some_and(char::is_whitespace));
        let (is_static, body) = static_body.map_or((false, body), |body| (true, body.trim()));
        let mut words = body.trim_end_matches(';').split_whitespace();
        let Some(written_path) = words.next() else {
            return;
        };
        let alias = (words.next() == Some("as"))
            .then(|| words.next().map(str::to_owned))
            .flatten();
        let is_wildcard = written_path.ends_with(".*");
        let path = written_path.trim_end_matches(".*").to_owned();
        if is_wildcard {
            if is_static {
                imports.add_static_wildcard(path);
            } else {
                imports.add_wildcard(path);
            }
        } else {
            let imported_name = alias
                .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path.as_str()).to_owned());
            if is_static {
                imports.add_static_import(imported_name, path);
            } else {
                imports.add_import(imported_name, path);
            }
        }
    }

    fn extract_class(
        node: &tree_sitter::Node,
        source: &str,
        package: &Option<String>,
        symbols: &mut Vec<Symbol>,
    ) {
        let name = Self::find_identifier(node, source);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let fqn = match package {
            Some(pkg) => format!("{}.{}", pkg, name),
            None => name.clone(),
        };

        let modifiers = Self::extract_modifiers(node, source);
        let (parent_class, interfaces) = Self::extract_superclass_and_interfaces(node, source);

        symbols.push(Symbol {
            fqn: fqn.clone(),
            simple_name: name.clone(),
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::CLASS,
            modifiers,
            container: package.clone(),
            return_type: None,
            parameters: vec![],
            parent_class,
            interfaces,
            documentation: None,
        });

        // Extract members from class_body
        Self::extract_class_members(node, source, &fqn, symbols);
    }

    fn extract_interface(
        node: &tree_sitter::Node,
        source: &str,
        package: &Option<String>,
        symbols: &mut Vec<Symbol>,
    ) {
        let name = Self::find_identifier(node, source);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let fqn = match package {
            Some(pkg) => format!("{}.{}", pkg, name),
            None => name.clone(),
        };

        let (_, interfaces) = Self::extract_superclass_and_interfaces(node, source);

        symbols.push(Symbol {
            fqn: fqn.clone(),
            simple_name: name.clone(),
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::INTERFACE,
            modifiers: Self::extract_modifiers(node, source),
            container: package.clone(),
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces,
            documentation: None,
        });

        // Extract members
        Self::extract_class_members(node, source, &fqn, symbols);
    }

    fn extract_enum(
        node: &tree_sitter::Node,
        source: &str,
        package: &Option<String>,
        symbols: &mut Vec<Symbol>,
    ) {
        let name = Self::find_identifier(node, source);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let fqn = match package {
            Some(pkg) => format!("{}.{}", pkg, name),
            None => name.clone(),
        };

        let (_, interfaces) = Self::extract_superclass_and_interfaces(node, source);

        symbols.push(Symbol {
            fqn: fqn.clone(),
            simple_name: name.clone(),
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::ENUM,
            modifiers: Self::extract_modifiers(node, source),
            container: package.clone(),
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces,
            documentation: None,
        });

        Self::extract_class_members(node, source, &fqn, symbols);
    }

    fn extract_class_members(
        node: &tree_sitter::Node,
        source: &str,
        container_fqn: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if matches!(child.kind(), "class_body" | "interface_body" | "enum_body") {
                let mut body_cursor = child.walk();
                for member in child.children(&mut body_cursor) {
                    match member.kind() {
                        "method_declaration" => {
                            Self::extract_method(&member, source, container_fqn, symbols);
                        }
                        "constructor_declaration" => {
                            Self::extract_constructor(&member, source, container_fqn, symbols);
                        }
                        "field_declaration" => {
                            Self::extract_field(&member, source, container_fqn, symbols);
                        }
                        "class_declaration" => {
                            // Nested class
                            Self::extract_class(
                                &member,
                                source,
                                &Some(container_fqn.to_string()),
                                symbols,
                            );
                        }
                        "interface_declaration" => {
                            Self::extract_interface(
                                &member,
                                source,
                                &Some(container_fqn.to_string()),
                                symbols,
                            );
                        }
                        "enum_declaration" => {
                            Self::extract_enum(
                                &member,
                                source,
                                &Some(container_fqn.to_string()),
                                symbols,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn extract_method(
        node: &tree_sitter::Node,
        source: &str,
        container_fqn: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let name = Self::find_identifier(node, source);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let return_type = Self::find_type(node, source);
        let parameters = Self::extract_parameters(node, source);

        symbols.push(Symbol {
            fqn: format!("{}.{}", container_fqn, name),
            simple_name: name,
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::METHOD,
            modifiers: Self::extract_modifiers(node, source),
            container: Some(container_fqn.to_string()),
            return_type,
            parameters,
            parent_class: None,
            interfaces: vec![],
            documentation: None,
        });
    }

    fn extract_constructor(
        node: &tree_sitter::Node,
        source: &str,
        container_fqn: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let simple_name = container_fqn.rsplit('.').next().unwrap_or(container_fqn);
        let parameters = Self::extract_parameters(node, source);

        symbols.push(Symbol {
            fqn: format!("{}.<init>", container_fqn),
            simple_name: simple_name.to_string(),
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::CONSTRUCTOR,
            modifiers: Self::extract_modifiers(node, source),
            container: Some(container_fqn.to_string()),
            return_type: None,
            parameters,
            parent_class: None,
            interfaces: vec![],
            documentation: None,
        });
    }

    fn extract_field(
        node: &tree_sitter::Node,
        source: &str,
        container_fqn: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        // Find variable_declarator for the field name
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name) = Self::find_identifier(&child, source) {
                    let field_type = Self::find_type(node, source);

                    symbols.push(Symbol {
                        fqn: format!("{}.{}", container_fqn, name),
                        simple_name: name,
                        range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
                        kind: SymbolKind::FIELD,
                        modifiers: Self::extract_modifiers(node, source),
                        container: Some(container_fqn.to_string()),
                        return_type: field_type,
                        parameters: vec![],
                        parent_class: None,
                        interfaces: vec![],
                        documentation: None,
                    });
                }
            }
        }
    }

    fn extract_top_level_method(
        node: &tree_sitter::Node,
        source: &str,
        package: &Option<String>,
        symbols: &mut Vec<Symbol>,
    ) {
        let name = Self::find_identifier(node, source);
        if name.is_none() {
            return;
        }
        let name = name.unwrap();

        let fqn = match package {
            Some(pkg) => format!("{}.{}", pkg, name),
            None => name.clone(),
        };

        symbols.push(Symbol {
            fqn,
            simple_name: name,
            range: TextRange::new(node.start_byte() as u32, node.end_byte() as u32),
            kind: SymbolKind::FUNCTION,
            modifiers: Self::extract_modifiers(node, source),
            container: package.clone(),
            return_type: Self::find_type(node, source),
            parameters: Self::extract_parameters(node, source),
            parent_class: None,
            interfaces: vec![],
            documentation: None,
        });
    }

    fn find_identifier(node: &tree_sitter::Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return Some(Self::node_text(&child, source).to_string());
            }
        }
        None
    }

    fn find_type(node: &tree_sitter::Node, source: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier"
                | "scoped_type_identifier"
                | "integral_type"
                | "floating_point_type"
                | "boolean_type"
                | "void_type"
                | "generic_type"
                | "array_type" => {
                    return Some(Self::node_text(&child, source).to_string());
                }
                _ => {}
            }
        }
        None
    }

    fn extract_parameters(node: &tree_sitter::Node, source: &str) -> Vec<String> {
        let mut params = vec![];
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "formal_parameters" {
                let mut params_cursor = child.walk();
                for param in child.children(&mut params_cursor) {
                    if param.kind() == "formal_parameter" {
                        if let Some(param_type) = Self::find_type(&param, source) {
                            params.push(param_type);
                        }
                    }
                }
            }
        }

        params
    }

    fn extract_modifiers(node: &tree_sitter::Node, source: &str) -> Modifiers {
        let mut modifiers = Modifiers::default();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let text = Self::node_text(&child, source);
                modifiers.is_public = text.contains("public");
                modifiers.is_private = text.contains("private");
                modifiers.is_protected = text.contains("protected");
                modifiers.is_static = text.contains("static");
                modifiers.is_final = text.contains("final");
                modifiers.is_abstract = text.contains("abstract");
            }
        }

        modifiers
    }

    fn extract_superclass_and_interfaces(
        node: &tree_sitter::Node,
        source: &str,
    ) -> (Option<String>, Vec<String>) {
        let mut parent_class = None;
        let mut interfaces = vec![];
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "superclass" => {
                    if let Some(type_id) = Self::find_type(&child, source) {
                        parent_class = Some(Self::erase_type_arguments(&type_id));
                    }
                }
                "super_interfaces" | "interfaces" | "extends_interfaces" => {
                    Self::collect_declared_types(&child, source, &mut interfaces);
                }
                _ => {}
            }
        }

        (parent_class, interfaces)
    }

    /// Collect the direct types in an `implements`/interface `extends` list.
    ///
    /// Tree-sitter nests the actual entries below a `type_list`; walking only
    /// the clause's immediate children therefore sees no interfaces at all.
    /// Stop at each complete type node so generic arguments are not mistaken
    /// for additional implemented interfaces.
    fn collect_declared_types(node: &tree_sitter::Node, source: &str, result: &mut Vec<String>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "scoped_type_identifier" | "generic_type" => {
                    result.push(Self::erase_type_arguments(Self::node_text(&child, source)));
                }
                _ => Self::collect_declared_types(&child, source, result),
            }
        }
    }

    fn erase_type_arguments(type_name: &str) -> String {
        let mut depth = 0_u32;
        type_name
            .chars()
            .filter(|ch| match ch {
                '<' => {
                    depth += 1;
                    false
                }
                '>' => {
                    depth = depth.saturating_sub(1);
                    false
                }
                _ => depth == 0,
            })
            .collect::<String>()
            .trim()
            .to_string()
    }

    fn node_text<'a>(node: &tree_sitter::Node, source: &'a str) -> &'a str {
        &source[node.start_byte()..node.end_byte()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_class_with_methods() {
        let source = r#"
package com.example

class User {
    String name
    int age

    String getName() {
        return name
    }

    void setName(String name) {
        this.name = name
    }
}
"#;

        let result = LspSymbolExtractor::extract(source).unwrap();

        assert_eq!(result.package.as_deref(), Some("com.example"));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.User" && s.kind == SymbolKind::CLASS));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.User.getName" && s.kind == SymbolKind::METHOD));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.User.setName" && s.kind == SymbolKind::METHOD));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.User.name" && s.kind == SymbolKind::FIELD));
    }

    #[test]
    fn test_extract_imports() {
        let source = r#"
package com.example

import java.util.List
import java.util.Map
import java.time.*
import static java.lang.Math.PI
import static java.lang.Math.max as maximum
import static java.util.Collections.*

class Test {}
"#;

        let result = LspSymbolExtractor::extract(source).unwrap();

        assert_eq!(
            result.imports.explicit.get("List"),
            Some(&"java.util.List".to_string())
        );
        assert_eq!(
            result.imports.explicit.get("Map"),
            Some(&"java.util.Map".to_string())
        );
        assert_eq!(
            result.imports.static_imports_named("PI").next(),
            Some("java.lang.Math.PI")
        );
        assert_eq!(
            result.imports.static_imports_named("maximum").next(),
            Some("java.lang.Math.max")
        );
        assert!(result.imports.wildcards.contains(&"java.time".to_string()));
        for implicit in [
            "java.io",
            "java.lang",
            "java.net",
            "java.util",
            "groovy.lang",
            "groovy.util",
        ] {
            assert!(
                result.imports.wildcards.contains(&implicit.to_string()),
                "missing Groovy default import {implicit}"
            );
        }
        assert_eq!(
            result.imports.explicit.get("BigDecimal"),
            Some(&"java.math.BigDecimal".to_string())
        );
        assert_eq!(
            result.imports.explicit.get("BigInteger"),
            Some(&"java.math.BigInteger".to_string())
        );
        assert_eq!(result.imports.static_wildcards, ["java.util.Collections"]);
    }

    #[test]
    fn explicit_import_overrides_groovy_default_type_import() {
        let result = LspSymbolExtractor::extract(
            "import custom.BigDecimal\nclass UsesCustom { BigDecimal value }",
        )
        .unwrap();

        assert_eq!(
            result.imports.explicit.get("BigDecimal"),
            Some(&"custom.BigDecimal".to_string())
        );
    }

    #[test]
    fn static_prefix_requires_a_token_boundary() {
        let result = LspSymbolExtractor::extract(
            "import staticfoo.Bar\nimport static java.lang.Math.PI\nclass UsesImports {}",
        )
        .unwrap();

        assert_eq!(
            result.imports.explicit.get("Bar").map(String::as_str),
            Some("staticfoo.Bar")
        );
        assert_eq!(
            result.imports.static_imports_named("PI").next(),
            Some("java.lang.Math.PI")
        );
    }

    #[test]
    fn test_extract_gradle_style() {
        let source = r#"
plugins {
    id 'java'
}

dependencies {
    implementation 'junit:junit:4.13'
}
"#;

        // Gradle DSL is script-style, no classes - should extract without errors
        let result = LspSymbolExtractor::extract(source).unwrap();
        assert!(result.package.is_none());
    }

    #[test]
    fn test_extracts_class_interface_and_enum_inheritance() {
        let source = r#"
package app

abstract class Child<T> extends Base<T> implements Api, local.Other {}
class Qualified extends local.Base {}
interface Combined extends Api, local.Other {}
enum Mode implements Api { ON }
"#;

        let result = LspSymbolExtractor::extract(source).unwrap();
        let symbol = |name: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.simple_name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };

        let child = symbol("Child");
        assert_eq!(child.parent_class.as_deref(), Some("Base"));
        assert_eq!(child.interfaces, ["Api", "local.Other"]);

        let qualified = symbol("Qualified");
        assert_eq!(qualified.parent_class.as_deref(), Some("local.Base"));

        let combined = symbol("Combined");
        assert_eq!(combined.interfaces, ["Api", "local.Other"]);

        let mode = symbol("Mode");
        assert_eq!(mode.interfaces, ["Api"]);
    }
}
