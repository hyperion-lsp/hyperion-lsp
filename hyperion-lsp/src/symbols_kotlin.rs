// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Document Symbols Provider
//!
//! Kotlin is parsed by `hyperion-kotlin` into its **own** rowan tree with its own
//! [`SyntaxKind`] enum. The Java provider in [`crate::symbols`] interprets raw
//! rowan kinds through `hyperion_parser::SyntaxKind`, so pointing it at a Kotlin
//! tree yields garbage (Kotlin `FUNCTION_DECL` == 44 is a completely different
//! Java kind). This module walks the Kotlin tree with the Kotlin kinds instead.
//!
//! # Parser shape notes
//!
//! The hand-written Kotlin parser has two quirks the walker has to accommodate:
//!
//! 1. **Modifiers are siblings, not children.** `parse_modifiers` opens and closes
//!    a `MODIFIERS` node *before* the declaration node starts, so `data class Foo`
//!    produces `MODIFIERS("data")` followed by `CLASS_DECL`. The walker therefore
//!    accumulates modifiers seen since the last declaration.
//! 2. **Soft modifiers land as loose tokens.** `enum`, `const`, `lateinit`,
//!    `annotation`, `inner`, … are not consumed by `parse_modifiers`, so the
//!    top-level/class-member loop hits them, fails to match a declaration, and
//!    bumps them as loose tokens directly into the container. `enum class Color`
//!    is therefore `IDENT("enum")` + `CLASS_DECL`, and enum *entries* are loose
//!    `IDENT` tokens inside the `CLASS_BODY`.
//!
//! Declaration nodes also frequently swallow *trailing* trivia (e.g.
//! `parse_property_decl` calls `skip_trivia()` before checking for accessors), so
//! every range is trimmed to the outermost non-trivia tokens.

use hyperion_kotlin::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{NodeOrToken, TextSize};
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind as LspSymbolKind};

use crate::position_utils::offset_to_position;

/// Where a declaration lives, which decides `fun` -> METHOD vs FUNCTION.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// Direct child of `SOURCE_FILE`.
    File,
    /// Inside a `CLASS_BODY` (class, object, interface, enum).
    Member,
}

/// Kotlin modifier words. `parse_modifiers` only handles a subset; the rest reach
/// us as loose tokens. Kept as an allowlist so error-recovery junk (identifiers
/// from malformed code) does not leak into `detail` strings.
const MODIFIER_WORDS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "expect",
    "actual",
    "final",
    "open",
    "abstract",
    "sealed",
    "const",
    "external",
    "override",
    "lateinit",
    "tailrec",
    "vararg",
    "suspend",
    "inner",
    "enum",
    "annotation",
    "value",
    "companion",
    "inline",
    "noinline",
    "crossinline",
    "reified",
    "infix",
    "operator",
    "data",
    "fun",
];

/// Modifiers that are part of a class's *kind*, and so are worth showing in the
/// outline's `detail`. Visibility is deliberately excluded — it is noise in an
/// outline and unstable across Kotlin styles.
const CLASS_DETAIL_WORDS: &[&str] = &[
    "expect",
    "actual",
    "external",
    "abstract",
    "open",
    "sealed",
    "data",
    "value",
    "inner",
    "enum",
    "annotation",
];

/// Node kinds that terminate a run of pending modifiers by being a declaration.
const fn is_declaration_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CLASS_DECL
            | SyntaxKind::OBJECT_DECL
            | SyntaxKind::COMPANION_OBJECT
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::FUNCTION_DECL
            | SyntaxKind::PROPERTY_DECL
            | SyntaxKind::CONSTRUCTOR_DECL
            | SyntaxKind::TYPEALIAS_DECL
            | SyntaxKind::INIT_BLOCK
    )
}

/// Build the document symbol tree for a Kotlin file.
///
/// `root` must be the `SOURCE_FILE` node of a tree produced by
/// `hyperion_kotlin::Parser`. `source` is the original text, used for byte-offset
/// to UTF-16 LSP position conversion.
pub fn kotlin_document_symbols(root: &SyntaxNode, source: &str) -> Vec<DocumentSymbol> {
    collect_container(root, source, Scope::File, None, false)
}

// ---------------------------------------------------------------------------
// Container walking
// ---------------------------------------------------------------------------

/// Accumulated modifiers preceding a declaration, plus the offset they start at
/// (so the declaration's range can cover `data class …`, not just `class …`).
#[derive(Default)]
struct Pending {
    words: Vec<String>,
    start: Option<TextSize>,
}

impl Pending {
    fn push_token(&mut self, token: &SyntaxToken) {
        self.start.get_or_insert(token.text_range().start());
        self.words.push(token.text().to_string());
    }

    /// Annotations (`@Foo`) contribute to the range but not to the word list.
    fn push_range_only(&mut self, start: TextSize) {
        self.start.get_or_insert(start);
    }

    fn has(&self, word: &str) -> bool {
        self.words.iter().any(|w| w == word)
    }

    fn clear(&mut self) {
        self.words.clear();
        self.start = None;
    }
}

/// Walk a `SOURCE_FILE` or `CLASS_BODY` and produce its symbols.
///
/// `enum_entries` enables the loose-`IDENT`-token scan used for enum bodies.
fn collect_container(
    container: &SyntaxNode,
    source: &str,
    scope: Scope,
    enclosing_name: Option<&str>,
    enum_entries: bool,
) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    let mut pending = Pending::default();

    // Enum-entry scanning state. `depth` counts bracket nesting among the
    // container's *direct* tokens; the CLASS_BODY's own `{`/`}` are direct
    // children, so entries sit at depth 1.
    let mut in_entries = enum_entries;
    let mut depth: i32 = 0;
    let mut entry: Option<EnumEntry> = None;

    for element in container.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind.is_trivia() {
                    continue;
                }

                if matches!(
                    kind,
                    SyntaxKind::L_BRACE
                        | SyntaxKind::L_PAREN
                        | SyntaxKind::L_BRACKET
                        | SyntaxKind::L_ANGLE
                ) {
                    depth += 1;
                    // `depth > 1` == "inside a group nested in the body", i.e. the
                    // entry's own argument list or body — never the body's own `{`.
                    if depth > 1 {
                        if let Some(e) = entry.as_mut() {
                            e.end = token.text_range().end();
                        }
                    }
                    continue;
                }
                if matches!(
                    kind,
                    SyntaxKind::R_BRACE
                        | SyntaxKind::R_PAREN
                        | SyntaxKind::R_BRACKET
                        | SyntaxKind::R_ANGLE
                ) {
                    depth -= 1;
                    // Decrement first: the class body's closing `}` lands at depth
                    // 0 and must not be pulled into the last enum entry's range.
                    if depth >= 1 {
                        if let Some(e) = entry.as_mut() {
                            e.end = token.text_range().end();
                        }
                    }
                    continue;
                }

                if in_entries && depth == 1 {
                    match kind {
                        SyntaxKind::COMMA => {
                            if let Some(e) = entry.take() {
                                out.push(e.finish(source));
                            }
                            continue;
                        }
                        SyntaxKind::SEMICOLON => {
                            if let Some(e) = entry.take() {
                                out.push(e.finish(source));
                            }
                            in_entries = false;
                            pending.clear();
                            continue;
                        }
                        SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT => {
                            if let Some(e) = entry.take() {
                                out.push(e.finish(source));
                            }
                            entry = Some(EnumEntry::new(&token, pending.start));
                            pending.clear();
                            continue;
                        }
                        _ => {}
                    }
                }

                if depth >= 1 {
                    if let Some(e) = entry.as_mut() {
                        e.end = token.text_range().end();
                        continue;
                    }
                }

                // Loose soft modifier (`enum`, `const`, `lateinit`, …) that the
                // parser's recovery path bumped into the container.
                if is_modifier_word(token.text()) {
                    pending.push_token(&token);
                } else {
                    pending.clear();
                }
            }

            NodeOrToken::Node(node) => {
                let kind = node.kind();

                // Anything nested inside a pending enum entry's argument list or
                // body just widens that entry (`RED { override fun f() {} }`).
                if depth > 1 {
                    if let Some(e) = entry.as_mut() {
                        e.end = node.text_range().end();
                        continue;
                    }
                }

                if kind == SyntaxKind::ENUM_ENTRY {
                    out.extend(enum_entry_symbol(&node, source));
                    pending.clear();
                    continue;
                }

                if kind == SyntaxKind::MODIFIERS {
                    if entry.is_some() {
                        // Zero-width MODIFIERS between an entry and its `,`/`;`.
                        continue;
                    }
                    // An empty MODIFIERS node is emitted on *every* pass of the
                    // declaration loop, including the retry after a loose soft
                    // modifier, so it must never clear what we already have.
                    for child in node.children_with_tokens() {
                        match child {
                            NodeOrToken::Token(t) if !t.kind().is_trivia() => {
                                if is_modifier_word(t.text()) {
                                    pending.push_token(&t);
                                } else {
                                    pending.push_range_only(t.text_range().start());
                                }
                            }
                            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ANNOTATION => {
                                pending.push_range_only(n.text_range().start());
                            }
                            _ => {}
                        }
                    }
                    continue;
                }

                // A real declaration at the top level of the body ends the enum
                // entry list (Kotlin also requires a `;`, but be forgiving).
                if in_entries && depth <= 1 && is_declaration_kind(kind) {
                    if let Some(e) = entry.take() {
                        out.push(e.finish(source));
                    }
                    in_entries = false;
                } else if let Some(e) = entry.as_mut() {
                    e.end = node.text_range().end();
                    continue;
                }

                if let Some(symbol) =
                    declaration_symbol(&node, source, scope, enclosing_name, &pending)
                {
                    out.push(symbol);
                }
                // Any non-MODIFIERS node ends the modifier run: either it consumed
                // them (a declaration) or they were error-recovery debris.
                pending.clear();
            }
        }
    }

    if let Some(e) = entry.take() {
        out.push(e.finish(source));
    }

    out
}

/// An `ENUM_ENTRY` node: `RED`, `RED(0xFF0000)`, `RED { … }`.
#[allow(deprecated)]
fn enum_entry_symbol(node: &SyntaxNode, source: &str) -> Option<DocumentSymbol> {
    let name_token = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT))?;
    let mut significant = node
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia());
    let start = significant.next()?.text_range().start();
    let end = significant
        .last()
        .map_or(name_token.text_range().end(), |token| {
            token.text_range().end()
        });
    Some(DocumentSymbol {
        name: name_token.text().trim_matches('`').to_string(),
        detail: Some("enum entry".to_string()),
        kind: LspSymbolKind::ENUM_MEMBER,
        range: to_range(source, start, end),
        selection_range: token_range(source, &name_token),
        children: None,
        tags: None,
        deprecated: None,
    })
}

/// A partially-scanned enum entry: `RED`, `RED(0xFF0000)`, `RED { … }`.
struct EnumEntry {
    name: String,
    name_range: rowan::TextRange,
    start: TextSize,
    end: TextSize,
}

impl EnumEntry {
    fn new(token: &SyntaxToken, modifier_start: Option<TextSize>) -> Self {
        let range = token.text_range();
        Self {
            name: token.text().to_string(),
            name_range: range,
            start: modifier_start.unwrap_or_else(|| range.start()),
            end: range.end(),
        }
    }

    #[allow(deprecated)]
    fn finish(self, source: &str) -> DocumentSymbol {
        DocumentSymbol {
            name: self.name,
            detail: Some("enum entry".to_string()),
            kind: LspSymbolKind::ENUM_MEMBER,
            range: to_range(source, self.start, self.end),
            selection_range: to_range(source, self.name_range.start(), self.name_range.end()),
            children: None,
            tags: None,
            deprecated: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[allow(deprecated)]
fn declaration_symbol(
    node: &SyntaxNode,
    source: &str,
    scope: Scope,
    enclosing_name: Option<&str>,
    pending: &Pending,
) -> Option<DocumentSymbol> {
    match node.kind() {
        SyntaxKind::CLASS_DECL => {
            let name_token = name_after(node, SyntaxKind::CLASS_KW, false)?;
            let name = name_token.text().to_string();
            let is_enum = pending.has("enum");

            let mut children = primary_constructor_properties(node, source);
            children.extend(class_body_members(node, source, &name, is_enum));

            Some(DocumentSymbol {
                name,
                detail: Some(class_detail(pending, "class")),
                kind: if is_enum {
                    LspSymbolKind::ENUM
                } else {
                    LspSymbolKind::CLASS
                },
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &name_token),
                children: none_if_empty(children),
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::OBJECT_DECL | SyntaxKind::COMPANION_OBJECT => {
            let is_companion = pending.has("companion");
            let name_token = name_after(node, SyntaxKind::OBJECT_KW, false);
            let keyword = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::OBJECT_KW);

            // `companion object { … }` may be anonymous; Kotlin names it `Companion`.
            let name = name_token.as_ref().map_or_else(
                || {
                    if is_companion {
                        "Companion".to_string()
                    } else {
                        "<anonymous>".to_string()
                    }
                },
                |t| t.text().to_string(),
            );

            let selection = name_token.as_ref().or(keyword.as_ref()).map_or_else(
                || decl_range(node, source, pending),
                |t| token_range(source, t),
            );

            let children = class_body_members(node, source, &name, false);

            Some(DocumentSymbol {
                name,
                detail: Some(
                    if is_companion {
                        "companion object"
                    } else {
                        "object"
                    }
                    .to_string(),
                ),
                kind: LspSymbolKind::OBJECT,
                range: decl_range(node, source, pending),
                selection_range: selection,
                children: none_if_empty(children),
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::INTERFACE_DECL => {
            let name_token = name_after(node, SyntaxKind::INTERFACE_KW, false)?;
            let name = name_token.text().to_string();
            let children = class_body_members(node, source, &name, false);

            Some(DocumentSymbol {
                name,
                detail: Some(
                    if pending.has("fun") {
                        "fun interface"
                    } else {
                        "interface"
                    }
                    .to_string(),
                ),
                kind: LspSymbolKind::INTERFACE,
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &name_token),
                children: none_if_empty(children),
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::FUNCTION_DECL => {
            let name_token = name_after(node, SyntaxKind::FUN_KW, true)?;
            Some(DocumentSymbol {
                name: name_token.text().to_string(),
                detail: Some(function_detail(node, &name_token)),
                kind: if scope == Scope::File {
                    LspSymbolKind::FUNCTION
                } else {
                    LspSymbolKind::METHOD
                },
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &name_token),
                children: None,
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::PROPERTY_DECL => {
            let keyword = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| matches!(t.kind(), SyntaxKind::VAL_KW | SyntaxKind::VAR_KW))?;
            let name_token = name_after(node, keyword.kind(), false)?;
            let type_text = first_type_ref(node);

            Some(DocumentSymbol {
                name: name_token.text().to_string(),
                detail: Some(property_detail(
                    pending,
                    keyword.text(),
                    name_token.text(),
                    type_text.as_deref(),
                )),
                kind: if pending.has("const") {
                    LspSymbolKind::CONSTANT
                } else {
                    LspSymbolKind::PROPERTY
                },
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &name_token),
                children: None,
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::CONSTRUCTOR_DECL => {
            let keyword = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::CONSTRUCTOR_KW)?;
            let params = parameter_list_text(node);

            Some(DocumentSymbol {
                name: enclosing_name.unwrap_or("constructor").to_string(),
                detail: Some(format!("constructor({params})")),
                kind: LspSymbolKind::CONSTRUCTOR,
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &keyword),
                children: None,
                tags: None,
                deprecated: None,
            })
        }

        SyntaxKind::TYPEALIAS_DECL => {
            let name_token = name_after(node, SyntaxKind::TYPEALIAS_KW, false)?;
            Some(DocumentSymbol {
                name: name_token.text().to_string(),
                detail: Some("typealias".to_string()),
                kind: LspSymbolKind::TYPE_PARAMETER,
                range: decl_range(node, source, pending),
                selection_range: token_range(source, &name_token),
                children: None,
                tags: None,
                deprecated: None,
            })
        }

        _ => None,
    }
}

/// Members of the `CLASS_BODY` child of a class/object/interface declaration.
fn class_body_members(
    node: &SyntaxNode,
    source: &str,
    name: &str,
    enum_entries: bool,
) -> Vec<DocumentSymbol> {
    node.children()
        .find(|c| c.kind() == SyntaxKind::CLASS_BODY)
        .map_or_else(Vec::new, |body| {
            collect_container(&body, source, Scope::Member, Some(name), enum_entries)
        })
}

/// `class Person(val name: String, var age: Int)` declares two properties. They
/// live in the class's `PARAMETER_LIST`, not its body.
#[allow(deprecated)]
fn primary_constructor_properties(node: &SyntaxNode, source: &str) -> Vec<DocumentSymbol> {
    let Some(list) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::PARAMETER_LIST)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for param in list
        .children()
        .filter(|c| c.kind() == SyntaxKind::PARAMETER)
    {
        let Some(keyword) = param
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| matches!(t.kind(), SyntaxKind::VAL_KW | SyntaxKind::VAR_KW))
        else {
            continue; // plain constructor parameter, not a property
        };
        let Some(name_token) = name_after(&param, keyword.kind(), false) else {
            continue;
        };
        let type_text = first_type_ref(&param);
        let detail = match type_text.as_deref() {
            Some(t) => format!("{} {}: {}", keyword.text(), name_token.text(), t),
            None => format!("{} {}", keyword.text(), name_token.text()),
        };

        out.push(DocumentSymbol {
            name: name_token.text().to_string(),
            detail: Some(detail),
            kind: LspSymbolKind::PROPERTY,
            range: trimmed_range(&param, source),
            selection_range: token_range(source, &name_token),
            children: None,
            tags: None,
            deprecated: None,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Detail strings
// ---------------------------------------------------------------------------

fn class_detail(pending: &Pending, keyword: &str) -> String {
    let mut parts: Vec<&str> = pending
        .words
        .iter()
        .filter(|w| CLASS_DETAIL_WORDS.contains(&w.as_str()))
        .map(String::as_str)
        .collect();
    parts.dedup();
    if parts.is_empty() {
        keyword.to_string()
    } else {
        format!("{} {keyword}", parts.join(" "))
    }
}

fn property_detail(
    pending: &Pending,
    keyword: &str,
    name: &str,
    type_text: Option<&str>,
) -> String {
    let mut detail = String::new();
    if pending.has("const") {
        detail.push_str("const ");
    }
    if pending.has("lateinit") {
        detail.push_str("lateinit ");
    }
    detail.push_str(keyword);
    detail.push(' ');
    detail.push_str(name);
    if let Some(t) = type_text {
        detail.push_str(": ");
        detail.push_str(t);
    }
    detail
}

/// `String.extFun(): Int`, `greet(): String`, `run(a: Int, b: String)`.
fn function_detail(node: &SyntaxNode, name_token: &SyntaxToken) -> String {
    let mut receiver = None;
    let mut return_type = None;
    let mut seen_params = false;

    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) => match child.kind() {
                SyntaxKind::PARAMETER_LIST => seen_params = true,
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE | SyntaxKind::FUNCTION_TYPE => {
                    if seen_params {
                        if return_type.is_none() {
                            return_type = Some(normalize_ws(&child.text().to_string()));
                        }
                    } else if receiver.is_none() {
                        receiver = Some(normalize_ws(&child.text().to_string()));
                    }
                }
                _ => {}
            },
            NodeOrToken::Token(_) => {}
        }
    }

    let mut detail = String::new();
    if let Some(r) = receiver {
        detail.push_str(&r);
        detail.push('.');
    }
    detail.push_str(name_token.text());
    detail.push('(');
    detail.push_str(&parameter_list_text(node));
    detail.push(')');
    if let Some(r) = return_type {
        detail.push_str(": ");
        detail.push_str(&r);
    }
    detail
}

/// `a: Int, b: String` for the declaration's `PARAMETER_LIST`.
fn parameter_list_text(node: &SyntaxNode) -> String {
    let Some(list) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::PARAMETER_LIST)
    else {
        return String::new();
    };

    list.children()
        .filter(|c| c.kind() == SyntaxKind::PARAMETER)
        .filter_map(|param| {
            let name = param
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| !t.kind().is_trivia())
                .find(|t| {
                    !matches!(
                        t.kind(),
                        SyntaxKind::VAL_KW
                            | SyntaxKind::VAR_KW
                            | SyntaxKind::VARARG_KW
                            | SyntaxKind::NOINLINE_KW
                            | SyntaxKind::CROSSINLINE_KW
                            | SyntaxKind::AT
                    )
                })?;
            if matches!(name.kind(), SyntaxKind::COLON | SyntaxKind::EQ) {
                return None;
            }
            Some(match first_type_ref(&param) {
                Some(t) => format!("{}: {}", name.text(), t),
                None => name.text().to_string(),
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn first_type_ref(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|c| {
            matches!(
                c.kind(),
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE | SyntaxKind::FUNCTION_TYPE
            )
        })
        .map(|t| normalize_ws(&t.text().to_string()))
}

// ---------------------------------------------------------------------------
// Tokens and ranges
// ---------------------------------------------------------------------------

fn is_modifier_word(text: &str) -> bool {
    MODIFIER_WORDS.contains(&text)
}

/// The first name-ish direct token after `keyword`.
///
/// `allow_keywords` is needed for functions: Kotlin lets soft keywords be
/// function names (`fun get()`), and the parser bumps them as keyword tokens.
fn name_after(node: &SyntaxNode, keyword: SyntaxKind, allow_keywords: bool) -> Option<SyntaxToken> {
    let mut seen = false;
    for token in node.children_with_tokens().filter_map(|e| e.into_token()) {
        let kind = token.kind();
        if kind.is_trivia() {
            continue;
        }
        if !seen {
            if kind == keyword {
                seen = true;
            }
            continue;
        }
        if matches!(kind, SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) {
            return Some(token);
        }
        // `fun String.ext()` puts a TYPE_REF node then a DOT before the name.
        if kind == SyntaxKind::DOT {
            continue;
        }
        if allow_keywords && !kind.is_trivia() && !is_punctuation(kind) {
            return Some(token);
        }
        if is_punctuation(kind) {
            continue;
        }
        return None;
    }
    None
}

const fn is_punctuation(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN
            | SyntaxKind::R_PAREN
            | SyntaxKind::L_BRACE
            | SyntaxKind::R_BRACE
            | SyntaxKind::L_BRACKET
            | SyntaxKind::R_BRACKET
            | SyntaxKind::L_ANGLE
            | SyntaxKind::R_ANGLE
            | SyntaxKind::LT
            | SyntaxKind::GT
            | SyntaxKind::COLON
            | SyntaxKind::SEMICOLON
            | SyntaxKind::COMMA
            | SyntaxKind::DOT
            | SyntaxKind::EQ
            | SyntaxKind::AT
            | SyntaxKind::QUESTION
    )
}

/// The declaration's range, widened to include preceding modifiers/annotations
/// and trimmed of the trailing trivia the parser swallows into declaration nodes.
fn decl_range(node: &SyntaxNode, source: &str, pending: &Pending) -> Range {
    let trimmed = trimmed_offsets(node);
    let start = match pending.start {
        Some(s) if s < trimmed.0 => s,
        _ => trimmed.0,
    };
    to_range(source, start, trimmed.1)
}

fn trimmed_range(node: &SyntaxNode, source: &str) -> Range {
    let (start, end) = trimmed_offsets(node);
    to_range(source, start, end)
}

/// Outermost non-trivia token offsets within `node`.
fn trimmed_offsets(node: &SyntaxNode) -> (TextSize, TextSize) {
    let range = node.text_range();
    let mut start = range.start();
    let mut end = range.end();

    let mut token = node.first_token();
    while let Some(t) = token {
        if t.text_range().start() >= range.end() {
            break;
        }
        if !t.kind().is_trivia() {
            start = t.text_range().start();
            break;
        }
        token = t.next_token();
    }

    let mut token = node.last_token();
    while let Some(t) = token {
        if t.text_range().end() <= range.start() {
            break;
        }
        if !t.kind().is_trivia() {
            end = t.text_range().end();
            break;
        }
        token = t.prev_token();
    }

    if end < start {
        end = start;
    }
    (start, end)
}

fn token_range(source: &str, token: &SyntaxToken) -> Range {
    to_range(source, token.text_range().start(), token.text_range().end())
}

fn to_range(source: &str, start: TextSize, end: TextSize) -> Range {
    Range {
        start: offset_to_position(source, usize::from(start)),
        end: offset_to_position(source, usize::from(end)),
    }
}

fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn none_if_empty(children: Vec<DocumentSymbol>) -> Option<Vec<DocumentSymbol>> {
    if children.is_empty() {
        None
    } else {
        Some(children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbols(source: &str) -> Vec<DocumentSymbol> {
        let tokens: Vec<_> = hyperion_kotlin::Lexer::new(source).collect();
        let (green, _errors) = hyperion_kotlin::Parser::parse(tokens);
        let root = SyntaxNode::new_root(green);
        kotlin_document_symbols(&root, source)
    }

    #[test]
    fn empty_file_has_no_symbols() {
        assert!(symbols("").is_empty());
    }

    #[test]
    fn top_level_function_is_function_not_method() {
        let syms = symbols("fun main() {}\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "main");
        assert_eq!(syms[0].kind, LspSymbolKind::FUNCTION);
        assert_eq!(syms[0].detail.as_deref(), Some("main()"));
        // range covers exactly `fun main() {}`
        assert_eq!(syms[0].range.start.character, 0);
        assert_eq!(syms[0].range.end.character, 13);
        // selection_range covers exactly `main`
        assert_eq!(syms[0].selection_range.start.character, 4);
        assert_eq!(syms[0].selection_range.end.character, 8);
    }

    #[test]
    fn data_class_detail_includes_modifier() {
        let syms = symbols("data class Point(val x: Int)\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].detail.as_deref(), Some("data class"));
        assert_eq!(syms[0].kind, LspSymbolKind::CLASS);
        // range starts at `data`, not at `class`
        assert_eq!(syms[0].range.start.character, 0);
    }

    #[test]
    fn property_declaration_range_excludes_trailing_trivia() {
        // parse_property_decl swallows trailing whitespace before checking for
        // accessors; the outline must not report a range spanning blank lines.
        let source = "val a = 1\n\n\nfun f() {}\n";
        let syms = symbols(source);
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0].name, "a");
        assert_eq!(syms[0].range.end.line, 0);
        assert_eq!(syms[0].range.end.character, 9);
    }

    #[test]
    fn member_function_is_method() {
        let syms = symbols("class A {\n    fun b() {}\n}\n");
        let children = syms[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind, LspSymbolKind::METHOD);
    }
}
