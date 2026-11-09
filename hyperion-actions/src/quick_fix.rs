// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Quick fix actions for common diagnostic issues
//!
//! This module provides automatic fixes for common Java coding issues:
//! - Remove unused imports
//! - Add @Override annotations
//! - Fix visibility modifiers
//! - Add missing exception handling

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

/// A quick fix that can be applied to resolve a diagnostic
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickFix {
    /// User-visible title of the fix
    pub title: String,
    /// Detailed description of what the fix does
    pub description: String,
    /// Text edits to apply
    pub edits: Vec<TextEdit>,
}

/// A text edit operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Range to replace
    pub range: TextRange,
    /// New text to insert
    pub new_text: String,
}

/// Provider for generating quick fixes from diagnostics
pub struct QuickFixProvider;

impl QuickFixProvider {
    /// Generate quick fixes for a diagnostic at the given node
    ///
    /// # Arguments
    /// * `node` - The syntax node where the diagnostic occurred
    /// * `diagnostic_message` - The diagnostic message text
    ///
    /// # Returns
    /// A list of applicable quick fixes
    pub fn get_quick_fixes(node: &SyntaxNode, diagnostic_message: &str) -> Vec<QuickFix> {
        let mut fixes = Vec::new();

        // Remove unused import
        let msg_lower = diagnostic_message.to_lowercase();
        if msg_lower.contains("unused") && msg_lower.contains("import") {
            // Need to find the IMPORT_DECL node - might be an ancestor
            if let Some(import_node) = Self::find_import_decl_ancestor(node) {
                fixes.push(Self::remove_unused_import(&import_node));
            }
        }

        // Add missing override annotation
        if msg_lower.contains("lacks @override annotation")
            || msg_lower.contains("missing @override")
            || msg_lower.contains("add @override")
        {
            if let Some(fix) = Self::add_override_annotation(node) {
                fixes.push(fix);
            }
        }

        // Add missing throws declaration
        if diagnostic_message.starts_with("unhandled exception:") {
            if let Some(fix) = Self::add_throws_declaration(node, diagnostic_message) {
                fixes.push(fix);
            }
        }

        // Make field final
        if diagnostic_message.contains("should be final") {
            if let Some(fix) = Self::add_final_modifier(node) {
                fixes.push(fix);
            }
        }

        // Empty catch block - add logging
        if diagnostic_message.contains("Empty catch block") {
            if let Some(fix) = Self::add_catch_logging(node) {
                fixes.push(fix);
            }
            // Also offer rethrow option
            if let Some(fix) = Self::rethrow_exception(node) {
                fixes.push(fix);
            }
        }

        // Redundant modifier
        if diagnostic_message.contains("redundant") && diagnostic_message.contains("Modifier") {
            if let Some(fix) = Self::remove_redundant_modifier(node, diagnostic_message) {
                fixes.push(fix);
            }
        }

        fixes
    }

    /// Find IMPORT_DECL node from current node or ancestors
    fn find_import_decl_ancestor(node: &SyntaxNode) -> Option<SyntaxNode> {
        if node.kind() == SyntaxKind::IMPORT_DECL {
            Some(node.clone())
        } else {
            node.ancestors()
                .find(|n| n.kind() == SyntaxKind::IMPORT_DECL)
        }
    }

    /// Generate a fix to remove an unused import
    fn remove_unused_import(node: &SyntaxNode) -> QuickFix {
        QuickFix {
            title: "Remove unused import".to_string(),
            description: "Remove this unused import statement".to_string(),
            edits: vec![TextEdit {
                range: node.text_range(),
                new_text: String::new(),
            }],
        }
    }

    /// Generate a fix to add @Override annotation
    fn add_override_annotation(node: &SyntaxNode) -> Option<QuickFix> {
        // Find method declaration
        let method_node = node
            .ancestors()
            .find(|n| n.kind() == SyntaxKind::METHOD_DECL)?;
        // The parser stores the modifier list as a sibling before METHOD_DECL.
        // Inserting at the method start would put @Override after `public`.
        let modifiers = method_node
            .prev_sibling()
            .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST);
        if modifiers.as_ref().is_some_and(|modifiers| {
            modifiers
                .children()
                .filter(|child| child.kind() == SyntaxKind::ANNOTATION)
                .any(|annotation| {
                    let compact: String = annotation
                        .text()
                        .to_string()
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    matches!(compact.as_str(), "@Override" | "@java.lang.Override")
                })
        }) {
            return None;
        }
        let insertion_node = modifiers.as_ref().unwrap_or(&method_node);
        let start = insertion_node.text_range().start();

        let indentation = Self::get_indentation(insertion_node);

        Some(QuickFix {
            title: "Add @Override annotation".to_string(),
            description: "Add @Override annotation to method".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(start, start),
                new_text: format!("@Override\n{indentation}"),
            }],
        })
    }

    /// Generate a fix to add throws declaration
    fn add_throws_declaration(node: &SyntaxNode, diagnostic_message: &str) -> Option<QuickFix> {
        // Extract exception type from diagnostic message
        let exception_type = Self::extract_exception_type(diagnostic_message)?;

        // Find method declaration
        let method_node = node
            .ancestors()
            .find(|n| n.kind() == SyntaxKind::METHOD_DECL)?;
        if method_node
            .children()
            .any(|child| child.kind() == SyntaxKind::THROWS_CLAUSE)
        {
            return None;
        }

        // Find the position to insert throws clause
        // Should be after parameter list and before method body
        let param_list = method_node
            .children()
            .find(|n| n.kind() == SyntaxKind::PARAM_LIST)?;

        let insert_pos = param_list.text_range().end();

        Some(QuickFix {
            title: format!("Add 'throws {}' to method signature", exception_type),
            description: format!("Declare that this method throws {}", exception_type),
            edits: vec![TextEdit {
                range: TextRange::new(insert_pos, insert_pos),
                new_text: format!(" throws {}", exception_type),
            }],
        })
    }

    /// Generate a fix to add final modifier to a field
    fn add_final_modifier(node: &SyntaxNode) -> Option<QuickFix> {
        // Find field declaration
        let field_node = node
            .ancestors()
            .find(|n| n.kind() == SyntaxKind::FIELD_DECL)?;

        // Modifier lists are siblings of FIELD_DECL, not its children.
        if field_node
            .prev_sibling()
            .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
            .is_some_and(|modifiers| {
                modifiers
                    .descendants_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::FINAL_KW)
            })
        {
            return None;
        }
        let insert_pos = field_node.text_range().start();
        Some(QuickFix {
            title: "Add 'final' modifier".to_string(),
            description: "Make this field final".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(insert_pos, insert_pos),
                new_text: "final ".to_string(),
            }],
        })
    }

    /// Generate a fix to add logging to an empty catch block
    fn add_catch_logging(node: &SyntaxNode) -> Option<QuickFix> {
        // Find catch clause
        let catch_node = if node.kind() == SyntaxKind::CATCH_CLAUSE {
            node.clone()
        } else {
            node.ancestors()
                .find(|n| n.kind() == SyntaxKind::CATCH_CLAUSE)?
        };

        // Find the block within catch
        let block = catch_node
            .children()
            .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)?;
        if !Self::is_empty_catch_block(&block) {
            return None;
        }

        // Find exception parameter name
        let exception_name = Self::extract_catch_param_name(&catch_node)?;

        // Insert logging statement after opening brace
        let insert_pos = block.text_range().start() + rowan::TextSize::from(1); // After '{'

        let indentation = Self::get_indentation(&block);

        Some(QuickFix {
            title: "Add logging statement".to_string(),
            description: "Log the exception with printStackTrace()".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(insert_pos, insert_pos),
                new_text: format!("\n{}    {}.printStackTrace();", indentation, exception_name),
            }],
        })
    }

    /// Generate a fix to rethrow exception
    fn rethrow_exception(node: &SyntaxNode) -> Option<QuickFix> {
        // Find catch clause
        let catch_node = if node.kind() == SyntaxKind::CATCH_CLAUSE {
            node.clone()
        } else {
            node.ancestors()
                .find(|n| n.kind() == SyntaxKind::CATCH_CLAUSE)?
        };

        // Find the block within catch
        let block = catch_node
            .children()
            .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)?;
        if !Self::is_empty_catch_block(&block) {
            return None;
        }

        // Find exception parameter name
        let exception_name = Self::extract_catch_param_name(&catch_node)?;

        // Insert throw statement after opening brace
        let insert_pos = block.text_range().start() + rowan::TextSize::from(1);

        let indentation = Self::get_indentation(&block);

        Some(QuickFix {
            title: "Rethrow as RuntimeException".to_string(),
            description: "Wrap and rethrow the exception".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(insert_pos, insert_pos),
                new_text: format!(
                    "\n{}    throw new RuntimeException({});",
                    indentation, exception_name
                ),
            }],
        })
    }

    /// Extract exception parameter name from catch clause
    fn extract_catch_param_name(catch_node: &SyntaxNode) -> Option<String> {
        // Look for IDENT in the catch clause (the exception variable name)
        for child in catch_node.children_with_tokens() {
            if child.kind() == SyntaxKind::IDENT {
                if let Some(token) = child.as_token() {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    fn is_empty_catch_block(block: &SyntaxNode) -> bool {
        block.children_with_tokens().all(|element| {
            matches!(
                element.kind(),
                SyntaxKind::L_BRACE | SyntaxKind::R_BRACE | SyntaxKind::WHITESPACE
            )
        })
    }

    /// Generate a fix to remove a redundant modifier
    fn remove_redundant_modifier(node: &SyntaxNode, message: &str) -> Option<QuickFix> {
        // Determine which modifier to remove from the message
        let (modifier, kind) = if message.contains("'public'") {
            ("public", SyntaxKind::PUBLIC_KW)
        } else if message.contains("'abstract'") {
            ("abstract", SyntaxKind::ABSTRACT_KW)
        } else {
            return None;
        };

        // Only remove the actual modifier named by the diagnostic. The caller
        // may pass a whole method or declaration, whose range would delete code.
        let modifier_node = node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::MODIFIER)?;
        let token = modifier_node
            .children_with_tokens()
            .find_map(|element| element.into_token())?;
        if token.kind() != kind {
            return None;
        }
        let range = token.text_range();

        Some(QuickFix {
            title: format!("Remove redundant '{modifier}' modifier"),
            description: format!("Remove the {modifier} modifier as it's implicit"),
            edits: vec![TextEdit {
                range,
                new_text: String::new(),
            }],
        })
    }

    /// Extract exception type from diagnostic message
    fn extract_exception_type(message: &str) -> Option<String> {
        let rest = message.strip_prefix("unhandled exception:")?.trim_start();
        let name = rest.split_whitespace().next()?;
        let valid = name.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first == '_' || first == '$' || first.is_alphabetic())
                && chars.all(|c| c == '_' || c == '$' || c.is_alphanumeric())
        });
        valid.then(|| name.to_string())
    }

    /// Get indentation from a node
    fn get_indentation(node: &SyntaxNode) -> String {
        // Look at preceding whitespace to determine indentation
        if let Some(prev) = node
            .prev_sibling_or_token()
            .filter(|t| t.kind() == SyntaxKind::WHITESPACE)
        {
            if let Some(token) = prev.as_token() {
                let text = token.text();
                if let Some(last_line) = text.lines().last() {
                    return last_line.to_string();
                }
            }
        }

        // Default to 4 spaces
        "    ".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    #[test]
    fn does_not_offer_unverified_or_empty_fixes() {
        let source =
            "class Main { Missing value; boolean same(String a, String b) { return a == b; } }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);

        assert!(QuickFixProvider::get_quick_fixes(&root, "unresolved Missing").is_empty());
        assert!(QuickFixProvider::get_quick_fixes(&root, "String comparison using ==").is_empty());
    }

    #[test]
    fn override_fix_precedes_modifiers_and_preserves_indentation() {
        let source = "class Main {\n    public String toString() { return \"x\"; }\n}";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let method = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .expect("method declaration");
        assert!(
            QuickFixProvider::get_quick_fixes(&method, "cannot override final method").is_empty()
        );
        let diagnostic = "Method 'toString' overrides Object method but lacks @Override annotation";
        let fixes = QuickFixProvider::get_quick_fixes(&method, diagnostic);
        let edit = &fixes[0].edits[0];
        let start: usize = edit.range.start().into();
        let result = format!("{}{}{}", &source[..start], edit.new_text, &source[start..]);
        assert!(result.contains("\n    @Override\n    public String toString()"));

        let (green, errors) = Parser::parse(Lexer::new(&result).collect());
        assert!(errors.is_empty(), "invalid generated Java: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let method = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .expect("method declaration");
        assert!(QuickFixProvider::get_quick_fixes(&method, diagnostic).is_empty());
    }

    #[test]
    fn throws_fix_declines_existing_clause_and_invalid_exception_name() {
        let source = "class C { void f() throws java.io.IOException { work(); } }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let method = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .expect("method");
        assert!(
            QuickFixProvider::get_quick_fixes(&method, "unhandled exception: Other").is_empty()
        );

        let source = "class C { void f() { work(); } }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let method = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .expect("method");
        for message in [
            "unhandled exception:",
            "unhandled exception: Broken;",
            "unhandled exception: a..B",
        ] {
            assert!(
                QuickFixProvider::get_quick_fixes(&method, message).is_empty(),
                "{message}"
            );
        }
        let fixes =
            QuickFixProvider::get_quick_fixes(&method, "unhandled exception: java.io.IOException");
        assert_eq!(fixes.len(), 1);
        let edit = &fixes[0].edits[0];
        let offset: usize = edit.range.start().into();
        let updated = format!(
            "{}{}{}",
            &source[..offset],
            edit.new_text,
            &source[offset..]
        );
        let (_, errors) = Parser::parse(Lexer::new(&updated).collect());
        assert!(errors.is_empty(), "invalid throws edit: {errors:?}");
    }

    #[test]
    fn final_fix_respects_sibling_modifiers() {
        let source = "class C { private int value; final int fixed = 1; }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let fields: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::FIELD_DECL)
            .collect();
        let fixes = QuickFixProvider::get_quick_fixes(&fields[0], "field should be final");
        let edit = &fixes[0].edits[0];
        let offset: usize = edit.range.start().into();
        let updated = format!(
            "{}{}{}",
            &source[..offset],
            edit.new_text,
            &source[offset..]
        );
        assert!(updated.contains("private final int value"));
        let (_, errors) = Parser::parse(Lexer::new(&updated).collect());
        assert!(errors.is_empty(), "invalid final edit: {errors:?}");
        assert!(QuickFixProvider::get_quick_fixes(&fields[1], "field should be final").is_empty());
    }

    #[test]
    fn catch_fixes_require_an_empty_block_and_actual_parameter() {
        let source = "class C { void f() { try {} catch (Exception problem) {} } }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let catch = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CATCH_CLAUSE)
            .expect("catch clause");
        let fixes = QuickFixProvider::get_quick_fixes(&catch, "Empty catch block");
        assert_eq!(fixes.len(), 2);
        assert!(fixes
            .iter()
            .all(|fix| fix.edits[0].new_text.contains("problem")));
        for fix in &fixes {
            let edit = &fix.edits[0];
            let offset: usize = edit.range.start().into();
            let updated = format!(
                "{}{}{}",
                &source[..offset],
                edit.new_text,
                &source[offset..]
            );
            let (_, errors) = Parser::parse(Lexer::new(&updated).collect());
            assert!(errors.is_empty(), "invalid catch edit: {errors:?}");
        }

        for body in ["problem.printStackTrace();", "/* retain note */"] {
            let source = format!(
                "class C {{ void f() {{ try {{}} catch (Exception problem) {{{body}}} }} }}"
            );
            let (green, errors) = Parser::parse(Lexer::new(&source).collect());
            assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
            let root = SyntaxNode::new_root(green);
            let catch = root
                .descendants()
                .find(|node| node.kind() == SyntaxKind::CATCH_CLAUSE)
                .expect("catch clause");
            assert!(QuickFixProvider::get_quick_fixes(&catch, "Empty catch block").is_empty());
        }
    }

    #[test]
    fn unused_variable_diagnostic_preserves_initializers_and_shared_declarations() {
        let source = "class C { void f() { int unused = send(); int a, b; } }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        for variable in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL)
        {
            assert!(QuickFixProvider::get_quick_fixes(&variable, "variable never used").is_empty());
        }
    }

    #[test]
    fn redundant_modifier_fix_only_removes_matching_modifier() {
        let source = "class Main { public abstract void value(); }";
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let modifiers: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::MODIFIER)
            .collect();
        let public = modifiers
            .iter()
            .find(|node| node.text() == "public")
            .expect("public modifier");
        let abstract_modifier = modifiers
            .iter()
            .find(|node| node.text() == "abstract")
            .expect("abstract modifier");
        assert!(
            QuickFixProvider::get_quick_fixes(public, "redundant 'abstract' Modifier").is_empty()
        );
        let fixes =
            QuickFixProvider::get_quick_fixes(abstract_modifier, "redundant 'abstract' Modifier");
        assert_eq!(fixes.len(), 1);
        let edit = &fixes[0].edits[0];
        assert_eq!(
            &source[usize::from(edit.range.start())..usize::from(edit.range.end())],
            "abstract"
        );
    }

    #[test]
    fn test_quick_fix_creation() {
        let fix = QuickFix {
            title: "Test fix".to_string(),
            description: "Test description".to_string(),
            edits: vec![TextEdit {
                range: TextRange::new(0.into(), 5.into()),
                new_text: "replacement".to_string(),
            }],
        };

        assert_eq!(fix.title, "Test fix");
        assert_eq!(fix.edits.len(), 1);
    }

    #[test]
    fn test_text_edit_creation() {
        let edit = TextEdit {
            range: TextRange::new(10.into(), 20.into()),
            new_text: "new text".to_string(),
        };

        assert_eq!(edit.new_text, "new text");
        assert_eq!(edit.range.start(), 10.into());
        assert_eq!(edit.range.end(), 20.into());
    }

    #[test]
    fn test_extract_exception_type() {
        let message = "unhandled exception: java.io.IOException must be caught";
        let result = QuickFixProvider::extract_exception_type(message);
        assert_eq!(result, Some("java.io.IOException".to_string()));
    }

    #[test]
    fn test_extract_exception_type_no_match() {
        let message = "some other error";
        let result = QuickFixProvider::extract_exception_type(message);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_indentation_default() {
        // This is a simplified test - in real usage we'd need a proper syntax tree
        // For now, just test that the function doesn't crash
        let indentation = QuickFixProvider::get_indentation(&rowan::SyntaxNode::<
            hyperion_parser::syntax::JavaLanguage,
        >::new_root(
            rowan::GreenNode::new(rowan::SyntaxKind(0), vec![]),
        ));
        assert_eq!(indentation, "    ");
    }
}
