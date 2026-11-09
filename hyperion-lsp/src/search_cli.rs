// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Headless semantic search for the CLI (`hyperion sym|refs|def|impls|grep`).
//!
//! Grep-shaped questions about Java code are usually symbol-shaped questions
//! in disguise: "who uses `CoreEvent.LOGIN`", "which enum declares `LOGIN`",
//! "what method encloses this match". This module answers them from the
//! workspace index without a running LSP session.
//!
//! Two cost tiers:
//! - `sym`/`refs`/`def`/`impls` build the full two-pass workspace index
//!   (declarations, then usages) via [`SearchSession::open`].
//! - `grep` never touches the index: it scans source files and parses only
//!   the files that contain a match, to annotate each hit with its enclosing
//!   declaration chain. Enclosure is computed from the syntax tree because
//!   indexed symbol ranges cover identifiers, not declaration bodies.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_workspace::scanner::ScannerBuilder;
use hyperion_workspace::{is_type_declaration_kind, Language, SymbolLocation, WorkspaceIndex};
use serde::Serialize;
use tower_lsp::lsp_types::SymbolKind;

/// One search result row, shared by every subcommand.
///
/// `line`/`col` are 1-based. `path` is relative to the workspace root when
/// the file lives under it.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub path: String,
    pub line: u32,
    pub col: u32,
    /// Source line the hit is on, trimmed.
    pub text: String,
    /// Enclosing declaration chain, e.g. `CoreEventHandler#receiveEvent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fqn: Option<String>,
}

/// Result of a query that had to resolve a name first.
#[derive(Debug)]
pub enum Outcome {
    Hits(Vec<Hit>),
    /// The query matched several unrelated declarations; rows describe the
    /// candidates so the caller can re-run with a qualified name.
    Ambiguous(Vec<Hit>),
    /// A complete exact reference set could not be proven.
    Unavailable(String),
}

/// A headless workspace session: root + fully built two-pass index.
pub struct SearchSession {
    index: Arc<WorkspaceIndex>,
    root: PathBuf,
    /// Per-file cache of source text, line starts, and (for Java) parse trees.
    files: std::cell::RefCell<HashMap<PathBuf, Rc<FileView>>>,
}

impl SearchSession {
    /// Scan and index the workspace under `root` (declarations + usages).
    // Preserve the public async constructor contract, even while indexing
    // currently completes synchronously.
    #[allow(clippy::unused_async_trait_impl)]
    pub async fn open(root: PathBuf) -> Result<Self> {
        let (index, root) = tokio::task::spawn_blocking(move || -> Result<_> {
            let root = root
                .canonicalize()
                .with_context(|| format!("workspace root not found: {}", root.display()))?;
            let index = Arc::new(WorkspaceIndex::new(root.clone()));
            index.set_root(root.clone());
            let stats = index.build().context("failed to index workspace")?;
            eprintln!(
                "indexed {} files, {} symbols",
                stats.files_indexed, stats.symbols_indexed
            );
            Ok((index, root))
        })
        .await
        .context("workspace indexing task failed")??;
        Ok(Self {
            index,
            root,
            files: std::cell::RefCell::new(HashMap::new()),
        })
    }

    /// `sym NAME`: list every declaration with this simple name.
    pub fn sym(&self, name: &str) -> Vec<Hit> {
        let mut locations = self.index.find_by_simple_name(name);
        dedup_locations(&mut locations);
        locations
            .iter()
            .filter_map(|loc| self.hit_for_declaration(loc))
            .collect()
    }

    /// `def QUERY`: resolve a (possibly qualified) name to declarations.
    pub fn def(&self, query: &str) -> Outcome {
        let locations = self.resolve_declarations(query);
        let hits = locations
            .iter()
            .filter_map(|loc| self.hit_for_declaration(loc))
            .collect();
        Outcome::Hits(hits)
    }

    /// `refs QUERY`: find usages of the declaration `query` resolves to.
    ///
    /// Overloads of one method are treated as a single target. Distinct
    /// declarations (e.g. `LOGIN` in two different enums) are ambiguous.
    pub fn refs(&self, query: &str) -> Outcome {
        let locations = self.resolve_declarations(query);
        let mut bases: Vec<&str> = locations
            .iter()
            .map(|loc| fqn_base(&loc.fqn))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let method_family = locations
            .iter()
            .all(|location| matches!(location.kind, SymbolKind::METHOD | SymbolKind::CONSTRUCTOR));

        if bases.len() > 1 || (locations.len() > 1 && !method_family) {
            let candidates = locations
                .iter()
                .filter_map(|loc| self.hit_for_declaration(loc))
                .collect();
            return Outcome::Ambiguous(candidates);
        }
        let Some(base) = bases.pop() else {
            return Outcome::Hits(Vec::new());
        };

        let usages = match if method_family {
            self.index.find_current_method_family_usages(base)
        } else {
            self.index.find_current_usages(base)
        } {
            Ok(usages) => usages,
            Err(error) => return Outcome::Unavailable(error.to_string()),
        };
        let mut hits = Vec::new();
        for file in &usages.files {
            let Some(indexed) = self.index.get_indexed_revision_content(file.revision) else {
                return Outcome::Unavailable(
                    "workspace changed while reference hits were rendered".to_string(),
                );
            };
            let view = FileView::new(&indexed.file_path, indexed.content.as_str().to_string());
            let display = display_path(&indexed.file_path, &self.root);
            for location in &file.locations {
                let (line, col) = view.lines.line_col(location.range.start);
                hits.push(Hit {
                    path: display.clone(),
                    line,
                    col,
                    text: view.lines.line_text(&view.source, line).trim().to_string(),
                    enclosing: view
                        .tree
                        .as_ref()
                        .and_then(|tree| enclosing_chain(tree, location.range.start)),
                    kind: None,
                    fqn: None,
                });
            }
        }
        if self.index.current_usage_locations_are_current(&usages) {
            Outcome::Hits(hits)
        } else {
            Outcome::Unavailable("workspace changed while reference hits were rendered".to_string())
        }
    }

    /// `impls QUERY`: implementations/subtypes of an interface or class.
    pub fn impls(&self, query: &str) -> Outcome {
        let types: Vec<SymbolLocation> = self
            .resolve_declarations(query)
            .into_iter()
            .filter(|loc| is_type_declaration_kind(loc.kind))
            .collect();

        if types.len() > 1 {
            let candidates = types
                .iter()
                .filter_map(|loc| self.hit_for_declaration(loc))
                .collect();
            return Outcome::Ambiguous(candidates);
        }

        // Supertypes outside the workspace (dependency JARs) have no indexed
        // declaration, but implementing classes still carry the resolved
        // supertype FQN - fall back to matching it by simple name.
        let implementations = types.first().map_or_else(
            || {
                let simple = query.rsplit('.').next().unwrap_or(query);
                self.index.find_implementations_by_simple_name(simple)
            },
            |target| self.index.find_implementations(&target.fqn),
        );

        let mut hits = Vec::new();
        for symbol in implementations {
            if let Some(loc) = self.index.find_symbol(&symbol.fqn) {
                if let Some(hit) = self.hit_for_declaration(&loc) {
                    hits.push(hit);
                }
            }
        }
        Outcome::Hits(hits)
    }

    /// Resolve a query string to declaration locations.
    ///
    /// Tried in order: exact FQN, `Type.member` with a simple type name
    /// (e.g. `CoreEvent.LOGIN`), then bare simple name.
    fn resolve_declarations(&self, query: &str) -> Vec<SymbolLocation> {
        if let Some(loc) = self.index.find_symbol(query) {
            return vec![loc];
        }

        if let Some((type_name, member)) = query.rsplit_once('.') {
            let simple_type = type_name.rsplit('.').next().unwrap_or(type_name);
            let mut out = Vec::new();
            for type_loc in self
                .index
                .find_type_declarations_by_simple_name(simple_type)
            {
                // Honor any extra qualification the user gave (Outer.Inner).
                if !fqn_ends_with_path(&type_loc.fqn, type_name) {
                    continue;
                }
                let member_prefix = format!("{}.", type_loc.fqn);
                out.extend(
                    self.index
                        .find_by_simple_name(member)
                        .into_iter()
                        .filter(|loc| loc.fqn.starts_with(&member_prefix)),
                );
            }
            if !out.is_empty() {
                dedup_locations(&mut out);
                return out;
            }
        }

        let mut out = self.index.find_by_simple_name(query);
        dedup_locations(&mut out);
        out
    }

    fn hit_for_declaration(&self, loc: &SymbolLocation) -> Option<Hit> {
        let mut hit = self.hit_at(loc)?;
        hit.kind = Some(kind_label(loc.kind));
        hit.fqn = Some(loc.fqn.to_string());
        hit.enclosing = None;
        Some(hit)
    }

    fn hit_at(&self, loc: &SymbolLocation) -> Option<Hit> {
        let path = self.index.get_file_path(loc.file_id)?;
        let view = self.file_view(&path)?;
        let (line, col) = view.lines.line_col(loc.range.start);
        Some(Hit {
            path: display_path(&path, &self.root),
            line,
            col,
            text: view.lines.line_text(&view.source, line).trim().to_string(),
            enclosing: view
                .tree
                .as_ref()
                .and_then(|tree| enclosing_chain(tree, loc.range.start)),
            kind: None,
            fqn: None,
        })
    }

    fn file_view(&self, path: &Path) -> Option<Rc<FileView>> {
        if let Some(view) = self.files.borrow().get(path) {
            return Some(Rc::clone(view));
        }
        let source = hyperion_workspace::read_source_file(path).ok()?;
        let view = Rc::new(FileView::new(path, source));
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), Rc::clone(&view));
        Some(view)
    }
}

/// Cached per-file state: source, line starts, and a Java parse tree when the
/// file is Java (used for enclosing-declaration annotation).
struct FileView {
    source: String,
    lines: LineIndex,
    tree: Option<SyntaxNode>,
}

impl FileView {
    fn new(path: &Path, source: String) -> Self {
        let tree = (Language::from_path(path) == Some(Language::Java)).then(|| parse_java(&source));
        let lines = LineIndex::new(&source);
        Self {
            source,
            lines,
            tree,
        }
    }
}

fn parse_java(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green)
}

/// `grep PATTERN`: regex search over workspace sources, each hit annotated
/// with its enclosing declaration chain. Index-free by design: only files
/// containing a match are parsed.
pub fn grep(root: &Path, pattern: &str, annotate: bool) -> Result<Vec<Hit>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("workspace root not found: {}", root.display()))?;
    let regex = regex::Regex::new(pattern).context("invalid regex pattern")?;
    let files = ScannerBuilder::new(root.clone())
        .scan()
        .context("failed to scan workspace")?;

    let mut hits = Vec::new();
    for path in files {
        let Ok(source) = hyperion_workspace::read_source_file(&path) else {
            continue;
        };
        if !regex.is_match(&source) {
            continue;
        }
        let tree = (annotate && Language::from_path(&path) == Some(Language::Java))
            .then(|| parse_java(&source));
        let display = display_path(&path, &root);
        // Line starts must be measured against the real source. Accumulating
        // `line.len() + 1` across `str::lines()` drifts one byte per line on
        // CRLF files, because `lines()` strips the `\r` - and the drifted
        // offset then annotates the hit with the wrong enclosing declaration.
        let lines = LineIndex::new(&source);

        for (line_index, line) in source.lines().enumerate() {
            let line_start = lines.line_start(line_index);
            for found in regex.find_iter(line) {
                let offset = line_start + offset_u32(found.start());
                hits.push(Hit {
                    path: display.clone(),
                    line: offset_u32(line_index) + 1,
                    col: offset_u32(found.start()) + 1,
                    text: line.trim().to_string(),
                    enclosing: tree.as_ref().and_then(|tree| enclosing_chain(tree, offset)),
                    kind: None,
                    fqn: None,
                });
            }
        }
    }
    Ok(hits)
}

/// Walk up from the token at `offset` and build `Outer.Inner#method`.
///
/// Symbol ranges in the index cover identifiers only (goto-definition
/// semantics), so enclosure must come from the syntax tree.
fn enclosing_chain(root: &SyntaxNode, offset: u32) -> Option<String> {
    let token = root
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()?;
    let mut type_names: Vec<String> = Vec::new();
    let mut method: Option<String> = None;

    let mut node = token.parent();
    while let Some(current) = node {
        let kind = current.kind();
        if kind.is_type_declaration() {
            if let Some(name) = decl_name(&current) {
                type_names.push(name);
            }
        } else if matches!(kind, SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL)
            && method.is_none()
        {
            method = decl_name(&current);
        }
        node = current.parent();
    }

    type_names.reverse();
    let mut chain = type_names.join(".");
    match (chain.is_empty(), method) {
        (true, None) => None,
        (true, Some(name)) => Some(format!("#{name}")),
        (false, None) => Some(chain),
        (false, Some(name)) => {
            let _ = write!(chain, "#{name}");
            Some(chain)
        }
    }
}

/// Name of a declaration node: its first direct `IDENT` token child.
///
/// Return types and parameter lists are child *nodes*, so for methods,
/// constructors, and type declarations the first direct token identifier is
/// the declared name.
fn decl_name(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
}

/// Strip a method signature's parameter list: `Foo.bar(String)` → `Foo.bar`.
fn fqn_base(fqn: &str) -> &str {
    fqn.find('(').map_or(fqn, |idx| &fqn[..idx])
}

/// Whether `fqn` ends with the dotted path `suffix` on a `.` boundary
/// (e.g. `com.acme.Outer.Inner` ends with `Outer.Inner`).
fn fqn_ends_with_path(fqn: &str, suffix: &str) -> bool {
    fqn.strip_suffix(suffix)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

fn dedup_locations(locations: &mut Vec<SymbolLocation>) {
    locations.sort_by(|a, b| {
        (a.file_id.0, a.range.start, a.range.end, &a.fqn).cmp(&(
            b.file_id.0,
            b.range.start,
            b.range.end,
            &b.fqn,
        ))
    });
    locations.dedup_by(|a, b| a.file_id == b.file_id && a.range == b.range && a.fqn == b.fqn);
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Clamps rather than wraps: a value past `u32::MAX` must not point at a different
/// position.
fn offset_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

const fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::CLASS => "class",
        SymbolKind::INTERFACE => "interface",
        SymbolKind::ENUM => "enum",
        SymbolKind::ENUM_MEMBER => "enum-member",
        SymbolKind::METHOD => "method",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRUCT => "record",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::PACKAGE => "package",
        SymbolKind::TYPE_PARAMETER => "type-parameter",
        _ => "symbol",
    }
}

/// 1-based line/column lookup over byte offsets.
struct LineIndex {
    line_starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (idx, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset_u32(idx) + 1);
            }
        }
        Self { line_starts }
    }

    /// Byte offset where the 0-based `line_index` starts.
    ///
    /// Always in range for `0..text.lines().count()`: `line_starts` holds one
    /// entry per `\n` plus a leading `0`, which is exactly one entry per line
    /// yielded by [`str::lines`] (plus a trailing entry when the text ends in
    /// a newline).
    fn line_start(&self, line_index: usize) -> u32 {
        self.line_starts.get(line_index).copied().unwrap_or(0)
    }

    /// Both components 1-based; column is a byte column.
    fn line_col(&self, offset: u32) -> (u32, u32) {
        let line_idx = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line_idx];
        (offset_u32(line_idx) + 1, col + 1)
    }

    fn line_text<'a>(&self, text: &'a str, line: u32) -> &'a str {
        let idx = line.saturating_sub(1) as usize;
        let Some(start) = self.line_starts.get(idx) else {
            return "";
        };
        let start = *start as usize;
        let end = self
            .line_starts
            .get(idx + 1)
            .map_or(text.len(), |next| (*next as usize).saturating_sub(1));
        text.get(start..end).unwrap_or("").trim_end_matches('\r')
    }
}

/// Render hits as stable, grep-like text: `path:line:col  [enclosing]  text`.
pub fn render_text(hits: &[Hit]) -> String {
    let mut out = String::new();
    for hit in hits {
        let _ = write!(out, "{}:{}:{}", hit.path, hit.line, hit.col);
        if let Some(kind) = hit.kind {
            let _ = write!(out, "  {kind}");
        }
        if let Some(fqn) = &hit.fqn {
            let _ = write!(out, "  {fqn}");
        }
        if let Some(enclosing) = &hit.enclosing {
            let _ = write!(out, "  [{enclosing}]");
        }
        if hit.kind.is_none() {
            let _ = write!(out, "  {}", hit.text);
        }
        out.push('\n');
    }
    out
}

/// Render hits as JSON lines (one object per line, `jq`-friendly).
pub fn render_json(hits: &[Hit]) -> String {
    let mut out = String::new();
    for hit in hits {
        if let Ok(line) = serde_json::to_string(hit) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_indexes_a_workspace_before_serving_queries() {
        let workspace = tempfile::TempDir::new().expect("scratch workspace");
        let source_dir = workspace.path().join("src/main/java/demo");
        std::fs::create_dir_all(&source_dir).expect("source directory");
        std::fs::write(
            source_dir.join("Circle.java"),
            "package demo; public class Circle {}",
        )
        .expect("source fixture");

        let session = SearchSession::open(workspace.path().to_path_buf())
            .await
            .expect("indexed session");
        let declarations = session.sym("Circle");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].fqn.as_deref(), Some("demo.Circle"));
    }

    #[tokio::test]
    async fn open_preserves_filesystem_error_context() {
        let workspace = tempfile::TempDir::new().expect("scratch workspace");
        let error = SearchSession::open(workspace.path().join("missing"))
            .await
            .err()
            .expect("missing roots must fail");
        assert!(error.to_string().contains("workspace root not found"));
    }

    const SAMPLE: &str = "\
package com.acme;

public class CoreEventHandler {
    private int count;

    public void receiveEvent(CoreEvent event) {
        if (event.getType() == CoreEvent.LOGIN) {
            count++;
        }
    }

    static class Inner {
        void helper() {
            System.out.println(\"in helper\");
        }
    }
}
";

    fn offset_of(needle: &str) -> u32 {
        offset_u32(SAMPLE.find(needle).expect("needle present in SAMPLE"))
    }

    #[test]
    fn enclosing_chain_finds_method_in_class() {
        let tree = parse_java(SAMPLE);
        let chain = enclosing_chain(&tree, offset_of("count++"));
        assert_eq!(chain.as_deref(), Some("CoreEventHandler#receiveEvent"));
    }

    #[test]
    fn enclosing_chain_handles_nested_types() {
        let tree = parse_java(SAMPLE);
        let chain = enclosing_chain(&tree, offset_of("in helper"));
        assert_eq!(chain.as_deref(), Some("CoreEventHandler.Inner#helper"));
    }

    #[test]
    fn enclosing_chain_outside_any_method_is_class_only() {
        let tree = parse_java(SAMPLE);
        let chain = enclosing_chain(&tree, offset_of("private int count"));
        assert_eq!(chain.as_deref(), Some("CoreEventHandler"));
    }

    #[test]
    fn enclosing_chain_outside_types_is_none() {
        let tree = parse_java(SAMPLE);
        assert_eq!(enclosing_chain(&tree, offset_of("package")), None);
    }

    #[test]
    fn line_index_round_trips_positions() {
        let index = LineIndex::new(SAMPLE);
        let offset = offset_of("public void receiveEvent");
        let (line, col) = index.line_col(offset);
        assert_eq!((line, col), (6, 5));
        assert_eq!(
            index.line_text(SAMPLE, line).trim(),
            "public void receiveEvent(CoreEvent event) {"
        );
    }

    #[test]
    fn line_index_first_and_last_lines() {
        let index = LineIndex::new("a\nbb\nccc");
        assert_eq!(index.line_col(0), (1, 1));
        assert_eq!(index.line_col(2), (2, 1));
        assert_eq!(index.line_text("a\nbb\nccc", 3), "ccc");
    }

    /// `grep` maps each `str::lines()` line back to a byte offset via
    /// [`LineIndex::line_start`] so that enclosing-declaration lookup lands on
    /// the right token. `lines()` strips the `\r` of a CRLF ending, so the
    /// two must agree on every line or the offset drifts one byte per line.
    #[test]
    fn line_starts_agree_with_lines_on_crlf() {
        for text in ["a\r\nbb\r\nccc", "a\r\nbb\r\nccc\r\n", "a\nbb\nccc", ""] {
            let index = LineIndex::new(text);
            for (line_index, line) in text.lines().enumerate() {
                let start = index.line_start(line_index) as usize;
                assert_eq!(
                    &text[start..start + line.len()],
                    line,
                    "line {line_index} of {text:?} starts at the wrong offset"
                );
            }
        }
    }

    #[test]
    fn fqn_helpers() {
        assert_eq!(fqn_base("com.acme.Foo.bar(String)"), "com.acme.Foo.bar");
        assert_eq!(fqn_base("com.acme.Foo.bar"), "com.acme.Foo.bar");
        assert!(fqn_ends_with_path("com.acme.Outer.Inner", "Outer.Inner"));
        assert!(fqn_ends_with_path("Outer.Inner", "Outer.Inner"));
        assert!(!fqn_ends_with_path(
            "com.acme.NotOuter.Inner",
            "Outer.Inner"
        ));
    }
}
