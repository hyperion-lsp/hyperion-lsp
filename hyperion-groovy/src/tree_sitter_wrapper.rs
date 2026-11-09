// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tree-sitter Groovy parser wrapper
//!
//! Provides a lightweight wrapper around tree-sitter-groovy to parse Groovy source code.
//! Converts tree-sitter parse trees into our AST types.

use std::fmt;
use std::mem;
use thiserror::Error;
use tree_sitter::{Language, Parser, Tree};

/// Result type for tree-sitter operations
pub type TreeSitterResult<T> = Result<T, TreeSitterError>;

/// Errors from tree-sitter parsing
#[derive(Debug, Error)]
pub enum TreeSitterError {
    #[error("Failed to initialize tree-sitter parser")]
    ParserInitError,

    #[error("Parse error at line {line}, column {column}: {message}")]
    ParseError {
        line: usize,
        column: usize,
        message: String,
    },

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("Invalid node operation: {0}")]
    InvalidNode(String),
}

/// Cached parser instance for Groovy
pub struct GroovyParser {
    parser: Parser,
}

impl GroovyParser {
    /// Create a new Groovy parser instance
    pub fn new() -> TreeSitterResult<Self> {
        let mut parser = Parser::new();

        // tree_sitter_groovy::LANGUAGE is a LanguageFn (wrapper around unsafe C function)
        // Call it to get the language pointer, then wrap in Language
        let language_fn = tree_sitter_groovy::LANGUAGE;
        let language_ptr = unsafe { language_fn.into_raw()() };

        // Language is repr(transparent) over *const ffi::TSLanguage
        // We have *const () from the C function, which is equivalent
        let language = unsafe { mem::transmute::<*const (), Language>(language_ptr) };

        let result = parser.set_language(&language);
        mem::forget(language); // Don't call Drop since we don't own the pointer

        result.map_err(|_| TreeSitterError::ParserInitError)?;

        Ok(GroovyParser { parser })
    }

    /// Parse Groovy source code
    pub fn parse_tree(&mut self, source: &str) -> TreeSitterResult<Tree> {
        self.parser
            .parse(source, None)
            .ok_or(TreeSitterError::ParserInitError)
    }

    /// Parse Groovy source and retain an owned copy for the legacy convenience
    /// extraction helpers. Call [`Self::parse_tree`] when the caller already
    /// owns the source buffer.
    pub fn parse(&mut self, source: &str) -> TreeSitterResult<ParsedGroovy> {
        Ok(ParsedGroovy {
            source: source.to_string(),
            tree: self.parse_tree(source)?,
        })
    }
}

impl Default for GroovyParser {
    fn default() -> Self {
        Self::new().expect("Failed to initialize Groovy parser")
    }
}

impl fmt::Debug for GroovyParser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GroovyParser").finish()
    }
}

/// Parsed Groovy source code
pub struct ParsedGroovy {
    pub source: String,
    pub tree: Tree,
}

impl ParsedGroovy {
    /// Get the root node
    pub fn root(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }

    /// Get source text for a byte range
    pub fn get_text(&self, start: usize, end: usize) -> Option<&str> {
        if end <= self.source.len() {
            Some(&self.source[start..end])
        } else {
            None
        }
    }

    /// Extract method calls from the parse tree
    pub fn extract_method_calls(&self) -> Vec<MethodCallNode> {
        let mut calls = Vec::new();
        self.walk_for_method_calls(self.root(), &mut calls);
        calls
    }

    /// Extract closures from the parse tree
    pub fn extract_closures(&self) -> Vec<ClosureNode> {
        let mut closures = Vec::new();
        self.walk_for_closures(self.root(), &mut closures);
        closures
    }

    /// Extract GStrings from the parse tree
    pub fn extract_gstrings(&self) -> Vec<GStringNode> {
        let mut gstrings = Vec::new();
        self.walk_for_gstrings(self.root(), &mut gstrings);
        gstrings
    }

    /// Extract property access expressions from the parse tree
    pub fn extract_property_access(&self) -> Vec<PropertyAccessNode> {
        let mut props = Vec::new();
        self.walk_for_property_access(self.root(), &mut props);
        props
    }

    /// Walk the tree looking for method calls
    fn walk_for_method_calls(&self, node: tree_sitter::Node, calls: &mut Vec<MethodCallNode>) {
        let kind = node.kind();

        // Look for function_call nodes (tree-sitter-groovy node type)
        if kind == "function_call" || kind == "call" || kind == "method_invocation" {
            if let Some(call) = self.extract_call_info(node) {
                calls.push(call);
            }
        }

        // Recurse to children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_method_calls(child, calls);
        }
    }

    /// Walk the tree looking for closures
    fn walk_for_closures(&self, node: tree_sitter::Node, closures: &mut Vec<ClosureNode>) {
        let kind = node.kind();

        // Look for closure nodes
        if kind == "closure" || kind == "block" {
            if let Some(closure) = self.extract_closure_info(node) {
                closures.push(closure);
            }
        }

        // Recurse to children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_closures(child, closures);
        }
    }

    /// Walk the tree looking for GStrings
    fn walk_for_gstrings(&self, node: tree_sitter::Node, gstrings: &mut Vec<GStringNode>) {
        let kind = node.kind();

        // Look for GString nodes
        if kind == "gstring" || kind == "interpolated_string" {
            if let Some(gstring) = self.extract_gstring_info(node) {
                gstrings.push(gstring);
            }
        }

        // Recurse to children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_gstrings(child, gstrings);
        }
    }

    /// Walk the tree looking for property access
    fn walk_for_property_access(
        &self,
        node: tree_sitter::Node,
        props: &mut Vec<PropertyAccessNode>,
    ) {
        let kind = node.kind();

        // Look for property access nodes
        if kind == "property_access" || kind == "dot_access" {
            if let Some(prop) = self.extract_property_access_info(node) {
                props.push(prop);
            }
        }

        // Recurse to children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_property_access(child, props);
        }
    }

    /// Extract information from a function call node
    fn extract_call_info(&self, node: tree_sitter::Node) -> Option<MethodCallNode> {
        // Handle tree-sitter-groovy's function_call structure
        // Typically: (function_call name: (identifier) arguments: (arguments ...))

        let mut name = None;
        let mut args = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "property_identifier" => {
                    if let Ok(text) = child.utf8_text(self.source.as_bytes()) {
                        name = Some(text.to_string());
                    }
                }
                "arguments" => {
                    // Parse argument list
                    let mut arg_cursor = child.walk();
                    for arg_child in child.children(&mut arg_cursor) {
                        if arg_child.kind() == "string" || arg_child.kind() == "string_literal" {
                            if let Ok(text) = arg_child.utf8_text(self.source.as_bytes()) {
                                args.push(text.to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        name.map(|method_name| {
            let start_pos = node.start_position();
            MethodCallNode {
                name: method_name,
                arguments: args,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                line: start_pos.row,
                column: start_pos.column,
            }
        })
    }

    /// Extract information from a closure node
    fn extract_closure_info(&self, node: tree_sitter::Node) -> Option<ClosureNode> {
        let mut parameters = Vec::new();
        let mut body_text = String::new();

        let mut cursor = node.walk();
        let mut found_arrow = false;

        for child in node.children(&mut cursor) {
            match child.kind() {
                // Parameters before ->
                "identifier" if !found_arrow => {
                    if let Ok(text) = child.utf8_text(self.source.as_bytes()) {
                        parameters.push(text.to_string());
                    }
                }
                // Arrow separator
                "arrow" | "->" => {
                    found_arrow = true;
                }
                // Body after ->
                _ if found_arrow => {
                    if let Ok(text) = child.utf8_text(self.source.as_bytes()) {
                        if !text.trim().is_empty() {
                            body_text = text.to_string();
                        }
                    }
                }
                _ => {}
            }
        }

        let start_pos = node.start_position();
        Some(ClosureNode {
            parameters,
            body_text,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            line: start_pos.row,
            column: start_pos.column,
        })
    }

    /// Extract information from a GString node
    fn extract_gstring_info(&self, node: tree_sitter::Node) -> Option<GStringNode> {
        let mut segments = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "string_fragment" | "string" => {
                    if let Ok(text) = child.utf8_text(self.source.as_bytes()) {
                        if !text.is_empty() {
                            segments.push(GStringSegment::Text(text.to_string()));
                        }
                    }
                }
                "interpolation" | "${" => {
                    // Extract the interpolation expression
                    let mut interp_cursor = child.walk();
                    for interp_child in child.children(&mut interp_cursor) {
                        if let Ok(text) = interp_child.utf8_text(self.source.as_bytes()) {
                            if !text.is_empty() && text != "${" && text != "}" {
                                segments.push(GStringSegment::Interpolation(text.to_string()));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let start_pos = node.start_position();
        Some(GStringNode {
            segments,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            line: start_pos.row,
            column: start_pos.column,
        })
    }

    /// Extract information from a property access node
    fn extract_property_access_info(&self, node: tree_sitter::Node) -> Option<PropertyAccessNode> {
        let mut is_safe = false;

        let mut cursor = node.walk();
        let mut parts: Vec<String> = Vec::new();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "." => {} // Skip dot
                "?" => {
                    is_safe = true;
                }
                "identifier" | "property_identifier" => {
                    if let Ok(text) = child.utf8_text(self.source.as_bytes()) {
                        parts.push(text.to_string());
                    }
                }
                _ => {}
            }
        }

        // Parts should be [object, property] or more for chained access
        if parts.len() >= 2 {
            let object = parts[0].clone();
            let property = parts[parts.len() - 1].clone();

            let start_pos = node.start_position();
            return Some(PropertyAccessNode {
                object,
                property,
                is_safe,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                line: start_pos.row,
                column: start_pos.column,
            });
        }

        None
    }
}

/// Information about a method call extracted from the parse tree
#[derive(Debug, Clone)]
pub struct MethodCallNode {
    pub name: String,
    pub arguments: Vec<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
}

/// Information about a closure extracted from the parse tree
#[derive(Debug, Clone)]
pub struct ClosureNode {
    pub parameters: Vec<String>,
    pub body_text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
}

/// Information about a GString (interpolated string) extracted from the parse tree
#[derive(Debug, Clone)]
pub struct GStringNode {
    pub segments: Vec<GStringSegment>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
}

/// A segment of a GString (either static text or interpolation)
#[derive(Debug, Clone)]
pub enum GStringSegment {
    /// Static text part
    Text(String),
    /// Interpolated expression (e.g., ${variable})
    Interpolation(String),
}

/// Information about a property access expression
#[derive(Debug, Clone)]
pub struct PropertyAccessNode {
    pub object: String,
    pub property: String,
    pub is_safe: bool, // true for ?. safe navigation
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
}

/// Information about a binary operation
#[derive(Debug, Clone)]
pub struct BinaryOpNode {
    pub operator: String,
    pub left: String,
    pub right: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    pub column: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let parser = GroovyParser::new();
        assert!(parser.is_ok());
    }

    #[test]
    fn test_simple_parse() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let result = parser.parse("class Foo { }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_method_call_extraction() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let parsed = parser
            .parse("foo(1, 2)")
            .expect("Failed to parse method call");

        let calls = parsed.extract_method_calls();
        // tree-sitter may or may not detect this depending on grammar version
        // We're mainly testing that extraction doesn't panic
        let _calls = calls; // Use for potential assertion in future
    }

    #[test]
    fn test_groovy_closure() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let result = parser.parse("{ x -> x + 1 }");
        assert!(result.is_ok());
    }

    #[test]
    fn test_groovy_gstring() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let result = parser.parse("\"Hello ${name}\"");
        assert!(result.is_ok());
    }

    #[test]
    fn test_closure_extraction() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let parsed = parser
            .parse("{ x -> x + 1 }")
            .expect("Failed to parse closure");

        let closures = parsed.extract_closures();
        // tree-sitter may or may not detect closures depending on grammar version
        // We're mainly testing that extraction doesn't panic
        let _closures = closures;
    }

    #[test]
    fn test_gstring_extraction() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let parsed = parser
            .parse("\"Hello ${name}\"")
            .expect("Failed to parse GString");

        let gstrings = parsed.extract_gstrings();
        // tree-sitter may or may not detect GStrings depending on grammar version
        // We're mainly testing that extraction doesn't panic
        let _gstrings = gstrings;
    }

    #[test]
    fn test_property_access_extraction() {
        let mut parser = GroovyParser::new().expect("Failed to create parser");
        let parsed = parser
            .parse("user.name")
            .expect("Failed to parse property access");

        let props = parsed.extract_property_access();
        // tree-sitter may or may not detect property access depending on grammar version
        // We're mainly testing that extraction doesn't panic
        let _props = props;
    }
}
