// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Hover Support
//!
//! This module provides hover functionality for Groovy files.

use crate::position_utils::position_to_offset;
use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use tracing::debug;

/// Helper for Groovy hover functionality
pub struct GroovyHoverHelper<'a> {
    workspace_index: &'a Arc<WorkspaceIndex>,
}

impl<'a> GroovyHoverHelper<'a> {
    /// Create a new GroovyHoverHelper
    pub const fn new(workspace_index: &'a Arc<WorkspaceIndex>) -> Self {
        Self { workspace_index }
    }

    /// Get hover information for a Groovy file position
    ///
    /// Uses tree-sitter to parse Groovy and extract hover information:
    /// - Class/method/field types from workspace index
    /// - Closure delegate types for Gradle blocks
    /// - GString interpolation variable types
    pub fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        use hyperion_groovy::tree_sitter_wrapper::GroovyParser;
        use hyperion_groovy::GroovyClosure;

        debug!("HOVER: Groovy hover for position {:?}", position);

        // Convert position to byte offset
        let offset = position_to_offset(source_text, position).map_err(|e| e.to_string())?;

        // Parse with tree-sitter
        let mut parser = GroovyParser::new().map_err(|e| format!("{}", e))?;
        let tree = parser
            .parse_tree(source_text)
            .map_err(|e| format!("{}", e))?;
        let root = tree.root_node();

        // Find the node at the cursor position
        let node = root.descendant_for_byte_range(offset, offset);
        let node = match node {
            Some(n) => n,
            None => {
                debug!("HOVER: No node found at offset {}", offset);
                return Ok(None);
            }
        };

        // BUG FIX: Use safe slicing to prevent panic on invalid UTF-8 boundaries
        let node_text = source_text
            .get(node.start_byte()..node.end_byte())
            .unwrap_or("");
        debug!(
            "HOVER: Found Groovy node: kind={}, text={:?}",
            node.kind(),
            node_text
        );

        // Check if we're inside a closure block (e.g., dependencies { ... })
        let mut current = Some(node);
        let mut closure_context: Option<GroovyClosure> = None;

        while let Some(n) = current {
            if n.kind() == "closure" || n.kind() == "block" {
                // Check if this is a Gradle DSL block
                if let Some(parent) = n.parent() {
                    if parent.kind() == "function_call" || parent.kind() == "method_invocation" {
                        // Get the method name
                        let mut cursor = parent.walk();
                        for child in parent.children(&mut cursor) {
                            if child.kind() == "identifier" {
                                // BUG FIX: Use safe slicing to prevent panic on invalid UTF-8 boundaries
                                let method_name = source_text
                                    .get(child.start_byte()..child.end_byte())
                                    .unwrap_or("");
                                // Detect Gradle DSL blocks
                                let delegate = match method_name {
                                    // Core Gradle blocks
                                    "dependencies" => Some("DependencyHandler"),
                                    "repositories" => Some("RepositoryHandler"),
                                    "plugins" => Some("PluginDependenciesSpec"),
                                    "tasks" => Some("TaskContainer"),
                                    "configurations" => Some("ConfigurationContainer"),
                                    "sourceSets" => Some("SourceSetContainer"),
                                    "subprojects" | "allprojects" => Some("Project"),
                                    "buildscript" => Some("ScriptHandler"),
                                    "artifacts" => Some("ArtifactHandler"),
                                    "ext" => Some("ExtraPropertiesExtension"),
                                    "buildCache" => Some("BuildCacheConfiguration"),
                                    // Publishing plugin
                                    "publishing" => Some("PublishingExtension"),
                                    "publications" => Some("PublicationContainer"),
                                    // Java plugin
                                    "java" => Some("JavaPluginExtension"),
                                    "application" => Some("JavaApplication"),
                                    // Kotlin plugin
                                    "kotlin" => Some("KotlinProjectExtension"),
                                    "kotlinOptions" => Some("KotlinJvmOptions"),
                                    // Code quality plugins
                                    "jacoco" => Some("JacocoPluginExtension"),
                                    "checkstyle" => Some("CheckstyleExtension"),
                                    "pmd" => Some("PmdExtension"),
                                    "spotbugs" => Some("SpotBugsExtension"),
                                    // Signing plugin
                                    "signing" => Some("SigningExtension"),
                                    // Android plugin
                                    "android" => Some("BaseExtension"),
                                    "defaultConfig" => Some("DefaultConfig"),
                                    "buildTypes" => Some("NamedDomainObjectContainer<BuildType>"),
                                    "productFlavors" => {
                                        Some("NamedDomainObjectContainer<ProductFlavor>")
                                    }
                                    "compileOptions" => Some("CompileOptions"),
                                    "packagingOptions" => Some("PackagingOptions"),
                                    "lintOptions" => Some("LintOptions"),
                                    // Testing
                                    "testOptions" => Some("TestOptions"),
                                    "testLogging" => Some("TestLoggingContainer"),
                                    // Other common blocks
                                    "wrapper" => Some("Wrapper"),
                                    _ => None,
                                };

                                if let Some(delegate_type) = delegate {
                                    closure_context = Some(GroovyClosure::gradle_block(
                                        delegate_type.to_string(),
                                        "Project".to_string(),
                                    ));
                                    debug!(
                                        "HOVER: Detected {} block with delegate {}",
                                        method_name, delegate_type
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
            }
            current = n.parent();
        }

        // Handle GString interpolation
        if node.kind() == "interpolation" || node.kind() == "gstring_content" {
            // Inside GString - try to get variable type
            let var_name = if node_text.starts_with('$') {
                node_text
                    .trim_start_matches('$')
                    .trim_start_matches('{')
                    .trim_end_matches('}')
            } else {
                node_text
            };

            if let Some(type_info) = self.resolve_variable_type(file_id, var_name) {
                let hover_text = format!(
                    "```groovy\n{}: {}\n```\n\n(in GString)",
                    var_name, type_info
                );
                return Ok(Some(self.create_hover(hover_text)));
            }
        }

        // Handle identifier hover
        if node.kind() == "identifier" || node.kind() == "type_identifier" {
            let identifier = node_text.to_string();

            // Check for 'it' parameter in closures
            if identifier == "it" {
                if let Some(ref closure) = closure_context {
                    if let Some(it_type) = closure.implicit_it_type() {
                        let hover_text = format!(
                            "```groovy\nit: {}\n```\n\nImplicit closure parameter",
                            it_type
                        );
                        return Ok(Some(self.create_hover(hover_text)));
                    }
                }
                // Default it type
                let hover_text =
                    "```groovy\nit: Object\n```\n\nImplicit closure parameter".to_string();
                return Ok(Some(self.create_hover(hover_text)));
            }

            // Check for delegate/owner/this in closures
            if let Some(ref closure) = closure_context {
                match identifier.as_str() {
                    "delegate" => {
                        if let Some(ref delegate) = closure.delegate {
                            let hover_text = format!(
                                "```groovy\ndelegate: {}\n```\n\nClosure delegate (resolve strategy: {:?})",
                                delegate, closure.resolve_strategy
                            );
                            return Ok(Some(self.create_hover(hover_text)));
                        }
                    }
                    "owner" => {
                        let hover_text =
                            format!("```groovy\nowner: {}\n```\n\nClosure owner", closure.owner);
                        return Ok(Some(self.create_hover(hover_text)));
                    }
                    "this" => {
                        let hover_text = format!(
                            "```groovy\nthis: {}\n```\n\nEnclosing class reference",
                            closure.this_object
                        );
                        return Ok(Some(self.create_hover(hover_text)));
                    }
                    _ => {}
                }
            }

            // Check if this is a local variable with 'def' - infer type from initializer
            if let Some(parent) = node.parent() {
                if parent.kind() == "variable_declarator" {
                    // Check for variable declaration with def type
                    if let Some(grandparent) = parent.parent() {
                        if grandparent.kind() == "variable_declaration"
                            || grandparent.kind() == "local_variable_declaration"
                        {
                            // Check if the type is 'def'
                            let mut cursor = grandparent.walk();
                            let mut is_def = false;
                            for child in grandparent.children(&mut cursor) {
                                // BUG FIX: Use safe slicing to prevent panic on invalid UTF-8 boundaries
                                if child.kind() == "def"
                                    || (child.kind() == "identifier"
                                        && source_text
                                            .get(child.start_byte()..child.end_byte())
                                            .unwrap_or("")
                                            == "def")
                                {
                                    is_def = true;
                                    break;
                                }
                            }

                            if is_def {
                                // Get the grandparent's text for type inference
                                // BUG FIX: Use safe slicing to prevent panic on invalid UTF-8 boundaries
                                let declaration_text = source_text
                                    .get(grandparent.start_byte()..grandparent.end_byte())
                                    .unwrap_or("");
                                // Find the initializer to infer type
                                if let Some(inferred_type) =
                                    self.infer_expression_type(declaration_text, file_id)
                                {
                                    let hover_text = format!(
                                        "```groovy\ndef {}: {} // inferred\n```\n\nLocal variable with inferred type",
                                        identifier, inferred_type
                                    );
                                    return Ok(Some(self.create_hover(hover_text)));
                                }
                            }
                        }
                    }
                }
            }

            if let Some(imports) = self.workspace_index.get_imports(file_id) {
                let usages = hyperion_workspace::groovy_usage_extractor::GroovyUsageExtractor::extract_from_tree(
                    source_text,
                    file_id,
                    &imports,
                    self.workspace_index,
                    &tree,
                )
                .map_err(|error| format!("Failed to resolve Groovy hover usages: {error}"))?;
                if let Some(symbol) = usages
                    .iter()
                    .find(|usage| usage.location.range.start as usize == node.start_byte())
                    .and_then(|usage| self.workspace_index.find_symbol(&usage.fqn))
                {
                    let hover_text = self.format_symbol_hover(&symbol);
                    return Ok(Some(self.create_hover(hover_text)));
                }

                if let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
                    .workspace_index
                    .resolve_type_name(file_id, &[], &identifier)
                {
                    if let Some(symbol) = self.workspace_index.find_symbol(&fqn) {
                        let hover_text = self.format_symbol_hover(&symbol);
                        return Ok(Some(self.create_hover(hover_text)));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Resolve a Groovy variable's type
    fn resolve_variable_type(&self, file_id: FileId, var_name: &str) -> Option<String> {
        if let Some(symbol) = self.best_simple_name_symbol(file_id, var_name) {
            if let Some(return_type) = &self
                .workspace_index
                .get_symbol(&symbol.fqn)
                .and_then(|s| s.return_type.as_ref().map(|s| s.to_string()))
            {
                return Some(return_type.to_string());
            }
        }
        None
    }

    fn best_simple_name_symbol(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        let candidates = self
            .workspace_index
            .find_by_simple_name(name)
            .into_iter()
            .filter(|candidate| candidate.file_id == file_id)
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        Some(candidate.clone())
    }

    /// Infer the type of a Groovy expression from the source text
    ///
    /// Handles common patterns:
    /// - `new ClassName()` -> ClassName
    /// - String/number/boolean literals
    /// - List/map literals
    pub fn infer_expression_type(&self, declaration_text: &str, file_id: FileId) -> Option<String> {
        // Look for common patterns in the declaration text
        let text = declaration_text.trim();

        // Pattern: new ClassName()
        if let Some(new_pos) = text.find("new ") {
            let after_new = &text[new_pos + 4..];
            // Extract the class name (until '(' or whitespace)
            let type_name: String = after_new
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !type_name.is_empty() {
                // Try to resolve the type through workspace index
                let imports = self.workspace_index.get_imports(file_id);
                let package = imports
                    .as_ref()
                    .and_then(|i| i.current_package.as_ref())
                    .cloned();

                // Check same package
                if let Some(pkg) = &package {
                    let fqn = format!("{}.{}", pkg, type_name);
                    if self.workspace_index.find_symbol(&fqn).is_some() {
                        return Some(type_name);
                    }
                }

                // Check explicit imports
                if let Some(imports) = imports {
                    if let Some(fqn) = imports.explicit.get(&type_name) {
                        if self.workspace_index.find_symbol(fqn).is_some() {
                            return Some(type_name);
                        }
                    }
                }

                // Check by simple name
                if self.workspace_index.find_symbol(&type_name).is_some() {
                    return Some(type_name);
                }

                // Return type name even if not in workspace (might be JDK type)
                return Some(type_name);
            }
        }

        // Pattern: string literals
        if text.contains('"') || text.contains('\'') {
            return Some("String".to_string());
        }

        // Pattern: list literal [...]
        if text.contains('[') && text.contains(']') && !text.contains(':') {
            return Some("List".to_string());
        }

        // Pattern: map literal [:]
        if text.contains('[') && text.contains(':') && text.contains(']') {
            return Some("Map".to_string());
        }

        // Pattern: closure { ... }
        if text.contains('{') && text.contains('}') {
            return Some("Closure".to_string());
        }

        // Pattern: boolean literals
        if text.contains("true") || text.contains("false") {
            return Some("Boolean".to_string());
        }

        // Pattern: numeric literals
        if text.chars().any(|c| c.is_ascii_digit()) {
            // Check if it's a number (not a method call like foo1())
            let after_equals = text.rsplit('=').next().unwrap_or("").trim();
            if after_equals
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
            {
                return Some("Number".to_string());
            }
        }

        // Pattern: method call return types (common Groovy/Java methods)
        let after_equals = text.rsplit('=').next().unwrap_or("").trim();
        if let Some(return_type) = Self::infer_method_return_type(after_equals) {
            return Some(return_type);
        }

        None
    }

    /// Infer return type from a Groovy/Java method call
    fn infer_method_return_type(expr: &str) -> Option<String> {
        // Extract the last method call in the expression
        // e.g., "list.collect { it.name }" -> "collect"
        // e.g., "users.findAll { it.active }.size()" -> "size"

        // Common collection method return types
        let method_return_types: &[(&str, &str)] = &[
            // Collection methods that return collections
            ("collect", "List"),
            ("findAll", "List"),
            ("grep", "List"),
            ("sort", "List"),
            ("reverse", "List"),
            ("unique", "List"),
            ("take", "List"),
            ("drop", "List"),
            ("takeWhile", "List"),
            ("dropWhile", "List"),
            ("flatten", "List"),
            ("plus", "List"),
            ("minus", "List"),
            ("intersect", "List"),
            // Collection methods that return single values
            ("find", "Object"),
            ("first", "Object"),
            ("last", "Object"),
            ("head", "Object"),
            ("tail", "List"),
            ("init", "List"),
            ("max", "Object"),
            ("min", "Object"),
            ("sum", "Number"),
            // Collection methods that return numbers
            ("size", "Integer"),
            ("count", "Integer"),
            ("indexOf", "Integer"),
            ("lastIndexOf", "Integer"),
            // Collection methods that return booleans
            ("any", "Boolean"),
            ("every", "Boolean"),
            ("contains", "Boolean"),
            ("isEmpty", "Boolean"),
            // Collection methods that return maps
            ("groupBy", "Map"),
            ("collectEntries", "Map"),
            ("withIndex", "List"),
            // String methods
            ("toUpperCase", "String"),
            ("toLowerCase", "String"),
            ("trim", "String"),
            ("substring", "String"),
            ("replace", "String"),
            ("replaceAll", "String"),
            ("split", "String[]"),
            ("join", "String"),
            ("padLeft", "String"),
            ("padRight", "String"),
            ("reverse", "String"),
            ("capitalize", "String"),
            // Type conversion methods
            ("toString", "String"),
            ("toInteger", "Integer"),
            ("toLong", "Long"),
            ("toDouble", "Double"),
            ("toFloat", "Float"),
            ("toBigDecimal", "BigDecimal"),
            ("toList", "List"),
            ("toSet", "Set"),
            ("asType", "Object"),
            // Iterator/stream methods
            ("iterator", "Iterator"),
            ("stream", "Stream"),
            ("parallelStream", "Stream"),
            // Map methods
            ("keySet", "Set"),
            ("values", "Collection"),
            ("entrySet", "Set"),
            ("getOrDefault", "Object"),
            ("computeIfAbsent", "Object"),
            // File methods (common in Gradle)
            ("getText", "String"),
            ("getBytes", "byte[]"),
            ("readLines", "List"),
            ("eachLine", "Object"),
            ("withReader", "Object"),
            ("withWriter", "Object"),
        ];

        // Check if any known method is in the expression
        for (method, return_type) in method_return_types {
            // Look for method call pattern: .methodName( or .methodName {
            let patterns = [
                format!(".{}(", method),
                format!(".{} {{", method),
                format!(".{} ", method),
            ];

            for pattern in &patterns {
                if expr.contains(pattern.as_str()) {
                    return Some(return_type.to_string());
                }
            }
        }

        None
    }

    /// Format a symbol for Groovy hover display
    fn format_symbol_hover(&self, symbol: &hyperion_workspace::SymbolLocation) -> String {
        if let Some(full_symbol) = self.workspace_index.get_symbol(&symbol.fqn) {
            let kind = match full_symbol.kind {
                lsp_types::SymbolKind::CLASS => "class",
                lsp_types::SymbolKind::INTERFACE => "interface",
                lsp_types::SymbolKind::ENUM => "enum",
                lsp_types::SymbolKind::METHOD => "method",
                lsp_types::SymbolKind::FIELD => "field",
                lsp_types::SymbolKind::CONSTRUCTOR => "constructor",
                _ => "symbol",
            };

            let mut hover = format!("```groovy\n{} {}", kind, full_symbol.simple_name);

            if let Some(ref return_type) = full_symbol.return_type {
                hover.push_str(&format!(": {}", return_type));
            }

            hover.push_str("\n```");

            if !full_symbol.parameters.is_empty() {
                hover.push_str(&format!(
                    "\n\nParameters: `{}`",
                    full_symbol.parameters.join(", ")
                ));
            }

            hover
        } else {
            format!("```groovy\n{}\n```", symbol.fqn)
        }
    }

    /// Create a Hover response with markup content
    const fn create_hover(&self, text: String) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: text,
            }),
            range: None,
        }
    }
}
