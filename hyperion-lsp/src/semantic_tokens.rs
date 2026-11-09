// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Semantic Tokens Provider
//!
//! Provides semantic syntax highlighting based on semantic analysis rather than just lexical tokens.
//! This enables editors to colorize code based on meaning (e.g., distinguish between local variables,
//! fields, and parameters).
//!
//! Token Types:
//! - class, interface, enum, typeParameter
//! - method, property, variable, parameter
//! - keyword, modifier, comment
//!
//! Token Modifiers:
//! - declaration, definition, readonly, static, abstract

use hyperion_parser::syntax::{
    SyntaxKind as JavaSyntaxKind, SyntaxNode as JavaSyntaxNode, SyntaxToken as JavaSyntaxToken,
};
use hyperion_semantics::db::{FileId, Language, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_semantics::file_semantics::FileSemantics;
use hyperion_semantics::resolved_name::ResolvedName;
use hyperion_types::{SemanticHighlight, SemanticHighlightKind, SemanticHighlightModifiers};
use rowan::TextRange;
use std::collections::HashSet;
use tower_lsp::lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
};
use tracing::debug;

use crate::semantic_host::ReadTxn;

// Kotlin imports
use hyperion_kotlin::syntax::{
    SyntaxKind as KotlinSyntaxKind, SyntaxNode as KotlinSyntaxNode,
    SyntaxToken as KotlinSyntaxToken,
};

/// Token types supported by the LSP specification
const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::CLASS,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::ENUM,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::METHOD,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
];

/// Token modifiers supported by the LSP specification
const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::ABSTRACT,
];

/// Provides semantic token information for syntax highlighting
#[derive(Default)]
pub struct SemanticTokensProvider;

/// Binds the shared coordinate index to the text used for this token batch.
struct LineIndex<'a> {
    text: &'a str,
    lines: crate::position_utils::LineIndex,
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            lines: crate::position_utils::LineIndex::new(text),
        }
    }

    fn offset_to_line_col(&self, offset: usize) -> (u32, u32) {
        let position = self.lines.offset_to_position(self.text, offset);
        (position.line, position.character)
    }
}

impl SemanticTokensProvider {
    /// Creates a new SemanticTokensProvider
    pub const fn new() -> Self {
        Self
    }

    /// Get the legend describing supported token types and modifiers
    pub fn legend() -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: TOKEN_TYPES.to_vec(),
            token_modifiers: TOKEN_MODIFIERS.to_vec(),
        }
    }

    /// Generate semantic tokens for a file
    ///
    /// Returns a vector of semantic tokens in LSP delta encoding format:
    /// Each token is encoded as [deltaLine, deltaStartChar, length, tokenType, tokenModifiers]
    pub fn semantic_tokens(
        &self,
        snapshot: &ReadTxn,
        file_id: FileId,
        source_text: &str,
    ) -> Result<Vec<SemanticToken>, String> {
        debug!("Generating semantic tokens for file {:?}", file_id);

        let language = snapshot.file_language(file_id);
        let parse_tree = snapshot.parse_file(file_id);
        let green_node = parse_tree.green.clone();

        // Query FileSemantics for Java files to classify identifier references
        let file_sem = if language == Language::Java {
            Some(snapshot.file_semantics(file_id))
        } else {
            None
        };

        // Collect tokens with ABSOLUTE positions - dispatch based on language
        let mut tokens = Vec::new();
        let line_index = LineIndex::new(source_text);
        match language {
            Language::Kotlin => {
                let root = KotlinSyntaxNode::new_root(green_node);
                self.collect_kotlin_tokens(&root, source_text, &line_index, &mut tokens);
            }
            Language::Java => {
                let root = JavaSyntaxNode::new_root(green_node);
                let mut classified_ranges = HashSet::new();
                self.collect_java_tokens(
                    &root,
                    source_text,
                    &line_index,
                    &mut tokens,
                    &mut classified_ranges,
                );

                // Overlay semantic classification for identifier references
                if let Some(ref file_sem) = file_sem {
                    self.overlay_semantic_tokens(
                        file_sem,
                        source_text,
                        &line_index,
                        &mut tokens,
                        &classified_ranges,
                    );
                }
            }
            Language::Groovy => {
                let highlights =
                    hyperion_groovy::semantic_highlight::semantic_highlights(source_text)
                        .map_err(|error| format!("Failed to highlight Groovy: {error}"))?;
                self.collect_shared_highlights(&highlights, source_text, &line_index, &mut tokens);
            }
            Language::Scala => {
                let highlights =
                    hyperion_scala::semantic_highlight::semantic_highlights(source_text)
                        .map_err(|error| format!("Failed to highlight Scala: {error}"))?;
                self.collect_shared_highlights(&highlights, source_text, &line_index, &mut tokens);
            }
        }

        // Sort by absolute position (required for delta encoding)
        // Note: delta_line/delta_start contain absolute positions at this stage
        tokens.sort_by_key(|t| (t.delta_line, t.delta_start));

        // Convert from absolute positions to delta encoding
        let delta_tokens = self.to_delta_encoding(tokens);

        debug!("Generated {} semantic tokens", delta_tokens.len());
        Ok(delta_tokens)
    }

    // =========================================================================
    // Java Semantic Token Collection
    // =========================================================================

    /// Collect semantic tokens by walking the Java AST
    fn collect_java_tokens(
        &self,
        node: &JavaSyntaxNode,
        source_text: &str,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
        classified_ranges: &mut HashSet<TextRange>,
    ) {
        use JavaSyntaxKind::{
            CLASS_DECL, ENUM_DECL, FIELD_DECL, INTERFACE_DECL, LOCAL_VAR_DECL, METHOD_DECL, PARAM,
        };

        match node.kind() {
            // Class/Interface/Enum declarations
            CLASS_DECL | INTERFACE_DECL | ENUM_DECL => {
                if let Some(name_token) = self.find_java_identifier_token(node) {
                    let token_type = match node.kind() {
                        CLASS_DECL => 0,     // CLASS
                        INTERFACE_DECL => 1, // INTERFACE
                        ENUM_DECL => 2,      // ENUM
                        _ => 0,
                    };
                    classified_ranges.insert(name_token.text_range());
                    self.add_java_token(
                        &name_token,
                        token_type,
                        1 << 0, // DECLARATION modifier
                        line_index,
                        tokens,
                    );
                }
            }

            // Method declarations
            METHOD_DECL => {
                if let Some(name_token) = self.find_java_method_name_token(node) {
                    let mut modifiers = 1 << 0; // DECLARATION

                    // Check for static modifier
                    if self.has_java_modifier(node, "static") {
                        modifiers |= 1 << 3; // STATIC
                    }

                    // Check for abstract modifier
                    if self.has_java_modifier(node, "abstract") {
                        modifiers |= 1 << 4; // ABSTRACT
                    }

                    classified_ranges.insert(name_token.text_range());
                    self.add_java_token(
                        &name_token,
                        4, // METHOD
                        modifiers,
                        line_index,
                        tokens,
                    );
                }
            }

            // Field declarations
            FIELD_DECL => {
                if let Some(name_token) = self.find_java_identifier_token(node) {
                    let mut modifiers = 1 << 0; // DECLARATION

                    if self.has_java_modifier(node, "static") {
                        modifiers |= 1 << 3; // STATIC
                    }
                    if self.has_java_modifier(node, "final") {
                        modifiers |= 1 << 2; // READONLY
                    }

                    classified_ranges.insert(name_token.text_range());
                    self.add_java_token(&name_token, 5, modifiers, line_index, tokens);
                }
            }

            // Variable declarations
            LOCAL_VAR_DECL => {
                if let Some(name_token) = self.find_java_identifier_token(node) {
                    classified_ranges.insert(name_token.text_range());
                    self.add_java_token(&name_token, 6, 1 << 0, line_index, tokens);
                }
            }

            // Parameters - use find_parameter_name_token to get the LAST IDENT
            // (parameter name comes after the type, e.g., "String[] args" -> "args")
            PARAM => {
                if let Some(name_token) = self.find_java_parameter_name_token(node) {
                    classified_ranges.insert(name_token.text_range());
                    self.add_java_token(
                        &name_token,
                        7,      // PARAMETER
                        1 << 0, // DECLARATION
                        line_index,
                        tokens,
                    );
                }
            }

            _ => {}
        }

        // Always recurse into children
        for child in node.children() {
            self.collect_java_tokens(&child, source_text, line_index, tokens, classified_ranges);
        }
    }

    /// Overlay semantic classification for identifier references using FileSemantics.
    ///
    /// This adds semantic tokens for identifiers that were NOT classified by the
    /// syntactic pass (declarations). For example, a type reference like `User` in
    /// `User user = new User()` gets classified as CLASS.
    fn overlay_semantic_tokens(
        &self,
        file_sem: &FileSemantics,
        source_text: &str,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
        classified_ranges: &HashSet<TextRange>,
    ) {
        for (range, resolved) in file_sem.resolved_names() {
            if classified_ranges.contains(range) {
                continue;
            }

            let token_type = match resolved {
                ResolvedName::Type { .. } | ResolvedName::External { .. } => 0, // CLASS
                ResolvedName::Method { .. } => 4,                               // METHOD
                ResolvedName::Field { .. } => 5,                                // PROPERTY
                ResolvedName::EnumConstant { .. } => 2,                         // ENUM (member)
                ResolvedName::LocalVariable { .. } => 6,                        // VARIABLE
                ResolvedName::TypeParameter { .. } => 3,                        // TYPE_PARAMETER
                ResolvedName::Package { .. } | ResolvedName::Rejected(_) => continue,
            };

            let start = usize::from(range.start());
            let end = usize::from(range.end());
            if end > source_text.len() {
                continue;
            }

            let (line, column) = line_index.offset_to_line_col(start);
            let token_text = &source_text[start..end];
            let length_utf16 = token_text.chars().map(char::len_utf16).sum::<usize>() as u32;

            tokens.push(SemanticToken {
                delta_line: line,
                delta_start: column,
                length: length_utf16,
                token_type,
                token_modifiers_bitset: 0, // No modifiers for references
            });
        }
    }

    /// Find the identifier token in a Java node
    fn find_java_identifier_token(&self, node: &JavaSyntaxNode) -> Option<JavaSyntaxToken> {
        node.descendants_with_tokens().find_map(|element| {
            element.as_token().and_then(|token| {
                if token.kind() == JavaSyntaxKind::IDENT {
                    Some(token.clone())
                } else {
                    None
                }
            })
        })
    }

    /// Find the parameter name token in Java (the LAST identifier in a PARAM node)
    fn find_java_parameter_name_token(&self, node: &JavaSyntaxNode) -> Option<JavaSyntaxToken> {
        node.descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == JavaSyntaxKind::IDENT)
            .last()
    }

    /// Find the method name token in Java (the identifier followed by parenthesis)
    fn find_java_method_name_token(&self, node: &JavaSyntaxNode) -> Option<JavaSyntaxToken> {
        let tokens: Vec<_> = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .collect();

        for (i, token) in tokens.iter().enumerate() {
            if token.kind() == JavaSyntaxKind::IDENT {
                if let Some(next) = tokens.get(i + 1) {
                    if next.kind() == JavaSyntaxKind::L_PAREN {
                        return Some(token.clone());
                    }
                }
            }
        }
        None
    }

    /// Check if a Java node has a specific modifier
    fn has_java_modifier(&self, node: &JavaSyntaxNode, modifier: &str) -> bool {
        let expected_kind = match modifier {
            "static" => JavaSyntaxKind::STATIC_KW,
            "abstract" => JavaSyntaxKind::ABSTRACT_KW,
            "final" => JavaSyntaxKind::FINAL_KW,
            "public" => JavaSyntaxKind::PUBLIC_KW,
            "private" => JavaSyntaxKind::PRIVATE_KW,
            "protected" => JavaSyntaxKind::PROTECTED_KW,
            "volatile" => JavaSyntaxKind::VOLATILE_KW,
            "transient" => JavaSyntaxKind::TRANSIENT_KW,
            "synchronized" => JavaSyntaxKind::SYNCHRONIZED_KW,
            "native" => JavaSyntaxKind::NATIVE_KW,
            "strictfp" => JavaSyntaxKind::STRICTFP_KW,
            _ => return false,
        };

        // First check if the modifier is in the node itself
        let in_node = node.descendants_with_tokens().any(|element| {
            element
                .as_token()
                .is_some_and(|t| t.kind() == expected_kind)
        });

        if in_node {
            return true;
        }

        // Check sibling MODIFIER_LIST node (for CLASS_BODY structure)
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if (sibling.kind() == JavaSyntaxKind::MODIFIER_LIST
                    || sibling.kind() == JavaSyntaxKind::MODIFIER)
                    && sibling.descendants_with_tokens().any(|element| {
                        element
                            .as_token()
                            .is_some_and(|t| t.kind() == expected_kind)
                    })
                {
                    return true;
                }
            }
        }

        false
    }

    /// Add a semantic token from a Java syntax token
    fn add_java_token(
        &self,
        syntax_token: &JavaSyntaxToken,
        token_type: u32,
        token_modifiers: u32,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
    ) {
        let range = syntax_token.text_range();
        let start = usize::from(range.start());

        let (line, column) = line_index.offset_to_line_col(start);

        let token_text = syntax_token.text();
        let length_utf16 = token_text.chars().map(char::len_utf16).sum::<usize>() as u32;

        tokens.push(SemanticToken {
            delta_line: line,
            delta_start: column,
            length: length_utf16,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });
    }

    // =========================================================================
    // Kotlin Semantic Token Collection
    // =========================================================================

    /// Collect semantic tokens by walking the Kotlin AST
    fn collect_kotlin_tokens(
        &self,
        node: &KotlinSyntaxNode,
        source_text: &str,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
    ) {
        use KotlinSyntaxKind::{
            CLASS_DECL, COMPANION_OBJECT, FUNCTION_DECL, INTERFACE_DECL, OBJECT_DECL, PARAMETER,
            PROPERTY_DECL,
        };

        match node.kind() {
            // Class/Interface/Object declarations
            CLASS_DECL | INTERFACE_DECL => {
                if let Some(name_token) = self.find_kotlin_identifier_token(node) {
                    let token_type = match node.kind() {
                        CLASS_DECL => 0,     // CLASS
                        INTERFACE_DECL => 1, // INTERFACE
                        _ => 0,
                    };

                    let mut modifiers = 1 << 0; // DECLARATION

                    // Check for data class, sealed class, abstract
                    if self.has_kotlin_modifier(node, "abstract") {
                        modifiers |= 1 << 4; // ABSTRACT
                    }

                    self.add_kotlin_token(&name_token, token_type, modifiers, line_index, tokens);
                }
            }

            // Object declarations (including companion objects)
            OBJECT_DECL | COMPANION_OBJECT => {
                if let Some(name_token) = self.find_kotlin_identifier_token(node) {
                    self.add_kotlin_token(
                        &name_token,
                        0,      // CLASS (objects are singleton classes)
                        1 << 0, // DECLARATION
                        line_index,
                        tokens,
                    );
                }
            }

            // Function declarations
            FUNCTION_DECL => {
                if let Some(name_token) = self.find_kotlin_function_name_token(node) {
                    let mut modifiers = 1 << 0; // DECLARATION

                    // Check for suspend modifier
                    if self.has_kotlin_modifier(node, "abstract") {
                        modifiers |= 1 << 4; // ABSTRACT
                    }

                    self.add_kotlin_token(
                        &name_token,
                        4, // METHOD (same token type for functions)
                        modifiers,
                        line_index,
                        tokens,
                    );
                }
            }

            // Property declarations (val/var)
            PROPERTY_DECL => {
                if let Some(name_token) = self.find_kotlin_property_name_token(node) {
                    let mut modifiers = 1 << 0; // DECLARATION

                    // val properties are readonly
                    if self.has_kotlin_val_keyword(node) {
                        modifiers |= 1 << 2; // READONLY
                    }

                    self.add_kotlin_token(&name_token, 5, modifiers, line_index, tokens);
                }
            }

            // Parameters
            PARAMETER => {
                if let Some(name_token) = self.find_kotlin_identifier_token(node) {
                    self.add_kotlin_token(
                        &name_token,
                        7,      // PARAMETER
                        1 << 0, // DECLARATION
                        line_index,
                        tokens,
                    );
                }
            }

            _ => {}
        }

        // Always recurse into children
        for child in node.children() {
            self.collect_kotlin_tokens(&child, source_text, line_index, tokens);
        }
    }

    /// Find the identifier token in a Kotlin node
    fn find_kotlin_identifier_token(&self, node: &KotlinSyntaxNode) -> Option<KotlinSyntaxToken> {
        node.descendants_with_tokens().find_map(|element| {
            element.as_token().and_then(|token| {
                if token.kind() == KotlinSyntaxKind::IDENT {
                    Some(token.clone())
                } else {
                    None
                }
            })
        })
    }

    /// Find the function name token in Kotlin (identifier after 'fun' keyword)
    fn find_kotlin_function_name_token(
        &self,
        node: &KotlinSyntaxNode,
    ) -> Option<KotlinSyntaxToken> {
        let tokens: Vec<_> = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .collect();

        // Find the IDENT that comes after FUN_KW
        let mut found_fun = false;
        for token in tokens {
            if token.kind() == KotlinSyntaxKind::FUN_KW {
                found_fun = true;
            } else if found_fun && token.kind() == KotlinSyntaxKind::IDENT {
                return Some(token);
            }
        }
        None
    }

    /// Find the property name token in Kotlin (identifier after 'val' or 'var')
    fn find_kotlin_property_name_token(
        &self,
        node: &KotlinSyntaxNode,
    ) -> Option<KotlinSyntaxToken> {
        let tokens: Vec<_> = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .collect();

        // Find the IDENT that comes after VAL_KW or VAR_KW
        let mut found_keyword = false;
        for token in tokens {
            if token.kind() == KotlinSyntaxKind::VAL_KW || token.kind() == KotlinSyntaxKind::VAR_KW
            {
                found_keyword = true;
            } else if found_keyword && token.kind() == KotlinSyntaxKind::IDENT {
                return Some(token);
            }
        }
        None
    }

    /// Check if a Kotlin property uses 'val' (immutable)
    fn has_kotlin_val_keyword(&self, node: &KotlinSyntaxNode) -> bool {
        node.descendants_with_tokens().any(|element| {
            element
                .as_token()
                .is_some_and(|t| t.kind() == KotlinSyntaxKind::VAL_KW)
        })
    }

    /// Check if a Kotlin node has a specific modifier
    fn has_kotlin_modifier(&self, node: &KotlinSyntaxNode, modifier: &str) -> bool {
        let expected_kind = match modifier {
            "abstract" => KotlinSyntaxKind::ABSTRACT_KW,
            "open" => KotlinSyntaxKind::OPEN_KW,
            "sealed" => KotlinSyntaxKind::SEALED_KW,
            "data" => KotlinSyntaxKind::DATA_KW,
            "suspend" => KotlinSyntaxKind::SUSPEND_KW,
            "override" => KotlinSyntaxKind::OVERRIDE_KW,
            "inline" => KotlinSyntaxKind::INLINE_KW,
            "private" => KotlinSyntaxKind::PRIVATE_KW,
            "public" => KotlinSyntaxKind::PUBLIC_KW,
            "protected" => KotlinSyntaxKind::PROTECTED_KW,
            "internal" => KotlinSyntaxKind::INTERNAL_KW,
            _ => return false,
        };

        // Check if the modifier is in the node or its MODIFIERS child
        node.descendants_with_tokens().any(|element| {
            element
                .as_token()
                .is_some_and(|t| t.kind() == expected_kind)
        })
    }

    /// Add a semantic token from a Kotlin syntax token
    fn add_kotlin_token(
        &self,
        syntax_token: &KotlinSyntaxToken,
        token_type: u32,
        token_modifiers: u32,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
    ) {
        let range = syntax_token.text_range();
        let start = usize::from(range.start());

        let (line, column) = line_index.offset_to_line_col(start);

        let token_text = syntax_token.text();
        let length_utf16 = token_text.chars().map(char::len_utf16).sum::<usize>() as u32;

        tokens.push(SemanticToken {
            delta_line: line,
            delta_start: column,
            length: length_utf16,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });
    }

    // =========================================================================
    // Common Utilities
    // =========================================================================

    fn collect_shared_highlights(
        &self,
        highlights: &[SemanticHighlight],
        source_text: &str,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
    ) {
        for highlight in highlights {
            let start = highlight.range.start as usize;
            let end = highlight.range.end as usize;
            let Some(text) = source_text.get(start..end) else {
                continue;
            };
            let token_type = Self::shared_token_type(highlight.kind);
            let modifiers = Self::shared_token_modifiers(highlight.modifiers);

            // LSP semantic tokens are single-line. Split block comments and
            // multiline strings while retaining byte-accurate UTF-16 columns.
            let mut segment_start = start;
            for (relative, character) in text.char_indices() {
                if matches!(character, '\r' | '\n') {
                    let segment_end = start + relative;
                    self.add_shared_token(
                        (segment_start, segment_end),
                        (token_type, modifiers),
                        source_text,
                        line_index,
                        tokens,
                    );
                    segment_start = segment_end + character.len_utf8();
                }
            }
            self.add_shared_token(
                (segment_start, end),
                (token_type, modifiers),
                source_text,
                line_index,
                tokens,
            );
        }
    }

    fn add_shared_token(
        &self,
        byte_range: (usize, usize),
        classification: (u32, u32),
        source_text: &str,
        line_index: &LineIndex<'_>,
        tokens: &mut Vec<SemanticToken>,
    ) {
        let (start, end) = byte_range;
        let (token_type, modifiers) = classification;
        if start >= end {
            return;
        }
        let Some(text) = source_text.get(start..end) else {
            return;
        };
        let (line, column) = line_index.offset_to_line_col(start);
        tokens.push(SemanticToken {
            delta_line: line,
            delta_start: column,
            length: text.encode_utf16().count() as u32,
            token_type,
            token_modifiers_bitset: modifiers,
        });
    }

    const fn shared_token_type(kind: SemanticHighlightKind) -> u32 {
        match kind {
            SemanticHighlightKind::Class => 0,
            SemanticHighlightKind::Interface => 1,
            SemanticHighlightKind::Enum => 2,
            SemanticHighlightKind::TypeParameter => 3,
            SemanticHighlightKind::Method => 4,
            SemanticHighlightKind::Property => 5,
            SemanticHighlightKind::Variable => 6,
            SemanticHighlightKind::Parameter => 7,
            SemanticHighlightKind::Keyword => 8,
            SemanticHighlightKind::Modifier => 9,
            SemanticHighlightKind::Comment => 10,
            SemanticHighlightKind::String => 11,
            SemanticHighlightKind::Number => 12,
        }
    }

    const fn shared_token_modifiers(modifiers: SemanticHighlightModifiers) -> u32 {
        (modifiers.declaration as u32)
            | ((modifiers.definition as u32) << 1)
            | ((modifiers.readonly as u32) << 2)
            | ((modifiers.static_member as u32) << 3)
            | ((modifiers.abstract_member as u32) << 4)
    }

    /// Convert offset to line and column (CRLF-aware, UTF-16 columns for LSP)
    #[cfg(test)]
    fn offset_to_line_col(&self, text: &str, offset: usize) -> (u32, u32) {
        LineIndex::new(text).offset_to_line_col(offset)
    }

    /// Convert absolute tokens to delta-encoded format
    ///
    /// Input: tokens with ABSOLUTE positions stored in delta_line/delta_start fields
    /// Output: tokens with ACTUAL delta encoding (relative to previous token)
    ///
    /// The LSP protocol requires delta encoding where:
    /// - delta_line = current_line - previous_line
    /// - delta_start = current_col - previous_col (if same line) OR current_col (if new line)
    fn to_delta_encoding(&self, tokens: Vec<SemanticToken>) -> Vec<SemanticToken> {
        let mut delta_tokens = Vec::new();
        let mut prev_line = 0;
        let mut prev_start = 0;

        for token in tokens {
            // Input token has ABSOLUTE positions (despite field names)
            let delta_line = token.delta_line - prev_line;
            let delta_start = if delta_line == 0 {
                token.delta_start - prev_start
            } else {
                token.delta_start
            };

            delta_tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: token.length,
                token_type: token.token_type,
                token_modifiers_bitset: token.token_modifiers_bitset,
            });

            // Store absolute positions from input token for next iteration
            prev_line = token.delta_line; // This is absolute line
            prev_start = token.delta_start; // This is absolute column
        }

        delta_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::Database;
    use std::sync::Arc;

    #[test]
    fn test_legend() {
        let legend = SemanticTokensProvider::legend();
        assert!(!legend.token_types.is_empty());
        assert!(!legend.token_modifiers.is_empty());
    }

    #[test]
    fn test_offset_to_line_col() {
        let provider = SemanticTokensProvider::new();

        let text = "class Foo {\n  int x;\n}";

        // "class" at offset 0
        assert_eq!(provider.offset_to_line_col(text, 0), (0, 0));

        // "Foo" at offset 6
        assert_eq!(provider.offset_to_line_col(text, 6), (0, 6));

        // "int" at offset 14
        assert_eq!(provider.offset_to_line_col(text, 14), (1, 2));
    }

    #[test]
    fn test_kotlin_semantic_tokens() {
        let mut db = Database::default();
        let file_id = FileId::new(0);

        // Kotlin source code
        let source = r#"
package com.example

class User {
    val name: String = ""

    fun greet() {
        println("Hello")
    }
}
"#
        .to_string();

        // Register file as Kotlin
        db.set_file_text(file_id, Arc::new(source.clone()));
        db.set_file_language(file_id, Language::Kotlin);

        let host = SemanticHost::with_database(
            db,
            Arc::new(hyperion_workspace::WorkspaceIndex::new(std::env::temp_dir())),
        );
        let provider = SemanticTokensProvider::new();

        let tokens = provider
            .semantic_tokens(&host.read_now(), file_id, &source)
            .unwrap();

        // Should have tokens for: User (class), name (property), greet (function)
        // At least 3 semantic tokens
        assert!(
            tokens.len() >= 3,
            "Expected at least 3 tokens for Kotlin file, got {}",
            tokens.len()
        );

        // Verify token types: Class is 0, Property is 5, Method is 4
        // First token should be the class User
        // The class token should have token_type = 0 (CLASS)
        let has_class_token = tokens.iter().any(|t| t.token_type == 0);
        assert!(has_class_token, "Should have a CLASS token for 'User'");

        // Should have a property token (type 5)
        let has_property_token = tokens.iter().any(|t| t.token_type == 5);
        assert!(
            has_property_token,
            "Should have a PROPERTY token for 'name'"
        );

        // Should have a method token (type 4)
        let has_method_token = tokens.iter().any(|t| t.token_type == 4);
        assert!(has_method_token, "Should have a METHOD token for 'greet'");
    }

    #[test]
    fn test_java_semantic_tokens() {
        let mut db = Database::default();
        let file_id = FileId::new(0);

        // Java source code
        let source = r#"
package com.example;

public class User {
    private String name;

    public void greet() {
        System.out.println("Hello");
    }
}
"#
        .to_string();

        // Register file as Java
        db.set_file_text(file_id, Arc::new(source.clone()));
        db.set_file_language(file_id, Language::Java);

        let host = SemanticHost::with_database(
            db,
            Arc::new(hyperion_workspace::WorkspaceIndex::new(std::env::temp_dir())),
        );
        let provider = SemanticTokensProvider::new();

        let tokens = provider
            .semantic_tokens(&host.read_now(), file_id, &source)
            .unwrap();

        // Should have tokens for: User (class), name (field), greet (method)
        assert!(
            tokens.len() >= 3,
            "Expected at least 3 tokens for Java file, got {}",
            tokens.len()
        );

        // Verify token types
        let has_class_token = tokens.iter().any(|t| t.token_type == 0);
        assert!(has_class_token, "Should have a CLASS token for 'User'");

        // Should have a property/field token (type 5)
        let has_property_token = tokens.iter().any(|t| t.token_type == 5);
        assert!(
            has_property_token,
            "Should have a PROPERTY token for 'name'"
        );

        // Should have a method token (type 4)
        let has_method_token = tokens.iter().any(|t| t.token_type == 4);
        assert!(has_method_token, "Should have a METHOD token for 'greet'");
    }

    #[test]
    fn test_offset_to_line_col_crlf() {
        let provider = SemanticTokensProvider::new();

        // "line1\r\nline2\r\n"
        let text = "line1\r\nline2\r\n";

        assert_eq!(provider.offset_to_line_col(text, 0), (0, 0)); // 'l' of line1
        assert_eq!(provider.offset_to_line_col(text, 5), (0, 5)); // '\r' — still on line 0
        assert_eq!(provider.offset_to_line_col(text, 7), (1, 0)); // 'l' of line2
        assert_eq!(provider.offset_to_line_col(text, 12), (1, 5)); // '\r' — still on line 1
    }

    #[test]
    fn test_offset_to_line_col_standalone_cr() {
        let provider = SemanticTokensProvider::new();

        // Old Mac style: \r only
        let text = "abc\rdef\r";

        assert_eq!(provider.offset_to_line_col(text, 0), (0, 0));
        assert_eq!(provider.offset_to_line_col(text, 3), (0, 3)); // '\r'
        assert_eq!(provider.offset_to_line_col(text, 4), (1, 0)); // 'd'
        assert_eq!(provider.offset_to_line_col(text, 7), (1, 3)); // '\r'
    }

    #[test]
    fn test_offset_to_line_col_mixed_endings() {
        let provider = SemanticTokensProvider::new();

        // Mixed: "a\r\nb\nc\rd"
        let text = "a\r\nb\nc\rd";

        assert_eq!(provider.offset_to_line_col(text, 0), (0, 0)); // 'a'
        assert_eq!(provider.offset_to_line_col(text, 3), (1, 0)); // 'b'
        assert_eq!(provider.offset_to_line_col(text, 5), (2, 0)); // 'c'
        assert_eq!(provider.offset_to_line_col(text, 7), (3, 0)); // 'd'
    }
}
