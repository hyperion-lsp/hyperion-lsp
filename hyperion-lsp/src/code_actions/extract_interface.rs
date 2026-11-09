// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Extract interface from a top-level class, from its CST.
//!
//! The plan is shared by two presentations: the interface next to the class
//! in the same file (works for every client) and, when the client supports
//! `CreateFile`, the interface in its own file.

use std::collections::HashMap;

use hyperion_parser::syntax::{
    AstNode, ClassDecl, MethodDecl, SyntaxKind, SyntaxNode, SyntaxToken,
};
use tower_lsp::lsp_types::{
    CodeAction, CreateFile, CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, ResourceOp, TextDocumentEdit, TextEdit as LspTextEdit,
    Url, WorkspaceEdit,
};

use super::kinds;
use crate::position_utils::offset_to_position;

pub struct ExtractInterface {
    pub class_name: String,
    pub interface_name: String,
    pub class_is_public: bool,
    /// `<T extends Comparable<T>>` of the class, if generic (declaration form).
    type_params: Option<String>,
    methods: Vec<String>,
    /// Byte offset where an in-file interface goes (before the class's doc comment).
    decl_start: usize,
    indent: String,
    implements_edit: (usize, String),
    imports: Vec<String>,
    package: Option<String>,
}

fn last_non_trivia_token_before(node: &SyntaxNode, limit: usize) -> Option<usize> {
    node.descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia())
        .map(|token| token.text_range())
        .filter(|range| usize::from(range.end()) <= limit)
        .map(|range| usize::from(range.end()))
        .max()
}

/// The `MODIFIER_LIST` that precedes a declaration node (modifiers and
/// annotations are siblings of the declaration in this CST).
fn preceding_modifiers(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut previous = node.prev_sibling_or_token();
    while let Some(element) = previous {
        match element.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT => {
                previous = element.prev_sibling_or_token();
            }
            SyntaxKind::MODIFIER_LIST => return element.into_node(),
            _ => return None,
        }
    }
    None
}

fn has_modifier(modifiers: &Option<SyntaxNode>, kind: SyntaxKind) -> bool {
    modifiers.as_ref().is_some_and(|list| {
        list.children_with_tokens()
            .any(|element| element.kind() == kind)
            || list
                .children()
                .any(|child| child.children_with_tokens().any(|e| e.kind() == kind))
    })
}

fn is_word_char(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

fn mentions(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(start, _)| {
        !text[..start].chars().next_back().is_some_and(is_word_char)
            && !text[start + name.len()..]
                .chars()
                .next()
                .is_some_and(is_word_char)
    })
}

fn method_signature(source: &str, method: &MethodDecl) -> Option<String> {
    let node = method.syntax();
    let modifiers = preceding_modifiers(node);
    if !has_modifier(&modifiers, SyntaxKind::PUBLIC_KW)
        || has_modifier(&modifiers, SyntaxKind::STATIC_KW)
    {
        return None;
    }
    let start = node
        .children_with_tokens()
        .find(|element| !element.kind().is_trivia())?
        .text_range()
        .start();
    let end = node
        .children_with_tokens()
        .filter(|element| {
            matches!(
                element.kind(),
                SyntaxKind::PARAM_LIST | SyntaxKind::THROWS_CLAUSE
            )
        })
        .map(|element| element.text_range().end())
        .max()?;
    let text = source.get(usize::from(start)..usize::from(end))?;
    Some(format!(
        "{};",
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    ))
}

/// Plan an extract-interface for the top-level class whose header contains
/// `offset`. `type_exists` reports whether a type with a given simple name is
/// already declared in the package.
pub fn plan(
    source: &str,
    offset: usize,
    type_exists: impl Fn(&str) -> bool,
) -> Option<ExtractInterface> {
    let tokens = hyperion_parser::lexer::Lexer::new(source).collect();
    let (green, _) = hyperion_parser::parser::Parser::parse(tokens);
    let root = SyntaxNode::new_root(green);

    let class = root.children().filter_map(ClassDecl::cast).find(|class| {
        let node = class.syntax();
        let header_start = preceding_modifiers(node)
            .map_or(node.text_range().start(), |list| list.text_range().start());
        let brace = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::L_BRACE)
            .map_or(usize::from(node.text_range().end()), |t| {
                usize::from(t.text_range().start())
            });
        usize::from(header_start) <= offset && offset <= brace
    })?;
    let node = class.syntax();
    let class_name = class.name()?.text().to_string();
    let interface_name = format!("I{class_name}");
    if type_exists(&interface_name) {
        return None;
    }
    let methods = class
        .methods()
        .filter_map(|method| method_signature(source, &method))
        .collect::<Vec<_>>();
    if methods.is_empty() {
        return None;
    }
    let class_modifiers = preceding_modifiers(node);
    let class_is_public = has_modifier(&class_modifiers, SyntaxKind::PUBLIC_KW);

    let type_params = class
        .type_params()
        .map(|params| params.syntax().text().to_string());
    let type_args = class.type_params().map(|params| {
        let names = params
            .syntax()
            .children()
            .filter(|child| child.kind() == SyntaxKind::TYPE_PARAM)
            .filter_map(|param| {
                param
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind().is_identifier() || token.kind().is_type_identifier())
                    .map(|token: SyntaxToken| token.text().to_string())
            })
            .collect::<Vec<_>>();
        format!("<{}>", names.join(", "))
    });
    let interface_ref = type_args.map_or(interface_name.clone(), |args| {
        format!("{interface_name}{args}")
    });

    // `implements` clause: append to an existing list, or add one after the
    // last header element (type parameters, extends type, class name).
    let brace = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind() == SyntaxKind::L_BRACE)?;
    let brace_start = usize::from(brace.text_range().start());
    let last_header = last_non_trivia_token_before(node, brace_start)?;
    let has_implements = node
        .children_with_tokens()
        .any(|element| element.kind() == SyntaxKind::IMPLEMENTS_KW);
    let implements_edit = (
        last_header,
        if has_implements {
            format!(", {interface_ref}")
        } else {
            format!(" implements {interface_ref}")
        },
    );

    // Where an in-file interface goes: before the class's leading doc comment
    // and modifiers.
    let decl_start = {
        let mut start = class_modifiers
            .as_ref()
            .map_or(node.text_range().start(), |list| list.text_range().start());
        let mut previous = class_modifiers
            .as_ref()
            .unwrap_or(node)
            .prev_sibling_or_token();
        while let Some(element) = previous {
            match element.kind() {
                SyntaxKind::WHITESPACE => previous = element.prev_sibling_or_token(),
                SyntaxKind::DOC_COMMENT => {
                    start = element.text_range().start();
                    break;
                }
                _ => break,
            }
        }
        usize::from(start)
    };
    let line_start = source[..decl_start].rfind('\n').map_or(0, |p| p + 1);
    let indent = source[line_start..decl_start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>();

    let signature_text = methods.join(" ");
    let header_text = type_params.clone().unwrap_or_default();
    let imports = root
        .children()
        .filter(|child| child.kind() == SyntaxKind::IMPORT_DECL)
        .filter_map(|import| {
            let text = import.text().to_string();
            let text = text.trim().to_string();
            let is_wildcard = text.trim_end_matches(';').ends_with(".*");
            let simple = text
                .trim_end_matches(';')
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            (is_wildcard || mentions(&signature_text, &simple) || mentions(&header_text, &simple))
                .then_some(text)
        })
        .collect::<Vec<_>>();
    let package = root
        .children()
        .find(|child| child.kind() == SyntaxKind::PACKAGE_DECL)
        .and_then(|package| {
            package
                .children()
                .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
                .map(|name| name.text().to_string())
        });

    Some(ExtractInterface {
        class_name,
        interface_name,
        class_is_public,
        type_params,
        methods,
        decl_start,
        indent,
        implements_edit,
        imports,
        package,
    })
}

impl ExtractInterface {
    fn interface_declaration(&self, public: bool, indent: &str) -> String {
        let visibility = if public { "public " } else { "" };
        let generics = self.type_params.as_deref().unwrap_or_default();
        let mut code = format!(
            "{indent}{visibility}interface {}{generics} {{\n",
            self.interface_name
        );
        for method in &self.methods {
            code.push_str(&format!("{indent}    {method}\n"));
        }
        code.push_str(&format!("{indent}}}"));
        code
    }

    fn implements_text_edit(&self, source: &str) -> LspTextEdit {
        let position = offset_to_position(source, self.implements_edit.0);
        LspTextEdit {
            range: tower_lsp::lsp_types::Range::new(position, position),
            new_text: self.implements_edit.1.clone(),
        }
    }

    /// The interface as a package-private type in front of the class.
    pub fn same_file_action(&self, uri: &Url, source: &str) -> CodeAction {
        let at = offset_to_position(source, self.decl_start);
        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![
                LspTextEdit {
                    range: tower_lsp::lsp_types::Range::new(at, at),
                    new_text: format!(
                        "{}\n\n{}",
                        self.interface_declaration(false, &self.indent),
                        self.indent
                    ),
                },
                self.implements_text_edit(source),
            ],
        );
        CodeAction {
            title: format!("Extract interface '{}'", self.interface_name),
            kind: Some(kinds::refactor_extract_interface()),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// The interface in its own file next to the class (`CreateFile`).
    pub fn new_file_action(
        &self,
        uri: &Url,
        source: &str,
        version: Option<i32>,
        siblings_exist: impl Fn(&std::path::Path) -> bool,
    ) -> Option<CodeAction> {
        let class_path = uri.to_file_path().ok()?;
        let new_path = class_path.with_file_name(format!("{}.java", self.interface_name));
        if siblings_exist(&new_path) {
            return None;
        }
        let new_uri = Url::from_file_path(&new_path).ok()?;
        let mut text = String::new();
        if let Some(package) = &self.package {
            text.push_str(&format!("package {package};\n\n"));
        }
        if !self.imports.is_empty() {
            for import in &self.imports {
                text.push_str(import);
                text.push('\n');
            }
            text.push('\n');
        }
        text.push_str(&self.interface_declaration(self.class_is_public, ""));
        text.push('\n');
        let start = tower_lsp::lsp_types::Position::new(0, 0);
        let operations = vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: new_uri.clone(),
                options: Some(CreateFileOptions {
                    overwrite: Some(false),
                    ignore_if_exists: Some(false),
                }),
                annotation_id: None,
            })),
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: new_uri,
                    version: None,
                },
                edits: vec![OneOf::Left(LspTextEdit {
                    range: tower_lsp::lsp_types::Range::new(start, start),
                    new_text: text,
                })],
            }),
            DocumentChangeOperation::Edit(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                edits: vec![OneOf::Left(self.implements_text_edit(source))],
            }),
        ];
        Some(CodeAction {
            title: format!("Extract interface '{}'", self.interface_name),
            kind: Some(kinds::refactor_extract_interface()),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(operations)),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}
