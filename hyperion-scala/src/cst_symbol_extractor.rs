// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! CST-based symbol extraction for Scala.
//!
//! The tree-sitter traversal provides:
//! - Robust symbol extraction using tree-sitter parse trees
//! - Accurate scoping for nested classes and companion objects
//! - Precise text ranges from the CST
//! - Support for Scala 2 and Scala 3 syntax

use crate::parser::{parse, NodeExt};
use crate::symbol_extractor::{ExtractionResult, ImportMap, Modifiers, Symbol, TextRange};
use crate::type_extractor;
use lsp_types::SymbolKind;
use tracing::{debug, warn};
use tree_sitter::Node;

/// Extract symbols from Scala source using CST parsing
pub fn extract_from_source(source: &str) -> anyhow::Result<ExtractionResult> {
    let parse_result = parse(source)?;

    if !parse_result.errors.is_empty() {
        const MAX_LOGGED_PARSE_ERRORS: usize = 3;
        let sample = parse_result
            .errors
            .iter()
            .take(MAX_LOGGED_PARSE_ERRORS)
            .collect::<Vec<_>>();
        warn!(
            error_count = parse_result.errors.len(),
            errors = ?sample,
            "Scala parse errors detected; symbol extraction is continuing"
        );
    }

    let package = extract_package(&parse_result.tree.root_node(), source);
    let mut imports = extract_imports(&parse_result.tree.root_node(), source);
    imports.current_package.clone_from(&package);
    let symbols = extract_symbols(&parse_result.tree.root_node(), source, package.as_deref());

    debug!(
        "Extracted {} symbols from Scala file (package: {:?})",
        symbols.len(),
        package
    );

    Ok(ExtractionResult {
        package,
        imports,
        symbols,
    })
}

/// Extract imports into the language-neutral workspace representation.
///
/// Scala selectors can import several names from one namespace, rename a
/// declaration, hide one with `_`, or import the whole namespace. Retaining
/// those distinctions is required for canonical cross-language inheritance.
fn extract_imports(root: &Node, source: &str) -> ImportMap {
    let mut imports = ImportMap::default();
    // Scala injects these into every compilation unit. Persist them in the
    // shared import model so indexing, hierarchy, invalidation, and request
    // resolution operate in the same environment.
    imports.add_wildcard("java.lang".to_string());
    imports.add_wildcard("scala".to_string());
    imports.add_static_wildcard("scala.Predef".to_string());
    collect_package_imports(root, source, &mut imports);
    imports
}

fn collect_package_imports(node: &Node, source: &str, imports: &mut ImportMap) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "import_declaration" => extract_import_declaration(&child, source, imports),
            // Imports inside declarations are scoped locally and must not
            // affect the file-level declaration context. Everything else here
            // is package/container syntax through which package-level imports
            // may be nested (for example `package app { ... }`).
            "class_definition"
            | "object_definition"
            | "trait_definition"
            | "enum_definition"
            | "function_definition"
            | "function_declaration" => {}
            _ => collect_package_imports(&child, source, imports),
        }
    }
}

fn extract_import_declaration(node: &Node, source: &str, imports: &mut ImportMap) {
    let mut path_cursor = node.walk();
    let path_nodes: Vec<_> = node
        .children_by_field_name("path", &mut path_cursor)
        .filter(Node::is_named)
        .collect();
    if path_nodes.is_empty() {
        return;
    }
    let namespace = path_nodes
        .iter()
        .map(|part| part.text(source).trim())
        .collect::<Vec<_>>()
        .join(".");
    if namespace.is_empty() {
        return;
    }

    let mut cursor = node.walk();
    let mut found_selector = false;
    for selector in node
        .named_children(&mut cursor)
        .filter(|child| !path_nodes.iter().any(|path| child.id() == path.id()))
    {
        found_selector = true;
        extract_import_selector(&selector, &namespace, source, imports);
    }

    if !found_selector {
        let simple_name = namespace.rsplit('.').next().unwrap_or(&namespace);
        imports.add_import(simple_name.to_string(), namespace);
    }
}

fn extract_import_selector(node: &Node, namespace: &str, source: &str, imports: &mut ImportMap) {
    match node.kind() {
        "namespace_wildcard" => imports.add_wildcard(namespace.to_string()),
        "identifier" => {
            let name = node.text(source);
            imports.add_import(name.to_string(), format!("{namespace}.{name}"));
        }
        "arrow_renamed_identifier" | "as_renamed_identifier" | "renamed_identifier" => {
            let Some(name) = node.child_by_field_name("name") else {
                return;
            };
            let Some(alias) = node.child_by_field_name("alias") else {
                return;
            };
            let alias = alias.text(source);
            if alias != "_" {
                imports.add_import(
                    alias.to_string(),
                    format!("{namespace}.{}", name.text(source)),
                );
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                extract_import_selector(&child, namespace, source, imports);
            }
        }
    }
}

/// Extract package declaration from CST
fn extract_package(root: &Node, source: &str) -> Option<String> {
    let mut cursor = root.walk();
    let mut parts = Vec::new();
    for node in root.named_children(&mut cursor) {
        if node.kind() != "package_clause" {
            if !parts.is_empty() {
                break;
            }
            continue;
        }
        if let Some(name) = extract_package_clause_name(&node, source) {
            parts.push(name);
        }
        if node.named_child_count() > 1 {
            // A braced/indented package owns its declarations. Subsequent
            // root package clauses would be siblings, not a continuation.
            break;
        }
    }
    (!parts.is_empty()).then(|| parts.join("."))
}

fn extract_package_clause_name(node: &Node, source: &str) -> Option<String> {
    if let Some(package_id) = node.child_by_field("package") {
        return Some(package_id.text(source).to_string());
    }
    node.named_children(&mut node.walk())
        .find(|child| child.kind() == "package_identifier")
        .map(|child| child.text(source).to_string())
}

/// Extract all symbols from the CST
fn extract_symbols(root: &Node, source: &str, package: Option<&str>) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut cursor = root.walk();

    // Traverse top-level definitions
    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            extract_symbol_from_node(&node, source, package, None, &mut symbols);

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    symbols
}

/// Extract symbol from a single node
fn extract_symbol_from_node(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    match node.kind() {
        "class_definition" => extract_class(node, source, package, container, symbols),
        "object_definition" => extract_object(node, source, package, container, symbols),
        "trait_definition" => extract_trait(node, source, package, container, symbols),
        "enum_definition" => extract_enum(node, source, package, container, symbols),
        "function_definition" | "function_declaration" => {
            extract_function(node, source, package, container, symbols);
        }
        "val_definition" | "var_definition" => {
            extract_field(node, source, package, container, symbols);
        }
        _ => {
            // Recurse into children for nested definitions
            let mut cursor = node.walk();
            if cursor.goto_first_child() {
                loop {
                    extract_symbol_from_node(&cursor.node(), source, package, container, symbols);
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
    }
}

/// Extract class symbol and its members
fn extract_class(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let range = find_identifier_range(node, source);
    let (parent_class, interfaces) = extract_class_supertypes(node, source);

    symbols.push(Symbol {
        fqn: fqn.clone(),
        simple_name: name,
        range,
        kind: SymbolKind::CLASS,
        container: container.map(String::from),
        return_type: None,
        parameters: extract_class_parameters(node, source),
        modifiers: extract_modifiers(node, source),
        parent_class,
        interfaces,
    });

    // Extract members (methods and fields) from template_body
    if let Some(body) = find_template_body(node) {
        extract_template_members(&body, source, package, Some(&fqn), symbols);
    }
}

/// Extract object symbol and its members
fn extract_object(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let range = find_identifier_range(node, source);
    let (parent_class, interfaces) = extract_class_supertypes(node, source);
    let mut modifiers = extract_modifiers(node, source);
    modifiers.is_final = true;

    symbols.push(Symbol {
        fqn: fqn.clone(),
        simple_name: name,
        range,
        kind: SymbolKind::OBJECT,
        container: container.map(String::from),
        return_type: None,
        parameters: vec![],
        modifiers,
        parent_class,
        interfaces,
    });

    // Extract members from template_body
    if let Some(body) = find_template_body(node) {
        extract_template_members(&body, source, package, Some(&fqn), symbols);
    }
}

/// Extract trait symbol and its members
fn extract_trait(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let range = find_identifier_range(node, source);
    let mut modifiers = extract_modifiers(node, source);
    modifiers.is_abstract = true;

    symbols.push(Symbol {
        fqn: fqn.clone(),
        simple_name: name,
        range,
        kind: SymbolKind::INTERFACE, // Traits map to INTERFACE
        container: container.map(String::from),
        return_type: None,
        parameters: vec![],
        modifiers,
        parent_class: None,
        interfaces: extract_declared_supertypes(node, source),
    });

    // Extract members from template_body
    if let Some(body) = find_template_body(node) {
        extract_template_members(&body, source, package, Some(&fqn), symbols);
    }
}

/// Extract a Scala enum as a class-like declaration. Its explicit `extends`
/// clause names implemented traits; the runtime enum superclass is implicit.
fn extract_enum(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let mut modifiers = extract_modifiers(node, source);
    modifiers.is_final = true;
    symbols.push(Symbol {
        fqn,
        simple_name: name,
        range: find_identifier_range(node, source),
        kind: SymbolKind::ENUM,
        container: container.map(String::from),
        return_type: None,
        parameters: vec![],
        modifiers,
        parent_class: None,
        interfaces: extract_declared_supertypes(node, source),
    });
}

/// Extract function/method symbol
fn extract_function(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let range = find_identifier_range(node, source);

    // Extract return type
    let return_type = extract_return_type(node, source);

    // Extract parameters
    let parameters = extract_parameters(node, source);

    symbols.push(Symbol {
        fqn,
        simple_name: name,
        range,
        kind: if container.is_some() {
            SymbolKind::METHOD
        } else {
            SymbolKind::FUNCTION
        },
        container: container.map(String::from),
        return_type,
        parameters,
        modifiers: extract_modifiers(node, source),
        parent_class: None,
        interfaces: vec![],
    });
}

/// Extract field (val/var) symbol
fn extract_field(
    node: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let name = find_identifier(node, source);
    if name.is_empty() {
        return;
    }

    let fqn = build_fqn(package, container, &name);
    let range = find_identifier_range(node, source);

    // Extract field type
    let field_type = extract_field_type(node, source);

    symbols.push(Symbol {
        fqn,
        simple_name: name,
        range,
        kind: SymbolKind::FIELD,
        container: container.map(String::from),
        return_type: field_type,
        parameters: vec![],
        modifiers: extract_modifiers(node, source),
        parent_class: None,
        interfaces: vec![],
    });
}

/// Extract members from a template_body node
fn extract_template_members(
    body: &Node,
    source: &str,
    package: Option<&str>,
    container: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = body.walk();

    if !cursor.goto_first_child() {
        return;
    }

    loop {
        let node = cursor.node();

        // Process all definition types
        match node.kind() {
            "function_definition" | "function_declaration" => {
                extract_function(&node, source, package, container, symbols);
            }
            "val_definition" | "var_definition" => {
                extract_field(&node, source, package, container, symbols);
            }
            "class_definition" => {
                // Nested class - pass current container
                extract_class(&node, source, package, container, symbols);
            }
            "object_definition" => {
                // Nested object - pass current container
                extract_object(&node, source, package, container, symbols);
            }
            "trait_definition" => {
                // Nested trait - pass current container
                extract_trait(&node, source, package, container, symbols);
            }
            "enum_definition" => {
                extract_enum(&node, source, package, container, symbols);
            }
            _ => {
                // Continue traversing for nested items
                extract_symbol_from_node(&node, source, package, container, symbols);
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn extract_modifiers(node: &Node, source: &str) -> Modifiers {
    let mut modifiers = Modifiers {
        is_public: true,
        ..Modifiers::default()
    };
    let declaration_name_start = node
        .named_children(&mut node.walk())
        .find(|child| child.kind() == "identifier")
        .map_or(node.end_byte(), |identifier| identifier.start_byte());
    let words: Vec<_> = source[node.start_byte()..declaration_name_start]
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|word| !word.is_empty())
        .collect();
    modifiers.is_private = words.contains(&"private");
    modifiers.is_protected = words.contains(&"protected");
    modifiers.is_public = !(modifiers.is_private || modifiers.is_protected);
    modifiers.is_final = words.contains(&"final");
    modifiers.is_mutable = node.kind() == "var_definition";
    modifiers.is_open = words.contains(&"open");
    modifiers.is_abstract = words.contains(&"abstract");
    modifiers.is_sealed = words.contains(&"sealed");
    modifiers.is_data = words.contains(&"case");
    modifiers
}

fn extract_class_supertypes(node: &Node, source: &str) -> (Option<String>, Vec<String>) {
    let mut supertypes = extract_declared_supertypes(node, source).into_iter();
    (supertypes.next(), supertypes.collect())
}

fn extract_declared_supertypes(node: &Node, source: &str) -> Vec<String> {
    let Some(extends) = node
        .named_children(&mut node.walk())
        .find(|child| child.kind() == "extends_clause")
    else {
        return Vec::new();
    };
    let mut cursor = extends.walk();
    extends
        .children_by_field_name("type", &mut cursor)
        .filter(Node::is_named)
        .map(|node| erase_type_arguments(node.text(source)))
        .filter(|name| !name.is_empty())
        .collect()
}

fn erase_type_arguments(type_name: &str) -> String {
    let mut square_depth = 0_u32;
    type_name
        .chars()
        .filter(|ch| match ch {
            '[' => {
                square_depth += 1;
                false
            }
            ']' => {
                square_depth = square_depth.saturating_sub(1);
                false
            }
            _ => square_depth == 0,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn extract_class_parameters(node: &Node, source: &str) -> Vec<String> {
    let Some(parameters) = node
        .named_children(&mut node.walk())
        .find(|child| child.kind() == "class_parameters")
    else {
        return Vec::new();
    };

    let mut result = Vec::new();
    let mut cursor = parameters.walk();
    for parameter in parameters.named_children(&mut cursor) {
        if parameter.kind() == "class_parameter" {
            if let Some(parameter_type) = parameter.child_by_field_name("type") {
                result.push(parameter_type.text(source).to_string());
            }
        }
    }
    result
}

/// Find the identifier child node and return its text
fn find_identifier(node: &Node, source: &str) -> String {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return String::new();
    }

    loop {
        let child = cursor.node();
        if child.kind() == "identifier" {
            return child.text(source).to_string();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    String::new()
}

/// Find the identifier node and return its range
fn find_identifier_range(node: &Node, _source: &str) -> TextRange {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return TextRange::new(
            u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
            u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
        );
    }

    loop {
        let child = cursor.node();
        if child.kind() == "identifier" {
            return TextRange::new(
                u32::try_from(child.start_byte()).unwrap_or(u32::MAX),
                u32::try_from(child.end_byte()).unwrap_or(u32::MAX),
            );
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    TextRange::new(
        u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
        u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
    )
}

/// Find template_body child node
fn find_template_body<'a>(node: &'a Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    loop {
        let child = cursor.node();
        if child.kind() == "template_body" {
            return Some(child);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    None
}

/// Extract return type from function node
///
/// This now supports complex types via the type_extractor module:
/// - Simple types: String, Int
/// - Generic types: List[String], Map[Int, String]
/// - Function types: (Int, String) => Boolean
/// - Tuple types: (Int, String)
fn extract_return_type(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    // Look for type node after ":" in function signature
    let mut found_colon = false;
    loop {
        let child = cursor.node();

        if child.kind() == ":" {
            found_colon = true;
        } else if found_colon && child.kind().contains("type") {
            // Use the comprehensive type extractor
            return type_extractor::extract_type(&child, source);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    None
}

/// Extract field type from val/var node
fn extract_field_type(node: &Node, source: &str) -> Option<String> {
    extract_return_type(node, source) // Same logic
}

/// Extract parameter types from function node
fn extract_parameters(node: &Node, source: &str) -> Vec<String> {
    let mut param_types = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return param_types;
    }

    // Find parameters node
    loop {
        let child = cursor.node();

        if child.kind() == "parameters" {
            extract_param_types_from_parameters(&child, source, &mut param_types);
            break;
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    param_types
}

/// Extract parameter types from a parameters node
fn extract_param_types_from_parameters(node: &Node, source: &str, param_types: &mut Vec<String>) {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return;
    }

    loop {
        let child = cursor.node();

        if child.kind() == "parameter" {
            // parameter contains identifier : type_identifier
            if let Some(param_type) = extract_type_from_parameter(&child, source) {
                param_types.push(param_type);
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Extract type from a parameter node
///
/// This now supports complex types via the type_extractor module.
fn extract_type_from_parameter(node: &Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return None;
    }

    let mut found_colon = false;
    loop {
        let child = cursor.node();

        if child.kind() == ":" {
            found_colon = true;
        } else if found_colon && child.kind().contains("type") {
            // Use the comprehensive type extractor
            return type_extractor::extract_type(&child, source);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    None
}

/// Build fully qualified name
fn build_fqn(package: Option<&str>, container: Option<&str>, name: &str) -> String {
    match (package, container) {
        // Container is already a FQN, don't add package again
        (_, Some(cont)) => format!("{cont}.{name}"),
        (Some(pkg), None) => format!("{pkg}.{name}"),
        (None, None) => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package() {
        let source = "package com.example\n\nclass Foo";
        let result = extract_from_source(source).unwrap();
        assert_eq!(result.package.as_deref(), Some("com.example"));
    }

    #[test]
    fn test_extract_class() {
        let source = "package com.example\n\nclass User";
        let result = extract_from_source(source).unwrap();

        let class_sym = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::CLASS)
            .expect("Should find User class");

        assert_eq!(class_sym.fqn, "com.example.User");
        assert_eq!(class_sym.simple_name, "User");
    }

    #[test]
    fn test_extract_object() {
        let source = "package com.example\n\nobject Utils";
        let result = extract_from_source(source).unwrap();

        let object_sym = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::OBJECT)
            .expect("Should find Utils object");

        assert_eq!(object_sym.fqn, "com.example.Utils");
        assert_eq!(object_sym.simple_name, "Utils");
    }

    #[test]
    fn test_extract_trait() {
        let source = "package com.example\n\ntrait Configurable";
        let result = extract_from_source(source).unwrap();

        let trait_sym = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::INTERFACE)
            .expect("Should find Configurable trait");

        assert_eq!(trait_sym.fqn, "com.example.Configurable");
        assert_eq!(trait_sym.simple_name, "Configurable");
    }

    #[test]
    fn test_extract_method() {
        let source = r#"
package com.example

class User {
  def getName(): String = "test"
}
"#;
        let result = extract_from_source(source).unwrap();

        let method_sym = result
            .symbols
            .iter()
            .find(|s| s.simple_name == "getName")
            .expect("Should find getName method");

        assert_eq!(method_sym.fqn, "com.example.User.getName");
        assert_eq!(method_sym.kind, SymbolKind::METHOD);
        assert_eq!(method_sym.return_type, Some("String".to_string()));
        assert_eq!(method_sym.container, Some("com.example.User".to_string()));
    }

    #[test]
    fn test_extract_field() {
        let source = r"
package com.example

class User {
  val id: Int = 42
}
";
        let result = extract_from_source(source).unwrap();

        let field_sym = result
            .symbols
            .iter()
            .find(|s| s.simple_name == "id")
            .expect("Should find id field");

        assert_eq!(field_sym.fqn, "com.example.User.id");
        assert_eq!(field_sym.kind, SymbolKind::FIELD);
        assert_eq!(field_sym.return_type, Some("Int".to_string()));
    }

    #[test]
    fn test_scala_val_and_var_fields_record_mutability() {
        let source = r#"
package com.example

class Counter {
  val label: String = "c"
  var count: Int = 0
}
"#;
        let result = extract_from_source(source).unwrap();
        let field = |name: &str| {
            result
                .symbols
                .iter()
                .find(|s| s.simple_name == name)
                .unwrap_or_else(|| panic!("missing field {name}"))
        };

        assert!(!field("label").modifiers.is_mutable, "val is not mutable");
        assert!(field("count").modifiers.is_mutable, "var is mutable");
    }

    #[test]
    fn test_extract_method_with_parameters() {
        let source = r"
package com.example

class Config {
  def addReconfigurable(r: Reconfigurable): Unit = {}
}
";
        let result = extract_from_source(source).unwrap();

        let method_sym = result
            .symbols
            .iter()
            .find(|s| s.simple_name == "addReconfigurable")
            .expect("Should find addReconfigurable method");

        assert_eq!(method_sym.parameters, vec!["Reconfigurable"]);
        assert_eq!(method_sym.return_type, Some("Unit".to_string()));
    }

    #[test]
    fn test_nested_classes() {
        let source = r"
package com.example

class Outer {
  class Inner {
    def method(): Unit = {}
  }
}
";
        let result = extract_from_source(source).unwrap();

        // Should extract Outer, Inner, and method
        assert!(result.symbols.iter().any(|s| s.fqn == "com.example.Outer"));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.Outer.Inner"));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.fqn == "com.example.Outer.Inner.method"));
    }

    #[test]
    fn test_extract_imports_inheritance_modifiers_and_enums() {
        let source = r#"
package app
import dependency.{Base => Parent, Api}
import other._
import ignored.{Drop => _, Keep}
import java.util.List

sealed abstract class Child[A](value: A) extends Parent[A] with Api with other.Mixin {
  override def run(): Unit = ()
}
trait Combined extends Api with other.Mixin
case object Singleton extends Parent[String] with Api
enum Mode extends Api:
  case On
"#;
        let result = extract_from_source(source).unwrap();
        assert_eq!(
            result.imports.explicit.get("Parent").map(String::as_str),
            Some("dependency.Base")
        );
        assert_eq!(
            result.imports.explicit.get("Api").map(String::as_str),
            Some("dependency.Api")
        );
        assert!(result.imports.wildcards.contains(&"other".to_string()));
        assert!(result.imports.wildcards.contains(&"java.lang".to_string()));
        assert!(result.imports.wildcards.contains(&"scala".to_string()));
        assert!(result
            .imports
            .static_wildcards
            .contains(&"scala.Predef".to_string()));
        assert!(!result.imports.explicit.contains_key("Drop"));
        assert!(!result.imports.explicit.contains_key("_"));
        assert_eq!(
            result.imports.explicit.get("Keep").map(String::as_str),
            Some("ignored.Keep")
        );
        assert_eq!(
            result.imports.explicit.get("List").map(String::as_str),
            Some("java.util.List")
        );

        let symbol = |name: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.simple_name == name)
                .unwrap_or_else(|| panic!("missing {name}"))
        };
        let child = symbol("Child");
        assert_eq!(child.parameters, ["A"]);
        assert!(child.modifiers.is_sealed);
        assert!(child.modifiers.is_abstract);
        assert_eq!(child.parent_class.as_deref(), Some("Parent"));
        assert_eq!(child.interfaces, ["Api", "other.Mixin"]);

        let combined = symbol("Combined");
        assert!(combined.modifiers.is_abstract);
        assert_eq!(combined.interfaces, ["Api", "other.Mixin"]);

        let singleton = symbol("Singleton");
        assert!(singleton.modifiers.is_final);
        assert!(singleton.modifiers.is_data);
        assert_eq!(singleton.parent_class.as_deref(), Some("Parent"));
        assert_eq!(singleton.interfaces, ["Api"]);

        let mode = symbol("Mode");
        assert_eq!(mode.kind, SymbolKind::ENUM);
        assert!(mode.modifiers.is_final);
        assert_eq!(mode.interfaces, ["Api"]);
    }

    #[test]
    fn top_level_def_is_a_function() {
        let result = extract_from_source("package app\n\ndef run(): Unit = ()").unwrap();
        let run = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name == "run")
            .expect("top-level function");
        assert_eq!(run.kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn scala_three_import_syntax_is_retained() {
        let source = "package app\nimport dependency.{Base as Parent, Api}\nimport mixins.*\nclass Child extends Parent with Api with Mixin";
        let result = extract_from_source(source).unwrap();
        assert_eq!(
            result.imports.explicit.get("Parent").map(String::as_str),
            Some("dependency.Base")
        );
        assert_eq!(
            result.imports.explicit.get("Api").map(String::as_str),
            Some("dependency.Api")
        );
        assert!(result.imports.wildcards.contains(&"mixins".to_string()));
    }

    #[test]
    fn braced_package_imports_are_retained() {
        let result = extract_from_source(
            "package app {\n  import dependency.Base\n  class Child extends Base\n}",
        )
        .unwrap();
        assert_eq!(result.package.as_deref(), Some("app"));
        assert_eq!(
            result.imports.explicit.get("Base").map(String::as_str),
            Some("dependency.Base")
        );
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.fqn == "app.Child"));
    }

    #[test]
    fn sequential_package_clauses_form_one_package() {
        let result = extract_from_source("package outer\npackage inner\nclass Child").unwrap();
        assert_eq!(result.package.as_deref(), Some("outer.inner"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.fqn == "outer.inner.Child"));
    }
}
