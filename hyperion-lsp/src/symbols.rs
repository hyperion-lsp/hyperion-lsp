// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Document Symbols Provider (Java)
//!
//! This module implements document symbols functionality for the Hyperion LSP.
//! It provides document outline information showing the structure of classes,
//! interfaces, methods, and fields in a source file.
//!
//! The tree walker below interprets raw rowan kinds through
//! `hyperion_parser::SyntaxKind`, so it is only valid for **Java** trees. Kotlin
//! is parsed by `hyperion-kotlin` into a tree with a completely different kind
//! numbering; [`document_symbols`](SymbolsProvider::document_symbols) therefore
//! hands Kotlin files to [`crate::symbols_kotlin`]. The LSP request itself is
//! dispatched through `LanguageHandler`, but the guard here keeps the mistake
//! unrepresentable for every other caller too.

use hyperion_parser::syntax::{AstNode, Param, RecordDecl, SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, Language, ParseDatabase, SourceDatabase};
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind as LspSymbolKind};
use tracing::debug;

use crate::position_utils::LineIndex;
use crate::semantic_host::ReadTxn;

/// Provides document symbols (outline) functionality
#[derive(Default)]
pub struct SymbolsProvider;

impl SymbolsProvider {
    /// Creates a new SymbolsProvider
    pub const fn new() -> Self {
        Self
    }

    /// Build a Java outline directly from the authoritative editor buffer.
    ///
    /// Document symbols do not need incremental semantic state. Parsing the supplied buffer keeps
    /// this request independent from background Salsa snapshots and from the separate workspace
    /// FileId allocation domain.
    pub fn java_document_symbols_from_source(
        &self,
        source_text: &str,
    ) -> Result<Vec<DocumentSymbol>, String> {
        let tokens = hyperion_parser::Lexer::new(source_text).collect();
        let (green, _errors) = hyperion_parser::Parser::parse(tokens);
        let root = SyntaxNode::new_root(green);

        let lines = LineIndex::new(source_text);
        let mut symbols = Vec::new();
        for node in root.children() {
            if let Some(symbol) = self.node_to_document_symbol(&node, source_text, &lines) {
                symbols.push(symbol);
            }
        }
        Ok(symbols)
    }

    /// Get document symbols (outline) for the given file
    ///
    /// # Arguments
    ///
    /// * `file_id` - The FileId of the document
    /// * `source_text` - The source text of the document
    ///
    /// # Returns
    ///
    /// A vector of DocumentSymbol representing the document structure
    pub fn document_symbols(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        source_text: &str,
    ) -> Result<Vec<DocumentSymbol>, String> {
        let (language, parse_tree) = {
            if txn.file_text(file_id).as_str() != source_text {
                return Ok(Vec::new());
            }
            (txn.file_language(file_id), txn.parse_file(file_id))
        };

        // Kotlin trees use hyperion-kotlin's SyntaxKind numbering. Walking them
        // with the Java kinds below produces wrong (and sometimes nonsensical)
        // symbols, so delegate instead of reinterpreting.
        if language == Language::Kotlin {
            let root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());
            let symbols = crate::symbols_kotlin::kotlin_document_symbols(&root, source_text);
            debug!("Found {} top-level Kotlin symbols", symbols.len());
            return Ok(symbols);
        }

        let lines = LineIndex::new(source_text);
        let mut symbols = Vec::new();

        // Walk top-level declarations
        for node in parse_tree.syntax().children() {
            if let Some(symbol) = self.node_to_document_symbol(&node, source_text, &lines) {
                symbols.push(symbol);
            }
        }

        debug!("Found {} top-level symbols", symbols.len());
        Ok(symbols)
    }

    /// Convert a syntax node to a DocumentSymbol
    #[allow(deprecated)]
    fn node_to_document_symbol(
        &self,
        node: &SyntaxNode,
        source: &str,
        lines: &LineIndex,
    ) -> Option<DocumentSymbol> {
        match node.kind() {
            SyntaxKind::CLASS_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                // Get class members as children
                let children = node
                    .children()
                    .filter_map(|child| self.node_to_document_symbol(&child, source, lines))
                    .collect::<Vec<_>>();

                Some(DocumentSymbol {
                    name,
                    detail: Some("class".to_string()),
                    kind: LspSymbolKind::CLASS,
                    range,
                    selection_range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::INTERFACE_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                let children = node
                    .children()
                    .filter_map(|child| self.node_to_document_symbol(&child, source, lines))
                    .collect::<Vec<_>>();

                Some(DocumentSymbol {
                    name,
                    detail: Some("interface".to_string()),
                    kind: LspSymbolKind::INTERFACE,
                    range,
                    selection_range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::ANNOTATION_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);
                let children = node
                    .children()
                    .filter_map(|child| self.node_to_document_symbol(&child, source, lines))
                    .collect::<Vec<_>>();

                Some(DocumentSymbol {
                    name,
                    detail: Some("annotation".to_string()),
                    kind: LspSymbolKind::INTERFACE,
                    range,
                    selection_range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::METHOD_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                // Try to extract method signature for detail
                let detail = self.extract_method_signature(node, source);

                Some(DocumentSymbol {
                    name,
                    detail,
                    kind: LspSymbolKind::METHOD,
                    range,
                    selection_range,
                    children: None,
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::CONSTRUCTOR_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                // Try to extract constructor signature for detail
                let detail = self.extract_constructor_signature(node);

                Some(DocumentSymbol {
                    name,
                    detail,
                    kind: LspSymbolKind::CONSTRUCTOR,
                    range,
                    selection_range,
                    children: None,
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::FIELD_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                // Try to extract field type for detail
                let detail = self.extract_field_type(node, source);

                Some(DocumentSymbol {
                    name,
                    detail,
                    kind: LspSymbolKind::FIELD,
                    range,
                    selection_range,
                    children: None,
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::ENUM_DECL => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                let children = node
                    .children()
                    .filter_map(|child| self.node_to_document_symbol(&child, source, lines))
                    .collect::<Vec<_>>();

                Some(DocumentSymbol {
                    name,
                    detail: Some("enum".to_string()),
                    kind: LspSymbolKind::ENUM,
                    range,
                    selection_range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::RECORD_DECL => {
                let record = RecordDecl::cast(node.clone())?;
                let name = record.name()?.text().to_string();
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                let mut children = Vec::new();
                if let Some(components) = record.components() {
                    children.extend(components.params().filter_map(|component| {
                        self.record_component_symbol(&component, source, lines)
                    }));
                }
                children.extend(
                    node.children()
                        .filter_map(|child| self.node_to_document_symbol(&child, source, lines)),
                );

                Some(DocumentSymbol {
                    name,
                    detail: Some("record".to_string()),
                    kind: LspSymbolKind::STRUCT,
                    range,
                    selection_range,
                    children: if children.is_empty() {
                        None
                    } else {
                        Some(children)
                    },
                    tags: None,
                    deprecated: None,
                })
            }

            SyntaxKind::ENUM_CONSTANT => {
                let name = self.extract_name(node)?;
                let range = self.node_to_range(node, source, lines);
                let selection_range = self.name_range(node, source, lines).unwrap_or(range);

                Some(DocumentSymbol {
                    name,
                    detail: Some("constant".to_string()),
                    kind: LspSymbolKind::ENUM_MEMBER,
                    range,
                    selection_range,
                    children: None,
                    tags: None,
                    deprecated: None,
                })
            }

            _ => None,
        }
    }

    #[allow(deprecated)]
    fn record_component_symbol(
        &self,
        component: &Param,
        source: &str,
        lines: &LineIndex,
    ) -> Option<DocumentSymbol> {
        let name_token = component.name()?;
        let range = self.node_to_range(component.syntax(), source, lines);
        let selection_range = {
            let text_range = name_token.text_range();
            Range {
                start: lines.offset_to_position(source, usize::from(text_range.start())),
                end: lines.offset_to_position(source, usize::from(text_range.end())),
            }
        };
        let detail = component
            .ty()
            .map(|ty| Self::extract_type_text(ty.syntax()));

        Some(DocumentSymbol {
            name: name_token.text().to_string(),
            detail,
            kind: LspSymbolKind::FIELD,
            range,
            selection_range,
            children: None,
            tags: None,
            deprecated: None,
        })
    }

    /// Extract the name from a declaration node
    fn extract_name(&self, node: &SyntaxNode) -> Option<String> {
        // Find IDENT token in children
        node.children_with_tokens().find_map(|elem| {
            elem.as_token()
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| token.text().to_string())
        })
    }

    /// Get the range of just the name token (IDENT) for selection_range
    fn name_range(&self, node: &SyntaxNode, source: &str, lines: &LineIndex) -> Option<Range> {
        node.children_with_tokens().find_map(|elem| {
            elem.as_token()
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| {
                    let text_range = token.text_range();
                    let start = lines.offset_to_position(source, usize::from(text_range.start()));
                    let end = lines.offset_to_position(source, usize::from(text_range.end()));
                    Range { start, end }
                })
        })
    }

    /// Convert a syntax node to an LSP Range
    fn node_to_range(&self, node: &SyntaxNode, source: &str, lines: &LineIndex) -> Range {
        let text_range = node.text_range();
        let start = lines.offset_to_position(source, usize::from(text_range.start()));
        let end = lines.offset_to_position(source, usize::from(text_range.end()));
        Range { start, end }
    }

    /// Extract method signature for display in outline
    fn extract_method_signature(&self, node: &SyntaxNode, _source: &str) -> Option<String> {
        let mut modifiers = Vec::new();
        let mut return_type = None;
        let mut method_name = None;
        let mut parameters = Vec::new();

        // Walk children to extract signature components
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::PUBLIC_KW
                    | SyntaxKind::PRIVATE_KW
                    | SyntaxKind::PROTECTED_KW
                    | SyntaxKind::STATIC_KW
                    | SyntaxKind::FINAL_KW
                    | SyntaxKind::ABSTRACT_KW
                    | SyntaxKind::SYNCHRONIZED_KW => {
                        modifiers.push(token.text().to_string());
                    }
                    SyntaxKind::IDENT if method_name.is_none() && return_type.is_some() => {
                        method_name = Some(token.text().to_string());
                    }
                    SyntaxKind::VOID_KW if return_type.is_none() => {
                        return_type = Some("void".to_string());
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(child_node) => {
                    match child_node.kind() {
                        SyntaxKind::CLASS_TYPE
                        | SyntaxKind::PRIMITIVE_TYPE
                        | SyntaxKind::ARRAY_TYPE
                            if return_type.is_none() =>
                        {
                            return_type = Some(Self::extract_type_text(&child_node));
                        }
                        SyntaxKind::PARAM_LIST => {
                            // Extract parameters
                            for param in child_node.children() {
                                if param.kind() == SyntaxKind::PARAM {
                                    if let Some(param_str) = self.extract_parameter_text(&param) {
                                        parameters.push(param_str);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Build signature
        let mut signature = String::new();

        // Add modifiers
        if !modifiers.is_empty() {
            signature.push_str(&modifiers.join(" "));
            signature.push(' ');
        }

        // Add return type
        if let Some(ret) = return_type {
            signature.push_str(&ret);
            signature.push(' ');
        }

        // Add method name
        {
            let name = method_name?;
            signature.push_str(&name);
        }

        // Add parameters
        signature.push('(');
        signature.push_str(&parameters.join(", "));
        signature.push(')');

        Some(signature)
    }

    /// Extract constructor signature for display in outline
    fn extract_constructor_signature(&self, node: &SyntaxNode) -> Option<String> {
        let mut modifiers = Vec::new();
        let mut constructor_name = None;
        let mut parameters = Vec::new();

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::PUBLIC_KW | SyntaxKind::PRIVATE_KW | SyntaxKind::PROTECTED_KW => {
                        modifiers.push(token.text().to_string());
                    }
                    SyntaxKind::IDENT if constructor_name.is_none() => {
                        constructor_name = Some(token.text().to_string());
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(child_node) => {
                    if child_node.kind() == SyntaxKind::PARAM_LIST {
                        for param in child_node.children() {
                            if param.kind() == SyntaxKind::PARAM {
                                if let Some(param_str) = self.extract_parameter_text(&param) {
                                    parameters.push(param_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build signature
        let mut signature = String::new();

        if !modifiers.is_empty() {
            signature.push_str(&modifiers.join(" "));
            signature.push(' ');
        }

        {
            let name = constructor_name?;
            signature.push_str(&name);
        }

        signature.push('(');
        signature.push_str(&parameters.join(", "));
        signature.push(')');

        Some(signature)
    }

    /// Extract field type for display in outline
    fn extract_field_type(&self, node: &SyntaxNode, _source: &str) -> Option<String> {
        let mut modifiers = Vec::new();
        let mut field_type = None;
        let mut field_name = None;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::PUBLIC_KW
                    | SyntaxKind::PRIVATE_KW
                    | SyntaxKind::PROTECTED_KW
                    | SyntaxKind::STATIC_KW
                    | SyntaxKind::FINAL_KW
                    | SyntaxKind::VOLATILE_KW
                    | SyntaxKind::TRANSIENT_KW => {
                        modifiers.push(token.text().to_string());
                    }
                    SyntaxKind::IDENT if field_name.is_none() && field_type.is_some() => {
                        field_name = Some(token.text().to_string());
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(child_node) => match child_node.kind() {
                    SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                        if field_type.is_none() =>
                    {
                        field_type = Some(Self::extract_type_text(&child_node));
                    }
                    _ => {}
                },
            }
        }

        // Build field type string
        let mut result = String::new();

        if !modifiers.is_empty() {
            result.push_str(&modifiers.join(" "));
            result.push(' ');
        }

        {
            let ftype = field_type?;
            result.push_str(&ftype);
        }

        Some(result)
    }

    /// Extract parameter text from a PARAM node
    fn extract_parameter_text(&self, node: &SyntaxNode) -> Option<String> {
        let mut param_type = None;
        let mut param_name = None;
        let mut is_final = false;
        let mut is_varargs = false;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::FINAL_KW => is_final = true,
                    SyntaxKind::ELLIPSIS => is_varargs = true,
                    SyntaxKind::IDENT if param_name.is_none() && param_type.is_some() => {
                        param_name = Some(token.text().to_string());
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(child_node) => match child_node.kind() {
                    SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                        if param_type.is_none() =>
                    {
                        param_type = Some(Self::extract_type_text(&child_node));
                    }
                    _ => {}
                },
            }
        }

        // Build parameter string
        let mut result = String::new();

        if is_final {
            result.push_str("final ");
        }

        if let Some(ptype) = param_type {
            result.push_str(&ptype);
            if is_varargs {
                result.push_str("...");
            }
            result.push(' ');
        }

        if let Some(name) = param_name {
            result.push_str(&name);
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Extract type text from a type node (CLASS_TYPE, PRIMITIVE_TYPE, ARRAY_TYPE)
    #[allow(clippy::only_used_in_recursion)]
    fn extract_type_text(node: &SyntaxNode) -> String {
        match node.kind() {
            SyntaxKind::PRIMITIVE_TYPE => {
                // Extract primitive type keyword
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if matches!(
                            token.kind(),
                            SyntaxKind::INT_KW
                                | SyntaxKind::LONG_KW
                                | SyntaxKind::SHORT_KW
                                | SyntaxKind::BYTE_KW
                                | SyntaxKind::CHAR_KW
                                | SyntaxKind::FLOAT_KW
                                | SyntaxKind::DOUBLE_KW
                                | SyntaxKind::BOOLEAN_KW
                        ) {
                            return token.text().to_string();
                        }
                    }
                }
                "unknown".to_string()
            }
            SyntaxKind::CLASS_TYPE => {
                // Qualified and generic class types are nested syntax trees;
                // using direct child tokens drops names such as `String`.
                node.text().to_string().trim().to_string()
            }
            SyntaxKind::ARRAY_TYPE => {
                // Extract element type and array brackets
                let mut element_type = String::new();
                let mut bracket_count = 0;

                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(child_node) => {
                            if matches!(
                                child_node.kind(),
                                SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::ARRAY_TYPE
                            ) {
                                element_type = Self::extract_type_text(&child_node);
                            }
                        }
                        rowan::NodeOrToken::Token(token) => {
                            if token.kind() == SyntaxKind::L_BRACKET {
                                bracket_count += 1;
                            }
                        }
                    }
                }

                format!("{}{}", element_type, "[]".repeat(bracket_count))
            }
            _ => node.text().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::{Language, SourceDatabase};
    use std::sync::Arc;

    #[test]
    fn mismatched_parse_source_does_not_produce_ranges() {
        let host = SemanticHost::for_tests();
        let file_id = FileId::new(1);
        let source = "class Example {\n // https://example.com/docs\n}";
        host.with_db(|db| {
            db.register_file(file_id, Arc::new(source.to_owned()), Language::Java);
        });
        let provider = SymbolsProvider::new();
        assert!(!provider
            .document_symbols(&host.read_now(), file_id, source)
            .unwrap()
            .is_empty());
        assert!(provider
            .document_symbols(&host.read_now(), file_id, &format!("\n{source}"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_symbols_provider_creation() {
        let _host = SemanticHost::for_tests();
        let _provider = SymbolsProvider::new();
        // Just verify it compiles and creates
    }

    #[test]
    fn test_document_symbols_empty_file() {
        let host = SemanticHost::for_tests();
        let provider = SymbolsProvider::new();

        let file_id = FileId::new(0);
        let source = "";

        // Set file text in database
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        let symbols = provider
            .document_symbols(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(symbols.len(), 0);
    }

    #[test]
    fn test_document_symbols_simple_class() {
        let host = SemanticHost::for_tests();
        let provider = SymbolsProvider::new();

        let file_id = FileId::new(0);
        let source = r"
class MyClass {
    int field;
    void method() {}
}
";

        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        let symbols = provider
            .document_symbols(&host.read_now(), file_id, source)
            .unwrap();

        // We should find at least the class
        // The exact structure depends on the parser
        assert!(!symbols.is_empty(), "Should find at least one symbol");
    }

    #[test]
    fn test_document_symbols_constructor() {
        let host = SemanticHost::for_tests();
        let provider = SymbolsProvider::new();

        let file_id = FileId::new(0);
        let source = r"
class MyClass {
    public MyClass() {}
    public MyClass(int x) {}
}
";

        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        let symbols = provider
            .document_symbols(&host.read_now(), file_id, source)
            .unwrap();

        // Should find the class
        assert_eq!(symbols.len(), 1);
        let class = &symbols[0];
        assert_eq!(class.name.as_str(), "MyClass");
        assert_eq!(class.kind, LspSymbolKind::CLASS);

        // Class should have constructor children
        let children = class.children.as_ref().expect("Class should have children");
        assert_eq!(
            children
                .iter()
                .filter(|s| s.kind == LspSymbolKind::CONSTRUCTOR)
                .count(),
            2,
            "Should find 2 constructors, found: {:?}",
            children
                .iter()
                .map(|s| (&s.name, &s.kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_document_symbols_selection_range() {
        let host = SemanticHost::for_tests();
        let provider = SymbolsProvider::new();

        let file_id = FileId::new(0);
        // Carefully craft source so we know exact positions
        let source = "class Foo {}";

        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        let symbols = provider
            .document_symbols(&host.read_now(), file_id, source)
            .unwrap();
        assert_eq!(symbols.len(), 1);

        let class = &symbols[0];
        // range should cover entire class
        assert_eq!(class.range.start.line, 0);
        assert_eq!(class.range.start.character, 0);
        assert_eq!(class.range.end.character, 12); // "class Foo {}" = 12 chars

        // selection_range should cover just "Foo"
        // "class " = 6 chars, "Foo" starts at char 6, ends at char 9
        assert_eq!(class.selection_range.start.line, 0);
        assert_eq!(class.selection_range.start.character, 6);
        assert_eq!(class.selection_range.end.character, 9);
    }
}
