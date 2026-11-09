// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(dead_code)]
// Diagnostic paths intentionally retain Debug formatting so escaped paths and quotes are visible.
#![allow(clippy::unnecessary_debug_formatting)]
// Mutable receivers are part of the established test-harness API and preserve exclusive access.
#![allow(clippy::needless_pass_by_ref_mut)]
//! LSP Test Framework
//!
//! Provides utilities for testing LSP functionality with multi-file workspaces.
//!
//! # Example
//!
//! ```no_run
//! use lsp_test_framework::*;
//!
//! #[test]
//! fn test_goto_definition() {
//!     let mut test = LspTest::new();
//!
//!     test.add_file("com/example/Foo.java", r#"
//!         package com.example;
//!         import com.example.Bar;
//!
//!         public class Foo {
//!             public void test() {
//!                 Bar b = new Bar();
//!                 b.baz<|>();  // cursor position
//!             }
//!         }
//!     "#);
//!
//!     test.add_file("com/example/Bar.java", r#"
//!         package com.example;
//!
//!         public class Bar {
//!             public void /*target:baz_method*/baz() {}
//!         }
//!     "#);
//!
//!     let result = test.goto_definition("com/example/Foo.java", "<|>");
//!     assert_goto_target!(test, result, "com/example/Bar.java", "target:baz_method");
//! }
//! ```

use fs2::FileExt;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::server::{create_lsp_server, Backend};
use hyperion_semantics::db::SourceDatabase;
use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::query::QueryEngine;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs::File, io};
use tempfile::TempDir;
use tower_lsp::lsp_types::*;

/// Result of setting up a fully-initialized workspace index and Salsa database.
///
/// This mirrors what `Backend::new()` does in production: the Salsa DB gets
/// a `WorkspaceIndexHandle` pointing to the real workspace index, `workspace_version`
/// is set, and all test files are registered with their text and language.
struct TestSetup {
    index: Arc<WorkspaceIndex>,
    host: Arc<SemanticHost>,
}

/// Create a workspace index + Salsa DB that mirrors production `Backend::new()`.
///
/// This is the **single source of truth** for test workspace setup. Every test method
/// should use this instead of ad-hoc index/DB creation. The key things this does that
/// ad-hoc setup was missing:
///
/// 1. Sets `workspace_index_handle` on the DB (required for `file_semantics` Salsa query)
/// 2. Sets `workspace_version` (required for cache invalidation tracking)
/// 3. Registers ALL test files in the DB (not just the active file)
fn setup_workspace_and_db(
    files: &BTreeMap<String, TestFile>,
    workspace_root: &Path,
    temp_dir: &Path,
) -> Result<TestSetup, String> {
    let workspace_project_root = temp_dir.join("workspace");
    let index = WorkspaceIndex::new(workspace_project_root);

    // Index all test files into workspace
    for (rel_path, test_file) in files {
        let full_path = workspace_root.join(rel_path);
        if let Err(e) = index.index_file_with_source(&full_path, &test_file.clean_content) {
            eprintln!("Warning: Failed to index {rel_path}: {e}");
        }
    }

    // Index JDK classes for method resolution
    if let Err(e) = index_jdk_with_cache(&index) {
        eprintln!("Warning: Failed to index JDK classes: {e}");
    }

    let index_arc = Arc::new(index);

    // The host binds the Salsa DB to the index (the handle `file_semantics`
    // needs), mirroring production `Backend::new()`.
    let host = SemanticHost::new(Arc::clone(&index_arc));
    host.with_db(|db| {
        // Register ALL test files (not just the active file)
        for (rel_path, test_file) in files {
            let full_path = workspace_root.join(rel_path);
            if let Some(file_id) = index_arc.get_file_id(&full_path) {
                db.set_file_text(file_id, Arc::new(test_file.clean_content.clone()));
                let language = hyperion_workspace::Language::from_path(&full_path)
                    .unwrap_or(hyperion_workspace::Language::Java);
                db.set_file_language(file_id, language);
            }
        }
    });

    Ok(TestSetup {
        index: index_arc,
        host,
    })
}

/// Marker types that can be embedded in test code
#[derive(Debug, Clone)]
pub enum Marker {
    /// Cursor position: `<|>` or `^`
    Cursor,
    /// Named target: `/*name*/` or `/*target:name*/`
    Target(String),
}

/// Parsed marker with position information
#[derive(Debug, Clone)]
pub struct MarkerInfo {
    pub name: String,
    pub line: u32,
    pub character: u32,
    pub offset: usize,
}

/// Canonical location key used for strict set-based assertions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocationKey {
    pub file: String,
    pub line: u32,
    pub character: u32,
}

impl std::fmt::Display for LocationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.character)
    }
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Test file with parsed content and markers
#[derive(Debug, Clone)]
pub struct TestFile {
    /// Original content with markers
    pub raw_content: String,
    /// Clean content (markers removed)
    pub clean_content: String,
    /// Parsed markers
    pub markers: Vec<MarkerInfo>,
}

impl TestFile {
    pub fn new(content: &str) -> Self {
        let (clean_content, markers) = parse_markers(content);
        Self {
            raw_content: content.to_string(),
            clean_content,
            markers,
        }
    }

    pub fn find_marker(&self, name: &str) -> Option<&MarkerInfo> {
        self.markers.iter().find(|m| m.name == name)
    }

    pub fn cursor_position(&self) -> Option<Position> {
        self.find_marker("<|>").map(|m| Position {
            line: m.line,
            character: m.character,
        })
    }
}

/// Main test harness for LSP testing
pub struct LspTest {
    // BTreeMap ensures deterministic iteration order, preventing test flakiness
    files: BTreeMap<String, TestFile>,
    temp_dir: TempDir,
    backend: Option<Backend>,
    workspace_root: PathBuf,
}

/// Shared test context for concurrent LSP requests against a single workspace.
pub struct SharedLspContext {
    files: BTreeMap<String, TestFile>,
    temp_dir: TempDir,
    workspace_root: PathBuf,
    /// The workspace index (public for integration tests that need direct access)
    pub index: Arc<hyperion_workspace::index::WorkspaceIndex>,
    host: Arc<SemanticHost>,
    query_engine: Arc<QueryEngine>,
    source_providers: Arc<hyperion_lsp::source_provider::SourceProviderChain>,
}

impl Default for LspTest {
    fn default() -> Self {
        Self::new()
    }
}

impl LspTest {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let workspace_root = temp_dir.path().join("workspace/src/main/java");
        fs::create_dir_all(&workspace_root).expect("Failed to create workspace structure");

        Self {
            files: BTreeMap::new(),
            temp_dir,
            backend: None,
            workspace_root,
        }
    }

    /// Build a shared, fully-indexed context for concurrent requests.
    pub fn into_shared_context(self) -> Result<SharedLspContext, String> {
        self.into_shared_context_with_options(true)
    }

    /// Build a shared, fully-indexed context with optional JDK indexing.
    pub fn into_shared_context_with_options(
        self,
        index_jdk: bool,
    ) -> Result<SharedLspContext, String> {
        let workspace_project_root = self.temp_dir.path().join("workspace");
        let index = WorkspaceIndex::new(workspace_project_root);

        for (rel_path, test_file) in &self.files {
            let full_path = self.workspace_root.join(rel_path);
            if let Err(e) = index.index_file_with_source(&full_path, &test_file.clean_content) {
                eprintln!("Warning: Failed to index {rel_path}: {e}");
            }
        }

        if index_jdk {
            if let Err(e) = index_jdk_with_cache(&index) {
                eprintln!("Warning: Failed to index JDK classes: {e}");
            }
        }

        let index_arc = Arc::new(index);
        let host = SemanticHost::new(Arc::clone(&index_arc));
        host.with_db(|db| {
            for (rel_path, test_file) in &self.files {
                let full_path = self.workspace_root.join(rel_path);
                if let Some(file_id) = index_arc.get_file_id(&full_path) {
                    db.set_file_text(file_id, Arc::new(test_file.clean_content.clone()));
                    let language = hyperion_workspace::Language::from_path(&full_path)
                        .unwrap_or(hyperion_workspace::Language::Java);
                    db.set_file_language(file_id, language);
                }
            }
        });

        let query_engine = Arc::new(QueryEngine::new((*index_arc).clone()));
        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());

        Ok(SharedLspContext {
            files: self.files,
            temp_dir: self.temp_dir,
            workspace_root: self.workspace_root,
            index: index_arc,
            host,
            query_engine,
            source_providers,
        })
    }

    /// Add a file to the test workspace
    ///
    /// # Arguments
    /// * `path` - Relative path from src/main/java (e.g., "com/example/Foo.java")
    /// * `content` - File content with optional markers
    pub fn add_file(&mut self, path: &str, content: &str) -> &mut Self {
        let test_file = TestFile::new(content);

        // Write clean content to disk
        let file_path = self.workspace_root.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent directories");
        }
        fs::write(&file_path, &test_file.clean_content).expect("Failed to write test file");

        self.files.insert(path.to_string(), test_file);
        self
    }

    /// Initialize the LSP server with the test workspace
    pub fn initialize(&mut self) -> Result<(), String> {
        // Create backend
        // Note: This is a simplified version - in real tests we'd need to handle
        // the full LSP initialization handshake
        let (_service, _socket) = create_lsp_server();

        // We can't easily extract Backend from LspService, so we'll create our own
        // For now, we'll use a different approach - direct API calls

        Ok(())
    }

    /// Send a goto-definition request
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "cursor", "target:foo")
    pub fn goto_definition(
        &mut self,
        file: &str,
        marker: &str,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        if std::env::var("HYPERION_DEBUG_ARRAY_CROSS_FILE").is_ok() {
            let off = marker_info.offset.min(test_file.clean_content.len());
            let start = off.saturating_sub(40);
            let end = (off + 40).min(test_file.clean_content.len());
            let mut ctx = test_file.clean_content[start..end].to_string();
            let caret = off - start;
            if caret <= ctx.len() {
                ctx.insert(caret, '▮');
            }
            eprintln!(
                "DEBUG LSP TEST: marker={} offset={} context=`{}`",
                marker,
                marker_info.offset,
                ctx.replace('\n', "\\n")
            );
        }

        let file_path = self.workspace_root.join(file);
        let _uri = Url::from_file_path(&file_path).map_err(|()| "Failed to create file URI")?;

        // For now, we'll use the internal API directly
        // In a full implementation, we'd send LSP protocol messages
        self.goto_definition_internal(file_path, marker_info.offset)
    }

    /// Get the position of a marker in a file
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "<|1>", "<def>")
    pub fn get_marker_position(&self, file: &str, marker: &str) -> Option<Position> {
        let test_file = self.files.get(file)?;
        let marker_info = test_file.find_marker(marker)?;

        Some(Position {
            line: marker_info.line,
            character: marker_info.character,
        })
    }

    /// Resolve marker aliases for a file.
    ///
    /// Supports:
    /// - bare names: `declaration`
    /// - guillemet markers: `«declaration»`
    /// - legacy angle markers: `<declaration>`
    /// - legacy comment markers: `target:declaration`
    pub fn resolve_marker_name(&self, file: &str, marker: &str) -> Result<String, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let raw = marker.trim();
        let mut candidates = vec![raw.to_string()];
        if !raw.starts_with('«') || !raw.ends_with('»') {
            candidates.push(format!("«{raw}»"));
        }
        if !raw.starts_with('<') || !raw.ends_with('>') {
            candidates.push(format!("<{raw}>"));
        }
        if !raw.starts_with("target:") {
            candidates.push(format!("target:{raw}"));
        }

        // Deduplicate while preserving candidate preference order.
        let mut deduped = Vec::new();
        for c in candidates {
            if !deduped.contains(&c) {
                deduped.push(c);
            }
        }

        for candidate in deduped {
            if test_file.find_marker(&candidate).is_some() {
                return Ok(candidate);
            }
        }

        let available = test_file
            .markers
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "Marker '{marker}' not found in file '{file}'. Available markers: [{available}]"
        ))
    }

    /// Resolve a marker to its canonical location key.
    pub fn marker_location_key(&self, file: &str, marker: &str) -> Result<LocationKey, String> {
        let resolved_name = self.resolve_marker_name(file, marker)?;
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;
        let marker_info = test_file.find_marker(&resolved_name).ok_or_else(|| {
            format!("Resolved marker '{resolved_name}' not found in file '{file}'")
        })?;

        Ok(LocationKey {
            file: normalize_path(file),
            line: marker_info.line,
            character: marker_info.character,
        })
    }

    /// Convert a location returned by LSP into a canonical key.
    pub fn location_to_key(&self, loc: &Location) -> Result<LocationKey, String> {
        let file_path = loc
            .uri
            .to_file_path()
            .map_err(|()| format!("Location URI is not a file path: {}", loc.uri))?;

        let rel = file_path
            .strip_prefix(&self.workspace_root)
            .map_err(|_| {
                format!(
                    "Location path '{}' is outside workspace root '{}'",
                    file_path.display(),
                    self.workspace_root.display()
                )
            })?
            .to_string_lossy()
            .to_string();

        Ok(LocationKey {
            file: normalize_path(&rel),
            line: loc.range.start.line,
            character: loc.range.start.character,
        })
    }

    /// Normalize raw LSP locations into a deduplicated, order-independent key set.
    pub fn locations_to_keys(
        &self,
        locations: &[Location],
    ) -> Result<BTreeSet<LocationKey>, String> {
        let mut keys = BTreeSet::new();
        for loc in locations {
            keys.insert(self.location_to_key(loc)?);
        }
        Ok(keys)
    }

    /// Convert marker map assertions into canonical key sets.
    ///
    /// Input format:
    /// - vec![("A.java", vec!["decl", "use1"]), ("B.java", vec!["x"])]
    pub fn marker_map_to_keys(
        &self,
        marker_map: &[(String, Vec<String>)],
    ) -> Result<BTreeSet<LocationKey>, String> {
        let mut keys = BTreeSet::new();
        for (file, markers) in marker_map {
            for marker in markers {
                keys.insert(self.marker_location_key(file, marker)?);
            }
        }
        Ok(keys)
    }

    /// Send a hover request
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "^")
    pub fn hover(&mut self, file: &str, marker: &str) -> Result<Option<Hover>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.hover_internal(file_path, marker_info.offset)
    }

    /// Send a find-references request
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "^")
    /// * `include_declaration` - Whether to include the symbol declaration
    pub fn find_references(
        &mut self,
        file: &str,
        marker: &str,
        include_declaration: bool,
    ) -> Result<Vec<Location>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.find_references_internal(file_path, marker_info.offset, include_declaration)
    }

    /// Internal implementation of goto-definition using direct API
    fn goto_definition_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        use hyperion_lsp::navigation::NavigationProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let query_engine = Arc::new(QueryEngine::new((*setup.index).clone()));

        let txn = setup.host.read_now();
        let nav_provider = NavigationProvider::new(
            &txn,
            setup.index,
            query_engine,
            Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
        );

        let position = offset_to_position(&source, offset);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        let location = nav_provider
            .goto_definition(file_id, position, &source, &uri)
            .map_err(|e| format!("goto_definition failed: {e}"))?;

        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    /// Send a goto-implementation request
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "cursor", "target:foo")
    pub fn goto_implementation(
        &mut self,
        file: &str,
        marker: &str,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.goto_implementation_internal(file_path, marker_info.offset)
    }

    /// Internal implementation of goto-implementation using direct API
    fn goto_implementation_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        use hyperion_lsp::navigation::NavigationProvider;
        use hyperion_lsp::type_hierarchy::TypeHierarchyProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let query_engine = Arc::new(QueryEngine::new((*setup.index).clone()));

        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
        let txn = setup.host.read_now();
        let nav_provider = NavigationProvider::new(
            &txn,
            setup.index.clone(),
            query_engine.clone(),
            Arc::clone(&source_providers),
        );

        let type_provider = TypeHierarchyProvider::new(setup.index.clone(), source_providers);

        // Convert offset to LSP Position
        let position = offset_to_position(&source, offset);

        // Get file URI
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        Ok(
            hyperion_lsp::handlers::navigation::resolve_goto_implementation(
                &nav_provider,
                &type_provider,
                file_id,
                position,
                &source,
                &uri,
            ),
        )
    }

    /// Internal implementation of hover using direct API
    fn hover_internal(&self, file_path: PathBuf, offset: usize) -> Result<Option<Hover>, String> {
        use hyperion_lsp::hover::HoverProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
        let txn = setup.host.read_now();
        let hover_provider = HoverProvider::new(&txn, setup.index, source_providers);

        let position = offset_to_position(&source, offset);

        // Call hover
        hover_provider.hover(file_id, position, &source)
    }

    /// Internal implementation of find-references using direct API
    fn find_references_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
        include_declaration: bool,
    ) -> Result<Vec<Location>, String> {
        use hyperion_lsp::navigation::NavigationProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let query_engine = Arc::new(QueryEngine::new((*setup.index).clone()));
        let txn = setup.host.read_now();
        let nav_provider = NavigationProvider::new(
            &txn,
            setup.index,
            query_engine,
            Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new()),
        );

        let position = offset_to_position(&source, offset);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        nav_provider
            .find_references(file_id, position, &source, &uri, include_declaration)
            .map_err(|e| format!("find_references failed: {e}"))
    }

    /// Perform signature help at a marker position
    pub fn signature_help(
        &mut self,
        file: &str,
        marker: &str,
    ) -> Result<Option<SignatureHelp>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.signature_help_internal(file_path, marker_info.offset)
    }

    fn signature_help_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
    ) -> Result<Option<SignatureHelp>, String> {
        use hyperion_lsp::signature_help::SignatureHelpProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let txn = setup.host.read_now();
        let sig_help_provider = SignatureHelpProvider::new(&txn, setup.index);

        let position = offset_to_position(&source, offset);

        sig_help_provider.signature_help(file_id, position, &source)
    }

    /// Get a file from the workspace
    pub fn get_file(&self, path: &str) -> Option<&TestFile> {
        self.files.get(path)
    }

    /// Get all files from the workspace
    pub const fn files(&self) -> &BTreeMap<String, TestFile> {
        &self.files
    }

    /// Get the workspace root (project root) path
    pub fn workspace_root_path(&self) -> PathBuf {
        self.temp_dir.path().join("workspace")
    }

    /// Get the absolute path for a file
    pub fn file_path(&self, path: &str) -> PathBuf {
        self.workspace_root.join(path)
    }

    /// Get the URI for a file
    pub fn file_uri(&self, path: &str) -> Result<Url, String> {
        let path = self.file_path(path);
        Url::from_file_path(path).map_err(|()| "Failed to create URI".to_string())
    }

    /// Send a `prepare_rename` request
    pub fn prepare_rename(
        &mut self,
        file: &str,
        marker: &str,
    ) -> Result<Option<PrepareRenameResponse>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.prepare_rename_internal(file_path, marker_info.offset)
    }

    /// Internal implementation of `prepare_rename`
    fn prepare_rename_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
    ) -> Result<Option<PrepareRenameResponse>, String> {
        use hyperion_lsp::rename::RenameProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let query_engine = Arc::new(QueryEngine::new((*setup.index).clone()));
        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
        let rename_provider = RenameProvider::new(setup.index, query_engine, source_providers);

        let position = offset_to_position(&source, offset);

        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;
        match rename_provider.prepare_rename(file_id, position, &source, &uri)? {
            Some(range) => Ok(Some(PrepareRenameResponse::Range(range))),
            None => Ok(None),
        }
    }

    /// Send a rename request
    pub fn rename(
        &mut self,
        file: &str,
        marker: &str,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        self.rename_internal(file_path, marker_info.offset, new_name, uri)
    }

    /// Internal implementation of rename
    fn rename_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
        new_name: &str,
        uri: Url,
    ) -> Result<Option<WorkspaceEdit>, String> {
        use hyperion_lsp::rename::RenameProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let query_engine = Arc::new(QueryEngine::new((*setup.index).clone()));
        let source_providers = Arc::new(hyperion_lsp::source_provider::SourceProviderChain::new());
        let rename_provider = RenameProvider::new(setup.index, query_engine, source_providers);

        let position = offset_to_position(&source, offset);

        rename_provider
            .rename(file_id, position, new_name, &source, &uri)
            .map(|result| result.map(|result| result.edit))
    }

    /// Send a `code_action` request
    pub fn code_action(
        &mut self,
        file: &str,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Option<Vec<CodeActionOrCommand>>, String> {
        let file_path = self.workspace_root.join(file);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        self.code_action_internal(file_path, range, diagnostics, uri, None, true)
    }

    /// Issue code actions against request bytes that were not published to the
    /// workspace index or semantic database.
    pub fn code_action_with_unindexed_source(
        &mut self,
        file: &str,
        range: Range,
        source: &str,
    ) -> Result<Option<Vec<CodeActionOrCommand>>, String> {
        let file_path = self.workspace_root.join(file);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;
        self.code_action_internal(file_path, range, vec![], uri, Some(source), true)
    }

    /// Exercise the production readiness gate around workspace-wide actions.
    pub fn code_action_with_workspace_ready(
        &mut self,
        file: &str,
        range: Range,
        workspace_ready: bool,
    ) -> Result<Option<Vec<CodeActionOrCommand>>, String> {
        let file_path = self.workspace_root.join(file);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;
        self.code_action_internal(file_path, range, vec![], uri, None, workspace_ready)
    }

    /// Send a completion request
    ///
    /// # Arguments
    /// * `file` - File path relative to workspace
    /// * `marker` - Marker name (e.g., "<|>", "^")
    pub fn completion(&mut self, file: &str, marker: &str) -> Result<Vec<CompletionItem>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;

        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);

        self.completion_internal(file_path, marker_info.offset)
    }

    /// Send a `prepareCallHierarchy` request
    pub fn prepare_call_hierarchy(
        &mut self,
        file: &str,
        position: Position,
    ) -> Result<Option<Vec<CallHierarchyItem>>, String> {
        let file_path = self.workspace_root.join(file);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        self.prepare_call_hierarchy_internal(file_path, position, uri)
    }

    /// Send an `incomingCalls` request
    pub fn incoming_calls(
        &mut self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>, String> {
        self.incoming_calls_internal(item)
    }

    /// Send an `outgoingCalls` request
    pub fn outgoing_calls(
        &mut self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>, String> {
        self.outgoing_calls_internal(item)
    }

    /// Verify that workspace index has core JDK classes
    ///
    /// This can be used to ensure tests requiring JDK classes will work
    pub fn verify_jdk_indexed(
        index: &hyperion_workspace::index::WorkspaceIndex,
    ) -> Result<(), String> {
        if index.has_core_jdk_classes() {
            Ok(())
        } else {
            Err(format!(
                "JDK classes not properly indexed!\n{}",
                index.get_index_diagnostic()
            ))
        }
    }

    /// Internal implementation of `code_action`
    fn code_action_internal(
        &self,
        file_path: PathBuf,
        range: Range,
        diagnostics: Vec<Diagnostic>,
        uri: Url,
        source_override: Option<&str>,
        workspace_ready: bool,
    ) -> Result<Option<Vec<CodeActionOrCommand>>, String> {
        use hyperion_lsp::code_actions::CodeActionProvider;

        let source = source_override.map_or_else(
            || fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}")),
            |source| Ok(source.to_string()),
        )?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let txn = setup.host.read_now();
        let code_action_provider = CodeActionProvider::new(&txn, setup.index);

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        code_action_provider.code_actions_cancellable(
            file_id,
            &params,
            &source,
            workspace_ready,
            || false,
        )
    }

    /// Internal implementation of completion
    fn completion_internal(
        &self,
        file_path: PathBuf,
        offset: usize,
    ) -> Result<Vec<CompletionItem>, String> {
        use hyperion_lsp::completion::CompletionProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let txn = setup.host.read_now();
        let completion_provider = CompletionProvider::new(setup.index);

        let position = offset_to_position(&source, offset);

        completion_provider.complete(&txn, file_id, position)
    }

    /// Internal implementation of prepare_call_hierarchy
    fn prepare_call_hierarchy_internal(
        &self,
        file_path: PathBuf,
        position: Position,
        uri: Url,
    ) -> Result<Option<Vec<CallHierarchyItem>>, String> {
        use hyperion_lsp::call_hierarchy::CallHierarchyProvider;

        let source =
            fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let file_id = setup
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let txn = setup.host.read_now();
        let provider = CallHierarchyProvider::new(&txn, setup.index);

        provider
            .prepare(file_id, position, &source, &uri)
            .map_err(|e| format!("Call hierarchy error: {e}"))
    }

    /// Internal implementation of incoming_calls
    fn incoming_calls_internal(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>, String> {
        use hyperion_lsp::call_hierarchy::CallHierarchyProvider;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let txn = setup.host.read_now();
        let provider = CallHierarchyProvider::new(&txn, setup.index);

        provider
            .incoming_calls(&item)
            .map(|calls| if calls.is_empty() { None } else { Some(calls) })
            .map_err(|e| format!("Incoming calls error: {e}"))
    }

    /// Internal implementation of outgoing_calls
    fn outgoing_calls_internal(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>, String> {
        use hyperion_lsp::call_hierarchy::CallHierarchyProvider;

        let setup =
            setup_workspace_and_db(&self.files, &self.workspace_root, self.temp_dir.path())?;

        let txn = setup.host.read_now();
        let provider = CallHierarchyProvider::new(&txn, setup.index);

        provider
            .outgoing_calls(&item)
            .map(|calls| if calls.is_empty() { None } else { Some(calls) })
            .map_err(|e| format!("Outgoing calls error: {e}"))
    }
}

impl SharedLspContext {
    /// Run the production diagnostics pipeline (parse errors, semantic
    /// resolution, type checking, nullability, inspections; not security)
    /// over one file with the default options.
    pub fn diagnostics(&self, file: &str) -> Result<Vec<Diagnostic>, String> {
        self.diagnostics_with_options(
            file,
            hyperion_lsp::diagnostic_engine::DiagnosticOptions {
                run_security: false,
                ..hyperion_lsp::diagnostic_engine::DiagnosticOptions::default()
            },
        )
    }

    /// Like [`Self::diagnostics`] with explicit pipeline options.
    pub fn diagnostics_with_options(
        &self,
        file: &str,
        options: hyperion_lsp::diagnostic_engine::DiagnosticOptions,
    ) -> Result<Vec<Diagnostic>, String> {
        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;
        let file_path = self.workspace_root.join(file);
        let file_id = self
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;
        let language_id = match hyperion_workspace::Language::from_path(&file_path) {
            Some(hyperion_workspace::Language::Kotlin) => "kotlin",
            _ => "java",
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("tokio runtime: {error}"))?;
        Ok(runtime.block_on(
            hyperion_lsp::diagnostic_engine::collect_diagnostics_for_file(
                file_id,
                language_id,
                &test_file.clean_content,
                &self.host,
                None,
                true,
                options,
            ),
        ))
    }

    /// Issue goto-definition against newer semantic/request bytes without
    /// publishing those bytes to the workspace index.
    pub fn goto_definition_with_unindexed_source(
        &self,
        file: &str,
        source_with_marker: &str,
        marker: &str,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        use hyperion_lsp::navigation::NavigationProvider;

        let source = TestFile::new(source_with_marker);
        let marker_info = source
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;
        let file_path = self.workspace_root.join(file);
        let file_id = self
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;
        self.host
            .with_db(|db| db.set_file_text(file_id, Arc::new(source.clean_content.clone())));

        let position = offset_to_position(&source.clean_content, marker_info.offset);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;
        let txn = self.host.read_now();
        NavigationProvider::new(
            &txn,
            self.index.clone(),
            self.query_engine.clone(),
            self.source_providers.clone(),
        )
        .goto_definition(file_id, position, &source.clean_content, &uri)
        .map(|location| location.map(GotoDefinitionResponse::Scalar))
    }

    /// Send a goto-definition request in a shared workspace.
    pub fn goto_definition(
        &self,
        file: &str,
        marker: &str,
    ) -> Result<Option<GotoDefinitionResponse>, String> {
        use hyperion_lsp::navigation::NavigationProvider;

        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;
        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);
        let file_id = self
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let source = &test_file.clean_content;
        let position = offset_to_position(source, marker_info.offset);
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI".to_string())?;

        let txn = self.host.read_now();
        let nav_provider = NavigationProvider::new(
            &txn,
            self.index.clone(),
            self.query_engine.clone(),
            self.source_providers.clone(),
        );

        let location = nav_provider
            .goto_definition(file_id, position, source, &uri)
            .map_err(|e| format!("goto_definition failed: {e}"))?;

        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    /// Send a hover request in a shared workspace.
    pub fn hover(&self, file: &str, marker: &str) -> Result<Option<Hover>, String> {
        use hyperion_lsp::hover::HoverProvider;

        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;
        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);
        let file_id = self
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let source = &test_file.clean_content;
        let position = offset_to_position(source, marker_info.offset);
        let txn = self.host.read_now();
        let hover_provider =
            HoverProvider::new(&txn, self.index.clone(), self.source_providers.clone());

        hover_provider
            .hover(file_id, position, source)
            .map_err(|e| format!("hover failed: {e}"))
    }

    /// Send a completion request in a shared workspace.
    pub fn completion(&self, file: &str, marker: &str) -> Result<Vec<CompletionItem>, String> {
        self.completion_with_snippets(file, marker, true)
    }

    /// Send a completion request as a client that does (or does not) declare
    /// `completionItem.snippetSupport`.
    pub fn completion_with_snippets(
        &self,
        file: &str,
        marker: &str,
        snippets: bool,
    ) -> Result<Vec<CompletionItem>, String> {
        use hyperion_lsp::completion::CompletionProvider;

        let test_file = self
            .files
            .get(file)
            .ok_or_else(|| format!("File not found: {file}"))?;
        let marker_info = test_file
            .find_marker(marker)
            .ok_or_else(|| format!("Marker not found: {marker}"))?;

        let file_path = self.workspace_root.join(file);
        let file_id = self
            .index
            .get_file_id(&file_path)
            .ok_or_else(|| format!("File not indexed: {file_path:?}"))?;

        let source = &test_file.clean_content;
        let position = offset_to_position(source, marker_info.offset);

        let completion_provider = CompletionProvider::new(self.index.clone());
        completion_provider.set_snippet_support(snippets);
        let txn = self.host.read_now();
        completion_provider
            .complete(&txn, file_id, position)
            .map_err(|e| format!("completion failed: {e}"))
    }
}

/// Normalize goto-definition response into plain locations.
pub fn goto_response_to_locations(response: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match response {
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Extract plain text from hover contents for macro-level assertions.
pub fn hover_to_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Scalar(scalar) => match scalar {
            MarkedString::String(s) => s.clone(),
            MarkedString::LanguageString(ls) => ls.value.clone(),
        },
        HoverContents::Array(arr) => arr
            .iter()
            .map(|s| match s {
                MarkedString::String(v) => v.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Render location key sets for readable assertion failures.
pub fn format_location_keys(keys: &BTreeSet<LocationKey>) -> String {
    if keys.is_empty() {
        return "  <none>".to_string();
    }
    keys.iter()
        .map(|k| format!("  - {}", k))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert byte offset to LSP Position (line, character)
fn offset_to_position(text: &str, offset: usize) -> Position {
    // Use the same function as navigation.rs to ensure consistency
    hyperion_lsp::position_utils::offset_to_position(text, offset)
}

/// Parse markers from source code
///
/// Supported markers:
/// - `<|>` - Cursor position (named "<|>")
/// - `^` - Cursor position on the previous line (named "^")
/// - `/*name*/` - Named target
/// - `/*target:name*/` - Named target with explicit prefix
///
/// Returns clean content and list of markers with positions
pub fn parse_markers(content: &str) -> (String, Vec<MarkerInfo>) {
    let mut clean = String::new();
    let mut markers = Vec::new();
    let mut line = 0u32;
    let mut character = 0u32;
    let mut offset = 0usize;

    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        // Check for guillemet markers (e.g., «decl», «use1», «|»)
        if ch == '«' {
            let marker_start_offset = offset;
            let marker_start_line = line;
            let marker_start_char = character;

            let mut marker_content = String::new();
            let mut found_close = false;

            while let Some(c) = chars.peek() {
                if *c == '»' {
                    chars.next(); // consume closing delimiter
                    found_close = true;
                    break;
                } else if *c == '\n' || *c == '«' {
                    // Not a valid marker if multiline or nested.
                    break;
                }
                marker_content.push(chars.next().unwrap());
            }

            if found_close && !marker_content.trim().is_empty() {
                let marker_name = format!("«{}»", marker_content.trim());
                markers.push(MarkerInfo {
                    name: marker_name,
                    line: marker_start_line,
                    character: marker_start_char,
                    offset: marker_start_offset,
                });
                continue;
            }

            // Not a valid marker, restore consumed text.
            clean.push('«');
            clean.push_str(&marker_content);
            offset += '«'.len_utf8() + marker_content.len();
            character += 1 + marker_content.chars().count() as u32;
            if found_close {
                clean.push('»');
                offset += '»'.len_utf8();
                character += 1;
            }
            continue;
        }

        // Check for <...> marker (e.g., <|>, <|1>, <def>, <ref>)
        if ch == '<' {
            let marker_start_offset = offset;
            let marker_start_line = line;
            let marker_start_char = character;

            // Collect marker content until '>'
            let mut marker_content = String::new();
            let mut found_close = false;

            while let Some(c) = chars.peek() {
                if *c == '>' {
                    chars.next(); // consume '>'
                    found_close = true;
                    break;
                } else if *c == '\n' || *c == ' ' || *c == '<' {
                    // Not a marker if it contains newline, space, or nested '<'
                    // (nested '<' indicates this is a generic type, not a marker)
                    break;
                }
                marker_content.push(chars.next().unwrap());
            }

            // Valid marker if we found '>' and content matches marker patterns
            // Markers are: <|>, <|N>, <name> where name starts with lowercase or |
            // Generic types like <User> are NOT markers (start with uppercase)
            let is_marker = found_close
                && !marker_content.is_empty()
                && (
                    marker_content.starts_with('|') ||  // <|> or <|1>
                marker_content.chars().next().is_some_and(char::is_lowercase)
                    // <def>, <ref>
                );

            if is_marker {
                let marker_name = format!("<{marker_content}>");
                markers.push(MarkerInfo {
                    name: marker_name,
                    line: marker_start_line,
                    character: marker_start_char,
                    offset: marker_start_offset,
                });
                continue;
            }
            // Not a marker, restore '<' and marker_content
            clean.push('<');
            clean.push_str(&marker_content);
            offset += 1 + marker_content.len();
            character += 1 + marker_content.len() as u32;
            if found_close {
                clean.push('>');
                offset += 1;
                character += 1;
            }
            continue;
        }

        // Check for /* ... */ comments that might be markers
        if ch == '/' && chars.peek() == Some(&'*') {
            let comment_start = offset;
            let comment_line = line;
            let comment_char = character;

            chars.next(); // consume '*'
            let mut comment_content = String::new();

            // Read until */
            let mut found_end = false;
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next(); // consume '/'
                    found_end = true;
                    break;
                }
                comment_content.push(c);
            }

            if found_end {
                let trimmed = comment_content.trim();

                // Marker comments are compact symbolic labels. Preserve
                // Javadocs and ordinary prose comments in the test source;
                // stripping every non-empty block comment made comment-aware
                // LSP tests exercise different bytes than production.
                let is_symbolic_marker = !trimmed.is_empty()
                    && trimmed.chars().all(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, '_' | '-' | ':' | '.')
                    });
                if trimmed.starts_with("target:") || is_symbolic_marker {
                    markers.push(MarkerInfo {
                        name: trimmed.to_string(),
                        line: comment_line,
                        character: comment_char,
                        offset: comment_start,
                    });
                    // Don't include marker comments in clean content
                    continue;
                }
            }

            // Not a marker comment, include it
            clean.push_str("/*");
            clean.push_str(&comment_content);
            if found_end {
                clean.push_str("*/");
            }

            offset = clean.len();
            character += comment_content.len() as u32 + 4;
            continue;
        }

        // Check for ^ marker (on its own line)
        if ch == '^' {
            // Check if it's standalone (line only has whitespace before it)
            let line_start = clean.rfind('\n').map_or(0, |i| i + 1);
            let before = &clean[line_start..];
            if before.trim().is_empty() {
                // Calculate the offset for the position on the previous line
                // Find the start of the previous line
                let prev_line_end = line_start.saturating_sub(1); // Position of '\n' before current line
                let prev_line_start = if prev_line_end > 0 {
                    clean[..prev_line_end].rfind('\n').map_or(0, |i| i + 1)
                } else {
                    0
                };

                // Calculate offset on previous line at the same column
                let target_offset = prev_line_start + character as usize;
                // Clamp to end of previous line
                let target_offset = target_offset.min(prev_line_end);

                markers.push(MarkerInfo {
                    name: "^".into(),
                    line: line.saturating_sub(1), // Point to previous line
                    character,                    // Column where ^ appears
                    offset: target_offset,
                });
                // Skip the rest of the line
                while let Some(&c) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }

        // Regular character
        clean.push(ch);
        offset += ch.len_utf8();

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    (clean, markers)
}

/// Assert that goto-definition result points to expected target
#[macro_export]
macro_rules! assert_goto_target {
    ($test:expr, $result:expr, $expected_file:expr, $expected_marker:expr) => {{
        let result = $result.expect("goto_definition returned error");
        assert!(
            result.is_some(),
            "Expected goto-definition result, got None"
        );

        let location = match result.unwrap() {
            GotoDefinitionResponse::Scalar(loc) => loc,
            GotoDefinitionResponse::Array(locs) => {
                assert!(!locs.is_empty(), "Expected non-empty location array");
                locs[0].clone()
            }
            GotoDefinitionResponse::Link(_) => panic!("Unexpected link response"),
        };

        // Check file
        let expected_uri = format!("file://{}", $expected_file);
        assert!(
            location.uri.as_str().ends_with($expected_file),
            "Expected file {}, got {}",
            expected_uri,
            location.uri
        );

        let expected = $test
            .get_marker_position($expected_file, $expected_marker)
            .unwrap_or_else(|| {
                panic!(
                    "marker {} not found in {}",
                    $expected_marker, $expected_file
                )
            });
        assert_eq!(
            location.range.start, expected,
            "goto-definition should land on marker {}",
            $expected_marker
        );
    }};
}

/// Assert that goto-definition returns None
#[macro_export]
macro_rules! assert_no_definition {
    ($result:expr) => {{
        let result = $result.expect("goto_definition returned error");
        assert!(
            result.is_none(),
            "Expected no definition, but got: {:?}",
            result
        );
    }};
}

/// Index JDK classes using cache if available (for test performance)
///
/// This function tries to load JDK symbols from the persistent cache first.
/// If cache is unavailable or invalid, falls back to bytecode indexing.
///
/// Returns the number of symbols indexed.
fn index_jdk_with_cache(
    index: &hyperion_workspace::index::WorkspaceIndex,
) -> Result<usize, String> {
    let total_start = std::time::Instant::now();

    // Try to detect JDK
    let jdk_info = match detect_jdk_for_test() {
        Ok(info) => info,
        Err(e) => {
            return Err(format!("Failed to detect JDK: {}", e));
        }
    };

    // Determine cache key
    let version_str = match jdk_info.version {
        8 => "8".into(),
        v => v.to_string(),
    };

    // Fast path: process-local in-memory cache avoids repeated disk reads +
    // deserialization in long-running test binaries.
    if let Ok(cache) = jdk_symbols_memory_cache().read() {
        if let Some(cached) = cache.get(&version_str).cloned() {
            let symbol_count = cached.symbols.len();
            index.add_external_index_data_responsive(
                cached.symbols.clone(),
                cached.generic_supertypes.clone(),
            );
            let total_time = total_start.elapsed();
            eprintln!(
                "TEST JDK memory cache HIT - Loaded {} symbols in {:.2}ms",
                symbol_count,
                total_time.as_secs_f64() * 1000.0
            );
            return Ok(symbol_count);
        }
    }

    // Singleflight within this process: prevent parallel test threads from all
    // loading/deserializing the same cache simultaneously when memory cache is cold.
    let singleflight_guard = jdk_cache_singleflight()
        .lock()
        .map_err(|_| "Failed to lock JDK cache singleflight mutex".to_string())?;

    // Another thread may have filled the memory cache while we waited for lock.
    let post_lock_cached = jdk_symbols_memory_cache()
        .read()
        .ok()
        .and_then(|cache| cache.get(&version_str).cloned());
    if let Some(cached) = post_lock_cached {
        // Keep the critical section small: release the singleflight lock before
        // doing per-index insertion work.
        drop(singleflight_guard);
        let symbol_count = cached.symbols.len();
        index.add_external_index_data_responsive(
            cached.symbols.clone(),
            cached.generic_supertypes.clone(),
        );
        let total_time = total_start.elapsed();
        eprintln!(
            "TEST JDK memory cache HIT (post-lock) - Loaded {} symbols in {:.2}ms",
            symbol_count,
            total_time.as_secs_f64() * 1000.0
        );
        return Ok(symbol_count);
    }

    // Try to load from cache
    let cache_dir = test_cache_root().join(format!("jdk-{}", version_str));

    let symbols_path = cache_dir.join("symbols.bin");
    let metadata_path = cache_dir.join("metadata.json");

    let cache_metadata = std::fs::read(&metadata_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<JdkCacheMetadata>(&bytes).ok());
    let cache_is_current = cache_metadata.is_some_and(|metadata| {
        metadata.version == hyperion_lsp::jdk_cache::JDK_CACHE_VERSION
            && metadata.jdk_version == version_str
            && metadata.java_home == jdk_info._java_home
    });

    if symbols_path.exists() && cache_is_current {
        let _cache_lock = match lock_cache_shared(&cache_dir) {
            Ok(lock) => Some(lock),
            Err(e) => {
                eprintln!(
                    "TEST JDK cache lock failed (continuing without lock): {}",
                    e
                );
                None
            }
        };
        let cache_start = std::time::Instant::now();
        // Try to load from cache
        if let Ok(bytes) = std::fs::read(&symbols_path) {
            if let Ok(data) = bincode::deserialize::<JdkCacheData>(&bytes) {
                // Use chunked parallel deserialization for better performance
                use rayon::prelude::*;
                let symbols: Vec<_> = data
                    .chunks
                    .par_iter()
                    .flat_map(|chunk| {
                        chunk
                            .iter()
                            .map(hyperion_workspace::cache::cached_to_symbol)
                            .collect::<Vec<_>>()
                    })
                    .collect();

                let symbol_count = symbols.len();

                // Store in process-local cache for subsequent test cases.
                if let Ok(mut cache) = jdk_symbols_memory_cache().write() {
                    cache.insert(
                        version_str.clone(),
                        Arc::new(JdkIndexData {
                            symbols: symbols.clone(),
                            generic_supertypes: data.generic_supertypes.clone(),
                        }),
                    );
                }

                // Match production: bounded batches keep test completion
                // responsive while the cached JDK is restored.
                index.add_external_index_data_responsive(symbols, data.generic_supertypes);

                let cache_time = cache_start.elapsed();
                let total_time = total_start.elapsed();
                eprintln!(
                    "TEST JDK cache HIT - Loaded {} symbols in {:.2}ms (total: {:.2}ms) [23x faster than bytecode indexing]",
                    symbol_count,
                    cache_time.as_secs_f64() * 1000.0,
                    total_time.as_secs_f64() * 1000.0
                );
                return Ok(symbol_count);
            }
        }
    }

    // Cache miss - fall back to bytecode indexing
    eprintln!(
        "TEST JDK cache MISS - cache not found at {:?}, indexing from bytecode...",
        cache_dir
    );

    let index_start = std::time::Instant::now();
    let result = hyperion_workspace::jdk_integration::index_jdk_classes(index)
        .map_err(|e| format!("JDK indexing failed: {}", e))?;
    let index_time = index_start.elapsed();

    // Populate process-local cache from freshly indexed symbols.
    let (indexed_symbols, generic_supertypes) = index.get_external_index_data();
    if !indexed_symbols.is_empty() {
        if let Ok(mut cache) = jdk_symbols_memory_cache().write() {
            cache.insert(
                version_str.clone(),
                Arc::new(JdkIndexData {
                    symbols: indexed_symbols,
                    generic_supertypes,
                }),
            );
        }
    }

    eprintln!(
        "TEST JDK indexed {} classes from bytecode in {:.2}ms",
        result,
        index_time.as_secs_f64() * 1000.0
    );

    // Save to cache for next time
    let save_start = std::time::Instant::now();
    if let Err(e) = save_jdk_cache(index, &version_str, &jdk_info._java_home) {
        eprintln!("Warning: Failed to save JDK cache: {}", e);
    } else {
        let save_time = save_start.elapsed();
        eprintln!(
            "TEST JDK cache saved in {:.2}ms at {:?}",
            save_time.as_secs_f64() * 1000.0,
            cache_dir
        );
    }

    let total_time = total_start.elapsed();
    eprintln!(
        "TEST JDK indexing complete in {:.2}ms (index: {:.2}ms + save: {:.2}ms)",
        total_time.as_secs_f64() * 1000.0,
        index_time.as_secs_f64() * 1000.0,
        save_start.elapsed().as_secs_f64() * 1000.0
    );

    Ok(result)
}

/// Use a test-local cache directory by default.
///
/// The host cache dir (e.g. ~/Library/Caches) may not be writable in sandboxed
/// environments, which would make tests flaky or fail outright.
fn test_cache_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("HYPERION_TEST_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    // Stable path so multiple test binaries can reuse the same JDK cache.
    std::env::temp_dir().join("hyperion-test-cache")
}

fn jdk_cache_singleflight() -> &'static std::sync::Mutex<()> {
    static SINGLEFLIGHT: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    SINGLEFLIGHT.get_or_init(|| std::sync::Mutex::new(()))
}

fn jdk_symbols_memory_cache() -> &'static std::sync::RwLock<HashMap<String, Arc<JdkIndexData>>> {
    static CACHE: std::sync::OnceLock<std::sync::RwLock<HashMap<String, Arc<JdkIndexData>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(HashMap::new()))
}

struct JdkIndexData {
    symbols: Vec<hyperion_workspace::Symbol>,
    generic_supertypes: Vec<hyperion_workspace::TypeGenericSupertypes>,
}

/// Detect the JDK with the same rules as the production server.
fn detect_jdk_for_test() -> Result<JdkTestInfo, String> {
    let info = hyperion_lsp::jdk_indexer::detect_jdk()
        .map_err(|error| format!("Could not detect JDK: {error}"))?;
    let version = match info.version {
        hyperion_lsp::jdk_indexer::JavaVersion::Java8 => 8,
        hyperion_lsp::jdk_indexer::JavaVersion::Java9Plus(major) => major,
    };

    Ok(JdkTestInfo {
        _java_home: info.java_home,
        version,
    })
}

struct JdkTestInfo {
    _java_home: PathBuf,
    version: u8,
}

/// Save JDK symbols to cache (for test suite optimization)
fn save_jdk_cache(
    index: &hyperion_workspace::index::WorkspaceIndex,
    jdk_version: &str,
    java_home: &std::path::Path,
) -> Result<(), String> {
    let cache_dir = test_cache_root().join(format!("jdk-{}", jdk_version));

    // Ensure cache directory exists
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;

    let _cache_lock = match lock_cache_exclusive(&cache_dir) {
        Ok(lock) => Some(lock),
        Err(e) => {
            eprintln!(
                "Warning: Failed to acquire JDK cache lock (continuing without lock): {}",
                e
            );
            None
        }
    };

    // Get all external symbols and generic hierarchy edges (JDK index data).
    let (symbols, generic_supertypes) = index.get_external_index_data();

    // Convert to cached format
    let cached_symbols: Vec<hyperion_workspace::CachedSymbol> = symbols
        .iter()
        .map(|s| hyperion_workspace::cache::symbol_to_cached(s, s.range.start))
        .collect();

    // Split into chunks for parallel deserialization (matching jdk_cache.rs format)
    const SYMBOLS_PER_CHUNK: usize = 1000;
    let chunks: Vec<Vec<hyperion_workspace::CachedSymbol>> = cached_symbols
        .chunks(SYMBOLS_PER_CHUNK)
        .map(|chunk| chunk.to_vec())
        .collect();

    let chunk_count = chunks.len();
    let data = JdkCacheData {
        chunks,
        generic_supertypes,
    };

    // Save symbols (atomic write)
    let symbols_path = cache_dir.join("symbols.bin");
    let temp_path = symbols_path.with_extension("tmp");

    let bytes = bincode::serialize(&data).map_err(|e| format!("Failed to serialize: {}", e))?;

    std::fs::write(&temp_path, &bytes).map_err(|e| format!("Failed to write: {}", e))?;

    std::fs::rename(&temp_path, &symbols_path).map_err(|e| format!("Failed to rename: {}", e))?;

    // Save metadata
    let metadata = JdkCacheMetadata {
        version: hyperion_lsp::jdk_cache::JDK_CACHE_VERSION,
        jdk_version: jdk_version.to_string(),
        java_home: java_home.to_path_buf(),
        symbol_count: symbols.len(),
        chunk_count,
    };

    let metadata_path = cache_dir.join("metadata.json");
    let metadata_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

    std::fs::write(&metadata_path, metadata_json)
        .map_err(|e| format!("Failed to write metadata: {}", e))?;

    Ok(())
}

fn lock_cache_shared(cache_dir: &std::path::Path) -> Result<File, String> {
    lock_cache_with(cache_dir, |file| file.lock_shared())
}

fn lock_cache_exclusive(cache_dir: &std::path::Path) -> Result<File, String> {
    lock_cache_with(cache_dir, |file| file.lock_exclusive())
}

fn lock_cache_with<F>(cache_dir: &std::path::Path, lock_fn: F) -> Result<File, String>
where
    F: FnOnce(&File) -> io::Result<()>,
{
    std::fs::create_dir_all(cache_dir)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;
    let lock_path = cache_dir.join(".lock");
    let file = File::create(lock_path).map_err(|e| format!("Failed to open cache lock: {}", e))?;
    lock_fn(&file).map_err(|e| format!("Failed to lock cache: {}", e))?;
    Ok(file)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JdkCacheMetadata {
    version: u32,
    jdk_version: String,
    java_home: PathBuf,
    symbol_count: usize,
    chunk_count: usize,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct JdkCacheData {
    chunks: Vec<Vec<hyperion_workspace::CachedSymbol>>,
    generic_supertypes: Vec<hyperion_workspace::TypeGenericSupertypes>,
}
