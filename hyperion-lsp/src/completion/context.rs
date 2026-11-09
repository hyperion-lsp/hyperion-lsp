// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion Context Detection
//!
//! Analyzes cursor position to determine what kind of completion is needed.
//! This module is the first step in the completion pipeline - it examines
//! the source code around the cursor to understand the semantic context.

use hyperion_semantics::db::{Database, FileId, ParseDatabase};
use tower_lsp::lsp_types::Position;
use tracing::debug;

/// The context in which completion is being requested
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    /// After a '.' or '->' (member access)
    MemberAccess,
    /// In a type position (variable declaration, parameter, return type, etc.)
    TypePosition,
    /// After 'new' keyword (constructor call)
    ConstructorCall,
    /// Inside generic type arguments (List<|>, Map<String, |>)
    GenericTypeArgument,
    /// In a position expecting a lambda/functional interface
    LambdaExpression {
        /// FQN of the functional interface expected
        interface_fqn: String,
        /// Parameter types of the SAM
        param_types: Vec<String>,
        /// Return type of the SAM
        return_type: String,
    },
    /// In a type body, suitable for override/implement method suggestions.
    ///
    /// Only the type being edited is carried. Which members it inherits is the
    /// canonical hierarchy walk's answer, not this context's: the written
    /// supertypes decide whether the context applies, and nothing more.
    OverrideImplement {
        /// FQN of the type declaration the cursor is in.
        class_fqn: String,
    },
    /// After `@` (annotation name position)
    Annotation,
    /// General context (statements, expressions)
    General,
}

/// Whether the innermost unclosed `{` before the cursor opens a type body
/// (`class A {`, `enum E {`, `new Foo() {`) rather than a method, initializer
/// or statement block. Comments, strings and char literals are skipped.
fn innermost_brace_is_type_body(text_before_cursor: &str) -> bool {
    let bytes = text_before_cursor.as_bytes();
    let mut stack: Vec<bool> = Vec::new();
    let mut segment_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            quote @ (b'"' | b'\'') => {
                let triple = quote == b'"' && bytes[i..].starts_with(b"\"\"\"");
                i += if triple { 3 } else { 1 };
                while i < bytes.len() {
                    if triple && bytes[i..].starts_with(b"\"\"\"") {
                        i += 3;
                        break;
                    }
                    if !triple && bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if !triple && (bytes[i] == quote || bytes[i] == b'\n') {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            b'{' => {
                let header = &text_before_cursor[segment_start..i];
                stack.push(header_opens_type_body(header));
                segment_start = i + 1;
            }
            b'}' => {
                stack.pop();
                segment_start = i + 1;
            }
            b';' => segment_start = i + 1,
            _ => {}
        }
        i += 1;
    }
    stack.last().copied().unwrap_or(false)
}

/// Whether the text between the previous delimiter and a `{` is a type header.
fn header_opens_type_body(header: &str) -> bool {
    let words: Vec<&str> = header
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .filter(|word| !word.is_empty())
        .collect();
    if words
        .iter()
        .any(|word| matches!(*word, "class" | "interface" | "enum"))
    {
        return true;
    }
    // `record Point(int x) {`
    if words
        .windows(2)
        .any(|pair| pair[0] == "record" && pair[1].chars().next().is_some_and(char::is_uppercase))
    {
        return true;
    }
    // Anonymous class body: `new Foo(args) {`
    header.trim_end().ends_with(')') && words.contains(&"new")
}

/// Whether `text` ends with `word` as a whole word (`renew` does not end with `new`).
fn ends_with_word(text: &str, word: &str) -> bool {
    text.strip_suffix(word).is_some_and(|before| {
        !before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
    })
}

/// Analyzes source code to determine completion context.
///
/// Detection is purely syntactic: supertype resolution and member election
/// both moved to the canonical hierarchy walk, so this no longer
/// needs the workspace index.
#[derive(Default)]
pub struct ContextAnalyzer;

impl ContextAnalyzer {
    /// Create a new context analyzer
    pub const fn new() -> Self {
        Self
    }

    /// Analyze the context at the given position
    pub fn analyze_context(
        &self,
        file_id: FileId,
        source_text: &str,
        position: Position,
        snapshot: &salsa::Snapshot<Database>,
    ) -> Result<CompletionContext, String> {
        use crate::position_utils::position_to_offset;

        let offset = position_to_offset(source_text, position)?;

        // Early check: if cursor is in comment or string, return General context
        // This prevents expensive pattern analysis for non-code positions
        let parse_tree = snapshot.parse_file(file_id);
        let syntax_root = parse_tree.syntax();

        // Check if it's a Java or Kotlin file and use appropriate check
        if crate::navigation::ast_queries::is_in_comment_or_string_java(&syntax_root, offset as u32)
        {
            debug!("Cursor is in comment or string, skipping pattern analysis");
            return Ok(CompletionContext::General);
        }

        // Look at the characters before the cursor
        if offset > 0 {
            let before_cursor = &source_text[..offset];

            // Check if we're in member access (after '.' with optional identifier prefix)
            let chars: Vec<_> = before_cursor.char_indices().collect();
            let mut idx = chars.len();

            while idx > 0 && chars[idx - 1].1.is_whitespace() {
                idx -= 1;
            }

            if idx > 0 && chars[idx - 1].1 == '.' {
                return Ok(CompletionContext::MemberAccess);
            }

            let mut ident_start = idx;
            while ident_start > 0 {
                let ch = chars[ident_start - 1].1;
                if ch.is_alphanumeric() || ch == '_' {
                    ident_start -= 1;
                } else {
                    break;
                }
            }

            let mut before_ident = ident_start;
            while before_ident > 0 && chars[before_ident - 1].1.is_whitespace() {
                before_ident -= 1;
            }

            if before_ident > 0 && chars[before_ident - 1].1 == '.' {
                return Ok(CompletionContext::MemberAccess);
            }

            // Check if we're in a type position
            // This is a simplified heuristic - look for common type position patterns
            let trimmed = before_cursor.trim_end();

            // The text before the identifier being typed decides what kind of
            // name is expected (`new Arr|`, `extends Bas|`, `@Over|`).
            let mut typed_start = chars.len();
            while typed_start > 0
                && (chars[typed_start - 1].1.is_alphanumeric() || chars[typed_start - 1].1 == '_')
            {
                typed_start -= 1;
            }
            let ident_byte = chars
                .get(typed_start)
                .map_or(before_cursor.len(), |(byte, _)| *byte);
            let head = before_cursor[..ident_byte].trim_end();

            if head.ends_with('@') {
                return Ok(CompletionContext::Annotation);
            }

            // Constructor call context (highest priority for "new")
            if ends_with_word(head, "new") {
                return Ok(CompletionContext::ConstructorCall);
            }

            if ["class", "extends", "implements", "instanceof", "throws"]
                .iter()
                .any(|keyword| ends_with_word(head, keyword))
            {
                return Ok(CompletionContext::TypePosition);
            }

            // Check if we're inside generic type arguments: List<|>, Map<String, |>
            // Count unbalanced angle brackets to detect type argument position
            let open_angles = trimmed.chars().filter(|&c| c == '<').count();
            let close_angles = trimmed.chars().filter(|&c| c == '>').count();
            if open_angles > close_angles {
                // We're inside angle brackets - check if this looks like a type context
                // Find the last '<' that isn't closed
                if let Some(last_open) = trimmed.rfind('<') {
                    let after_last_open = &trimmed[last_open + 1..];
                    // Check if after the '<' we have type-like content (commas, type names, whitespace)
                    // and we're not in a comparison expression
                    let looks_like_generic = after_last_open.chars().all(|c| {
                        c.is_alphanumeric()
                            || c == ','
                            || c == ' '
                            || c == '_'
                            || c == '.'
                            || c == '?'
                            || c == '\t'
                    });
                    if looks_like_generic {
                        // Improved check to avoid false positives from comparison operators
                        // Check character before '<' to ensure it's attached to a type name
                        // Note: last_open is a byte offset, so we need to find the char before it properly
                        let before_angle = trimmed.get(..last_open).unwrap_or("");
                        let char_before = before_angle.chars().next_back();

                        if !before_angle.is_empty() {
                            // Reject if there's a space before '<' - indicates comparison (x < y)
                            if char_before == Some(' ') || char_before == Some('\t') {
                                // Don't return generic context - likely a comparison
                            } else if let Some(c) = char_before {
                                if c.is_alphanumeric() || c == '_' {
                                    // Additional check: look for control flow keywords that suggest comparison
                                    // if (x <, while (x <, for (x <

                                    // Helper to check if control flow keyword has unclosed paren
                                    let has_unclosed_control_flow = |keyword: &str| -> bool {
                                        if let Some(pos) = before_angle.rfind(keyword) {
                                            !before_angle[pos..].contains(')')
                                        } else {
                                            false
                                        }
                                    };

                                    let has_control_flow = before_angle.ends_with("if (")
                                        || before_angle.ends_with("while (")
                                        || before_angle.ends_with("for (")
                                        || has_unclosed_control_flow("if (")
                                        || has_unclosed_control_flow("while (")
                                        || has_unclosed_control_flow("for (");

                                    if !has_control_flow {
                                        return Ok(CompletionContext::GenericTypeArgument);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check for variable declaration patterns
            // This is simplified - ideally we'd parse the syntax tree
            let words: Vec<&str> = trimmed.split_whitespace().collect();
            if !words.is_empty() {
                let last_word = words[words.len() - 1];
                // If the last word is a modifier or primitive type, we might be in a type position
                if matches!(
                    last_word,
                    "public" | "private" | "protected" | "static" | "final" | "abstract"
                ) {
                    return Ok(CompletionContext::TypePosition);
                }
            }

            // Check for lambda expression context (method argument expecting functional interface)
            // Heuristic: look for common patterns like forEach(|), map(|), filter(|)
            if let Some(lambda_ctx) = self.detect_lambda_context(before_cursor) {
                return Ok(lambda_ctx);
            }

            // Check for override/implement context
            if let Some(override_ctx) =
                self.detect_override_implement_context(file_id, source_text, position, snapshot)
            {
                return Ok(override_ctx);
            }
        }

        // Default to general context
        Ok(CompletionContext::General)
    }

    /// Detect if cursor is in a position expecting a lambda expression
    fn detect_lambda_context(&self, text_before_cursor: &str) -> Option<CompletionContext> {
        use crate::completion::functional_interface;

        // Look for patterns like: methodName(
        // Find the last opening paren that's not closed
        let open_parens = text_before_cursor.matches('(').count();
        let close_parens = text_before_cursor.matches(')').count();

        if open_parens <= close_parens {
            return None; // Not inside arguments
        }

        // Find the last unclosed '(' and extract the method name before it
        let last_open_paren = text_before_cursor.rfind('(')?;
        let before_paren = &text_before_cursor[..last_open_paren];

        // Extract method name (walk backwards from paren to find identifier)
        let method_name = before_paren
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();

        if method_name.is_empty() {
            return None;
        }

        // Map common method names to their functional interfaces
        let functional_interface_fqn = match method_name.as_str() {
            // Stream API
            "forEach" => "java.util.function.Consumer",
            "map" => "java.util.function.Function",
            "filter" => "java.util.function.Predicate",
            "flatMap" => "java.util.function.Function",
            "peek" => "java.util.function.Consumer",
            "sorted" => "java.util.Comparator",
            "removeIf" => "java.util.function.Predicate",
            "replaceAll" => "java.util.function.UnaryOperator",
            "compute" => "java.util.function.BiFunction",
            "computeIfAbsent" => "java.util.function.Function",
            "computeIfPresent" => "java.util.function.BiFunction",
            "merge" => "java.util.function.BiFunction",
            _ => return None,
        };

        // Get functional interface info
        let info =
            functional_interface::get_functional_interface_info(functional_interface_fqn, &[])?;

        Some(CompletionContext::LambdaExpression {
            interface_fqn: info.interface_fqn,
            param_types: info.parameter_types,
            return_type: info.return_type,
        })
    }

    /// Detect if cursor is in a position suitable for override/implement completion
    ///
    /// This checks if:
    /// - Cursor is in a class body
    /// - Class has parent class or implements interfaces
    /// - Cursor is at a position suitable for method insertion
    pub(crate) fn detect_override_implement_context(
        &self,
        file_id: FileId,
        source_text: &str,
        position: Position,
        snapshot: &salsa::Snapshot<Database>,
    ) -> Option<CompletionContext> {
        use crate::position_utils::position_to_offset;
        let offset = position_to_offset(source_text, position).ok()?;

        // Override/implement stubs belong directly in a type body. Being
        // somewhere inside a class that has supertypes is not enough: the
        // cursor in a method body wants expressions and types. Error
        // recovery (`Str` typed before an existing statement parses as a
        // method header) hides that from the tree, so decide from the braces.
        if !innermost_brace_is_type_body(&source_text[..offset]) {
            return None;
        }

        // Parse the file
        let root = {
            let parse_tree = snapshot.parse_file(file_id);
            parse_tree.syntax()
        };

        // Find token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::from(offset as u32))
            .right_biased()?;

        // The type declaration the cursor is in, resolved once, for every type
        // declaration kind. Walking to the nearest `CLASS_DECL` was dead in
        // interfaces, enums and records, and naming the type by its innermost
        // simple name alone produced the wrong FQN for a nested declaration.
        let node = crate::inherited_members::type_declaration_at(&token)?;
        let class_fqn = crate::inherited_members::type_declaration_fqn(&node)?;

        // A written supertype is what makes this context applicable at all.
        // Which members it contributes -- and whether they come from it, from
        // its own supertypes, or from a grandparent -- is the canonical
        // hierarchy walk's answer, so the spellings are not resolved here.
        let (parent_class, interfaces) = self.extract_parent_and_interfaces(&node);
        if parent_class.is_none() && interfaces.is_empty() {
            return None;
        }

        Some(CompletionContext::OverrideImplement { class_fqn })
    }

    /// Extract parent class and implemented interfaces from CLASS_DECL node
    fn extract_parent_and_interfaces(
        &self,
        class_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> (Option<String>, Vec<String>) {
        use hyperion_parser::syntax::SyntaxKind;

        let mut parent_class: Option<String> = None;
        let mut interfaces = Vec::new();
        let mut in_extends = false;
        let mut in_implements = false;

        for child in class_node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::EXTENDS_KW => {
                    in_extends = true;
                    in_implements = false;
                }
                SyntaxKind::IMPLEMENTS_KW => {
                    in_extends = false;
                    in_implements = true;
                }
                SyntaxKind::CLASS_TYPE => {
                    if let Some(n) = child.as_node() {
                        let type_name = n.text().to_string().trim().to_string();
                        if in_extends && parent_class.is_none() {
                            parent_class = Some(type_name);
                        } else if in_implements {
                            interfaces.push(type_name);
                        }
                    }
                }
                SyntaxKind::L_BRACE => break, // End of class header
                _ => {}
            }
        }

        (parent_class, interfaces)
    }
}

#[cfg(test)]
mod tests {
    use super::innermost_brace_is_type_body as in_type_body;

    #[test]
    fn type_body_versus_blocks() {
        assert!(in_type_body("class A extends B {\n  "));
        assert!(in_type_body(
            "package p;\npublic final class A<T> implements X {\n int f;\n"
        ));
        assert!(in_type_body("class A { void m() { } "));
        assert!(in_type_body("enum E { A, B; "));
        assert!(in_type_body("class A { void m() { new Foo(1) {\n "));
        assert!(!in_type_body("class A { void m() {\n  "));
        assert!(!in_type_body("class A { A() { if (x) {\n"));
        assert!(!in_type_body("class A { void m() { Runnable r = () -> {\n"));
        assert!(!in_type_body(
            "class A { void m() { for (int i = 0; i < 3; i++) {\n"
        ));
        assert!(!in_type_body(""));
    }

    #[test]
    fn braces_in_strings_and_comments_do_not_count() {
        assert!(in_type_body("class A { String s = \"}{\"; // }\n /* { */ "));
        assert!(!in_type_body(
            "class A { void m() { char c = '{'; String t = \"}\";\n"
        ));
    }
}
