// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin-native inspections.
//!
//! These walk the **Kotlin** green tree (`hyperion_kotlin::SyntaxNode`) with Kotlin
//! semantics. They are deliberately *not* a port of the Java inspections in the sibling
//! modules: Java inspections compare `hyperion_parser::SyntaxKind` discriminants, which
//! mean something entirely different in a Kotlin tree.
//!
//! # Design priority: zero false positives
//!
//! A missed warning is invisible. A wrong warning is corrosive — users stop trusting
//! every diagnostic the server emits. Every rule below therefore errs towards silence:
//!
//! * The whole registry is skipped when the file has **any** parse error (or any
//!   `ERROR` node), because error recovery relocates identifiers and a relocated
//!   identifier is indistinguishable from a missing reference.
//! * "Is this name referenced?" is answered by an intentionally *over-broad* textual
//!   reference index (string-template interpolations and KDoc `[links]` count as
//!   references), so ambiguity resolves to "used".
//! * Shadowed / re-declared names are skipped outright rather than guessed at.

mod redundant_type;
mod unreachable_code;
mod unused_import;
mod unused_local;

use std::collections::HashMap;

use hyperion_kotlin::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{NodeOrToken, TextRange};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString};

use crate::position_utils::text_range_to_lsp_range;

/// `Diagnostic::source` reported for every Kotlin inspection.
pub const KOTLIN_LINT_SOURCE: &str = "hyperion-lint";

/// A single inspection over a Kotlin syntax tree.
///
/// Mirrors the Java framework's shape (one self-contained check per rule, aggregated by
/// a registry) but is trait-based so the registry can be enumerated — the diagnostic
/// codes are part of the public contract and are asserted by tests.
pub trait KotlinInspection: Sync {
    /// Stable diagnostic code, e.g. `KT-UNUSED-IMPORT`.
    fn code(&self) -> &'static str;

    /// Runs the inspection over a pre-analysed file.
    fn run(&self, ctx: &KotlinInspectionContext<'_>) -> Vec<Diagnostic>;
}

/// The registry of Kotlin-native inspections, in reporting order.
static REGISTRY: &[&dyn KotlinInspection] = &[
    &unused_import::UnusedImport,
    &unused_local::UnusedLocalVariable,
    &redundant_type::RedundantExplicitType,
    &unreachable_code::UnreachableCode,
];

/// All registered Kotlin inspections.
pub fn registry() -> &'static [&'static dyn KotlinInspection] {
    REGISTRY
}

/// Every diagnostic code this module can emit.
pub fn kotlin_inspection_codes() -> Vec<&'static str> {
    REGISTRY
        .iter()
        .map(|inspection| inspection.code())
        .collect()
}

/// Runs all Kotlin inspections against a parsed Kotlin file.
///
/// `has_parse_errors` comes from the parser's error list. Inspections are suppressed
/// entirely when it is `true` — see the module docs.
pub fn run_kotlin_inspections(
    root: &SyntaxNode,
    source_text: &str,
    has_parse_errors: bool,
) -> Vec<Diagnostic> {
    if has_parse_errors || tree_contains_error_node(root) {
        return Vec::new();
    }

    let ctx = KotlinInspectionContext::new(root, source_text);

    let mut diagnostics = Vec::new();
    for inspection in REGISTRY {
        diagnostics.extend(inspection.run(&ctx));
    }
    diagnostics
}

fn tree_contains_error_node(root: &SyntaxNode) -> bool {
    root.descendants()
        .any(|node| node.kind() == SyntaxKind::ERROR)
}

// ── Reference index ──

/// How an identifier occurrence was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefKind {
    /// Bare identifier in expression / type position. Can resolve to a local.
    Bare,
    /// Identifier following `.`, `?.` or `::` — always a member selector, never a local.
    Selector,
    /// Name mentioned inside a string-template interpolation (`"$x"`, `"${x.y}"`).
    Template,
    /// Name mentioned inside a KDoc-style `[link]`.
    Doc,
    /// Bare identifier that is the target of a plain statement-level `=` assignment.
    /// A write is not a use.
    Write,
}

impl RefKind {
    /// Whether this occurrence counts as *reading* a local binding.
    const fn reads_local(self) -> bool {
        matches!(self, Self::Bare | Self::Template | Self::Doc)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Reference {
    pub(crate) name: String,
    pub(crate) range: TextRange,
    pub(crate) kind: RefKind,
}

/// Pre-computed analysis shared by every inspection in the registry.
pub struct KotlinInspectionContext<'a> {
    root: &'a SyntaxNode,
    source_text: &'a str,
    /// Every identifier-ish occurrence in the file, excluding `import`/`package` headers.
    references: Vec<Reference>,
    /// How many times each name appears in a *declaration* position anywhere in the file.
    /// Used to bail out of shadowing / re-declaration entirely.
    declaration_counts: HashMap<String, usize>,
}

impl<'a> KotlinInspectionContext<'a> {
    fn new(root: &'a SyntaxNode, source_text: &'a str) -> Self {
        Self {
            root,
            source_text,
            references: collect_references(root),
            declaration_counts: collect_declaration_counts(root),
        }
    }

    pub(crate) const fn root(&self) -> &SyntaxNode {
        self.root
    }

    pub(crate) const fn source_text(&self) -> &str {
        self.source_text
    }

    /// True when `name` occurs anywhere outside the import/package header, in any form.
    ///
    /// Deliberately includes member selectors: `import a.b.Outer.Inner` is genuinely used
    /// by `Outer.Inner`, where `Inner` is written as a selector.
    pub(crate) fn name_occurs_anywhere(&self, name: &str) -> bool {
        self.references.iter().any(|r| r.name == name)
    }

    /// True when `name` is *read* somewhere inside `scope`, ignoring the declaration site.
    pub(crate) fn name_read_in_scope(
        &self,
        name: &str,
        scope: TextRange,
        declaration_site: TextRange,
    ) -> bool {
        self.references.iter().any(|r| {
            r.kind.reads_local()
                && r.name == name
                && r.range != declaration_site
                && scope.contains_range(r.range)
        })
    }

    /// True when the name is bound more than once in this file (shadowing, re-declaration,
    /// same-named parameter, ...). Such names are never reported.
    pub(crate) fn is_ambiguously_declared(&self, name: &str) -> bool {
        self.declaration_counts
            .get(name)
            .copied()
            .unwrap_or_default()
            > 1
    }

    pub(crate) fn diagnostic(
        &self,
        range: TextRange,
        code: &'static str,
        message: impl Into<String>,
        tags: Option<Vec<DiagnosticTag>>,
    ) -> Diagnostic {
        Diagnostic {
            range: text_range_to_lsp_range(range, self.source_text),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(code.to_string())),
            code_description: None,
            source: Some(KOTLIN_LINT_SOURCE.to_string()),
            message: message.into(),
            related_information: None,
            tags,
            data: None,
        }
    }
}

fn collect_references(root: &SyntaxNode) -> Vec<Reference> {
    let mut references = Vec::new();

    for element in root.descendants_with_tokens() {
        let NodeOrToken::Token(token) = element else {
            continue;
        };

        let kind = token.kind();
        if is_identifier_kind(kind) {
            if is_in_header(&token) {
                continue;
            }
            let ref_kind = if is_member_selector(&token) {
                RefKind::Selector
            } else if is_statement_assignment_target(&token) {
                RefKind::Write
            } else {
                RefKind::Bare
            };
            references.push(Reference {
                name: identifier_text(&token),
                range: token.text_range(),
                kind: ref_kind,
            });
        } else if kind.is_string_literal() {
            for name in scan_template_names(token.text()) {
                references.push(Reference {
                    name,
                    range: token.text_range(),
                    kind: RefKind::Template,
                });
            }
        } else if kind.is_comment() {
            for name in scan_doc_link_names(token.text()) {
                references.push(Reference {
                    name,
                    range: token.text_range(),
                    kind: RefKind::Doc,
                });
            }
        }
    }

    references
}

fn collect_declaration_counts(root: &SyntaxNode) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for node in root.descendants() {
        match node.kind() {
            // Binding forms whose *leading* direct identifiers are all names.
            SyntaxKind::PROPERTY_DECL | SyntaxKind::DESTRUCTURING_DECL => {
                for name in leading_declaration_names(&node) {
                    *counts.entry(name).or_default() += 1;
                }
            }
            // Binding forms with exactly one name: the first direct identifier.
            SyntaxKind::PARAMETER
            | SyntaxKind::LAMBDA_PARAMETERS
            | SyntaxKind::FOR_LOOP_VAR
            | SyntaxKind::TYPE_PARAMETER
            | SyntaxKind::FUNCTION_DECL
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::OBJECT_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::TYPEALIAS_DECL => {
                if let Some(name) = first_direct_identifier(&node) {
                    *counts.entry(name).or_default() += 1;
                }
            }
            _ => {}
        }
    }

    counts
}

/// Direct-child identifiers appearing before the `=` of a binding form.
///
/// `val x = ...` yields `["x"]`; `val (a, b) = ...` yields `["a", "b"]`.
fn leading_declaration_names(node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    for element in node.children_with_tokens() {
        let NodeOrToken::Token(token) = element else {
            // A TYPE_REF (or any node) before `=` is not a name; keep scanning.
            continue;
        };
        match token.kind() {
            SyntaxKind::EQ | SyntaxKind::BY_KW | SyntaxKind::COLON => break,
            k if is_identifier_kind(k) => names.push(identifier_text(&token)),
            _ => {}
        }
    }
    names
}

fn first_direct_identifier(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .find_map(|element| match element {
            NodeOrToken::Token(token) if is_identifier_kind(token.kind()) => {
                Some(identifier_text(&token))
            }
            _ => None,
        })
}

// ── Token helpers ──

pub(crate) const fn is_identifier_kind(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT)
}

/// Text of an identifier with backticks stripped, so `` `foo` `` and `foo` unify.
pub(crate) fn identifier_text(token: &SyntaxToken) -> String {
    token.text().trim_matches('`').to_string()
}

fn is_in_header(token: &SyntaxToken) -> bool {
    let mut current = token.parent();
    while let Some(node) = current {
        if matches!(
            node.kind(),
            SyntaxKind::IMPORT_DECL | SyntaxKind::PACKAGE_DECL
        ) {
            return true;
        }
        current = node.parent();
    }
    false
}

fn previous_significant_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    let mut current = token.prev_token();
    while let Some(candidate) = current {
        if !candidate.kind().is_trivia() {
            return Some(candidate);
        }
        current = candidate.prev_token();
    }
    None
}

/// An identifier directly after `.`, `?.` or `::` selects a member and can never be a
/// reference to a local binding.
fn is_member_selector(token: &SyntaxToken) -> bool {
    matches!(
        previous_significant_token(token).map(|t| t.kind()),
        Some(SyntaxKind::DOT | SyntaxKind::SAFE_CALL | SyntaxKind::COLONCOLON)
    )
}

/// `x = expr` as a statement — a pure write.
///
/// Restricted to statement position on purpose: `foo(x = 1)` parses with the same
/// `BINARY_EXPR` shape but is a named argument, and treating that as a write would be a
/// false positive waiting to happen. Compound assignments (`x += 1`) parse as
/// `IDENT PLUS EQ` and therefore correctly fall through to `Bare` (a read).
fn is_statement_assignment_target(token: &SyntaxToken) -> bool {
    let Some(parent) = token.parent() else {
        return false;
    };
    if parent.kind() != SyntaxKind::BINARY_EXPR {
        return false;
    }
    let grandparent_kind = parent.parent().map(|node| node.kind());
    if !matches!(
        grandparent_kind,
        Some(SyntaxKind::EXPR_STMT | SyntaxKind::BLOCK | SyntaxKind::LAMBDA_EXPR)
    ) {
        return false;
    }

    let significant: Vec<_> = parent
        .children_with_tokens()
        .filter(|element| !element.kind().is_trivia())
        .collect();

    let is_first = matches!(
        significant.first(),
        Some(NodeOrToken::Token(first)) if first.text_range() == token.text_range()
    );
    let followed_by_eq = matches!(
        significant.get(1),
        Some(NodeOrToken::Token(next)) if next.kind() == SyntaxKind::EQ
    );

    is_first && followed_by_eq
}

/// The range of a node with leading/trailing trivia removed.
pub(crate) fn trimmed_range(node: &SyntaxNode) -> Option<TextRange> {
    let mut tokens = node
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) if !token.kind().is_trivia() => Some(token.text_range()),
            _ => None,
        });
    let first = tokens.next()?;
    let last = tokens.last().unwrap_or(first);
    Some(TextRange::new(first.start(), last.end()))
}

/// Zero-based line index of a byte offset.
pub(crate) fn line_of(offset: usize, source_text: &str) -> usize {
    source_text
        .get(..offset.min(source_text.len()))
        .map_or(0, |prefix| prefix.bytes().filter(|b| *b == b'\n').count())
}

// ── Textual scanners ──

/// Identifier-ish words used inside string-template interpolations.
///
/// `"$x"` yields `x`; `"${a.b + c}"` yields `a`, `b`, `c`. Over-collecting is intentional:
/// the result is only ever used to decide that something *is* used.
fn scan_template_names(text: &str) -> Vec<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut names = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != '$' {
            index += 1;
            continue;
        }
        // `\$` is an escaped dollar, not an interpolation.
        if index > 0 && bytes[index - 1] == '\\' {
            index += 1;
            continue;
        }
        index += 1;
        if index >= bytes.len() {
            break;
        }

        if bytes[index] == '{' {
            let mut depth = 1;
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && depth > 0 {
                match bytes[end] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    end += 1;
                }
            }
            let inner: String = bytes[start..end.min(bytes.len())].iter().collect();
            names.extend(identifier_words(&inner));
            index = end + 1;
        } else {
            let start = index;
            while index < bytes.len() && is_identifier_char(bytes[index]) {
                index += 1;
            }
            if index > start {
                let word: String = bytes[start..index].iter().collect();
                if !word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    names.push(word);
                }
            }
        }
    }

    names
}

/// Names referenced from a KDoc `[link]`.
///
/// kotlinc does not resolve KDoc and would report such an import as unused. We treat the
/// link as a use instead: an import that only documentation needs is still needed, and
/// removing it breaks the doc link. That trade favours the "never warn wrongly" rule.
fn scan_doc_link_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut names = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '[' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < chars.len() && chars[end] != ']' {
            end += 1;
        }
        if end >= chars.len() {
            break;
        }
        let inner: String = chars[start..end].iter().collect();
        names.extend(identifier_words(&inner));
        index = end + 1;
    }

    names
}

fn is_identifier_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn identifier_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if is_identifier_char(c) {
            current.push(c);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
        .into_iter()
        .filter(|word| !word.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .collect()
}

/// Scopes in which a `val`/`var` is a *local* binding rather than a member/top-level one.
pub(crate) const fn is_local_scope(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::BLOCK | SyntaxKind::LAMBDA_EXPR)
}

/// Names that a file can use without ever spelling them out.
///
/// Kotlin resolves operators, destructuring and delegation to conventionally named
/// functions, so `import a.b.plus` is used by `x + y` with no textual occurrence of
/// `plus`. Flagging those would be a false positive, so the whole convention set is
/// exempt from the unused-import rule.
pub(crate) fn is_convention_name(name: &str) -> bool {
    const CONVENTIONS: &[&str] = &[
        "compareTo",
        "contains",
        "dec",
        "div",
        "divAssign",
        "equals",
        "get",
        "getValue",
        "hasNext",
        "hashCode",
        "inc",
        "invoke",
        "iterator",
        "minus",
        "minusAssign",
        "mod",
        "modAssign",
        "next",
        "not",
        "plus",
        "plusAssign",
        "provideDelegate",
        "rangeTo",
        "rangeUntil",
        "rem",
        "remAssign",
        "set",
        "setValue",
        "times",
        "timesAssign",
        "toString",
        "unaryMinus",
        "unaryPlus",
    ];

    if CONVENTIONS.contains(&name) {
        return true;
    }

    // componentN, used by destructuring declarations.
    name.strip_prefix("component")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
pub(crate) fn parse_for_test(source: &str) -> (SyntaxNode, bool) {
    let tree = hyperion_kotlin::parse(source);
    let root = SyntaxNode::new_root(tree.green_node.clone());
    (root, !tree.errors.is_empty())
}

#[cfg(test)]
pub(crate) fn inspect(source: &str) -> Vec<Diagnostic> {
    let (root, has_errors) = parse_for_test(source);
    run_kotlin_inspections(&root, source, has_errors)
}

#[cfg(test)]
pub(crate) fn inspect_code(source: &str, code: &str) -> Vec<Diagnostic> {
    inspect(source)
        .into_iter()
        .filter(|d| d.code == Some(NumberOrString::String(code.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_codes_are_stable_and_unique() {
        let codes = kotlin_inspection_codes();
        assert_eq!(
            codes,
            vec![
                "KT-UNUSED-IMPORT",
                "KT-UNUSED-VARIABLE",
                "KT-REDUNDANT-EXPLICIT-TYPE",
                "KT-UNREACHABLE-CODE",
            ]
        );
        let unique: HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "diagnostic codes must be unique");
    }

    #[test]
    fn every_inspection_reports_warning_severity() {
        let source = r#"
import a.b.Unused

fun f(): Int {
    val dead = 1
    val typed: Int = 5
    println(typed)
    return 1
    println("after")
}
"#;
        let diagnostics = inspect(source);
        assert!(!diagnostics.is_empty());
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
            assert_eq!(diagnostic.source.as_deref(), Some(KOTLIN_LINT_SOURCE));
        }
        assert!(diagnostics.iter().any(|d| d.message.contains("dead")));
    }

    #[test]
    fn parse_errors_suppress_every_inspection() {
        // A declaration without a name is a parse error; the error gate must silence
        // the whole registry rather than emit guesses from a recovered tree.
        let source = r"
import a.b.Unused

fun f() {
    val = 1
}
";
        let (root, has_errors) = parse_for_test(source);
        assert!(has_errors, "fixture must actually fail to parse");
        assert!(run_kotlin_inspections(&root, source, has_errors).is_empty());
    }

    #[test]
    fn template_scanner_finds_simple_and_braced_interpolations() {
        assert_eq!(scan_template_names("\"$x\""), vec!["x".to_string()]);
        assert_eq!(
            scan_template_names("\"${a.b + c}\""),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(scan_template_names("\"\\$x\"").is_empty());
        assert!(scan_template_names("\"plain\"").is_empty());
    }

    #[test]
    fn doc_scanner_finds_kdoc_links() {
        assert_eq!(
            scan_doc_link_names("/** See [Widget] for details. */"),
            vec!["Widget".to_string()]
        );
        assert_eq!(
            scan_doc_link_names("/** [Outer.Inner] */"),
            vec!["Outer".to_string(), "Inner".to_string()]
        );
        assert!(scan_doc_link_names("/** no links */").is_empty());
    }

    #[test]
    fn convention_names_cover_operators_and_components() {
        assert!(is_convention_name("plus"));
        assert!(is_convention_name("getValue"));
        assert!(is_convention_name("component1"));
        assert!(is_convention_name("component12"));
        assert!(!is_convention_name("component"));
        assert!(!is_convention_name("componentX"));
        assert!(!is_convention_name("Widget"));
    }
}
