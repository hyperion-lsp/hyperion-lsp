// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Refactoring Actions for LSP
//!
//! This module implements refactoring code actions including:
//! - Inline variable/method
//! - Convert to for-each
//! - Safe delete
//! - Change method signature info

use hyperion_semantics::db::{FileId, ParseDatabase, SourceDatabase};
use hyperion_workspace::{
    DeclarationNamespace, FileRevisionToken, WorkspaceIndex, WorkspaceRevision,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Position, Range, TextEdit as LspTextEdit, Url, WorkspaceEdit,
};
use tracing::debug;

use crate::position_utils::offset_to_position;
use crate::semantic_host::ReadTxn;

/// Java declaration shapes currently supported by safe delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SafeDeleteDeclarationKind {
    Type,
    Method,
    Field,
}

pub(crate) struct SafeDeleteProof {
    pub(crate) edit: WorkspaceEdit,
    pub(crate) workspace_revision: WorkspaceRevision,
    pub(crate) scanned_revisions: Vec<FileRevisionToken>,
    pub(crate) resource_proof: Option<Arc<hyperion_workspace::RefactoringResourceProof>>,
}

struct SafeDeleteEvaluation {
    action: CodeAction,
    proof: Option<(
        WorkspaceRevision,
        Vec<FileRevisionToken>,
        Option<Arc<hyperion_workspace::RefactoringResourceProof>>,
    )>,
}

/// Provides refactoring action functionality
#[derive(Clone)]
pub struct RefactoringActions<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
}

impl<'txn> RefactoringActions<'txn> {
    /// Creates a new RefactoringActions
    pub const fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            txn,
            workspace_index,
        }
    }

    // ===== Inline Variable =====

    /// Generate an "Inline Variable" refactoring action
    ///
    /// When cursor is on a local variable declaration with an initializer,
    /// this action replaces all usages of the variable with its initializer
    /// and removes the declaration.
    pub fn inline_variable_action(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
    ) -> Option<CodeAction> {
        use crate::position_utils::position_to_offset;
        use hyperion_parser::syntax::SyntaxKind;

        debug!("Checking inline variable action at {:?}", cursor);

        // Parse the file. try_lock_for: degrade to no action rather than
        // freeze if did_change is currently holding the DB mutex.
        let root = {
            let db = self.txn;
            let parse_tree = db.parse_file(file_id);
            parse_tree.syntax()
        };

        let offset = position_to_offset(source_text, cursor).ok()? as u32;

        // Find the token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::new(offset))
            .right_biased()?;

        // Only handle identifiers
        if !token.kind().is_identifier() {
            return None;
        }

        let var_name = token.text().to_string();

        // Find if we're on a local variable declaration
        let mut current = token.parent();
        let mut local_var_decl = None;

        while let Some(node) = current {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                local_var_decl = Some(node);
                break;
            }
            current = node.parent();
        }

        let decl_node = local_var_decl?;

        // Extract the initializer expression
        let initializer = self.extract_variable_initializer(&decl_node, &var_name)?;

        // Find all usages of this variable in the containing method/block. A variable
        // assigned after its declaration has no single value to substitute, so the
        // action is withheld rather than rewriting `x = 3` into `5 = 3`.
        let decl_start: usize = decl_node.text_range().start().into();
        let decl_end: usize = decl_node.text_range().end().into();
        let containing_block = self.find_containing_block(&decl_node)?;
        let usages = self.find_variable_usages(
            &containing_block,
            &var_name,
            source_text,
            (decl_start, decl_end),
        )?;

        // Skip if no usages (nothing to inline)
        if usages.is_empty() {
            debug!("No usages found for variable '{}'", var_name);
            return None;
        }

        // The declaration's line, including its newline when the declaration is alone on it.
        let line_start = source_text[..decl_start].rfind('\n').map_or(0, |p| p + 1);
        let line_end = source_text[decl_end..]
            .find('\n')
            .map_or(decl_end, |p| decl_end + p + 1);
        let alone_on_line = source_text[line_start..decl_start].trim().is_empty()
            && source_text[decl_end..line_end].trim().is_empty();
        // Another statement on the same line stays: only the declaration goes, with the
        // spaces that separated it from that statement.
        let removal_end = if alone_on_line {
            line_end
        } else {
            decl_end
                + source_text[decl_end..]
                    .bytes()
                    .take_while(|byte| *byte == b' ' || *byte == b'\t')
                    .count()
        };
        let (removal_start, removal_end) = if alone_on_line {
            (line_start, removal_end)
        } else {
            (decl_start, removal_end)
        };

        // Build edits: replace all usages with initializer, then delete declaration
        let mut edits: Vec<LspTextEdit> = Vec::new();

        // Add usage replacements (in reverse order to preserve offsets)
        let mut sorted_usages = usages;
        sorted_usages.sort_by_key(|&(start, _)| std::cmp::Reverse(start)); // Sort by start offset descending

        for (start, end) in sorted_usages {
            // Skip if this usage is the declaration itself
            if start >= decl_start && end <= decl_end {
                continue;
            }

            let start_pos = offset_to_position(source_text, start);
            let end_pos = offset_to_position(source_text, end);

            // Wrap initializer in parentheses if it's a complex expression
            let replacement = if self.needs_parentheses(&initializer) {
                format!("({initializer})")
            } else {
                initializer.clone()
            };

            edits.push(LspTextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: replacement,
            });
        }

        // Add declaration removal (must be applied last due to offset changes)
        let decl_start_pos = offset_to_position(source_text, removal_start);
        let decl_end_pos = offset_to_position(source_text, removal_end);

        edits.push(LspTextEdit {
            range: Range {
                start: decl_start_pos,
                end: decl_end_pos,
            },
            new_text: String::new(),
        });

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), edits);

        Some(CodeAction {
            title: format!("Inline variable '{var_name}'"),
            kind: Some(CodeActionKind::REFACTOR_INLINE),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
        })
    }

    /// Extract the initializer expression from a local variable declaration
    fn extract_variable_initializer(
        &self,
        decl_node: &hyperion_parser::syntax::SyntaxNode,
        var_name: &str,
    ) -> Option<String> {
        use hyperion_parser::syntax::SyntaxKind;

        // LOCAL_VAR_DECL structure: [modifiers] Type IDENT [= expression] ;
        // We need to find the IDENT matching var_name and get the expression after EQ
        let mut found_name = false;
        let mut found_eq = false;

        for child in decl_node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::IDENT => {
                    if let Some(token) = child.as_token() {
                        if token.text() == var_name {
                            found_name = true;
                        }
                    }
                }
                SyntaxKind::EQ => {
                    if found_name {
                        found_eq = true;
                    }
                }
                _ => {
                    // After finding name and '=', the next node is the initializer expression
                    if found_name && found_eq {
                        if let Some(node) = child.as_node() {
                            return Some(node.text().to_string().trim().to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Find the containing block (method body, loop body, etc.)
    fn find_containing_block(
        &self,
        node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<hyperion_parser::syntax::SyntaxNode> {
        use hyperion_parser::syntax::SyntaxKind;

        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                SyntaxKind::BLOCK_STMT | SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                    return Some(parent);
                }
                _ => current = parent.parent(),
            }
        }
        None
    }

    /// Find the local usages of a variable in a block, leaving out the declaration itself
    /// and member accesses such as `other.x`. Returns `None` when the variable is written
    /// after its declaration (assignment, compound assignment, or `++`/`--`).
    fn find_variable_usages(
        &self,
        block: &hyperion_parser::syntax::SyntaxNode,
        var_name: &str,
        source_text: &str,
        declaration: (usize, usize),
    ) -> Option<Vec<(usize, usize)>> {
        let mut usages = Vec::new();

        for token in block.descendants_with_tokens() {
            if let Some(token) = token.as_token() {
                if token.kind().is_identifier() && token.text() == var_name {
                    let start: usize = token.text_range().start().into();
                    let end: usize = token.text_range().end().into();
                    // A local's scope begins at its declaration. Before it, and within it, the
                    // name refers to something else (a field, or the declaration itself).
                    if start < declaration.1 {
                        continue;
                    }
                    if source_text[..start].trim_end().ends_with('.') {
                        continue;
                    }
                    if declared_in_nested_scope(token, block, var_name) {
                        continue;
                    }
                    if is_write_at(source_text, start, end) {
                        return None;
                    }
                    usages.push((start, end));
                }
            }
        }

        Some(usages)
    }

    /// Whether an initializer must be parenthesized where it is inlined. A primary
    /// expression (a name, a literal, a call, or a `new`) is safe bare. Anything with an
    /// operator, a cast, or a leading parenthesis at the top level is not.
    fn needs_parentheses(&self, expr: &str) -> bool {
        let trimmed = expr.trim();
        if trimmed.starts_with('(') {
            return true;
        }
        has_top_level_operator(trimmed)
    }

    // ===== Inline Method =====

    /// Generate an "Inline Method" refactoring action
    ///
    /// When cursor is on a method declaration or call, this action replaces
    /// all call sites with the method body (substituting parameters).
    pub fn inline_method_action(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
    ) -> Option<CodeAction> {
        use crate::position_utils::position_to_offset;
        use hyperion_parser::syntax::SyntaxKind;

        debug!("Checking inline method action at {:?}", cursor);

        // Parse the file. try_lock_for: degrade to no action rather than
        // freeze if did_change is currently holding the DB mutex.
        let root = {
            let db = self.txn;
            let parse_tree = db.parse_file(file_id);
            parse_tree.syntax()
        };

        let offset = position_to_offset(source_text, cursor).ok()? as u32;

        // Find the token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::new(offset))
            .right_biased()?;

        // Only handle identifiers
        if !token.kind().is_identifier() {
            return None;
        }

        let method_name = token.text().to_string();

        // Find if we're on a method declaration
        let mut current = token.parent();
        let mut method_decl = None;

        while let Some(node) = current {
            if node.kind() == SyntaxKind::METHOD_DECL {
                // Verify this is the method with our name
                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::IDENT {
                        if let Some(t) = child.as_token() {
                            if t.text() == method_name {
                                method_decl = Some(node);
                                break;
                            }
                        }
                    }
                }
                break;
            }
            current = node.parent();
        }

        let decl = method_decl?;

        // Only this file is searched for call sites. A method that is not private may be
        // called from another file, so inlining it could leave those calls undone.
        if !hyperion_parser::modifiers::declaration_modifiers(&decl).is_private() {
            debug!(
                "Method '{}' is not private, so its callers may be in other files",
                method_name
            );
            return None;
        }

        // Another method with the same name (an overload, or one in another class of this
        // file) would have its calls rewritten with this method's body.
        let same_named = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .filter(|node| {
                node.children_with_tokens().any(|child| {
                    child.kind() == SyntaxKind::IDENT
                        && child.as_token().is_some_and(|t| t.text() == method_name)
                })
            })
            .count();
        if same_named > 1 {
            debug!(
                "Method '{}' has overloads in this file, skipping inline",
                method_name
            );
            return None;
        }

        // Extract method body and parameters
        let (params, body) = self.extract_method_signature_and_body(&decl)?;

        // Only inline simple methods (no return statements in complex positions)
        if body.matches("return").count() > 1 {
            debug!("Method has multiple return statements, skipping inline");
            return None;
        }

        // Find all call sites in the file
        let call_sites = self.find_method_call_sites(&root, &method_name);

        if call_sites.is_empty() {
            debug!("No call sites found for method '{}'", method_name);
            return None;
        }

        // Build edits
        let mut edits: Vec<LspTextEdit> = Vec::new();

        // Replace call sites with inlined body (in reverse order)
        let mut sorted_calls = call_sites;
        sorted_calls.sort_by_key(|call| std::cmp::Reverse(call.0));

        for (start, end, args) in sorted_calls {
            let substituted = self.substitute_parameters(&body, &params, &args);
            // The body replaces a call inside a larger expression, so an operator in it
            // must keep its grouping: `2 * add(1, 2)` is `2 * (1 + 2)`.
            let inlined = if self.needs_parentheses(&substituted) {
                format!("({substituted})")
            } else {
                substituted
            };

            let start_pos = offset_to_position(source_text, start);
            let end_pos = offset_to_position(source_text, end);

            edits.push(LspTextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: inlined,
            });
        }

        // Optionally remove the method declaration (if it's private and all usages are inlined)
        // For now, we leave the declaration - user can delete manually

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), edits);

        Some(CodeAction {
            title: format!("Inline method '{method_name}'"),
            kind: Some(CodeActionKind::REFACTOR_INLINE),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
        })
    }

    /// Extract method parameters and body from a method declaration
    fn extract_method_signature_and_body(
        &self,
        method_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<(Vec<String>, String)> {
        use hyperion_parser::syntax::SyntaxKind;

        let mut params = Vec::new();
        let mut body = None;

        for child in method_node.children() {
            match child.kind() {
                SyntaxKind::PARAM_LIST => {
                    // Extract parameter names
                    for param in child.descendants() {
                        if param.kind() == SyntaxKind::PARAM {
                            // Find the parameter name (last IDENT in param)
                            let mut param_name = None;
                            for token in param.children_with_tokens() {
                                if token.kind() == SyntaxKind::IDENT {
                                    if let Some(t) = token.as_token() {
                                        param_name = Some(t.text().to_string());
                                    }
                                }
                            }
                            if let Some(name) = param_name {
                                params.push(name);
                            }
                        }
                    }
                }
                SyntaxKind::BLOCK_STMT => {
                    // Extract body (without outer braces)
                    let block_text = child.text().to_string();
                    let trimmed = block_text.trim();
                    if trimmed.starts_with('{') && trimmed.ends_with('}') {
                        let inner = trimmed[1..trimmed.len() - 1].trim();
                        // Only a body that is exactly `return <expression>;` is an expression
                        // that can replace a call. Any other statement, or a void method,
                        // would be spliced in as an expression and break the code, so no body
                        // is returned and the action is withheld.
                        if let Some(expression) = inner
                            .strip_prefix("return ")
                            .and_then(|rest| rest.strip_suffix(';'))
                            .filter(|expression| !expression.contains(';'))
                        {
                            body = Some(expression.trim().to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        Some((params, body?))
    }

    /// Find all call sites of a method in the syntax tree
    fn find_method_call_sites(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        method_name: &str,
    ) -> Vec<(usize, usize, Vec<String>)> {
        use hyperion_parser::syntax::SyntaxKind;

        let mut call_sites = Vec::new();

        for node in root.descendants() {
            if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                // Check if this calls our method. An unqualified call names the method in
                // its first child, a NAME_EXPR; a qualified call (`obj.m()`) runs on another
                // receiver, which an inlined body would not see, so it is not a site.
                let found_method = node.children_with_tokens().next().is_some_and(|first| {
                    first.kind() == SyntaxKind::NAME_EXPR
                        && first
                            .as_node()
                            .is_some_and(|name| name.text() == method_name)
                });
                let mut args = Vec::new();

                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::ARG_LIST {
                        if let Some(arg_node) = child.as_node() {
                            // Extract arguments
                            for arg_child in arg_node.children() {
                                let arg_text = arg_child.text().to_string().trim().to_string();
                                if !arg_text.is_empty() && arg_text != "," {
                                    args.push(arg_text);
                                }
                            }
                        }
                    }
                }

                if found_method {
                    let start: usize = node.text_range().start().into();
                    let end: usize = node.text_range().end().into();
                    call_sites.push((start, end, args));
                }
            }
        }

        call_sites
    }

    /// Substitute parameter names with argument values in method body. An argument with an
    /// operator is parenthesized, so `twice(a + b)` with the body `x * 2` is `(a + b) * 2`.
    /// A body that is only the parameter is left to the caller, which parenthesizes the
    /// whole result.
    fn substitute_parameters(&self, body: &str, params: &[String], args: &[String]) -> String {
        if params.iter().any(|param| param == body.trim()) {
            return substitute_identifiers(body, params, args);
        }
        let grouped: Vec<String> = args
            .iter()
            .map(|arg| {
                if self.needs_parentheses(arg) {
                    format!("({arg})")
                } else {
                    arg.clone()
                }
            })
            .collect();
        substitute_identifiers(body, params, &grouped)
    }

    // ===== Convert to For-Each =====

    /// Generate a "Convert to for-each" refactoring action
    ///
    /// When cursor is on an indexed for loop, converts it to enhanced for loop.
    pub fn convert_to_foreach_action(
        &self,
        range: &Range,
        uri: &Url,
        source_text: &str,
    ) -> Option<CodeAction> {
        use crate::position_utils::position_to_offset;

        let start_offset = position_to_offset(source_text, range.start).ok()?;

        // Find enclosing for loop
        let before_cursor = &source_text[..start_offset];
        let for_pos = before_cursor.rfind("for")?;

        // Check if it's an indexed for loop pattern: for (int i = 0; i < arr.length; i++)
        let after_for = &source_text[for_pos..];
        let open_paren = after_for.find('(')?;
        let close_paren = after_for.find(')')?;

        if close_paren <= open_paren {
            return None;
        }

        let for_header = &after_for[open_paren + 1..close_paren];

        // Parse traditional for loop pattern: "int i = 0; i < collection.size(); i++"
        let parts: Vec<&str> = for_header.split(';').collect();
        if parts.len() != 3 {
            return None;
        }

        // Extract index variable
        let init_part = parts[0].trim();
        let (index_type, _index_var, _) = self.parse_for_init(init_part)?;

        // Extract collection from condition
        let condition = parts[1].trim();
        let collection = self.extract_collection_from_condition(condition)?;

        // Find the for loop body
        let body_start = for_pos + close_paren + 1;
        let body_brace = source_text[body_start..].find('{')?;
        let body_open = body_start + body_brace;

        // Find matching close brace
        let mut brace_count = 1;
        let mut body_close = body_open + 1;
        for (i, c) in source_text[body_open + 1..].chars().enumerate() {
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    body_close = body_open + 1 + i;
                    break;
                }
            }
        }

        let body = &source_text[body_open + 1..body_close];

        // Generate element variable name
        let element_var = self.singular_form(&collection);

        // Generate new for-each loop
        let element_type = if index_type == "int" { "var" } else { "Object" };
        let new_for = format!("for ({element_type} {element_var} : {collection}) {{{body}}}");

        let for_end = for_pos + close_paren + 1 + body_brace + (body_close - body_open);

        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![LspTextEdit {
                range: Range::new(
                    offset_to_position(source_text, for_pos),
                    offset_to_position(source_text, for_end + 1),
                ),
                new_text: new_for,
            }],
        );

        Some(CodeAction {
            title: "Convert to for-each loop".to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Parse for loop initialization like "int i = 0"
    fn parse_for_init(&self, init: &str) -> Option<(String, String, String)> {
        let parts: Vec<&str> = init.split_whitespace().collect();
        if parts.len() >= 4 && parts[2] == "=" {
            Some((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[3].to_string(),
            ))
        } else if parts.len() >= 3 {
            // Handle "int i=0" without spaces
            let eq_pos = init.find('=')?;
            let before_eq = init[..eq_pos].trim();
            let type_var: Vec<&str> = before_eq.split_whitespace().collect();
            if type_var.len() >= 2 {
                Some((
                    type_var[0].to_string(),
                    type_var[1].to_string(),
                    init[eq_pos + 1..].trim().to_string(),
                ))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Extract collection from condition like "i < list.size()"
    fn extract_collection_from_condition(&self, condition: &str) -> Option<String> {
        // Pattern: i < collection.size() or i < collection.length
        if let Some(lt_pos) = condition.find('<') {
            let rhs = condition[lt_pos + 1..].trim();
            if let Some(dot_pos) = rhs.rfind('.') {
                let collection = &rhs[..dot_pos];
                return Some(collection.trim().to_string());
            }
        }
        None
    }

    /// Convert plural to singular (simple heuristic)
    fn singular_form(&self, name: &str) -> String {
        if let Some(stem) = name.strip_suffix("ies") {
            format!("{stem}y")
        } else if let Some(stem) = name.strip_suffix("es") {
            stem.to_string()
        } else if let Some(stem) = name.strip_suffix('s') {
            if name.ends_with("ss") {
                format!("{name}Item")
            } else {
                stem.to_string()
            }
        } else {
            format!("{name}Item")
        }
    }

    // ===== Safe Delete =====

    /// Cheap, file-local validation for advertising the deferred safe-delete
    /// command. The authoritative hierarchy and usage proof intentionally runs
    /// only when the user executes that command.
    pub(crate) fn safe_delete_candidate_name(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
    ) -> Option<String> {
        use crate::position_utils::position_to_offset;
        use hyperion_parser::syntax::SyntaxKind;

        let request_path = uri.to_file_path().ok()?;
        if !request_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
        {
            return None;
        }
        let file_revision = self.workspace_index.current_file_revision(file_id)?;
        let indexed = self
            .workspace_index
            .get_indexed_revision_content(file_revision)?;
        if indexed.file_path != request_path || indexed.content.as_str() != source_text {
            return None;
        }
        let root = {
            let db = self.txn;
            if db.file_text(file_id).as_str() != source_text {
                return None;
            }
            let parse = db.parse_file(file_id);
            parse.errors.is_empty().then(|| parse.syntax())?
        };
        let offset = position_to_offset(source_text, cursor).ok()? as u32;
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;
        if !token.kind().is_identifier() {
            return None;
        }
        let mut node = token.parent()?;
        let namespace = loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL => break DeclarationNamespace::Callable,
                SyntaxKind::FIELD_DECL => break DeclarationNamespace::Value,
                SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL => break DeclarationNamespace::Type,
                SyntaxKind::ANNOTATION_DECL
                | SyntaxKind::CONSTRUCTOR_DECL
                | SyntaxKind::SOURCE_FILE
                | SyntaxKind::ERROR => return None,
                _ => node = node.parent()?,
            }
        };
        let token_range = token.text_range();
        let definition = self
            .workspace_index
            .find_symbol_definition_at_revision_range(
                file_revision,
                self.workspace_index.current_workspace_revision(),
                hyperion_workspace::TextRange::new(
                    u32::from(token_range.start()),
                    u32::from(token_range.end()),
                ),
                namespace,
            )?;
        (definition.symbol.simple_name.as_ref() == token.text())
            .then(|| definition.symbol.simple_name.to_string())
    }

    /// Generate a "Safe Delete" refactoring action
    ///
    /// Checks if a symbol (class, method, field) has any usages in the workspace.
    /// If no usages are found, offers to delete the symbol declaration.
    /// If usages exist, shows a disabled action with usage count.
    /// Revision-exact, cancellable implementation of Java safe delete.
    ///
    /// An enabled result is a proof over one source publication and workspace
    /// graph: the cursor binds an exact declaration, every disappearing target
    /// is scanned, and the candidate edit reparses before it is returned.
    pub fn safe_delete_action_cancellable<F>(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
        is_cancelled: F,
    ) -> Option<CodeAction>
    where
        F: Fn() -> bool,
    {
        self.safe_delete_evaluation_cancellable(file_id, uri, source_text, cursor, is_cancelled)
            .map(|evaluation| evaluation.action)
    }

    pub(crate) fn safe_delete_proof_cancellable<F>(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
        is_cancelled: F,
    ) -> Option<SafeDeleteProof>
    where
        F: Fn() -> bool,
    {
        let mut evaluation = self.safe_delete_evaluation_cancellable(
            file_id,
            uri,
            source_text,
            cursor,
            is_cancelled,
        )?;
        let (workspace_revision, scanned_revisions, resource_proof) = evaluation.proof?;
        Some(SafeDeleteProof {
            edit: evaluation.action.edit.take()?,
            workspace_revision,
            scanned_revisions,
            resource_proof,
        })
    }

    fn safe_delete_evaluation_cancellable<F>(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
        is_cancelled: F,
    ) -> Option<SafeDeleteEvaluation>
    where
        F: Fn() -> bool,
    {
        use crate::position_utils::position_to_offset;
        use hyperion_parser::syntax::SyntaxKind;

        debug!("Checking safe delete action at {:?}", cursor);
        if is_cancelled() {
            return None;
        }

        let request_path = uri.to_file_path().ok()?;
        if !request_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
        {
            return None;
        }

        // Bind request bytes to the exact indexed source before consulting any
        // syntax range. A stale editor/DB/index combination must not produce a
        // destructive edit.
        let file_revision = self.workspace_index.current_file_revision(file_id)?;
        let indexed = self
            .workspace_index
            .get_indexed_revision_content(file_revision)?;
        if indexed.file_path != request_path || indexed.content.as_str() != source_text {
            return None;
        }

        // The cached parse must describe the same bytes and already be valid.
        // Safe delete deliberately does not operate through parser recovery.
        let root = {
            let db = self.txn;
            if db.file_text(file_id).as_str() != source_text {
                return None;
            }
            let parse_tree = db.parse_file(file_id);
            if !parse_tree.errors.is_empty() {
                return None;
            }
            parse_tree.syntax()
        };

        let offset = position_to_offset(source_text, cursor).ok()? as u32;

        // Find token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;

        // Only handle identifiers
        if !token.kind().is_identifier() {
            return None;
        }

        // Walk to the containing supported declaration. Exact range binding
        // below distinguishes its name from a parameter, type, initializer, or
        // header identifier inside that same node.
        let mut node = token.parent()?;
        let (decl_kind, decl_node) = loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL => break (SafeDeleteDeclarationKind::Method, node),
                SyntaxKind::FIELD_DECL => break (SafeDeleteDeclarationKind::Field, node),
                SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL => {
                    break (SafeDeleteDeclarationKind::Type, node);
                }
                // Annotation usages are not yet modeled as exact current type
                // usages, so annotation declarations remain unsupported.
                SyntaxKind::ANNOTATION_DECL | SyntaxKind::CONSTRUCTOR_DECL => return None,
                SyntaxKind::SOURCE_FILE | SyntaxKind::ERROR => return None,
                _ => {
                    node = node.parent()?;
                }
            }
        };

        let namespace = match decl_kind {
            SafeDeleteDeclarationKind::Type => DeclarationNamespace::Type,
            SafeDeleteDeclarationKind::Method => DeclarationNamespace::Callable,
            SafeDeleteDeclarationKind::Field => DeclarationNamespace::Value,
        };
        if decl_kind == SafeDeleteDeclarationKind::Method
            && decl_node
                .ancestors()
                .skip(1)
                .any(|ancestor| ancestor.kind() == SyntaxKind::ANNOTATION_DECL)
        {
            return None;
        }
        let token_range = token.text_range();
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let definition = self
            .workspace_index
            .find_symbol_definition_at_revision_range(
                file_revision,
                workspace_revision,
                hyperion_workspace::TextRange::new(
                    u32::from(token_range.start()),
                    u32::from(token_range.end()),
                ),
                namespace,
            )?;
        let symbol_name = definition.symbol.simple_name.to_string();
        if definition.symbol.simple_name.as_ref() != token.text() {
            return None;
        }

        let declaration_core = hyperion_workspace::TextRange::new(
            u32::from(decl_node.text_range().start()),
            u32::from(decl_node.text_range().end()),
        );
        let mut structural_blockers = Vec::new();
        let mut has_unsafe_initializer = false;
        let annotated_owner = decl_node.ancestors().skip(1).find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
            )
        });
        if Self::java_declaration_has_annotations(&decl_node)
            || annotated_owner
                .as_ref()
                .is_some_and(Self::java_declaration_has_annotations)
        {
            structural_blockers.push(
                "declaration or owner annotations may create name-free runtime dependencies"
                    .to_string(),
            );
        }
        if decl_kind == SafeDeleteDeclarationKind::Method {
            let has_body = decl_node
                .descendants()
                .any(|node| node.kind() == SyntaxKind::BLOCK_STMT);
            let is_native = Self::java_declaration_has_modifier(&decl_node, SyntaxKind::NATIVE_KW);
            if definition.symbol.modifiers.is_abstract || !has_body || is_native {
                structural_blockers.push(
                    "abstract, native, or bodyless callables can have name-free runtime dependencies"
                        .to_string(),
                );
            }
            if is_jvm_entry_point(&definition.symbol) {
                structural_blockers.push(
                    "the JVM entry point is invoked without a source-level usage".to_string(),
                );
            }
            if is_java_serialization_hook(&self.workspace_index, &definition.symbol) {
                structural_blockers
                    .push("Java serialization invokes this hook by convention".to_string());
            }
        }
        if decl_kind == SafeDeleteDeclarationKind::Field {
            let declarator_names: Vec<_> = decl_node
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|candidate| candidate.kind().is_identifier())
                .collect();
            if declarator_names.len() != 1 || declarator_names[0].text_range() != token.text_range()
            {
                return None;
            }
            if decl_node
                .children_with_tokens()
                .any(|item| item.kind() == SyntaxKind::EQ)
            {
                has_unsafe_initializer = true;
                structural_blockers
                    .push("the field initializer may have observable side effects".to_string());
            }
            if is_java_serialization_field(&self.workspace_index, &definition.symbol) {
                structural_blockers
                    .push("Java serialization reads this field by convention".to_string());
            }
        }

        // A type deletion removes the complete declaration subtree, not only
        // its nominal target. A method deletion scans the exact dispatch family
        // and separately records structural override/implementation obligations.
        let (targets, related_declarations) = match decl_kind {
            SafeDeleteDeclarationKind::Method => {
                let family = self
                    .workspace_index
                    .find_current_callable_override_family_for_refactoring_cancellable(
                        &definition.target,
                        definition.workspace_revision,
                        &is_cancelled,
                    )
                    .ok()?;
                if !family.iter().any(|candidate| {
                    candidate.target == definition.target
                        && candidate.location.file_id == definition.location.file_id
                        && candidate.location.range == definition.location.range
                }) {
                    return None;
                }
                let related = family.len().saturating_sub(1);
                (
                    family
                        .into_iter()
                        .map(|candidate| candidate.target)
                        .collect(),
                    related,
                )
            }
            SafeDeleteDeclarationKind::Type => {
                let definitions = self
                    .workspace_index
                    .find_symbol_definitions_at_revision_within_range(
                        file_revision,
                        definition.workspace_revision,
                        declaration_core,
                    )
                    .ok()?;
                if !definitions
                    .iter()
                    .any(|candidate| candidate.target == definition.target)
                {
                    return None;
                }
                (
                    definitions
                        .into_iter()
                        .map(|candidate| candidate.target)
                        .collect(),
                    0,
                )
            }
            SafeDeleteDeclarationKind::Field => (vec![definition.target.clone()], 0),
        };
        if is_cancelled() {
            return None;
        }

        let structural_type_dependent_count = if decl_kind == SafeDeleteDeclarationKind::Type {
            self.workspace_index
                .count_current_structural_type_dependents(&targets, definition.workspace_revision)
                .ok()?
        } else {
            0
        };
        let import_dependency_count = self
            .workspace_index
            .count_current_import_dependencies_for_symbol_targets(
                &targets,
                definition.workspace_revision,
            )
            .ok()?;
        if related_declarations > 0 {
            structural_blockers.push(format!(
                "the method participates in a dispatch family with {related_declarations} other declaration{}",
                if related_declarations == 1 { "" } else { "s" }
            ));
        }
        if structural_type_dependent_count > 0 {
            structural_blockers.push(format!(
                "{structural_type_dependent_count} subtype or implementation declaration{} depend{} on it",
                if structural_type_dependent_count == 1 {
                    ""
                } else {
                    "s"
                },
                if structural_type_dependent_count == 1 {
                    "s"
                } else {
                    ""
                }
            ));
        }
        if import_dependency_count > 0 {
            structural_blockers.push(format!(
                "{import_dependency_count} import declaration{} depend{} on it",
                if import_dependency_count == 1 {
                    ""
                } else {
                    "s"
                },
                if import_dependency_count == 1 {
                    "s"
                } else {
                    ""
                }
            ));
        }
        if !structural_blockers.is_empty() {
            let title_suffix = if related_declarations > 0 {
                "dispatch family"
            } else if structural_type_dependent_count > 0 {
                "type dependency"
            } else if import_dependency_count > 0 {
                "import dependency"
            } else if has_unsafe_initializer {
                "unsafe initializer"
            } else {
                "structural dependency"
            };
            return Some(SafeDeleteEvaluation {
                action: Self::disabled_safe_delete_action(
                    &symbol_name,
                    title_suffix,
                    &structural_blockers,
                ),
                proof: None,
            });
        }
        let deletion = Self::java_declaration_delete_range(&decl_node, source_text)?;
        let deletion_range = hyperion_workspace::TextRange::new(
            u32::try_from(deletion.start).ok()?,
            u32::try_from(deletion.end).ok()?,
        );
        let usages = self
            .workspace_index
            .find_current_usages_for_symbol_targets_for_refactoring(
                &targets,
                definition.workspace_revision,
                &[(file_revision, deletion_range)],
                &is_cancelled,
            )
            .ok()?;
        let usage_count = usages
            .files
            .iter()
            .map(|file| {
                file.locations
                    .iter()
                    .filter(|location| {
                        file.revision != file_revision
                            || location.range.start < declaration_core.start
                            || declaration_core.end < location.range.end
                    })
                    .count()
            })
            .sum::<usize>();
        if is_cancelled()
            || !self
                .workspace_index
                .current_usage_locations_are_current(&usages)
        {
            return None;
        }

        debug!("Safe delete: {} has {} usages", symbol_name, usage_count);

        let mut edited = String::with_capacity(source_text.len() - deletion.len());
        edited.push_str(source_text.get(..deletion.start)?);
        edited.push_str(source_text.get(deletion.end..)?);
        let (_, edited_errors) =
            hyperion_parser::Parser::parse(hyperion_parser::Lexer::new(&edited).collect());
        if !edited_errors.is_empty() {
            return None;
        }

        let delete_range = Range::new(
            offset_to_position(source_text, deletion.start),
            offset_to_position(source_text, deletion.end),
        );
        if is_cancelled()
            || !self
                .workspace_index
                .current_usage_locations_are_current(&usages)
        {
            return None;
        }

        if usage_count > 0 {
            structural_blockers.push(format!(
                "{usage_count} workspace usage{} remain{} outside the declaration",
                if usage_count == 1 { "" } else { "s" },
                if usage_count == 1 { "s" } else { "" }
            ));
        }

        if structural_blockers.is_empty() {
            // Safe to delete - create the action
            let mut changes = HashMap::new();
            changes.insert(
                uri.clone(),
                vec![LspTextEdit {
                    range: delete_range,
                    new_text: String::new(),
                }],
            );

            Some(SafeDeleteEvaluation {
                action: CodeAction {
                    title: format!("Safe Delete '{symbol_name}' (no usages)"),
                    kind: Some(CodeActionKind::REFACTOR),
                    data: Some(serde_json::json!({ "hyperionSafeDelete": true })),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                proof: Some((
                    usages.workspace_revision,
                    usages.scanned_revisions().to_vec(),
                    usages.resource_proof().cloned(),
                )),
            })
        } else {
            let title_suffix = format!(
                "{usage_count} usage{}",
                if usage_count == 1 { "" } else { "s" }
            );
            Some(SafeDeleteEvaluation {
                action: Self::disabled_safe_delete_action(
                    &symbol_name,
                    &title_suffix,
                    &structural_blockers,
                ),
                proof: None,
            })
        }
    }

    fn disabled_safe_delete_action(
        symbol_name: &str,
        title_suffix: &str,
        blockers: &[String],
    ) -> CodeAction {
        CodeAction {
            title: format!("Safe Delete '{symbol_name}' ({title_suffix})"),
            kind: Some(CodeActionKind::REFACTOR),
            disabled: Some(tower_lsp::lsp_types::CodeActionDisabled {
                reason: format!("Cannot safely delete: {}", blockers.join("; ")),
            }),
            ..Default::default()
        }
    }

    fn java_declaration_has_annotations(declaration: &hyperion_parser::syntax::SyntaxNode) -> bool {
        use hyperion_parser::syntax::SyntaxKind;

        let Some(parent) = declaration.parent() else {
            return false;
        };
        let mut preceding = None;
        for sibling in parent.children() {
            if sibling == *declaration {
                break;
            }
            preceding = Some(sibling);
        }
        preceding
            .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
            .is_some_and(|modifiers| {
                modifiers
                    .descendants()
                    .any(|node| node.kind() == SyntaxKind::ANNOTATION)
            })
    }

    fn java_declaration_has_modifier(
        declaration: &hyperion_parser::syntax::SyntaxNode,
        modifier: hyperion_parser::syntax::SyntaxKind,
    ) -> bool {
        use hyperion_parser::syntax::SyntaxKind;

        let Some(parent) = declaration.parent() else {
            return false;
        };
        let mut preceding = None;
        for sibling in parent.children() {
            if sibling == *declaration {
                break;
            }
            preceding = Some(sibling);
        }
        preceding
            .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
            .is_some_and(|modifiers| {
                modifiers
                    .descendants_with_tokens()
                    .any(|item| item.kind() == modifier)
            })
    }

    /// Compute the source envelope owned by one Java declaration without
    /// crossing an adjacent declaration or comment.
    fn java_declaration_delete_range(
        declaration: &hyperion_parser::syntax::SyntaxNode,
        source: &str,
    ) -> Option<std::ops::Range<usize>> {
        use hyperion_parser::syntax::SyntaxKind;

        let core = declaration.text_range();
        let mut start = usize::from(core.start());
        let mut end = usize::from(core.end());
        if end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return None;
        }

        // Modifier lists are siblings in Hyperion's Java CST. Only the last
        // node before this declaration can be its attached modifier list.
        let parent = declaration.parent()?;
        let mut preceding_node = None;
        for sibling in parent.children() {
            if sibling == *declaration {
                break;
            }
            preceding_node = Some(sibling);
        }
        if let Some(modifiers) =
            preceding_node.filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
        {
            start = usize::from(modifiers.text_range().start());
        }

        // Own only an immediately attached Javadoc. Ordinary comments are
        // deliberately preserved because their ownership is ambiguous.
        let parent_start = usize::from(parent.text_range().start());
        let mut possible_doc_end = start;
        while possible_doc_end > parent_start
            && source.as_bytes()[possible_doc_end - 1].is_ascii_whitespace()
        {
            possible_doc_end -= 1;
        }
        let gap = &source[possible_doc_end..start];
        if gap.bytes().filter(|byte| *byte == b'\n').count() <= 1 {
            let prefix = &source[parent_start..possible_doc_end];
            if prefix.ends_with("*/") {
                if let Some(relative_start) = prefix
                    .rfind("/*")
                    .filter(|relative_start| prefix[*relative_start..].starts_with("/**"))
                {
                    let doc_start = parent_start + relative_start;
                    let doc_line_start = source[..doc_start]
                        .rfind('\n')
                        .map_or(0, |newline| newline + 1);
                    if source[doc_line_start..doc_start]
                        .bytes()
                        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
                    {
                        start = doc_start;
                    }
                }
            }
        }

        let line_start = source[..start].rfind('\n').map_or(0, |newline| newline + 1);
        if source[line_start..start]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            start = line_start;
        }

        let newline = source[end..].find('\n').map(|relative| end + relative);
        let line_end = newline.unwrap_or(source.len());
        if source[end..line_end]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            end = newline.map_or(line_end, |newline| newline + 1);
        }
        Some(start..end)
    }

    /// Get method name from METHOD_DECL node
    fn get_method_name(&self, node: &hyperion_parser::syntax::SyntaxNode) -> Option<String> {
        use hyperion_parser::syntax::SyntaxKind;

        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    /// Get class name from CLASS_DECL node
    fn get_class_name(&self, node: &hyperion_parser::syntax::SyntaxNode) -> Option<String> {
        use hyperion_parser::syntax::SyntaxKind;

        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    // ===== Change Method Signature =====

    /// Generate a "Change Method Signature" info action
    ///
    /// When cursor is on a method name, shows the current signature and call site count.
    /// This provides foundational information for signature refactoring.
    pub fn change_signature_info_action(
        &self,
        file_id: FileId,
        _uri: &Url,
        source_text: &str,
        cursor: Position,
    ) -> Option<CodeAction> {
        use crate::position_utils::position_to_offset;
        use hyperion_parser::syntax::SyntaxKind;
        use hyperion_refactor::signature::ChangeSignatureRefactoring;

        debug!("Checking change signature info action at {:?}", cursor);

        // Parse the file. try_lock_for: degrade to no action rather than
        // freeze if did_change is currently holding the DB mutex.
        let root = {
            let db = self.txn;
            let parse_tree = db.parse_file(file_id);
            parse_tree.syntax()
        };

        let offset = position_to_offset(source_text, cursor).ok()? as u32;

        // Find token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;

        // Only handle identifiers
        if !token.kind().is_identifier() {
            return None;
        }

        let symbol_name = token.text().to_string();

        // Walk up to find METHOD_DECL
        let mut node = token.parent()?;
        let method_node = loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL => {
                    // Check if cursor is on method name
                    let method_name = self.get_method_name(&node)?;
                    if method_name == symbol_name {
                        break node;
                    }
                    return None;
                }
                SyntaxKind::SOURCE_FILE | SyntaxKind::ERROR => return None,
                _ => {
                    node = node.parent()?;
                }
            }
        };

        // Get package and class context
        let imports = self.workspace_index.get_imports(file_id)?;
        let package = imports.current_package.as_ref();

        // Find containing class
        let mut class_name = None;
        let mut parent = method_node.parent();
        while let Some(p) = parent {
            if p.kind() == SyntaxKind::CLASS_DECL {
                class_name = self.get_class_name(&p);
                break;
            }
            parent = p.parent();
        }

        // Use the signature module to extract method signature
        let signature = ChangeSignatureRefactoring::extract_signature_from_node(
            file_id,
            &method_node,
            class_name.as_deref(),
            package.map(std::string::String::as_str),
        )
        .ok()?;

        // Bind the parsed declaration to the exact indexed bytes and typed
        // callable identity. A descriptorless `Owner.method` string cannot
        // distinguish overloads and must never drive request-time results.
        let file_revision = self.workspace_index.current_file_revision(file_id)?;
        let indexed = self
            .workspace_index
            .get_indexed_revision_content(file_revision)?;
        if indexed.content.as_str() != source_text {
            return None;
        }
        let workspace_revision = self.workspace_index.current_workspace_revision();
        let name_range = signature.name_range?;
        let definition = self
            .workspace_index
            .find_symbol_definition_at_revision_range(
                file_revision,
                workspace_revision,
                hyperion_workspace::TextRange::new(
                    u32::from(name_range.start()),
                    u32::from(name_range.end()),
                ),
                DeclarationNamespace::Callable,
            )?;
        let usages = self
            .workspace_index
            .find_current_usages_for_symbol_targets(
                std::slice::from_ref(&definition.target),
                Some(definition.workspace_revision),
                || false,
            )
            .ok()?;
        let call_site_count = usages.locations().count();
        if !self
            .workspace_index
            .current_usage_locations_are_current(&usages)
        {
            return None;
        }

        // Format signature using the signature module's format method
        let formatted_signature = signature.format();

        // Create informational code action with more details
        let modifiers_str = if signature.modifiers.is_empty() {
            String::new()
        } else {
            format!("{} ", signature.modifiers.join(" "))
        };

        Some(CodeAction {
            title: format!(
                "Change Signature: {}{} - {} call site{}",
                modifiers_str,
                formatted_signature,
                call_site_count,
                if call_site_count == 1 { "" } else { "s" }
            ),
            kind: Some(CodeActionKind::new("refactor.rewrite.signature")),
            disabled: Some(tower_lsp::lsp_types::CodeActionDisabled {
                reason:
                    "Change Signature requires IDE dialog support. Use rename for name changes."
                        .to_string(),
            }),
            ..Default::default()
        })
    }
}

fn indexed_parameter_type(parameter: &str) -> &str {
    parameter.split_whitespace().next().unwrap_or(parameter)
}

fn indexed_type_matches(actual: &str, expected_simple: &str) -> bool {
    actual == expected_simple
        || actual
            .strip_suffix(expected_simple)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn is_jvm_entry_point(symbol: &hyperion_workspace::Symbol) -> bool {
    symbol.simple_name.as_ref() == "main"
        && symbol.modifiers.is_public
        && symbol.modifiers.is_static
        && symbol.return_type.as_deref() == Some("void")
        && symbol.parameters.len() == 1
        && matches!(
            indexed_parameter_type(&symbol.parameters[0]),
            "String[]" | "java.lang.String[]" | "String..." | "java.lang.String..."
        )
}

fn type_has_supertype(workspace_index: &WorkspaceIndex, type_fqn: &str, marker: &str) -> bool {
    let mut pending = VecDeque::from([type_fqn.to_string()]);
    let mut visited = HashSet::new();
    while let Some(candidate) = pending.pop_front() {
        if !visited.insert(candidate.clone()) {
            continue;
        }
        if candidate == marker {
            return true;
        }
        pending.extend(workspace_index.direct_supertypes(&candidate));
    }
    false
}

fn type_is_serializable(workspace_index: &WorkspaceIndex, type_fqn: &str) -> bool {
    type_has_supertype(workspace_index, type_fqn, "java.io.Serializable")
        || type_has_supertype(workspace_index, type_fqn, "java.io.Externalizable")
}

fn type_package(workspace_index: &WorkspaceIndex, type_fqn: &str) -> Option<String> {
    let mut current = type_fqn.to_string();
    loop {
        let symbol = workspace_index.get_symbol_target(&hyperion_workspace::SymbolTarget::new(
            current.as_str(),
            DeclarationNamespace::Type,
        ))?;
        let Some(owner) = symbol.container.as_deref() else {
            return Some(
                current
                    .rsplit_once('.')
                    .map_or_else(String::new, |(package, _)| package.to_string()),
            );
        };
        current = owner.to_string();
    }
}

fn has_serializable_descendant(
    workspace_index: &WorkspaceIndex,
    owner: &str,
    package_private: bool,
) -> bool {
    let owner_package = package_private
        .then(|| type_package(workspace_index, owner))
        .flatten();
    let mut pending = VecDeque::from([owner.to_string()]);
    let mut visited = HashSet::new();
    while let Some(candidate) = pending.pop_front() {
        if !visited.insert(candidate.clone()) {
            continue;
        }
        if candidate != owner
            && (!package_private
                || owner_package.as_ref().is_some_and(|owner_package| {
                    type_package(workspace_index, &candidate).as_ref() == Some(owner_package)
                }))
            && type_is_serializable(workspace_index, &candidate)
        {
            return true;
        }
        pending.extend(
            workspace_index
                .find_direct_subtypes(&candidate)
                .into_iter()
                .map(|symbol| symbol.fqn.to_string()),
        );
    }
    false
}

fn serialization_can_invoke_member(
    workspace_index: &WorkspaceIndex,
    symbol: &hyperion_workspace::Symbol,
    inheritable: bool,
) -> bool {
    let Some(owner) = symbol.container.as_deref() else {
        return false;
    };
    type_is_serializable(workspace_index, owner)
        || (inheritable
            && !symbol.modifiers.is_private
            && has_serializable_descendant(
                workspace_index,
                owner,
                !symbol.modifiers.is_public && !symbol.modifiers.is_protected,
            ))
}

fn is_reference_return_type(return_type: Option<&str>) -> bool {
    return_type.is_some_and(|ty| {
        !matches!(
            ty.trim(),
            "void" | "boolean" | "byte" | "char" | "short" | "int" | "long" | "float" | "double"
        )
    })
}

fn is_java_serialization_hook(
    workspace_index: &WorkspaceIndex,
    symbol: &hyperion_workspace::Symbol,
) -> bool {
    if symbol.modifiers.is_static || symbol.modifiers.is_abstract {
        return false;
    }
    let return_type = symbol.return_type.as_deref();
    let signature_matches = match symbol.simple_name.as_ref() {
        "readObject" => {
            symbol.modifiers.is_private
                && symbol.parameters.len() == 1
                && indexed_type_matches(
                    indexed_parameter_type(&symbol.parameters[0]),
                    "ObjectInputStream",
                )
                && return_type == Some("void")
        }
        "writeObject" => {
            symbol.modifiers.is_private
                && symbol.parameters.len() == 1
                && indexed_type_matches(
                    indexed_parameter_type(&symbol.parameters[0]),
                    "ObjectOutputStream",
                )
                && return_type == Some("void")
        }
        "readObjectNoData" => {
            symbol.modifiers.is_private
                && symbol.parameters.is_empty()
                && return_type == Some("void")
        }
        "readResolve" | "writeReplace" => {
            symbol.parameters.is_empty() && is_reference_return_type(return_type)
        }
        _ => false,
    };
    let owner_uses_default_serialization = symbol
        .container
        .as_deref()
        .is_some_and(|owner| !type_has_supertype(workspace_index, owner, "java.io.Externalizable"));
    signature_matches
        && (matches!(symbol.simple_name.as_ref(), "readResolve" | "writeReplace")
            || owner_uses_default_serialization)
        && serialization_can_invoke_member(
            workspace_index,
            symbol,
            matches!(symbol.simple_name.as_ref(), "readResolve" | "writeReplace"),
        )
}

fn is_java_serialization_field(
    workspace_index: &WorkspaceIndex,
    symbol: &hyperion_workspace::Symbol,
) -> bool {
    if !symbol.modifiers.is_static || !symbol.modifiers.is_final {
        return false;
    }
    let signature_matches = match symbol.simple_name.as_ref() {
        "serialVersionUID" => symbol.return_type.as_deref() == Some("long"),
        "serialPersistentFields" => {
            symbol.modifiers.is_private
                && symbol.return_type.as_deref().is_some_and(|ty| {
                    indexed_type_matches(ty.strip_suffix("[]").unwrap_or(ty), "ObjectStreamField")
                        && ty.ends_with("[]")
                })
        }
        _ => false,
    };
    let owner_uses_default_serialization = symbol
        .container
        .as_deref()
        .is_some_and(|owner| !type_has_supertype(workspace_index, owner, "java.io.Externalizable"));
    signature_matches
        && (symbol.simple_name.as_ref() == "serialVersionUID" || owner_uses_default_serialization)
        && serialization_can_invoke_member(workspace_index, symbol, false)
}

/// Whether a scope between `token` and `block` declares `name` itself. Then the token names
/// that declaration, not the local of `block`. Such a scope is an anonymous or nested class
/// with a field of that name, a method or constructor with a parameter of it, or a lambda
/// with it as a parameter.
fn declared_in_nested_scope(
    token: &hyperion_parser::syntax::SyntaxToken,
    block: &hyperion_parser::syntax::SyntaxNode,
    name: &str,
) -> bool {
    use hyperion_parser::syntax::SyntaxKind;

    let mut current = token.parent();
    while let Some(node) = current {
        if &node == block {
            return false;
        }
        for child in node.children() {
            let declares = match child.kind() {
                SyntaxKind::FIELD_DECL => declares_identifier(&child, name),
                SyntaxKind::PARAM => declares_identifier(&child, name),
                SyntaxKind::PARAM_LIST => child.children().any(|param| {
                    param.kind() == SyntaxKind::PARAM && declares_identifier(&param, name)
                }),
                _ => false,
            };
            if declares {
                return true;
            }
        }
        // A lambda without parentheses names its one parameter directly, before the arrow.
        if node.kind() == SyntaxKind::LAMBDA_EXPR {
            for element in node.children_with_tokens() {
                let Some(token) = element.as_token() else {
                    continue;
                };
                if token.text() == "->" {
                    break;
                }
                if token.kind().is_identifier() && token.text() == name {
                    return true;
                }
            }
        }
        current = node.parent();
    }
    false
}

/// Whether `node` has `name` as one of its own identifier tokens (not in a child node).
fn declares_identifier(node: &hyperion_parser::syntax::SyntaxNode, name: &str) -> bool {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .any(|token| token.kind().is_identifier() && token.text() == name)
}

/// Whether the identifier at `start..end` is the target of an assignment or an increment.
fn is_write_at(source_text: &str, start: usize, end: usize) -> bool {
    const ASSIGNMENTS: [&str; 12] = [
        "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", ">>>=",
    ];
    let after = source_text[end..].trim_start();
    if after.starts_with("++") || after.starts_with("--") {
        return true;
    }
    if after.starts_with('=') && !after.starts_with("==") {
        return true;
    }
    if ASSIGNMENTS
        .iter()
        .any(|op| *op != "=" && after.starts_with(op))
    {
        return true;
    }
    let before = source_text[..start].trim_end();
    before.ends_with("++") || before.ends_with("--")
}

/// Whether an expression has an operator outside any parentheses, brackets, braces or
/// string and character literals. Such an expression changes meaning when substituted
/// into a larger one. Calls, `new`, member access and array indexing are primary.
fn has_top_level_operator(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    let mut depth = 0_i32;
    let mut index = 0;
    let mut words_at_top: Vec<&str> = Vec::new();
    let mut word_start: Option<usize> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        let at_top = depth == 0;
        if byte == b'"' || byte == b'\'' {
            // Skip the literal, honouring backslash escapes.
            index += 1;
            while index < bytes.len() && bytes[index] != byte {
                index += if bytes[index] == b'\\' { 2 } else { 1 };
            }
            index += 1;
            continue;
        }
        let is_word_byte = byte.is_ascii_alphanumeric() || byte == b'_';
        if at_top && !is_word_byte {
            if let Some(start) = word_start.take() {
                words_at_top.push(&expr[start..index]);
            }
        }
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if !at_top => {}
            _ if is_word_byte => {
                word_start.get_or_insert(index);
            }
            b' ' | b'\t' | b'.' => {}
            _ => return true,
        }
        index += 1;
    }
    if let Some(start) = word_start {
        words_at_top.push(&expr[start..]);
    }
    // `a instanceof B` is a binary operator spelled as a word.
    words_at_top.len() > 1 && words_at_top.contains(&"instanceof")
}

/// Replace each parameter name in a body with the argument at its position, in one pass.
/// Replacing the parameters one after another would substitute into an argument that names
/// a later parameter: with add(a, b) called as add(b, 1), the body a + b would become 1 + 1.
/// String and character literals are copied as written, and a name after a dot is a member.
fn substitute_identifiers(body: &str, params: &[String], args: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(first) = rest.chars().next() {
        if first == '"' || first == '\'' {
            // A literal is copied as written, escapes included.
            let mut end = first.len_utf8();
            let mut escaped = false;
            for character in rest[end..].chars() {
                end += character.len_utf8();
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == first {
                    break;
                }
            }
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else if first.is_alphanumeric() || first == '_' || first == '$' {
            // A word is the whole run of name characters, so `2x` is not the name `x`.
            let end = rest
                .char_indices()
                .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '$'))
                .map_or(rest.len(), |(index, _)| index);
            let word = &rest[..end];
            // A name after a dot is a member, not the parameter.
            let member = out.trim_end().ends_with('.');
            match params.iter().position(|param| param == word) {
                Some(index) if !member => {
                    out.push_str(args.get(index).map_or(word, String::as_str));
                }
                _ => out.push_str(word),
            }
            rest = &rest[end..];
        } else {
            out.push(first);
            rest = &rest[first.len_utf8()..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::substitute_identifiers;

    fn replace_word(text: &str, word: &str, replacement: &str) -> String {
        substitute_identifiers(text, &[word.to_string()], &[replacement.to_string()])
    }

    // === Basic replacement ===

    #[test]
    fn simple_replacement() {
        assert_eq!(replace_word("return x;", "x", "value"), "return value;");
    }

    #[test]
    fn multiple_occurrences() {
        assert_eq!(replace_word("x + x * x", "x", "y"), "y + y * y");
    }

    #[test]
    fn no_match() {
        assert_eq!(replace_word("return value;", "x", "y"), "return value;");
    }

    // === Word boundary behavior ===

    #[test]
    fn no_replace_inside_word() {
        assert_eq!(replace_word("index = 0;", "in", "out"), "index = 0;");
    }

    #[test]
    fn no_replace_prefix() {
        assert_eq!(replace_word("xValue = 1;", "x", "y"), "xValue = 1;");
    }

    #[test]
    fn no_replace_suffix() {
        assert_eq!(replace_word("getX()", "x", "y"), "getX()");
    }

    #[test]
    fn underscore_is_word_char() {
        // _x and x_ should NOT be boundaries for "x"
        assert_eq!(replace_word("_x + x_", "x", "y"), "_x + x_");
    }

    #[test]
    fn digit_is_word_char() {
        assert_eq!(replace_word("x2 + 2x", "x", "y"), "x2 + 2x");
    }

    #[test]
    fn boundary_at_operators() {
        assert_eq!(replace_word("x+x-x/x%x", "x", "n"), "n+n-n/n%n");
    }

    #[test]
    fn boundary_at_parens() {
        assert_eq!(replace_word("foo(x)", "x", "val"), "foo(val)");
    }

    #[test]
    fn boundary_at_brackets() {
        assert_eq!(replace_word("arr[i]", "i", "idx"), "arr[idx]");
    }

    #[test]
    fn boundary_at_dot() {
        assert_eq!(replace_word("x.toString()", "x", "obj"), "obj.toString()");
    }

    // === Position edge cases ===

    #[test]
    fn word_at_start() {
        assert_eq!(replace_word("x = 1;", "x", "y"), "y = 1;");
    }

    #[test]
    fn word_at_end() {
        assert_eq!(replace_word("return x", "x", "y"), "return y");
    }

    #[test]
    fn word_is_entire_string() {
        assert_eq!(replace_word("x", "x", "y"), "y");
    }

    #[test]
    fn adjacent_non_boundary_matches() {
        // "aa" contains "a" at positions 0 and 1, neither at word boundary
        // because the adjacent char is also a word char
        assert_eq!(replace_word("aa", "a", "b"), "aa");
    }

    // === Replacement content edge cases ===

    #[test]
    fn replacement_contains_word() {
        // Should not re-match inside the replacement
        assert_eq!(replace_word("x + 1", "x", "x + 1"), "x + 1 + 1");
    }

    #[test]
    fn replacement_is_empty() {
        assert_eq!(replace_word("a + x + b", "x", ""), "a +  + b");
    }

    #[test]
    fn replacement_longer_than_word() {
        assert_eq!(
            replace_word("return x;", "x", "longVariableName"),
            "return longVariableName;"
        );
    }

    #[test]
    fn dollar_in_replacement() {
        // Java identifiers can contain $. The old regex replace_all would
        // interpret $1 as a capture group reference. We insert literally.
        assert_eq!(replace_word("return x;", "x", "R$id"), "return R$id;");
    }

    // === Empty/degenerate inputs ===

    #[test]
    fn empty_word_returns_text_unchanged() {
        assert_eq!(replace_word("hello", "", "x"), "hello");
    }

    #[test]
    fn empty_text() {
        assert_eq!(replace_word("", "x", "y"), "");
    }

    #[test]
    fn both_empty() {
        assert_eq!(replace_word("", "", "y"), "");
    }

    // === Multi-word parameters (realistic refactoring scenarios) ===

    #[test]
    fn java_method_body_substitution() {
        let body = "return name.length() + age;";
        let result = replace_word(body, "name", "person.getName()");
        let result = replace_word(&result, "age", "42");
        assert_eq!(result, "return person.getName().length() + 42;");
    }

    #[test]
    fn parameter_shadowed_in_longer_name() {
        // Replacing "i" should not touch "size" or "item" or "list"
        let body = "for (int i = 0; i < list.size(); i++) { item = list.get(i); }";
        assert_eq!(
            replace_word(body, "i", "idx"),
            "for (int idx = 0; idx < list.size(); idx++) { item = list.get(idx); }"
        );
    }

    // === Non-ASCII ===

    #[test]
    fn non_ascii_context_is_boundary() {
        // Non-ASCII chars aren't word chars, so they act as boundaries
        assert_eq!(replace_word("état=x", "x", "y"), "état=y");
    }

    #[test]
    fn non_ascii_does_not_break_scanning() {
        assert_eq!(
            replace_word("/* café */ x = 1;", "x", "y"),
            "/* café */ y = 1;"
        );
    }
}
