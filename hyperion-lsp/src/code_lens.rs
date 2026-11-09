// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Lens Provider
//!
//! Provides inline actionable information above code elements:
//! - "Run" buttons above executable Java entry points
//!
//! Implementation counts are deliberately not emitted until Hyperion has a
//! portable command that can present a list of locations to the client.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, ParseDatabase};
use tower_lsp::lsp_types::{CodeLens, Command, Position, Range};
use tracing::debug;

use crate::commands::RUN_COMMAND;
use crate::position_utils::offset_to_position;
use crate::semantic_host::ReadTxn;

/// Provides code lens functionality
#[derive(Default)]
pub struct CodeLensProvider;

impl CodeLensProvider {
    /// Creates a new CodeLensProvider
    pub const fn new() -> Self {
        Self
    }

    /// Get code lenses for a file
    pub fn code_lens(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        source_text: &str,
    ) -> Result<Vec<CodeLens>, String> {
        debug!("Computing code lenses for file {:?}", file_id);

        let root = txn.parse_file(file_id).syntax();

        let mut lenses = Vec::new();

        // Only publish actions that Hyperion can execute. Test-framework
        // execution is not implemented, so test methods deliberately do not
        // receive a misleading "Run Test" lens.
        for node in root.descendants() {
            match node.kind() {
                // Check if this is a `public static void main(String[] args)`
                // entry point ("▶ Run" lens).
                SyntaxKind::METHOD_DECL if self.is_main_method(&node) => {
                    if let Some(lens) = self.create_run_lens(&node, source_text) {
                        lenses.push(lens);
                    }
                }
                _ => {}
            }
        }

        debug!("Generated {} code lenses", lenses.len());
        Ok(lenses)
    }

    /// Check if a method is a valid Java entry point:
    /// `public static void main(String[] args)`.
    ///
    /// Deliberately does NOT require the enclosing class to be `public` --
    /// `java ClassName` happily launches a package-private top-level class,
    /// only the method's own modifiers/signature matter. Rejects the classic
    /// near-miss signatures: missing `static`, a non-array `String` param
    /// (`main(String args)`), and no-arg `main()`.
    fn is_main_method(&self, method_node: &SyntaxNode) -> bool {
        if self.get_method_name(method_node).as_deref() != Some("main") {
            return false;
        }

        // Modifiers live in the METHOD_DECL's preceding MODIFIER_LIST
        // sibling, wrapped one level deeper in MODIFIER nodes, so use
        // descendants_with_tokens to
        // see through that wrapper rather than children_with_tokens.
        let Some(prev_sibling) = method_node.prev_sibling() else {
            return false;
        };
        if prev_sibling.kind() != SyntaxKind::MODIFIER_LIST {
            return false;
        }
        let mut has_public = false;
        let mut has_static = false;
        for element in prev_sibling.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                match token.kind() {
                    SyntaxKind::PUBLIC_KW => has_public = true,
                    SyntaxKind::STATIC_KW => has_static = true,
                    _ => {}
                }
            }
        }
        if !has_public || !has_static {
            return false;
        }

        // Return type must be `void`. METHOD_DECL's first child node is
        // always the return type (PRIMITIVE_TYPE/CLASS_TYPE/...).
        let is_void_return = method_node.children().next().is_some_and(|first| {
            first.kind() == SyntaxKind::PRIMITIVE_TYPE
                && first
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .any(|t| t.kind() == SyntaxKind::VOID_KW)
        });
        if !is_void_return {
            return false;
        }

        // Exactly one parameter, of type `String[]` (a CLASS_TYPE "String"
        // sibling plus an ARRAY_TYPE sibling within the same PARAM node --
        // this grammar represents `Type[] name` as trailing-bracket
        // siblings, not a nested array-of type).
        let Some(param_list) = method_node
            .children()
            .find(|c| c.kind() == SyntaxKind::PARAM_LIST)
        else {
            return false;
        };
        let params: Vec<_> = param_list
            .children()
            .filter(|c| c.kind() == SyntaxKind::PARAM)
            .collect();
        let [param] = params.as_slice() else {
            return false;
        };

        let has_string_type = param
            .children()
            .filter(|c| c.kind() == SyntaxKind::CLASS_TYPE)
            .any(|class_type| {
                class_type
                    .descendants_with_tokens()
                    .filter_map(|el| el.into_token())
                    .filter(|t| t.kind() == SyntaxKind::IDENT)
                    .last()
                    .is_some_and(|t| t.text() == "String")
            });
        let has_array = param.children().any(|c| c.kind() == SyntaxKind::ARRAY_TYPE);

        has_string_type && has_array
    }

    /// Get method name from method declaration node
    fn get_method_name(&self, method_node: &SyntaxNode) -> Option<String> {
        for child in method_node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    /// Get class/interface name from declaration node
    fn get_class_name(&self, node: &SyntaxNode) -> Option<String> {
        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    /// Fully qualified JVM binary name (`pkg.Outer$Inner`) of the class that
    /// declares `method_node`; this is what `java` and `hyperion.run` need.
    fn qualified_class_name(&self, method_node: &SyntaxNode) -> Option<String> {
        let mut names: Vec<String> = method_node
            .ancestors()
            .filter(|ancestor| ancestor.kind().is_type_declaration())
            .filter_map(|ancestor| self.get_class_name(&ancestor))
            .collect();
        if names.is_empty() {
            return None;
        }
        names.reverse();
        let binary = names.join("$");
        let package = method_node
            .ancestors()
            .last()?
            .children()
            .find(|child| child.kind() == SyntaxKind::PACKAGE_DECL)
            .and_then(|decl| {
                decl.children()
                    .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
                    .map(|name| {
                        name.text()
                            .to_string()
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                    })
            });
        Some(match package {
            Some(package) if !package.is_empty() => format!("{package}.{binary}"),
            _ => binary,
        })
    }

    /// Create a code lens for a `public static void main(String[] args)` entry point.
    fn create_run_lens(&self, method_node: &SyntaxNode, source_text: &str) -> Option<CodeLens> {
        let method_name = self.get_method_name(method_node)?;
        let class_name = self.qualified_class_name(method_node)?;

        let start_offset = method_node.text_range().start();
        let position = offset_to_position(source_text, start_offset.into());

        // LSP positions use UTF-16 code units, not UTF-8 bytes.
        let range = Range {
            start: position,
            end: Position {
                line: position.line,
                character: position.character + method_name.encode_utf16().count() as u32,
            },
        };

        Some(CodeLens {
            range,
            command: Some(Command {
                title: "▶ Run".to_string(),
                command: RUN_COMMAND.to_string(),
                arguments: Some(vec![serde_json::json!({
                    "className": class_name,
                })]),
            }),
            data: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    // "▶ Run" lens / main-method detection ----------------

    fn make_provider() -> CodeLensProvider {
        CodeLensProvider::new()
    }

    fn parse(code: &str) -> SyntaxNode {
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    /// Find the METHOD_DECL named `name` anywhere in `node`'s subtree.
    fn find_method_named(
        node: &SyntaxNode,
        name: &str,
        provider: &CodeLensProvider,
    ) -> Option<SyntaxNode> {
        if node.kind() == SyntaxKind::METHOD_DECL
            && provider.get_method_name(node).as_deref() == Some(name)
        {
            return Some(node.clone());
        }
        for child in node.children() {
            if let Some(found) = find_method_named(&child, name, provider) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn test_is_main_method_accepts_valid_signature() {
        let provider = make_provider();
        let root = parse(
            r"
            public class Main {
                public static void main(String[] args) {
                }
            }
        ",
        );
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        assert!(provider.is_main_method(&method));
    }

    #[test]
    fn test_is_main_method_accepts_non_public_class() {
        // `java ClassName` doesn't require the class itself to be `public`,
        // only the method's own modifiers/signature matter.
        let provider = make_provider();
        let root = parse(
            r"
            class Launcher {
                public static void main(String[] args) {
                }
            }
        ",
        );
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        assert!(
            provider.is_main_method(&method),
            "main in a non-public top-level class must still be detected"
        );
    }

    #[test]
    fn test_is_main_method_rejects_non_array_string_param() {
        let provider = make_provider();
        let root = parse(
            r"
            public class Main {
                public static void main(String args) {
                }
            }
        ",
        );
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        assert!(
            !provider.is_main_method(&method),
            "main(String args) (no array) must be rejected"
        );
    }

    #[test]
    fn test_is_main_method_rejects_no_args_no_static() {
        let provider = make_provider();
        let root = parse(
            r"
            public class Main {
                static void main() {
                }
            }
        ",
        );
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        assert!(
            !provider.is_main_method(&method),
            "static void main() with no params (and no `public`) must be rejected"
        );
    }

    #[test]
    fn test_is_main_method_rejects_non_static() {
        let provider = make_provider();
        let root = parse(
            r"
            public class Main {
                public void main(String[] args) {
                }
            }
        ",
        );
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        assert!(
            !provider.is_main_method(&method),
            "non-static main must be rejected"
        );
    }

    #[test]
    fn test_is_main_method_rejects_wrong_name() {
        let provider = make_provider();
        let root = parse(
            r"
            public class Main {
                public static void notMain(String[] args) {
                }
            }
        ",
        );
        let method = find_method_named(&root, "notMain", &provider).expect("notMain not found");
        assert!(!provider.is_main_method(&method));
    }

    #[test]
    fn test_code_lens_emits_run_lens_for_main_method() {
        let provider = make_provider();
        let code = r"
            public class Main {
                public static void main(String[] args) {
                }
            }
        ";
        let root = parse(code);
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        let lens = provider
            .create_run_lens(&method, code)
            .expect("Should produce a Run lens");

        assert_eq!(lens.command.as_ref().unwrap().title, "▶ Run");
        assert_eq!(lens.command.as_ref().unwrap().command, RUN_COMMAND);
        let args = lens.command.unwrap().arguments.unwrap();
        assert_eq!(args[0]["className"], "Main");
    }

    #[test]
    fn test_run_lens_class_name_is_fully_qualified_binary_name() {
        let provider = make_provider();
        let code = r"
            package com.example.app;

            public class Outer {
                public static class Inner {
                    public static void main(String[] args) {
                    }
                }
            }
        ";
        let root = parse(code);
        let method = find_method_named(&root, "main", &provider).expect("main not found");
        let lens = provider
            .create_run_lens(&method, code)
            .expect("Should produce a Run lens");
        let args = lens.command.unwrap().arguments.unwrap();
        assert_eq!(args[0]["className"], "com.example.app.Outer$Inner");
    }
}
