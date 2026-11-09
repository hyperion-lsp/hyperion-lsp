// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical on-disk workspace fixture for lightweight LSP integration tests.
//!
//! This module intentionally has no tests of its own. Integration-test crates
//! import the adapter matching the production object they exercise.

#![allow(dead_code)]

use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use hyperion_workspace::{query::QueryEngine, WorkspaceIndex};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

struct IndexedWorkspace {
    temp_dir: TempDir,
    index: WorkspaceIndex,
    files: Vec<(PathBuf, FileId, String)>,
}

impl IndexedWorkspace {
    fn create(files: Vec<(&str, &str)>) -> Self {
        let temp_dir = TempDir::new().expect("create temporary test workspace");
        let workspace_root = temp_dir.path().join("workspace");
        std::fs::create_dir_all(&workspace_root).expect("create test workspace root");

        let index = WorkspaceIndex::new(workspace_root.clone());
        let indexed_files = files
            .into_iter()
            .map(|(relative_path, content)| {
                let path = workspace_root.join(relative_path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap_or_else(|error| {
                        panic!("create fixture directory {}: {error}", parent.display())
                    });
                }
                std::fs::write(&path, content).unwrap_or_else(|error| {
                    panic!("write fixture source {}: {error}", path.display())
                });
                index
                    .index_file_with_source(&path, content)
                    .unwrap_or_else(|error| {
                        panic!("index fixture source {}: {error}", path.display())
                    });
                let file_id = index.get_file_id(&path).unwrap_or_else(|| {
                    panic!(
                        "fixture source was indexed without a file ID: {}",
                        path.display()
                    )
                });
                (path, file_id, content.to_owned())
            })
            .collect();

        Self {
            temp_dir,
            index,
            files: indexed_files,
        }
    }
}

pub fn indexed_workspace(
    files: Vec<(&str, &str)>,
) -> (TempDir, WorkspaceIndex, Vec<(PathBuf, FileId)>) {
    let workspace = IndexedWorkspace::create(files);
    let indexed_files = workspace
        .files
        .into_iter()
        .map(|(path, file_id, _)| (path, file_id))
        .collect();
    (workspace.temp_dir, workspace.index, indexed_files)
}

pub fn indexed_workspace_paths(
    files: Vec<(&str, &str)>,
) -> (TempDir, WorkspaceIndex, Vec<PathBuf>) {
    let workspace = IndexedWorkspace::create(files);
    let paths = workspace
        .files
        .into_iter()
        .map(|(path, _, _)| path)
        .collect();
    (workspace.temp_dir, workspace.index, paths)
}

pub fn query_workspace(files: Vec<(&str, &str)>) -> (TempDir, QueryEngine) {
    let workspace = IndexedWorkspace::create(files);
    (workspace.temp_dir, QueryEngine::new(workspace.index))
}

pub fn semantic_workspace(
    files: Vec<(&str, &str, Language)>,
) -> (
    TempDir,
    Arc<WorkspaceIndex>,
    Arc<SemanticHost>,
    Vec<(PathBuf, FileId, String)>,
) {
    let languages: Vec<Language> = files.iter().map(|(_, _, language)| *language).collect();
    let workspace = IndexedWorkspace::create(
        files
            .into_iter()
            .map(|(path, content, _)| (path, content))
            .collect(),
    );
    let index = Arc::new(workspace.index);
    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|db| {
        for ((_, file_id, content), language) in workspace.files.iter().zip(languages) {
            db.set_file_text(*file_id, Arc::new(content.clone()));
            db.set_file_language(*file_id, language);
        }
    });

    (workspace.temp_dir, index, host, workspace.files)
}
