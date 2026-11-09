// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import organization utilities
//!
//! This module provides functionality to organize and clean up import statements:
//! - Sort imports alphabetically
//! - Group imports by package
//! - Remove duplicate imports
//! - Format import spacing

use hyperion_parser::syntax::{AstNode, ImportDecl, SyntaxKind, SyntaxNode};
use rowan::TextRange;
use std::collections::BTreeSet;

/// Text edit for import organization
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Range to replace
    pub range: TextRange,
    /// New text to insert
    pub new_text: String,
}

/// Import organizer for Java files
pub struct ImportOrganizer;

impl ImportOrganizer {
    /// Organize imports in a file
    ///
    /// This will:
    /// 1. Collect all import statements
    /// 2. Sort them alphabetically
    /// 3. Remove duplicates
    /// 4. Group by package (java.*, javax.*, others)
    /// 5. Generate a clean import block
    ///
    /// # Arguments
    /// * `root` - The root syntax node of the file
    ///
    /// # Returns
    /// A list of text edits to apply
    pub fn organize_imports(root: &SyntaxNode) -> Vec<TextEdit> {
        let import_nodes: Vec<_> = root
            .children()
            .filter(|node| node.kind() == SyntaxKind::IMPORT_DECL)
            .collect();
        let (Some(first), Some(last)) = (import_nodes.first(), import_nodes.last()) else {
            return vec![];
        };

        let range = TextRange::new(first.text_range().start(), last.text_range().end());
        let source = root.text().to_string();
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        // Comments can document an individual import or the whole group. Moving
        // imports without carrying that association would silently discard or
        // misplace user text, so leave such blocks alone for now. Likewise, an
        // incomplete import must not be reconstructed as a valid declaration.
        if root
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .any(|token| {
                let token_range = token.text_range();
                if token.kind() == SyntaxKind::ERROR {
                    return token_range.start() >= range.start()
                        && token_range.start() < range.end();
                }
                if !token.kind().is_comment() {
                    return false;
                }

                let comment_start: usize = token_range.start().into();
                let comment_end: usize = token_range.end().into();
                if comment_start >= start && comment_start < end {
                    return true;
                }
                // A comment on the last import line, or directly above/below
                // the block, may describe a declaration that sorting moves.
                let gap = if comment_end <= start {
                    source.get(comment_end..start)
                } else if comment_start >= end {
                    source.get(end..comment_start)
                } else {
                    None
                };
                gap.is_some_and(|gap| {
                    gap.chars().all(char::is_whitespace)
                        && gap.bytes().filter(|byte| *byte == b'\n').count() <= 1
                })
            })
        {
            return vec![];
        }

        let mut imports = BTreeSet::new();
        for node in import_nodes {
            let Some(import_text) = Self::extract_import(&node) else {
                return vec![];
            };
            if !node
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::SEMICOLON)
            {
                return vec![];
            }
            imports.insert(import_text);
        }

        // Organize imports into groups
        let organized = Self::organize_into_groups(&imports);
        if source.get(start..end) == Some(organized.as_str()) {
            return vec![];
        }
        vec![TextEdit {
            range,
            new_text: organized,
        }]
    }

    /// Extract import statement text from a node
    fn extract_import(node: &SyntaxNode) -> Option<String> {
        if let Some(import_decl) = ImportDecl::cast(node.clone()) {
            // Get the import path
            if let Some(path) = import_decl.path() {
                let path_str = path.as_string();
                let is_static = import_decl.is_static();
                let is_wildcard = import_decl.is_wildcard();

                let mut result = String::from("import ");
                if is_static {
                    result.push_str("static ");
                }
                result.push_str(&path_str);
                if is_wildcard {
                    result.push_str(".*");
                }
                result.push(';');

                return Some(result);
            }
        }

        None
    }

    /// Organize imports into groups
    fn organize_into_groups(imports: &BTreeSet<String>) -> String {
        let mut java_imports = Vec::new();
        let mut javax_imports = Vec::new();
        let mut other_imports = Vec::new();
        let mut static_imports = Vec::new();

        for import in imports {
            if import.starts_with("import static ") {
                static_imports.push(import.clone());
            } else if import.starts_with("import java.") {
                java_imports.push(import.clone());
            } else if import.starts_with("import javax.") {
                javax_imports.push(import.clone());
            } else {
                other_imports.push(import.clone());
            }
        }

        let mut result = String::new();

        // Add static imports first
        if !static_imports.is_empty() {
            for import in static_imports {
                result.push_str(&import);
                result.push('\n');
            }
            result.push('\n');
        }

        // Add java.* imports
        if !java_imports.is_empty() {
            for import in java_imports {
                result.push_str(&import);
                result.push('\n');
            }
            if !javax_imports.is_empty() || !other_imports.is_empty() {
                result.push('\n');
            }
        }

        // Add javax.* imports
        if !javax_imports.is_empty() {
            for import in javax_imports {
                result.push_str(&import);
                result.push('\n');
            }
            if !other_imports.is_empty() {
                result.push('\n');
            }
        }

        // Add other imports
        if !other_imports.is_empty() {
            for import in other_imports {
                result.push_str(&import);
                result.push('\n');
            }
        }

        // Remove trailing newline
        if result.ends_with('\n') {
            result.pop();
        }

        result
    }

    /// Remove duplicate imports
    pub fn remove_duplicates(imports: Vec<String>) -> Vec<String> {
        let set: BTreeSet<String> = imports.into_iter().collect();
        set.into_iter().collect()
    }

    /// Sort imports alphabetically
    pub fn sort_imports(mut imports: Vec<String>) -> Vec<String> {
        imports.sort();
        imports
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn organize(source: &str) -> Option<String> {
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let edits = ImportOrganizer::organize_imports(&root);
        assert!(edits.len() <= 1);
        edits.first().map(|edit| {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();
            format!("{}{}{}", &source[..start], edit.new_text, &source[end..])
        })
    }

    #[test]
    fn organizes_real_java_file_and_is_idempotent() {
        let source = "package example;\n\nimport com.acme.java.Foo;\nimport javax.swing.JFrame;\nimport java.util.List;\nimport java.util.List;\n\nclass Main {}\n";
        let organized = organize(source).expect("imports should be organized");
        assert_eq!(organized, "package example;\n\nimport java.util.List;\n\nimport javax.swing.JFrame;\n\nimport com.acme.java.Foo;\n\nclass Main {}\n");
        assert_eq!(organize(&organized), None);
    }

    #[test]
    fn preserves_comments_in_import_block() {
        let source = "import java.util.Map;\n// Keep this reason with List.\nimport java.util.List;\nclass Main {}";
        assert_eq!(organize(source), None);

        let source =
            "import java.util.Map; /* intentional */\nimport java.util.List;\nclass Main {}";
        assert_eq!(organize(source), None);

        let source = "// List is required by the API.\nimport java.util.List;\nimport java.util.Map;\nclass Main {}";
        assert_eq!(organize(source), None);

        let source = "import java.util.Map;\nimport java.util.List; // List is required by the API.\nclass Main {}";
        assert_eq!(organize(source), None);

        let source =
            "// File license\n\nimport java.util.Map;\nimport java.util.List;\nclass Main {}";
        assert!(
            organize(source).is_some(),
            "detached file header is preserved"
        );
    }

    #[test]
    fn test_remove_duplicates() {
        let imports = vec![
            "import java.util.List;".to_string(),
            "import java.util.Map;".to_string(),
            "import java.util.List;".to_string(),
        ];

        let result = ImportOrganizer::remove_duplicates(imports);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"import java.util.List;".to_string()));
        assert!(result.contains(&"import java.util.Map;".to_string()));
    }

    #[test]
    fn test_sort_imports() {
        let imports = vec![
            "import java.util.Map;".to_string(),
            "import java.util.List;".to_string(),
            "import java.io.File;".to_string(),
        ];

        let result = ImportOrganizer::sort_imports(imports);
        assert_eq!(result[0], "import java.io.File;");
        assert_eq!(result[1], "import java.util.List;");
        assert_eq!(result[2], "import java.util.Map;");
    }

    #[test]
    fn test_organize_into_groups_java() {
        let mut imports = BTreeSet::new();
        imports.insert("import java.util.List;".to_string());
        imports.insert("import javax.swing.JFrame;".to_string());
        imports.insert("import com.example.Foo;".to_string());

        let result = ImportOrganizer::organize_into_groups(&imports);

        // Should have java, then javax, then others with blank lines between
        assert!(result.contains("import java.util.List;"));
        assert!(result.contains("import javax.swing.JFrame;"));
        assert!(result.contains("import com.example.Foo;"));

        // Java should come before javax
        let java_pos = result.find("java.util").unwrap();
        let javax_pos = result.find("javax.swing").unwrap();
        assert!(java_pos < javax_pos);
    }

    #[test]
    fn test_organize_into_groups_static() {
        let mut imports = BTreeSet::new();
        imports.insert("import static org.junit.Assert.assertEquals;".to_string());
        imports.insert("import java.util.List;".to_string());

        let result = ImportOrganizer::organize_into_groups(&imports);

        // Static imports should come first
        let static_pos = result.find("import static").unwrap();
        let java_pos = result.find("import java").unwrap();
        assert!(static_pos < java_pos);
    }

    #[test]
    fn test_organize_into_groups_empty() {
        let imports = BTreeSet::new();
        let result = ImportOrganizer::organize_into_groups(&imports);
        assert_eq!(result, "");
    }

    #[test]
    fn test_text_edit_creation() {
        let edit = TextEdit {
            range: TextRange::new(0.into(), 10.into()),
            new_text: "import java.util.List;".to_string(),
        };

        assert_eq!(edit.new_text, "import java.util.List;");
        assert_eq!(edit.range.start(), 0.into());
        assert_eq!(edit.range.end(), 10.into());
    }
}
