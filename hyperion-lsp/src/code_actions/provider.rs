// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Actions Provider for LSP
//!
//! This module implements code actions (quick fixes and refactorings) for the Hyperion LSP.
//! Code actions provide contextual suggestions for improving or fixing code.

use hyperion_actions::{ImportOrganizer, QuickFixProvider};
use hyperion_semantics::db::{FileId, ParseDatabase, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    Diagnostic, Position, Range, TextEdit as LspTextEdit, Url, WorkspaceEdit,
};
use tracing::debug;

use super::diagnostic_symbol::{unresolved_method, unresolved_symbol};
use super::generation::GenerationActions;
use super::refactoring::RefactoringActions;
use crate::position_utils::offset_to_position;
use crate::semantic_host::ReadTxn;

/// Provides code action functionality
#[derive(Clone)]
pub struct CodeActionProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
    generation: GenerationActions<'txn>,
    refactoring: RefactoringActions<'txn>,
}

impl<'txn> CodeActionProvider<'txn> {
    /// Creates a new CodeActionProvider
    pub fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        let generation = GenerationActions::new(txn, Arc::clone(&workspace_index));
        let refactoring = RefactoringActions::new(txn, Arc::clone(&workspace_index));
        Self {
            txn,
            workspace_index,
            generation,
            refactoring,
        }
    }

    pub(crate) fn safe_delete_proof_cancellable<F>(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
        is_cancelled: F,
    ) -> Option<super::refactoring::SafeDeleteProof>
    where
        F: Fn() -> bool,
    {
        self.refactoring.safe_delete_proof_cancellable(
            file_id,
            uri,
            source_text,
            cursor,
            is_cancelled,
        )
    }

    pub(crate) fn safe_delete_candidate_name(
        &self,
        file_id: FileId,
        uri: &Url,
        source_text: &str,
        cursor: Position,
    ) -> Option<String> {
        self.refactoring
            .safe_delete_candidate_name(file_id, uri, source_text, cursor)
    }

    /// Get code actions for a given range in a document
    ///
    /// Returns a list of available code actions (quick fixes, refactorings, etc.)
    pub fn code_actions(
        &self,
        file_id: FileId,
        params: &CodeActionParams,
        source_text: &str,
    ) -> Result<Option<CodeActionResponse>, String> {
        self.code_actions_cancellable(file_id, params, source_text, true, || false)
    }

    /// Compute code actions while allowing workspace-wide refactorings to
    /// observe initialization state and request cancellation.
    pub fn code_actions_cancellable<F>(
        &self,
        file_id: FileId,
        params: &CodeActionParams,
        source_text: &str,
        workspace_ready: bool,
        is_cancelled: F,
    ) -> Result<Option<CodeActionResponse>, String>
    where
        F: Fn() -> bool,
    {
        debug!(
            "Code actions requested for range {:?} in {:?}",
            params.range, params.text_document.uri
        );

        let mut actions = Vec::new();

        // Add diagnostic-based quick fixes
        if !params.context.diagnostics.is_empty() {
            for diagnostic in &params.context.diagnostics {
                if let Some(action) = self.quick_fix_for_diagnostic(
                    file_id,
                    diagnostic,
                    &params.text_document.uri,
                    source_text,
                ) {
                    actions.push(action);
                }

                // Add import suggestions for unresolved symbols
                actions.extend(self.add_import_suggestions(
                    diagnostic,
                    &params.text_document.uri,
                    source_text,
                ));

                actions.extend(
                    self.add_symbol_replacement_suggestions(diagnostic, &params.text_document.uri),
                );

                // Add create field/method suggestions
                actions.extend(self.create_member_suggestions(
                    diagnostic,
                    &params.text_document.uri,
                    source_text,
                ));
            }
        }

        // Add general refactoring actions if a range is selected
        if params.range.start != params.range.end {
            // Extract variable (delegated to extractors module)
            if let Some(action) = super::extractors::extract_variable_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Extract method (delegated to extractors module)
            if let Some(action) = super::extractors::extract_method_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Introduce constant (delegated to extractors module)
            if let Some(action) = super::extractors::introduce_constant_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Surround with actions (delegated to surround module)
            actions.extend(super::surround::surround_with_actions(
                &params.range,
                &params.text_document.uri,
                source_text,
            ));
        }

        // Extract interface (works with cursor position, not selection). The
        // server upgrades this to an interface in its own file when the client
        // supports `CreateFile`.
        if let Ok(offset) = position_to_offset(source_text, params.range.start) {
            let package = uri_package(&self.workspace_index, &params.text_document.uri);
            let index = Arc::clone(&self.workspace_index);
            if let Some(plan) =
                super::extract_interface::plan(source_text, offset as usize, |name| {
                    let fqn = if package.is_empty() {
                        name.to_string()
                    } else {
                        format!("{package}.{name}")
                    };
                    index.has_class(&fqn)
                })
            {
                actions.push(plan.same_file_action(&params.text_document.uri, source_text));
            }
        }

        // Move static method to utility class (works with cursor position)
        if let Some(action) = super::extractors::move_static_method_action(
            &params.range,
            &params.text_document.uri,
            source_text,
            &self.workspace_index,
        ) {
            actions.push(action);
        }

        // Move method to another class (works with cursor on method declaration)
        actions.extend(super::move_refactoring::move_method_actions(
            &params.range,
            &params.text_document.uri,
            source_text,
            &self.workspace_index,
        ));

        // Generate test class (works with cursor position on class)
        if let Some(action) = super::extractors::generate_test_class_action(
            &params.range,
            &params.text_document.uri,
            source_text,
        ) {
            actions.push(action);
        }

        // Unambiguous missing imports (shared by "Add all missing imports"
        // and organize imports, IntelliJ's optimize-imports behaviour).
        let missing = self.missing_imports(file_id, &params.text_document.uri, source_text);
        if !missing.is_empty() {
            if let Some(action) = self.add_all_missing_imports_action(
                &params.text_document.uri,
                source_text,
                &missing,
            ) {
                actions.push(action);
            }
        }

        // Add import organization action
        if let Some(action) =
            self.organize_imports_action(&params.text_document.uri, source_text, &missing)
        {
            actions.push(action);
        }

        // Add code generation actions based on cursor context
        if params.text_document.uri.to_file_path().is_ok() {
            // Generate constructor
            if let Some(action) = self.generation.generate_constructor_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Generate getters/setters
            if let Some(action) = self.generation.generate_getters_setters_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Implement interface methods
            if let Some(action) = self.generation.implement_interface_methods_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Override superclass methods
            if let Some(action) = self.generation.override_methods_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Generate equals and hashCode
            if let Some(action) = self.generation.generate_equals_hashcode_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Generate toString
            if let Some(action) = self.generation.generate_tostring_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Inline variable (cursor on variable declaration or usage)
            if let Some(action) = self.refactoring.inline_variable_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Inline method (cursor on method declaration or call)
            if let Some(action) = self.refactoring.inline_method_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }

            // Introduce field (delegated to extractors module)
            if let Some(action) = super::extractors::introduce_field_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Introduce parameter (delegated to extractors module)
            if let Some(action) = super::extractors::introduce_parameter_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Convert to for-each (indexed for -> enhanced for)
            if let Some(action) = self.refactoring.convert_to_foreach_action(
                &params.range,
                &params.text_document.uri,
                source_text,
            ) {
                actions.push(action);
            }

            // Safe Delete (delete symbol only if no usages)
            if workspace_ready && !is_cancelled() {
                if let Some(action) = self.refactoring.safe_delete_action_cancellable(
                    file_id,
                    &params.text_document.uri,
                    source_text,
                    params.range.start,
                    &is_cancelled,
                ) {
                    actions.push(action);
                }
            }

            // Change Method Signature info (shows signature and call site count)
            if let Some(action) = self.refactoring.change_signature_info_action(
                file_id,
                &params.text_document.uri,
                source_text,
                params.range.start,
            ) {
                actions.push(action);
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect(),
            ))
        }
    }

    /// Generate a quick fix for a diagnostic
    fn quick_fix_for_diagnostic(
        &self,
        file_id: FileId,
        diagnostic: &Diagnostic,
        uri: &Url,
        source_text: &str,
    ) -> Option<CodeAction> {
        debug!(
            "Generating quick fix for diagnostic: {:?}",
            diagnostic.message
        );

        // Parse the file to get the syntax tree - use Salsa snapshot to avoid lock contention.
        // try_lock_for: degrade to no quick fix rather than freeze if did_change is currently
        // holding the DB mutex.
        let root = {
            let snapshot = {
                let db = self.txn;
                if db.file_text(file_id).as_str() != source_text {
                    return None;
                }
                db.snapshot()
            }; // Lock released here - concurrent code action requests can now run

            let parse_tree = snapshot.parse_file(file_id);
            parse_tree.syntax()
        };

        // QuickFixProvider walks ancestors from the diagnostic site to locate
        // declarations. Passing SOURCE_FILE prevents those searches from
        // finding anything, and may associate a fix with the wrong declaration.
        let lines = crate::position_utils::LineIndex::new(source_text);
        let (start, end) = lines.range_to_offsets(source_text, diagnostic.range).ok()?;
        let start = rowan::TextSize::from(u32::try_from(start).ok()?);
        let end = rowan::TextSize::from(u32::try_from(end).ok()?);
        let element = root.covering_element(rowan::TextRange::new(start, end));
        let node = element
            .as_node()
            .cloned()
            .or_else(|| element.as_token()?.parent())?;
        let fixes = QuickFixProvider::get_quick_fixes(&node, &diagnostic.message);

        if fixes.is_empty() {
            return None;
        }

        // Convert first quick fix to LSP CodeAction
        let fix = &fixes[0];

        // Convert hyperion-actions TextEdit to LSP TextEdit
        let lsp_edits: Vec<LspTextEdit> = fix
            .edits
            .iter()
            .map(|edit| LspTextEdit {
                range: lines.text_range_to_lsp_range(source_text, edit.range),
                new_text: edit.new_text.clone(),
            })
            .collect();

        // Create workspace edit
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), lsp_edits);

        let workspace_edit = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };

        Some(CodeAction {
            title: fix.title.clone(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(workspace_edit),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        })
    }

    /// Generate import suggestions for unresolved symbols
    ///
    /// When a diagnostic indicates an unresolved symbol (e.g., "cannot find symbol: List"),
    /// this method looks up the workspace index to find matching classes and suggests imports.
    fn add_import_suggestions(
        &self,
        diagnostic: &Diagnostic,
        uri: &Url,
        source_text: &str,
    ) -> Vec<CodeAction> {
        let mut actions = Vec::new();

        // Check if diagnostic is about unresolved symbol
        let msg = &diagnostic.message;
        if !msg.contains("cannot find symbol")
            && !msg.contains("cannot resolve")
            && !msg.contains("unresolved")
        {
            return actions;
        }

        // Extract the symbol name from the diagnostic
        let Some(symbol_name) = unresolved_symbol(msg) else {
            return actions;
        };

        debug!("Looking up import candidates for: {}", symbol_name);
        // A qualified or lower-case subject is not a type awaiting an import.
        if symbol_name.contains('.')
            || !symbol_name
                .chars()
                .next()
                .is_some_and(|first| first.is_uppercase())
        {
            return actions;
        }

        let current_package = uri
            .to_file_path()
            .ok()
            .and_then(|path| self.workspace_index.get_file_id(&path))
            .and_then(|file_id| self.workspace_index.get_imports(file_id))
            .and_then(|imports| imports.current_package)
            .unwrap_or_default();
        let candidates = super::imports::import_candidates(
            &self.workspace_index,
            &symbol_name,
            &current_package,
        );
        let winner = super::imports::unambiguous(&candidates).map(|c| c.fqn.clone());

        for candidate in candidates.iter().take(8) {
            if let Some(mut action) =
                self.create_import_action(uri, source_text, &candidate.fqn, diagnostic)
            {
                action.is_preferred = Some(winner.as_deref() == Some(candidate.fqn.as_str()));
                actions.push(action);
            }
        }

        actions
    }

    fn add_symbol_replacement_suggestions(
        &self,
        diagnostic: &Diagnostic,
        uri: &Url,
    ) -> Vec<CodeAction> {
        let mut actions = Vec::new();

        let Some(data) = diagnostic.data.as_ref() else {
            return actions;
        };
        let Some(suggested) = data.get("suggested_symbol").and_then(|v| v.as_str()) else {
            return actions;
        };

        let edit = LspTextEdit {
            range: diagnostic.range,
            new_text: suggested.to_string(),
        };

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![edit]);

        actions.push(CodeAction {
            title: format!("Change to {suggested}"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        });

        actions
    }

    /// Create an import action for a given FQN
    fn create_import_action(
        &self,
        uri: &Url,
        source_text: &str,
        fqn: &str,
        diagnostic: &Diagnostic,
    ) -> Option<CodeAction> {
        // Find import insertion point
        let import_manager =
            crate::import_manager::ImportManager::new(Arc::clone(&self.workspace_index));
        let path = uri.to_file_path().ok()?;
        let edit = if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("kt" | "kts")
        ) {
            import_manager.generate_kotlin_import_edit(fqn, source_text)
        } else {
            import_manager.generate_import_edit(
                self.workspace_index.get_file_id(&path)?,
                fqn,
                source_text,
            )?
        };

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![edit]);

        Some(CodeAction {
            title: format!("Import '{fqn}'"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Generate create field/method suggestions for undefined members
    fn create_member_suggestions(
        &self,
        diagnostic: &Diagnostic,
        uri: &Url,
        source_text: &str,
    ) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        let msg = &diagnostic.message;

        // Check for undefined field
        if msg.contains("cannot find symbol") && (msg.contains("variable") || msg.contains("field"))
        {
            if let Some(field_name) = unresolved_symbol(msg) {
                if let Some(action) =
                    self.create_field_action(uri, source_text, &field_name, diagnostic)
                {
                    actions.push(action);
                }
            }
        }

        // Check for undefined method
        if msg.contains("cannot find symbol") && msg.contains("method") {
            if let Some(method) = unresolved_method(msg) {
                if let Some(action) = self.create_method_action(
                    uri,
                    source_text,
                    &method.name,
                    &method.parameter_types,
                    diagnostic,
                ) {
                    actions.push(action);
                }
            }
        }

        actions
    }

    /// Create a "Create field" action
    fn create_field_action(
        &self,
        uri: &Url,
        source_text: &str,
        field_name: &str,
        diagnostic: &Diagnostic,
    ) -> Option<CodeAction> {
        // Find class declaration to insert field
        let diag_offset = position_to_offset(source_text, diagnostic.range.start).ok()? as usize;
        let class_start = source_text[..diag_offset].rfind("class ")?;
        let brace_pos = source_text[class_start..].find('{')?;
        let insert_pos = class_start + brace_pos + 1;

        // Get indentation
        let line_start = source_text[..insert_pos].rfind('\n').map_or(0, |p| p + 1);
        let base_indent = &source_text[line_start..insert_pos]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>();
        let field_indent = format!("{base_indent}    ");

        let field_decl = format!("\n{field_indent}private Object {field_name};");

        let insert_range = Range::new(
            offset_to_position(source_text, insert_pos),
            offset_to_position(source_text, insert_pos),
        );

        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![LspTextEdit {
                range: insert_range,
                new_text: field_decl,
            }],
        );

        Some(CodeAction {
            title: format!("Create field '{field_name}'"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Create a "Create method" action
    fn create_method_action(
        &self,
        uri: &Url,
        source_text: &str,
        method_name: &str,
        parameter_types: &[String],
        diagnostic: &Diagnostic,
    ) -> Option<CodeAction> {
        // Find class declaration to insert method
        let diag_offset = position_to_offset(source_text, diagnostic.range.start).ok()? as usize;
        let class_start = source_text[..diag_offset].rfind("class ")?;

        // Find the end of the class (last closing brace before EOF or next class)
        let class_body_start = source_text[class_start..].find('{')?;
        let class_body_offset = class_start + class_body_start;

        // Find matching close brace (with proper string handling)
        let mut brace_count = 1;
        let mut insert_pos = class_body_offset + 1;
        let mut in_string = false;
        let mut in_char = false;
        let mut escape_next = false;
        for (byte_offset, c) in source_text[class_body_offset + 1..].char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }
            if c == '\\' && (in_string || in_char) {
                escape_next = true;
                continue;
            }
            if c == '"' && !in_char {
                in_string = !in_string;
                continue;
            }
            if c == '\'' && !in_string {
                in_char = !in_char;
                continue;
            }
            if in_string || in_char {
                continue;
            }
            if c == '{' {
                brace_count += 1;
            } else if c == '}' {
                brace_count -= 1;
                if brace_count == 0 {
                    insert_pos = class_body_offset + 1 + byte_offset;
                    break;
                }
            }
        }

        // Get indentation
        let line_start = source_text[..insert_pos].rfind('\n').map_or(0, |p| p + 1);
        let base_indent = &source_text[line_start..insert_pos]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>();
        let method_indent = format!("{base_indent}    ");

        let parameters = parameter_types
            .iter()
            .enumerate()
            .map(|(index, parameter_type)| format!("{parameter_type} arg{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let method_decl = format!(
            "\n\n{method_indent}private void {method_name}({parameters}) {{\n{method_indent}    // TODO: implement\n{method_indent}}}"
        );

        let insert_range = Range::new(
            offset_to_position(source_text, insert_pos),
            offset_to_position(source_text, insert_pos),
        );

        let mut changes = HashMap::new();
        changes.insert(
            uri.clone(),
            vec![LspTextEdit {
                range: insert_range,
                new_text: method_decl,
            }],
        );

        Some(CodeAction {
            title: format!("Create method '{method_name}'"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Fully qualified names to import for the file's unresolved simple type
    /// names that have exactly one sensible candidate.
    fn missing_imports(&self, file_id: FileId, uri: &Url, source_text: &str) -> Vec<String> {
        use hyperion_semantics::{SemanticDatabase, SemanticErrorKind};
        let names: Vec<String> = {
            if self.txn.file_text(file_id).as_str() != source_text {
                return Vec::new();
            }
            let file_semantics = self.txn.file_semantics(file_id);
            file_semantics
                .diagnostics()
                .iter()
                .filter_map(|error| match &error.kind {
                    SemanticErrorKind::UndefinedSymbol { symbol_name, .. } => {
                        Some(symbol_name.to_string())
                    }
                    _ => None,
                })
                .collect()
        };
        let current_package = uri
            .to_file_path()
            .ok()
            .and_then(|path| self.workspace_index.get_file_id(&path))
            .and_then(|id| self.workspace_index.get_imports(id))
            .and_then(|imports| imports.current_package)
            .unwrap_or_default();
        let mut fqns = std::collections::BTreeSet::new();
        let mut seen = std::collections::BTreeSet::new();
        for name in names {
            if name.contains('.')
                || !name.chars().next().is_some_and(char::is_uppercase)
                || !seen.insert(name.clone())
            {
                continue;
            }
            let candidates =
                super::imports::import_candidates(&self.workspace_index, &name, &current_package);
            if let Some(winner) = super::imports::unambiguous(&candidates) {
                fqns.insert(winner.fqn.clone());
            }
        }
        fqns.into_iter().collect()
    }

    fn add_all_missing_imports_action(
        &self,
        uri: &Url,
        source_text: &str,
        missing: &[String],
    ) -> Option<CodeAction> {
        let path = uri.to_file_path().ok()?;
        let edits = crate::file_move::add_import_edits(source_text, &path, missing);
        if edits.is_empty() {
            return None;
        }
        let lsp_edits = edits
            .into_iter()
            .map(|edit| LspTextEdit {
                range: Range {
                    start: offset_to_position(source_text, edit.start),
                    end: offset_to_position(source_text, edit.end),
                },
                new_text: edit.text,
            })
            .collect();
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), lsp_edits);
        Some(CodeAction {
            title: "Add all missing imports".to_string(),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// Generate an "Organize Imports" action. Besides sorting and grouping,
    /// it adds the unambiguous missing imports (`missing`).
    fn organize_imports_action(
        &self,
        uri: &Url,
        source_text: &str,
        missing: &[String],
    ) -> Option<CodeAction> {
        debug!("Generating organize imports action");

        // Check if there is anything to organize or add
        if !source_text.contains("import ") && missing.is_empty() {
            return None;
        }

        let file_id = self
            .workspace_index
            .get_file_id(&(uri.to_file_path().ok()?))?;

        // Parse the file to get the syntax tree - use Salsa snapshot to avoid lock contention.
        // try_lock_for: degrade to no organize-imports rather than freeze if did_change is
        // currently holding the DB mutex.
        let root = {
            let snapshot = self.txn.snapshot(); // Lock released here - concurrent organize imports requests can now run

            let parse_tree = snapshot.parse_file(file_id);
            parse_tree.syntax()
        };

        // Add the missing imports first, then sort/group the combined block.
        let path = uri.to_file_path().ok()?;
        let additions = crate::file_move::add_import_edits(source_text, &path, missing);
        let with_additions = crate::file_move::apply_byte_edits(source_text, &additions);
        let organized = if additions.is_empty() {
            let edits = ImportOrganizer::organize_imports(&root);
            let mut ordered = edits;
            ordered.sort_by_key(|edit| edit.range.start());
            let mut text = source_text.to_string();
            for edit in ordered.into_iter().rev() {
                let start: usize = edit.range.start().into();
                let end: usize = edit.range.end().into();
                text.replace_range(start..end, &edit.new_text);
            }
            text
        } else {
            let tokens = hyperion_parser::lexer::Lexer::new(&with_additions).collect();
            let (green, _) = hyperion_parser::parser::Parser::parse(tokens);
            let root = hyperion_parser::syntax::SyntaxNode::new_root(green);
            let mut ordered = ImportOrganizer::organize_imports(&root);
            ordered.sort_by_key(|edit| edit.range.start());
            let mut text = with_additions;
            for edit in ordered.into_iter().rev() {
                let start: usize = edit.range.start().into();
                let end: usize = edit.range.end().into();
                text.replace_range(start..end, &edit.new_text);
            }
            text
        };

        if organized == source_text {
            debug!("No import organization needed");
            return None;
        }

        // Express the result as one replacement of the import block (or, when
        // the file had no imports, the plain insertions).
        let lsp_edits: Vec<LspTextEdit> = match (
            crate::file_move::import_region(source_text, &path),
            crate::file_move::import_region(&organized, &path),
        ) {
            (Some((old_start, old_end)), Some((new_start, new_end))) => vec![LspTextEdit {
                range: Range {
                    start: offset_to_position(source_text, old_start),
                    end: offset_to_position(source_text, old_end),
                },
                new_text: organized[new_start..new_end].to_string(),
            }],
            _ => additions
                .into_iter()
                .map(|edit| LspTextEdit {
                    range: Range {
                        start: offset_to_position(source_text, edit.start),
                        end: offset_to_position(source_text, edit.end),
                    },
                    new_text: edit.text,
                })
                .collect(),
        };
        if lsp_edits.is_empty() {
            return None;
        }

        // Create workspace edit
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), lsp_edits);

        let workspace_edit = WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        };

        Some(CodeAction {
            title: "Organize imports".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            diagnostics: None,
            edit: Some(workspace_edit),
            command: None,
            is_preferred: None,
            disabled: None,
            data: None,
        })
    }
}

/// Package of the file at `uri` according to the index (empty for none).
fn uri_package(index: &WorkspaceIndex, uri: &Url) -> String {
    uri.to_file_path()
        .ok()
        .and_then(|path| index.get_file_id(&path))
        .and_then(|id| index.get_imports(id))
        .and_then(|imports| imports.current_package)
        .unwrap_or_default()
}

/// Convert LSP Position to byte offset
///
/// BUG FIX: Previously this function incorrectly counted UTF-8 chars instead of
/// UTF-16 code units for the character position. Now delegates to the correct
/// implementation in position_utils which properly handles UTF-16.
fn position_to_offset(
    source: &str,
    position: Position,
) -> Result<u32, crate::position_utils::PositionError> {
    crate::position_utils::position_to_offset_u32(source, position)
}
