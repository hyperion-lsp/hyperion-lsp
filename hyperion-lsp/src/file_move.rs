// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Moving Java/Kotlin source files between packages.
//!
//! One planner serves every entry point that relocates source files: the
//! "Move to package" code action, `workspace/willRenameFiles` (a move in the
//! client's file explorer, including whole directories), and package rename.
//! Given `old path -> new path` pairs it computes, in one pass:
//!
//! * the moved file's new `package` declaration,
//! * imports the moved file now needs for the same-package types it used,
//! * every other file's import/qualified-name/implicit-package references to
//!   the moved types, and
//! * the file renames themselves.
//!
//! Reference discovery uses the index's strict current-usage query (which
//! accounts for imports and Javadoc references), so an unresolvable spelling
//! fails the whole move rather than producing a partial one.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use hyperion_workspace::{
    DeclarationNamespace, ImportMap, SymbolTarget, WorkspaceIndex, WorkspaceRevision,
};
use tower_lsp::lsp_types::{
    DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
    RenameFile, RenameFileOptions, ResourceOp, TextDocumentEdit, TextEdit as LspTextEdit, Url,
    WorkspaceEdit,
};

use crate::position_utils::offset_to_position;

/// One requested relocation of a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMove {
    pub old: PathBuf,
    pub new: PathBuf,
}

/// A byte-range edit against a file's current text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteEdit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Result of planning: edits keyed by the file's *old* path plus the renames.
#[derive(Debug, Default)]
pub struct MovePlan {
    pub file_edits: BTreeMap<PathBuf, Vec<ByteEdit>>,
    pub renames: Vec<(PathBuf, PathBuf)>,
    /// Whole-directory renames (package rename). When present the client is
    /// asked to rename these directories instead of each file.
    pub dir_renames: Vec<(PathBuf, PathBuf)>,
    /// Non-fatal notes (files whose package could not be derived, etc.).
    pub warnings: Vec<String>,
    contents: HashMap<PathBuf, String>,
}

/// How to express the plan to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditStyle {
    /// `workspace/willRenameFiles`: plain edits addressed to the OLD uris; the
    /// client performs the rename itself afterwards.
    BeforeClientRename,
    /// Code action: `documentChanges` with the edits (old uris) followed by
    /// `RenameFile` operations.
    ResourceOperations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    Java,
    Kotlin,
}

fn lang_of(path: &Path) -> Option<Lang> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("java") => Some(Lang::Java),
        Some("kt") => Some(Lang::Kotlin),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct Seg {
    text: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct ImportInfo {
    start: usize,
    end: usize,
    segs: Vec<Seg>,
    is_static: bool,
    wildcard: bool,
    aliased: bool,
}

impl ImportInfo {
    fn path(&self) -> String {
        self.segs
            .iter()
            .map(|seg| seg.text.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, Default)]
struct FileFacts {
    package: Option<(String, usize, usize)>,
    package_segs: Vec<Seg>,
    package_decl_end: Option<usize>,
    imports: Vec<ImportInfo>,
    top_types: Vec<String>,
    /// `(name, declaration start, opening brace)` of Java top-level types.
    top_type_spans: Vec<(String, usize, usize)>,
}

fn analyze(lang: Lang, text: &str) -> FileFacts {
    match lang {
        Lang::Java => analyze_java(text),
        Lang::Kotlin => analyze_kotlin(text),
    }
}

fn analyze_java(text: &str) -> FileFacts {
    use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
    let tokens = hyperion_parser::lexer::Lexer::new(text).collect();
    let (green, _) = hyperion_parser::parser::Parser::parse(tokens);
    let root = SyntaxNode::new_root(green);
    let mut facts = FileFacts::default();
    let seg = |token: &hyperion_parser::syntax::SyntaxToken| Seg {
        text: token.text().to_string(),
        start: u32::from(token.text_range().start()) as usize,
        end: u32::from(token.text_range().end()) as usize,
    };
    for node in root.children() {
        match node.kind() {
            SyntaxKind::PACKAGE_DECL => {
                let segs = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
                    .into_iter()
                    .flat_map(|name| name.children_with_tokens())
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .map(|token| seg(&token))
                    .collect::<Vec<_>>();
                if let (Some(first), Some(last)) = (segs.first(), segs.last()) {
                    let name = segs
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    facts.package = Some((name, first.start, last.end));
                }
                facts.package_segs = segs;
                facts.package_decl_end = Some(trimmed_end(text, &node));
            }
            SyntaxKind::IMPORT_DECL => {
                let segs = node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
                    .into_iter()
                    .flat_map(|name| name.children_with_tokens())
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .map(|token| seg(&token))
                    .collect::<Vec<_>>();
                let is_static = node
                    .children_with_tokens()
                    .any(|c| c.kind() == SyntaxKind::STATIC_KW);
                let wildcard = node
                    .children_with_tokens()
                    .any(|c| c.kind() == SyntaxKind::STAR);
                let (start, end) = trimmed_bounds(text, &node);
                facts.imports.push(ImportInfo {
                    start,
                    end,
                    segs,
                    is_static,
                    wildcard,
                    aliased: false,
                });
            }
            SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL => {
                if let Some(name) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind().is_identifier())
                {
                    facts.top_types.push(name.text().to_string());
                    let (node_start, _) = trimmed_bounds(text, &node);
                    // Modifiers are siblings that precede the declaration.
                    let mut start = node_start;
                    let mut previous = node.prev_sibling_or_token();
                    while let Some(element) = previous {
                        match element.kind() {
                            SyntaxKind::WHITESPACE => previous = element.prev_sibling_or_token(),
                            SyntaxKind::MODIFIER_LIST => {
                                start = usize::from(element.text_range().start());
                                break;
                            }
                            _ => break,
                        }
                    }
                    let brace = node
                        .children_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .find(|token| token.kind() == SyntaxKind::L_BRACE)
                        .map_or_else(
                            || trimmed_end(text, &node),
                            |token| u32::from(token.text_range().start()) as usize,
                        );
                    facts
                        .top_type_spans
                        .push((name.text().to_string(), start, brace));
                }
            }
            _ => {}
        }
    }
    facts
}

fn trimmed_bounds<L: rowan::Language>(text: &str, node: &rowan::SyntaxNode<L>) -> (usize, usize) {
    let range = node.text_range();
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    (start + lead, end - trail)
}

fn trimmed_end<L: rowan::Language>(text: &str, node: &rowan::SyntaxNode<L>) -> usize {
    trimmed_bounds(text, node).1
}

fn analyze_kotlin(text: &str) -> FileFacts {
    use hyperion_kotlin::SyntaxKind as Kt;
    let tokens = hyperion_kotlin::Lexer::new(text).collect::<Vec<_>>();
    let (green, _) = hyperion_kotlin::Parser::parse(tokens);
    let root = hyperion_kotlin::SyntaxNode::new_root(green);
    let mut facts = FileFacts::default();
    for node in root.children() {
        match node.kind() {
            Kt::PACKAGE_DECL => {
                let segs = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| {
                        !token.kind().is_trivia()
                            && !matches!(token.kind(), Kt::PACKAGE_KW | Kt::DOT | Kt::SEMICOLON)
                    })
                    .map(|token| Seg {
                        text: token.text().to_string(),
                        start: u32::from(token.text_range().start()) as usize,
                        end: u32::from(token.text_range().end()) as usize,
                    })
                    .collect::<Vec<_>>();
                if let (Some(first), Some(last)) = (segs.first(), segs.last()) {
                    let name = segs
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    facts.package = Some((name, first.start, last.end));
                }
                facts.package_segs = segs;
                facts.package_decl_end = Some(trimmed_end(text, &node));
            }
            Kt::IMPORT_DECL => {
                let mut segs = Vec::new();
                let mut wildcard = false;
                let mut aliased = false;
                for token in node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                {
                    match token.kind() {
                        Kt::IMPORT_KW | Kt::DOT | Kt::SEMICOLON => {}
                        Kt::STAR => wildcard = true,
                        Kt::AS_KW => {
                            aliased = true;
                            break;
                        }
                        kind if kind.is_trivia() => {}
                        _ => segs.push(Seg {
                            text: token.text().to_string(),
                            start: u32::from(token.text_range().start()) as usize,
                            end: u32::from(token.text_range().end()) as usize,
                        }),
                    }
                }
                let (start, end) = trimmed_bounds(text, &node);
                facts.imports.push(ImportInfo {
                    start,
                    end,
                    segs,
                    is_static: false,
                    wildcard,
                    aliased,
                });
            }
            Kt::CLASS_DECL | Kt::OBJECT_DECL | Kt::INTERFACE_DECL => {
                if let Some(name) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == Kt::IDENT)
                {
                    facts.top_types.push(name.text().to_string());
                }
            }
            _ => {}
        }
    }
    facts
}

/// Offset of the first code token, staying in front of a leading doc comment
/// (which belongs to the declaration, not to the file header).
fn first_code_offset(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut at = 0;
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let rest = &text[at..];
        if rest.starts_with("//") {
            at += rest.find('\n').map_or(rest.len(), |i| i + 1);
        } else if rest.starts_with("/**") && !rest.starts_with("/**/") {
            return at;
        } else if rest.starts_with("/*") {
            at += rest.find("*/").map_or(rest.len(), |i| i + 2);
        } else {
            return at;
        }
    }
}

fn line_end_after(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index + 1)
}

fn line_start_before(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

/// Directory-derived package roots of every indexed source file.
fn source_roots(index: &WorkspaceIndex, workspace_root: &Path) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    for (file_id, path) in index.workspace_files_under(workspace_root) {
        let Some(lang) = lang_of(&path) else { continue };
        // Standard layouts are roots even when no file sits in them yet.
        let components = path.components().collect::<Vec<_>>();
        for (i, window) in components.windows(3).enumerate() {
            let names = window
                .iter()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>();
            if names[0] == "src" && matches!(names[2].as_str(), "java" | "kotlin") {
                roots.insert(components[..i + 3].iter().collect::<PathBuf>());
            }
        }
        if lang != Lang::Java {
            continue;
        }
        let Some(package) = index
            .get_imports(file_id)
            .and_then(|imports| imports.current_package)
            .filter(|package| !package.is_empty())
        else {
            continue;
        };
        let mut dir = path.parent().map(Path::to_path_buf);
        let mut matches = true;
        for segment in package.split('.').rev() {
            match dir {
                Some(ref current)
                    if current.file_name().and_then(|n| n.to_str()) == Some(segment) =>
                {
                    dir = current.parent().map(Path::to_path_buf);
                }
                _ => {
                    matches = false;
                    break;
                }
            }
        }
        if matches {
            if let Some(root) = dir {
                roots.insert(root);
            }
        }
    }
    roots
}

/// Package implied by a file's location, or `None` if it is outside every root.
pub fn package_for_path(roots: &BTreeSet<PathBuf>, file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let root = roots
        .iter()
        .filter(|root| dir.starts_with(root))
        .max_by_key(|root| root.components().count())?;
    let relative = dir.strip_prefix(root).ok()?;
    let parts = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    Some(parts.join("."))
}

/// Source roots of the workspace (derived from indexed files and layouts).
pub fn workspace_source_roots(index: &WorkspaceIndex, workspace_root: &Path) -> BTreeSet<PathBuf> {
    source_roots(index, workspace_root)
}

struct MovedFile {
    old: PathBuf,
    lang: Lang,
    old_pkg: String,
    new_pkg: String,
    facts: FileFacts,
}

/// Plan a set of file moves. `moves` may contain non-source files (ignored
/// except for the rename itself).
pub fn plan_file_moves(
    index: &WorkspaceIndex,
    workspace_root: &Path,
    moves: &[FileMove],
) -> Result<MovePlan, String> {
    plan_file_moves_cached(index, workspace_root, moves, &mut None)
}

/// Reference discovery is independent of the destination package, so callers
/// that plan several destinations for the same source files can share it.
#[derive(Default)]
pub struct Discovery {
    keys: Vec<String>,
    refs: BTreeMap<PathBuf, Vec<(usize, usize, String)>>,
    contents: HashMap<PathBuf, String>,
}

pub fn plan_file_moves_cached(
    index: &WorkspaceIndex,
    workspace_root: &Path,
    moves: &[FileMove],
    cache: &mut Option<Discovery>,
) -> Result<MovePlan, String> {
    let mut plan = MovePlan::default();
    let roots = source_roots(index, workspace_root);
    let mut moved: Vec<MovedFile> = Vec::new();
    let mut moved_by_old: HashMap<PathBuf, usize> = HashMap::new();
    // old top-level fqn -> new top-level fqn
    let mut type_moves: BTreeMap<String, String> = BTreeMap::new();

    for file_move in moves {
        plan.renames
            .push((file_move.old.clone(), file_move.new.clone()));
        let Some(lang) = lang_of(&file_move.old) else {
            continue;
        };
        let Some(file_id) = index.get_file_id(&file_move.old) else {
            continue;
        };
        let Some(content) = index.get_file_contents(file_id) else {
            continue;
        };
        let facts = analyze(lang, &content);
        let old_pkg = facts
            .package
            .as_ref()
            .map(|(name, _, _)| name.clone())
            .unwrap_or_default();
        let Some(new_pkg) = package_for_path(&roots, &file_move.new) else {
            plan.warnings.push(format!(
                "{} is moved outside every source root; its package was left unchanged",
                file_move.new.display()
            ));
            continue;
        };
        plan.contents
            .insert(file_move.old.clone(), content.to_string());
        if new_pkg == old_pkg {
            continue;
        }
        if new_pkg.is_empty() && lang == Lang::Java {
            return Err("Moving a class into the default package is not supported".to_string());
        }
        for ty in &facts.top_types {
            let old_fqn = if old_pkg.is_empty() {
                ty.clone()
            } else {
                format!("{old_pkg}.{ty}")
            };
            type_moves.insert(old_fqn, format!("{new_pkg}.{ty}"));
        }
        moved_by_old.insert(file_move.old.clone(), moved.len());
        moved.push(MovedFile {
            old: file_move.old.clone(),
            lang,
            old_pkg,
            new_pkg,
            facts,
        });
    }
    if moved.is_empty() {
        return Ok(plan);
    }
    for file_move in moves {
        if file_move.new != file_move.old && file_move.new.exists() {
            return Err(format!(
                "Cannot move: {} already exists",
                file_move.new.display()
            ));
        }
    }

    // ---- reference discovery -------------------------------------------------
    let keys = type_moves.keys().cloned().collect::<Vec<_>>();
    if cache.as_ref().is_none_or(|found| found.keys != keys) {
        *cache = Some(discover(index, keys)?);
    }
    let found = cache.as_ref().expect("discovery was just computed");
    for (path, text) in &found.contents {
        plan.contents
            .entry(path.clone())
            .or_insert_with(|| text.clone());
    }
    let refs = &found.refs;

    // ---- per-file edits ------------------------------------------------------
    // Files to process: every referencing file plus every moved file.
    let mut files: BTreeSet<PathBuf> = refs.keys().cloned().collect();
    files.extend(moved.iter().map(|m| m.old.clone()));

    for path in files {
        let Some(lang) = lang_of(&path) else { continue };
        let Some(text) = plan.contents.get(&path).cloned() else {
            continue;
        };
        let owned_facts;
        let facts = if let Some(&i) = moved_by_old.get(&path) {
            &moved[i].facts
        } else {
            owned_facts = analyze(lang, &text);
            &owned_facts
        };
        let moved_here = moved_by_old.get(&path).map(|&i| &moved[i]);
        let file_pkg = facts
            .package
            .as_ref()
            .map(|(name, _, _)| name.clone())
            .unwrap_or_default();
        let new_file_pkg = moved_here.map_or(file_pkg.clone(), |m| m.new_pkg.clone());
        let mut edits: Vec<ByteEdit> = Vec::new();
        // fqns (new form) that must be imported
        let mut needed: BTreeSet<String> = BTreeSet::new();

        // The file's own types never need imports for themselves.
        let own_types: BTreeSet<String> = facts
            .top_types
            .iter()
            .map(|ty| {
                if file_pkg.is_empty() {
                    ty.clone()
                } else {
                    format!("{file_pkg}.{ty}")
                }
            })
            .collect();

        if let Some(moved_here) = moved_here {
            match &facts.package {
                Some((_, start, end)) => edits.push(ByteEdit {
                    start: *start,
                    end: *end,
                    text: moved_here.new_pkg.clone(),
                }),
                None => {
                    let at = first_code_offset(&text);
                    let terminator = if moved_here.lang == Lang::Java {
                        ";"
                    } else {
                        ""
                    };
                    edits.push(ByteEdit {
                        start: at,
                        end: at,
                        text: format!("package {}{terminator}\n\n", moved_here.new_pkg),
                    });
                }
            }
        }

        let mut removed_imports: BTreeSet<usize> = BTreeSet::new();
        let explicit_imports: BTreeSet<String> = facts
            .imports
            .iter()
            .filter(|import| !import.is_static && !import.wildcard)
            .map(ImportInfo::path)
            .collect();

        for (start, end, old_fqn) in refs.get(&path).cloned().unwrap_or_default() {
            if own_types.contains(&old_fqn) {
                continue;
            }
            let Some(new_fqn) = type_moves.get(&old_fqn) else {
                continue;
            };
            let old_pkg = old_fqn.rsplit_once('.').map_or("", |(pkg, _)| pkg);
            let new_pkg = new_fqn.rsplit_once('.').map_or("", |(pkg, _)| pkg);
            if let Some(import) = facts
                .imports
                .iter()
                .find(|import| import.start <= start && end <= import.end)
            {
                // Rewrite the package prefix of the import path.
                let Some(first) = import.segs.first() else {
                    continue;
                };
                if !import.is_static
                    && !import.wildcard
                    && !import.aliased
                    && import.path() == old_fqn
                    && new_pkg == new_file_pkg
                {
                    if removed_imports.insert(import.start) {
                        let from = line_start_before(&text, import.start);
                        let to = line_end_after(&text, import.end);
                        edits.push(ByteEdit {
                            start: from,
                            end: to,
                            text: String::new(),
                        });
                    }
                    continue;
                }
                if first.start < start && !old_pkg.is_empty() {
                    edits.push(ByteEdit {
                        start: first.start,
                        end: start,
                        text: format!("{new_pkg}."),
                    });
                }
                continue;
            }
            // Fully qualified spelling `old.pkg.Type`.
            let qualifier = format!("{old_pkg}.");
            if !old_pkg.is_empty()
                && start >= qualifier.len()
                && text[..start].ends_with(&qualifier)
            {
                edits.push(ByteEdit {
                    start: start - qualifier.len(),
                    end: start,
                    text: format!("{new_pkg}."),
                });
                continue;
            }
            // Simple-name use (same package or on-demand import).
            if text[..start].ends_with('.') {
                continue;
            }
            if new_pkg != new_file_pkg
                && new_pkg != "java.lang"
                && !explicit_imports.contains(&old_fqn)
            {
                needed.insert(new_fqn.clone());
            }
        }

        // A moved file loses implicit visibility of its old package.
        if let Some(moved_here) = moved_here {
            if let Some(file_id) = index.get_file_id(&path) {
                let imports: ImportMap = index.get_imports(file_id).unwrap_or_default();
                let used = match moved_here.lang {
                    Lang::Java => hyperion_workspace::usage_extractor::UsageExtractor::extract(
                        &text, file_id, &imports, index,
                    ),
                    Lang::Kotlin => {
                        hyperion_workspace::kotlin_usage_extractor::KotlinUsageExtractor::extract(
                            &text, file_id, &imports, index,
                        )
                    }
                }
                .map_err(|error| {
                    format!(
                        "cannot find the references the move must rewrite in {}: {error}",
                        path.display()
                    )
                })?;
                for usage in used {
                    if moved_here.old_pkg.is_empty() {
                        continue;
                    }
                    let Some(rest) = usage.fqn.strip_prefix(&format!("{}.", moved_here.old_pkg))
                    else {
                        continue;
                    };
                    let top = rest.split(['.', '(']).next().unwrap_or_default();
                    if top.is_empty() || !top.chars().next().is_some_and(char::is_uppercase) {
                        continue;
                    }
                    let old_top = format!("{}.{top}", moved_here.old_pkg);
                    if own_types.contains(&old_top)
                        || type_moves.contains_key(&old_top)
                        || explicit_imports.contains(&old_top)
                    {
                        continue;
                    }
                    if moved_here.new_pkg == moved_here.old_pkg {
                        continue;
                    }
                    needed.insert(old_top);
                }
            }
        }

        needed.retain(|fqn| !explicit_imports.contains(fqn));
        if !needed.is_empty() {
            edits.extend(import_insertions(&text, facts, &needed, lang));
        }
        if !edits.is_empty() {
            edits.sort_by_key(|edit| (edit.start, edit.end));
            edits.dedup();
            plan.file_edits.insert(path.clone(), edits);
        }
    }
    Ok(plan)
}

fn discover(index: &WorkspaceIndex, keys: Vec<String>) -> Result<Discovery, String> {
    let targets = keys
        .iter()
        .map(|fqn| SymbolTarget::new(fqn.as_str(), DeclarationNamespace::Type))
        .collect::<Vec<_>>();
    let revision: WorkspaceRevision = index.current_workspace_revision();
    let usages = index
        .find_current_usages_for_symbol_targets_for_refactoring(&targets, revision, &[], || false)
        .map_err(|error| format!("Cannot move: reference discovery is incomplete: {error}"))?;
    let mut found = Discovery {
        keys,
        ..Discovery::default()
    };
    for file in &usages.files {
        let Some(indexed) = index.get_indexed_revision_content(file.revision) else {
            return Err("Cannot move: a source revision changed during the scan".to_string());
        };
        found
            .contents
            .entry(indexed.file_path.clone())
            .or_insert_with(|| indexed.content.to_string());
        for location in &file.locations {
            found
                .refs
                .entry(indexed.file_path.clone())
                .or_default()
                .push((
                    location.range.start as usize,
                    location.range.end as usize,
                    location.fqn.to_string(),
                ));
        }
    }
    Ok(found)
}

fn import_insertions(
    text: &str,
    facts: &FileFacts,
    needed: &BTreeSet<String>,
    lang: Lang,
) -> Vec<ByteEdit> {
    let terminator = if lang == Lang::Java { ";" } else { "" };
    let plain = facts
        .imports
        .iter()
        .filter(|import| !import.is_static && !import.wildcard)
        .collect::<Vec<_>>();
    // offset -> (prefix, body, suffix)
    let mut at_offset: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
    for fqn in needed {
        let line = format!("import {fqn}{terminator}\n");
        let (offset, prefix, suffix) = if let Some(next) = plain.iter().find(|i| i.path() > *fqn) {
            (line_start_before(text, next.start), "", "")
        } else if let Some(last) = facts.imports.last() {
            (line_end_after(text, last.end), "", "")
        } else if let Some(end) = facts.package_decl_end {
            (line_end_after(text, end), "\n", "")
        } else {
            (first_code_offset(text), "", "\n")
        };
        let entry = at_offset.entry(offset).or_default();
        if entry.1.is_empty() {
            entry.0 = prefix.to_string();
            entry.2 = suffix.to_string();
        }
        entry.1.push_str(&line);
    }
    let mut out = Vec::new();
    for (offset, (prefix, body, suffix)) in at_offset {
        let block = format!("{prefix}{body}{suffix}");
        // A package line without a trailing newline needs one first.
        let needs_newline = offset == text.len() && !text.ends_with('\n') && !text.is_empty();
        out.push(ByteEdit {
            start: offset,
            end: offset,
            text: if needs_newline {
                format!("\n{block}")
            } else {
                block
            },
        });
    }
    out
}

/// Convert a plan to an LSP workspace edit.
pub fn plan_to_workspace_edit(
    plan: &MovePlan,
    style: EditStyle,
    open_versions: &HashMap<Url, i32>,
) -> Option<WorkspaceEdit> {
    let to_lsp = |path: &Path, edits: &[ByteEdit]| -> Option<Vec<LspTextEdit>> {
        let text = plan.contents.get(path)?;
        Some(
            edits
                .iter()
                .map(|edit| LspTextEdit {
                    range: tower_lsp::lsp_types::Range {
                        start: offset_to_position(text, edit.start),
                        end: offset_to_position(text, edit.end),
                    },
                    new_text: edit.text.clone(),
                })
                .collect(),
        )
    };
    match style {
        EditStyle::BeforeClientRename => {
            let mut changes: HashMap<Url, Vec<LspTextEdit>> = HashMap::new();
            for (path, edits) in &plan.file_edits {
                changes.insert(Url::from_file_path(path).ok()?, to_lsp(path, edits)?);
            }
            Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            })
        }
        EditStyle::ResourceOperations => {
            // Edits address the pre-move URIs (so open-document version guards
            // stay valid); the renames come last.
            let mut ops: Vec<DocumentChangeOperation> = Vec::new();
            for (path, edits) in &plan.file_edits {
                let uri = Url::from_file_path(path).ok()?;
                ops.push(DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        version: open_versions.get(&uri).copied(),
                        uri,
                    },
                    edits: to_lsp(path, edits)?.into_iter().map(OneOf::Left).collect(),
                }));
            }
            let renames: &[(PathBuf, PathBuf)] = if plan.dir_renames.is_empty() {
                &plan.renames
            } else {
                &plan.dir_renames
            };
            for (old, new) in renames.iter().filter(|(old, new)| old != new) {
                ops.push(DocumentChangeOperation::Op(ResourceOp::Rename(
                    RenameFile {
                        old_uri: Url::from_file_path(old).ok()?,
                        new_uri: Url::from_file_path(new).ok()?,
                        options: Some(RenameFileOptions {
                            overwrite: Some(false),
                            ignore_if_exists: Some(false),
                        }),
                        annotation_id: None,
                    },
                )));
            }
            Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(ops)),
                ..WorkspaceEdit::default()
            })
        }
    }
}

/// Client support required to express a file move as resource operations.
#[derive(Debug, Clone, Copy)]
pub struct MoveCapabilities {
    pub document_changes: bool,
    pub rename_file: bool,
}

/// "Move to package" code actions for the top-level Java type at `offset`.
///
/// Destinations are the existing packages of the same source root, nearest
/// first (parent, siblings, then by shared prefix). Each action carries the
/// complete edit: package declaration, imports on both sides, references in
/// every other file, and the `RenameFile` that relocates the source.
pub fn move_to_package_actions(
    index: &WorkspaceIndex,
    workspace_root: &Path,
    file: &Path,
    source: &str,
    offset: usize,
    capabilities: MoveCapabilities,
    open_versions: &HashMap<Url, i32>,
) -> Vec<tower_lsp::lsp_types::CodeAction> {
    use tower_lsp::lsp_types::CodeAction;
    if !capabilities.document_changes
        || !capabilities.rename_file
        || lang_of(file) != Some(Lang::Java)
    {
        return Vec::new();
    }
    let facts = analyze_java(source);
    let Some((type_name, _, _)) = facts
        .top_type_spans
        .iter()
        .find(|(_, start, brace)| *start <= offset && offset <= *brace)
    else {
        return Vec::new();
    };
    let current = facts
        .package
        .as_ref()
        .map(|(name, _, _)| name.clone())
        .unwrap_or_default();
    let Some(file_name) = file.file_name() else {
        return Vec::new();
    };
    // Root of this file: strip the package directories.
    let mut root = file.parent().map(Path::to_path_buf);
    for _ in current.split('.').filter(|segment| !segment.is_empty()) {
        root = root.and_then(|dir| dir.parent().map(Path::to_path_buf));
    }
    let Some(root) = root else {
        return Vec::new();
    };
    let mut packages: BTreeSet<String> = BTreeSet::new();
    for (file_id, path) in index.workspace_files_under(&root) {
        if lang_of(&path).is_none() {
            continue;
        }
        if let Some(package) = index
            .get_imports(file_id)
            .and_then(|imports| imports.current_package)
            .filter(|package| !package.is_empty() && *package != current)
        {
            packages.insert(package);
        }
    }
    let parent = current
        .rsplit_once('.')
        .map(|(parent, _)| parent.to_string());
    if let Some(parent) = &parent {
        packages.insert(parent.clone());
    }
    let shared_prefix = |candidate: &str| {
        candidate
            .split('.')
            .zip(current.split('.'))
            .take_while(|(a, b)| a == b)
            .count()
    };
    let mut ranked = packages.into_iter().collect::<Vec<_>>();
    ranked.sort_by_key(|candidate| {
        (
            Some(candidate) != parent.as_ref(),
            std::cmp::Reverse(shared_prefix(candidate)),
            candidate.clone(),
        )
    });
    ranked.truncate(8);

    let mut cache = None;
    let mut actions = Vec::new();
    for package in ranked {
        let mut target = root.clone();
        for segment in package.split('.') {
            target.push(segment);
        }
        target.push(file_name);
        let moves = [FileMove {
            old: file.to_path_buf(),
            new: target,
        }];
        match plan_file_moves_cached(index, workspace_root, &moves, &mut cache) {
            Ok(plan) => {
                let Some(edit) =
                    plan_to_workspace_edit(&plan, EditStyle::ResourceOperations, open_versions)
                else {
                    continue;
                };
                actions.push(CodeAction {
                    title: format!("Move '{type_name}' to package '{package}'"),
                    kind: Some(crate::code_actions::kinds::refactor_move()),
                    edit: Some(edit),
                    ..CodeAction::default()
                });
            }
            Err(reason) => {
                actions.push(CodeAction {
                    title: format!("Move '{type_name}' to another package"),
                    kind: Some(crate::code_actions::kinds::refactor_move()),
                    disabled: Some(tower_lsp::lsp_types::CodeActionDisabled { reason }),
                    ..CodeAction::default()
                });
                break;
            }
        }
    }
    actions
}

/// A package name under the cursor in a `package` or `import` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTarget {
    /// Fully qualified package up to and including the segment under the cursor.
    pub package: String,
    pub segment: String,
    pub start: usize,
    pub end: usize,
}

fn known_packages(index: &WorkspaceIndex, workspace_root: &Path) -> BTreeSet<String> {
    let mut packages = BTreeSet::new();
    for (file_id, path) in index.workspace_files_under(workspace_root) {
        if lang_of(&path).is_none() {
            continue;
        }
        if let Some(package) = index
            .get_imports(file_id)
            .and_then(|imports| imports.current_package)
        {
            packages.insert(package);
        }
    }
    packages
}

/// The package segment at `offset`, if the identifier there names a package
/// (a `package` declaration segment, or an `import` prefix that is a package).
pub fn package_at(
    index: &WorkspaceIndex,
    workspace_root: &Path,
    file: &Path,
    source: &str,
    offset: usize,
) -> Option<PackageTarget> {
    let lang = lang_of(file)?;
    let facts = analyze(lang, source);
    let target = |segs: &[Seg], at: usize| {
        let position = segs
            .iter()
            .position(|seg| seg.start <= offset && offset <= seg.end)?;
        let package = segs[..=position]
            .iter()
            .map(|seg| seg.text.as_str())
            .collect::<Vec<_>>()
            .join(".");
        Some((package, position, segs[position].clone(), at))
    };
    if let Some((package, _, seg, _)) = target(&facts.package_segs, 0) {
        return Some(PackageTarget {
            package,
            segment: seg.text,
            start: seg.start,
            end: seg.end,
        });
    }
    let packages = known_packages(index, workspace_root);
    for import in &facts.imports {
        let Some((package, position, seg, _)) = target(&import.segs, 0) else {
            continue;
        };
        // The last segment of a non-wildcard import is a member, not a package.
        let is_last = position + 1 == import.segs.len();
        if is_last && !import.wildcard {
            return None;
        }
        let known = packages
            .iter()
            .any(|known| *known == package || known.starts_with(&format!("{package}.")));
        if known && !index.has_class(&package) {
            return Some(PackageTarget {
                package,
                segment: seg.text,
                start: seg.start,
                end: seg.end,
            });
        }
        return None;
    }
    None
}

/// Rename the last segment of `target.package` to `new_name`: every source
/// directory of the package moves, and every declaration/import follows.
pub fn plan_package_rename(
    index: &WorkspaceIndex,
    workspace_root: &Path,
    target: &PackageTarget,
    new_name: &str,
) -> Result<MovePlan, String> {
    if new_name == target.segment {
        return Ok(MovePlan::default());
    }
    if new_name.is_empty()
        || !new_name
            .chars()
            .all(|c| c == '_' || c == '$' || c.is_alphanumeric())
        || new_name.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return Err(format!("'{new_name}' is not a valid package name"));
    }
    let new_package = match target.package.rsplit_once('.') {
        Some((parent, _)) => format!("{parent}.{new_name}"),
        None => new_name.to_string(),
    };
    let roots = source_roots(index, workspace_root);
    let mut moves = Vec::new();
    let mut dir_renames = Vec::new();
    for root in &roots {
        let mut old_dir = root.clone();
        for segment in target.package.split('.') {
            old_dir.push(segment);
        }
        if !old_dir.is_dir() {
            continue;
        }
        let mut new_dir = root.clone();
        for segment in new_package.split('.') {
            new_dir.push(segment);
        }
        if new_dir.exists() {
            return Err(format!("Package '{new_package}' already exists"));
        }
        for (_, path) in index.workspace_files_under(&old_dir) {
            if lang_of(&path).is_none() {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&old_dir) else {
                continue;
            };
            moves.push(FileMove {
                new: new_dir.join(relative),
                old: path,
            });
        }
        dir_renames.push((old_dir, new_dir));
    }
    if dir_renames.is_empty() {
        return Err(format!(
            "No source directory found for package '{}'",
            target.package
        ));
    }
    let mut plan = plan_file_moves(index, workspace_root, &moves)?;
    plan.dir_renames = dir_renames;
    Ok(plan)
}

/// Apply LSP text edits (non-overlapping) to `text`.
pub fn apply_text_edits(text: &str, edits: &[LspTextEdit]) -> String {
    let offset = |position: tower_lsp::lsp_types::Position| {
        crate::position_utils::position_to_offset(text, position).unwrap_or(text.len())
    };
    let mut ordered = edits.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
    let mut out = text.to_string();
    for edit in ordered.into_iter().rev() {
        out.replace_range(
            offset(edit.range.start)..offset(edit.range.end),
            &edit.new_text,
        );
    }
    out
}

/// True when the Java source at `uri` declares no top-level type (only a
/// package line, imports, comments).
pub fn declares_no_types(uri: &Url, text: &str) -> bool {
    uri.to_file_path()
        .is_ok_and(|path| lang_of(&path) == Some(Lang::Java))
        && analyze_java(text).top_types.is_empty()
}

/// Byte edits inserting `import` lines for `fqns` at their sorted positions.
/// Imports already present (explicitly) are skipped.
pub fn add_import_edits(text: &str, path: &Path, fqns: &[String]) -> Vec<ByteEdit> {
    let Some(lang) = lang_of(path).or(Some(Lang::Java)) else {
        return Vec::new();
    };
    let facts = analyze(lang, text);
    let existing: BTreeSet<String> = facts
        .imports
        .iter()
        .filter(|import| !import.is_static && !import.wildcard)
        .map(ImportInfo::path)
        .collect();
    let needed: BTreeSet<String> = fqns
        .iter()
        .filter(|fqn| !existing.contains(*fqn))
        .cloned()
        .collect();
    if needed.is_empty() {
        return Vec::new();
    }
    import_insertions(text, &facts, &needed, lang)
}

/// Byte range covering all import declarations of `text` (first import start
/// to last import end), if there are any.
pub fn import_region(text: &str, path: &Path) -> Option<(usize, usize)> {
    let facts = analyze(lang_of(path).unwrap_or(Lang::Java), text);
    let first = facts.imports.first()?;
    let last = facts.imports.last()?;
    Some((first.start, last.end))
}

/// Apply byte edits (non-overlapping) to `text`.
pub fn apply_byte_edits(text: &str, edits: &[ByteEdit]) -> String {
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|edit| (edit.start, edit.end));
    let mut out = text.to_string();
    for edit in ordered.into_iter().rev() {
        out.replace_range(edit.start..edit.end, &edit.text);
    }
    out
}
