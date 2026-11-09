// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Client file notifications and file-explorer refactoring.
//!
//! * `workspace/willRenameFiles` turns a file or directory move in the
//!   client's explorer into the edits that keep the code compiling (package
//!   declarations, imports, references; a renamed public class follows its
//!   file). The client applies the edit, then performs the rename.
//! * `workspace/didChangeWatchedFiles`, `didCreateFiles`, `didRenameFiles` and
//!   `didDeleteFiles` feed the same idempotent reconciler as the OS file
//!   watcher (see `handlers::lifecycle::start_file_watcher`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CreateFilesParams, DeleteFilesParams, DidChangeWatchedFilesParams, FileChangeType, MessageType,
    RenameFilesParams, Url, WorkspaceEdit,
};
use tracing::{debug, warn};

use crate::file_move::{plan_file_moves, plan_to_workspace_edit, EditStyle, FileMove};
use crate::file_watcher::FileChangeEvent;
use crate::server::Backend;

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("java" | "kt")
    )
}

impl Backend {
    async fn dispatch_file_events(&self, events: Vec<FileChangeEvent>) {
        let Some(ctx) = self.workspace.file_events.get().cloned() else {
            return;
        };
        for event in events {
            crate::handlers::file_events::handle_file_event(&ctx, event).await;
        }
    }

    fn client_relevant_path(&self, path: &Path) -> bool {
        let root = self.workspace.root.read().clone().unwrap_or_default();
        crate::file_watcher::FileWatcher::is_relevant_source_path(&root, path)
    }

    pub(crate) async fn handle_did_change_watched_files(
        &self,
        params: DidChangeWatchedFilesParams,
    ) {
        let mut events = Vec::new();
        for change in params.changes {
            let Ok(path) = change.uri.to_file_path() else {
                continue;
            };
            if !self.client_relevant_path(&path) {
                continue;
            }
            events.push(match change.typ {
                FileChangeType::CREATED => FileChangeEvent::Created(path),
                FileChangeType::CHANGED => FileChangeEvent::Modified(path),
                FileChangeType::DELETED => FileChangeEvent::Deleted(path),
                _ => continue,
            });
        }
        self.dispatch_file_events(events).await;
    }

    pub(crate) async fn handle_did_create_files(&self, params: CreateFilesParams) {
        let events = params
            .files
            .iter()
            .filter_map(|file| Url::parse(&file.uri).ok()?.to_file_path().ok())
            .filter(|path| self.client_relevant_path(path) && path.is_file())
            .map(FileChangeEvent::Created)
            .collect();
        self.dispatch_file_events(events).await;
    }

    pub(crate) async fn handle_did_delete_files(&self, params: DeleteFilesParams) {
        let events = params
            .files
            .iter()
            .filter_map(|file| Url::parse(&file.uri).ok()?.to_file_path().ok())
            .filter(|path| self.client_relevant_path(path))
            .map(|path| {
                if is_source_file(&path) || hyperion_layout::is_build_input(&path) {
                    FileChangeEvent::Deleted(path)
                } else {
                    FileChangeEvent::DirectoryDeleted(path)
                }
            })
            .collect();
        self.dispatch_file_events(events).await;
    }

    pub(crate) async fn handle_did_rename_files(&self, params: RenameFilesParams) {
        let mut events = Vec::new();
        for file in &params.files {
            let (Some(old), Some(new)) = (
                Url::parse(&file.old_uri)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok()),
                Url::parse(&file.new_uri)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok()),
            ) else {
                continue;
            };
            if !self.client_relevant_path(&old) && !self.client_relevant_path(&new) {
                continue;
            }
            events.push(if new.is_dir() {
                FileChangeEvent::DirectoryRenamed { from: old, to: new }
            } else {
                FileChangeEvent::Renamed(old, new)
            });
        }
        self.dispatch_file_events(events).await;
    }

    /// `workspace/willRenameFiles`: edits that keep Java/Kotlin sources
    /// consistent when the client moves files or directories.
    pub(crate) async fn handle_will_rename_files(
        &self,
        params: RenameFilesParams,
    ) -> Result<Option<WorkspaceEdit>> {
        if self.has_desynchronized_open_document() {
            return Ok(None);
        }
        let Some(root) = self.workspace.root.read().clone() else {
            return Ok(None);
        };
        if !self
            .workspace
            .ready
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(None);
        }

        let index = Arc::clone(&self.workspace.index);
        let mut moves: Vec<FileMove> = Vec::new();
        let mut class_renames: Vec<FileMove> = Vec::new();
        for file in &params.files {
            let (Some(old), Some(new)) = (
                Url::parse(&file.old_uri)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok()),
                Url::parse(&file.new_uri)
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok()),
            ) else {
                continue;
            };
            if old.is_dir() {
                for (_, path) in index.workspace_files_under(&old) {
                    if !is_source_file(&path) {
                        continue;
                    }
                    let Ok(relative) = path.strip_prefix(&old) else {
                        continue;
                    };
                    moves.push(FileMove {
                        new: new.join(relative),
                        old: path,
                    });
                }
            } else if is_source_file(&old) {
                if old.parent() == new.parent() && old.file_stem() != new.file_stem() {
                    class_renames.push(FileMove { old, new });
                } else {
                    moves.push(FileMove { old, new });
                }
            }
        }
        if moves.is_empty() && class_renames.is_empty() {
            return Ok(None);
        }

        let Ok(permit) = self.workspace.host.read_interactive().await else {
            return Ok(None);
        };
        let plan_index = Arc::clone(&index);
        let plan_root = root.clone();
        let planned = tokio::task::spawn_blocking(move || {
            if moves.is_empty() {
                return Ok(None);
            }
            plan_file_moves(&plan_index, &plan_root, &moves).map(Some)
        })
        .await;
        let mut edit = match planned {
            Ok(Ok(Some(plan))) => {
                plan_to_workspace_edit(&plan, EditStyle::BeforeClientRename, &HashMap::new())
            }
            Ok(Ok(None)) => None,
            Ok(Err(reason)) => {
                drop(permit);
                self.client
                    .show_message(
                        MessageType::WARNING,
                        format!("Hyperion could not update imports for this move: {reason}"),
                    )
                    .await;
                return Ok(None);
            }
            Err(error) => {
                warn!("willRenameFiles planning task failed: {error}");
                drop(permit);
                return Ok(None);
            }
        };
        drop(permit);

        // A renamed public class follows its file: run the rename refactoring
        // for the type and keep only its text edits (the client renames the file).
        for class_rename in class_renames {
            if let Some(class_edit) = self.class_rename_for_file_rename(&class_rename).await {
                merge_changes(&mut edit, class_edit);
            }
        }

        let open_versions: HashMap<Url, i32> = self
            .documents
            .by_uri
            .iter()
            .filter_map(|state| Url::parse(&state.uri).ok().map(|uri| (uri, state.version)))
            .collect();
        Ok(edit.map(|edit| {
            if self.supports_versioned_workspace_edits() {
                super::refactoring::prepare_workspace_edit_for_client(
                    edit,
                    &open_versions,
                    true,
                    false,
                )
            } else {
                edit
            }
        }))
    }

    async fn class_rename_for_file_rename(&self, file_move: &FileMove) -> Option<WorkspaceEdit> {
        let old_stem = file_move.old.file_stem()?.to_str()?.to_string();
        let new_stem = file_move.new.file_stem()?.to_str()?.to_string();
        if file_move.old.extension().and_then(|e| e.to_str()) != Some("java") {
            return None;
        }
        let file_id = self.workspace.index.get_file_id(&file_move.old)?;
        let text = self.workspace.index.get_file_contents(file_id)?;
        // Only a top-level type named like the file moves with it.
        let offset = {
            let tree = self.workspace.index.cached_java_parse(file_id, &text);
            let root = tree.map_or_else(
                || {
                    let tokens = hyperion_parser::Lexer::new(&text).collect();
                    let (green, _) = hyperion_parser::Parser::parse(tokens);
                    hyperion_parser::SyntaxNode::new_root(green)
                },
                |tree| hyperion_parser::SyntaxNode::new_root(tree.as_ref().clone()),
            );
            top_level_type_name_offset(&root, &old_stem)?
        };
        let position = crate::position_utils::offset_to_position(&text, offset);
        let uri = Url::from_file_path(&file_move.old).ok()?;
        let provider = self.services.rename.clone();
        let cancellation = tower_lsp::CancellationToken::current();
        let workspace_complete = self.workspace.destructive_refactoring.is_ready();
        let proof = tokio::task::spawn_blocking(move || {
            provider.rename_proof_cancellable_with_workspace_completeness(
                file_id,
                position,
                &new_stem,
                &text,
                &uri,
                workspace_complete,
                &cancellation,
            )
        })
        .await
        .ok()?;
        match proof {
            Ok(Some(proof)) => Some(proof.edit),
            Ok(None) => None,
            Err(reason) => {
                debug!("willRename class rename skipped: {reason}");
                self.client
                    .show_message(
                        MessageType::WARNING,
                        format!("Hyperion did not rename class '{old_stem}': {reason}"),
                    )
                    .await;
                None
            }
        }
    }
}

fn top_level_type_name_offset(root: &hyperion_parser::SyntaxNode, name: &str) -> Option<usize> {
    root.children()
        .filter_map(hyperion_parser::syntax::TypeDecl::cast)
        .filter_map(|declaration| declaration.name())
        .find(|token| token.text() == name)
        .map(|token| usize::from(token.text_range().start()))
}

fn merge_changes(target: &mut Option<WorkspaceEdit>, other: WorkspaceEdit) {
    let Some(other_changes) = other.changes else {
        return;
    };
    let edit = target.get_or_insert_with(WorkspaceEdit::default);
    let changes = edit.changes.get_or_insert_with(HashMap::new);
    for (uri, edits) in other_changes {
        changes.entry(uri).or_default().extend(edits);
    }
}

#[cfg(test)]
mod tests {
    use super::top_level_type_name_offset;

    fn name_offset(source: &str, name: &str) -> Option<usize> {
        let tokens = hyperion_parser::Lexer::new(source).collect();
        let (green, _) = hyperion_parser::Parser::parse(tokens);
        top_level_type_name_offset(&hyperion_parser::SyntaxNode::new_root(green), name)
    }

    #[test]
    fn explorer_rename_selects_exact_top_level_type_identifier() {
        let source = "// class Circle\r\nclass CircleHelper {}\r\nclass Holder { class Circle {} }\r\npublic class\r\n/* declaration */ Circle {}";
        assert_eq!(name_offset(source, "Circle"), source.rfind("Circle {}"));
        assert_eq!(name_offset(source, "Missing"), None);
        assert_eq!(
            name_offset("class Holder { class Circle {} }", "Circle"),
            None
        );
    }

    #[test]
    fn explorer_rename_handles_all_java_top_level_type_kinds() {
        for declaration in [
            "public class\tCircle {}",
            "public interface\nCircle {}",
            "public enum /* explanation */ Circle { ONE }",
            "public record\nCircle(int radius) {}",
            "public @interface\nCircle {}",
        ] {
            assert_eq!(
                name_offset(declaration, "Circle"),
                declaration.find("Circle"),
                "{declaration}"
            );
        }
    }
}
