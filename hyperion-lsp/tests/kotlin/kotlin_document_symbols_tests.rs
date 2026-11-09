// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin document symbols (`textDocument/documentSymbol`).
//!
//! Kotlin is parsed by `hyperion-kotlin` into its own rowan tree with its own
//! `SyntaxKind` numbering. These tests pin the outline that
//! `SymbolsProvider::document_symbols` produces for a Kotlin buffer, and guard
//! against the regression where the Kotlin tree was walked with the *Java*
//! `SyntaxKind` enum.

use hyperion_lsp::position_utils::position_to_offset;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::symbols::SymbolsProvider;
use hyperion_semantics::db::{FileId, Language, SourceDatabase};
use std::sync::Arc;
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn symbols_for(source: &str, language: Language) -> Vec<DocumentSymbol> {
    let host = SemanticHost::for_tests();
    let provider = SymbolsProvider::new();
    let file_id = FileId::new(0);
    host.with_db(|guard| {
        guard.set_file_text(file_id, Arc::new(source.to_string()));
        guard.set_file_language(file_id, language);
    });
    provider
        .document_symbols(&host.read_now(), file_id, source)
        .unwrap()
}

fn kotlin_symbols(source: &str) -> Vec<DocumentSymbol> {
    symbols_for(source, Language::Kotlin)
}

/// The exact source text a symbol's range covers.
fn slice<'a>(source: &'a str, range: &Range) -> &'a str {
    let start = position_to_offset(source, range.start).expect("range start");
    let end = position_to_offset(source, range.end).expect("range end");
    &source[start..end]
}

fn kind_name(kind: SymbolKind) -> &'static str {
    const NAMES: &[(SymbolKind, &str)] = &[
        (SymbolKind::CLASS, "CLASS"),
        (SymbolKind::INTERFACE, "INTERFACE"),
        (SymbolKind::ENUM, "ENUM"),
        (SymbolKind::ENUM_MEMBER, "ENUM_MEMBER"),
        (SymbolKind::OBJECT, "OBJECT"),
        (SymbolKind::METHOD, "METHOD"),
        (SymbolKind::FUNCTION, "FUNCTION"),
        (SymbolKind::CONSTRUCTOR, "CONSTRUCTOR"),
        (SymbolKind::PROPERTY, "PROPERTY"),
        (SymbolKind::FIELD, "FIELD"),
        (SymbolKind::CONSTANT, "CONSTANT"),
        (SymbolKind::VARIABLE, "VARIABLE"),
        (SymbolKind::TYPE_PARAMETER, "TYPE_PARAMETER"),
    ];
    NAMES.iter().find(|(k, _)| *k == kind).map_or_else(
        || panic!("unexpected SymbolKind in Kotlin outline: {kind:?}"),
        |(_, name)| *name,
    )
}

/// Flatten the hierarchy into `"<indent><KIND> <name> :: <detail>"` lines so a
/// single `assert_eq!` pins names, kinds, details *and* nesting.
fn outline(symbols: &[DocumentSymbol]) -> Vec<String> {
    let mut lines = Vec::new();
    push_outline(symbols, 0, &mut lines);
    lines
}

fn push_outline(symbols: &[DocumentSymbol], depth: usize, lines: &mut Vec<String>) {
    for symbol in symbols {
        lines.push(format!(
            "{:indent$}{} {} :: {}",
            "",
            kind_name(symbol.kind),
            symbol.name,
            symbol.detail.as_deref().unwrap_or("-"),
            indent = depth * 2
        ));
        if let Some(children) = &symbol.children {
            push_outline(children, depth + 1, lines);
        }
    }
}

/// Every declaration kind the provider is expected to recognise.
const KITCHEN_SINK: &str = r#"package com.example

typealias Handler = (String) -> Unit

const val MAX: Int = 10

var counter = 0

fun topLevel(a: Int, b: String): Boolean = true

fun String.shout(): String = uppercase()

data class Person(val name: String, var age: Int) {
    val label: String get() = name

    fun greet(): String = "hi"

    constructor(name: String) : this(name, 0)

    companion object {
        const val UNKNOWN = "?"
        fun of(name: String): Person = Person(name)
    }

    class Address(val city: String)
}

sealed class Shape {
    class Circle(val radius: Double) : Shape()
    object Empty : Shape()
}

interface Greeter {
    val prefix: String
    fun greet(name: String): String
}

object Registry : Greeter {
    override val prefix = ">"
    override fun greet(name: String) = prefix + name
}

enum class Color(val rgb: Int) {
    RED(0xFF0000),
    GREEN(0x00FF00);

    fun hex(): String = ""
}
"#;

// ---------------------------------------------------------------------------
// Structure: names, kinds, details, nesting
// ---------------------------------------------------------------------------

#[test]
fn kitchen_sink_outline_is_exact() {
    let symbols = kotlin_symbols(KITCHEN_SINK);
    let expected = vec![
        "TYPE_PARAMETER Handler :: typealias",
        "CONSTANT MAX :: const val MAX: Int",
        "PROPERTY counter :: var counter",
        "FUNCTION topLevel :: topLevel(a: Int, b: String): Boolean",
        "FUNCTION shout :: String.shout(): String",
        "CLASS Person :: data class",
        "  PROPERTY name :: val name: String",
        "  PROPERTY age :: var age: Int",
        "  PROPERTY label :: val label: String",
        "  METHOD greet :: greet(): String",
        "  CONSTRUCTOR Person :: constructor(name: String)",
        "  OBJECT Companion :: companion object",
        "    CONSTANT UNKNOWN :: const val UNKNOWN",
        "    METHOD of :: of(name: String): Person",
        "  CLASS Address :: class",
        "    PROPERTY city :: val city: String",
        "CLASS Shape :: sealed class",
        "  CLASS Circle :: class",
        "    PROPERTY radius :: val radius: Double",
        "  OBJECT Empty :: object",
        "INTERFACE Greeter :: interface",
        "  PROPERTY prefix :: val prefix: String",
        "  METHOD greet :: greet(name: String): String",
        "OBJECT Registry :: object",
        "  PROPERTY prefix :: val prefix",
        "  METHOD greet :: greet(name: String)",
        "ENUM Color :: enum class",
        "  PROPERTY rgb :: val rgb: Int",
        "  ENUM_MEMBER RED :: enum entry",
        "  ENUM_MEMBER GREEN :: enum entry",
        "  METHOD hex :: hex(): String",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();

    assert_eq!(outline(&symbols), expected);
}

// ---------------------------------------------------------------------------
// Ranges
// ---------------------------------------------------------------------------

/// `range` must cover the whole declaration (modifiers included) and `nothing`
/// more; `selection_range` must cover exactly the name.
#[test]
fn ranges_cover_declarations_and_names_exactly() {
    let symbols = kotlin_symbols(KITCHEN_SINK);

    let handler = &symbols[0];
    assert_eq!(
        slice(KITCHEN_SINK, &handler.range),
        "typealias Handler = (String) -> Unit"
    );
    assert_eq!(slice(KITCHEN_SINK, &handler.selection_range), "Handler");

    // Soft modifiers (`const`) reach the walker as loose tokens; the range must
    // still start at `const`, not at `val`.
    let max = &symbols[1];
    assert_eq!(slice(KITCHEN_SINK, &max.range), "const val MAX: Int = 10");
    assert_eq!(slice(KITCHEN_SINK, &max.selection_range), "MAX");

    let top_level = &symbols[3];
    assert_eq!(
        slice(KITCHEN_SINK, &top_level.range),
        "fun topLevel(a: Int, b: String): Boolean = true"
    );
    assert_eq!(slice(KITCHEN_SINK, &top_level.selection_range), "topLevel");

    let shout = &symbols[4];
    assert_eq!(
        slice(KITCHEN_SINK, &shout.range),
        "fun String.shout(): String = uppercase()"
    );
    assert_eq!(slice(KITCHEN_SINK, &shout.selection_range), "shout");

    // `data` is parsed into a MODIFIERS node that is a *sibling* of CLASS_DECL.
    let person = &symbols[5];
    assert!(
        slice(KITCHEN_SINK, &person.range).starts_with("data class Person(val name: String,"),
        "class range must start at the `data` modifier, got: {:?}",
        slice(KITCHEN_SINK, &person.range)
    );
    assert!(slice(KITCHEN_SINK, &person.range).ends_with('}'));
    assert_eq!(slice(KITCHEN_SINK, &person.selection_range), "Person");

    let person_children = person.children.as_ref().unwrap();
    assert_eq!(
        slice(KITCHEN_SINK, &person_children[0].range),
        "val name: String"
    );
    // A property with an accessor: the parser swallows trailing trivia into
    // PROPERTY_DECL, so the range has to be trimmed back to `name`.
    assert_eq!(
        slice(KITCHEN_SINK, &person_children[2].range),
        "val label: String get() = name"
    );
    assert_eq!(
        slice(KITCHEN_SINK, &person_children[4].range),
        "constructor(name: String) : this(name, 0)"
    );
    assert_eq!(
        slice(KITCHEN_SINK, &person_children[4].selection_range),
        "constructor"
    );

    // An abstract interface member has no body, so its declaration node ends
    // with the return type's trailing newline. That must be trimmed.
    let greeter = &symbols[7];
    assert_eq!(greeter.name, "Greeter");
    let greeter_children = greeter.children.as_ref().unwrap();
    assert_eq!(
        slice(KITCHEN_SINK, &greeter_children[1].range),
        "fun greet(name: String): String"
    );

    // Enum entries: the range covers the constructor arguments too.
    let color = &symbols[9];
    assert_eq!(color.name, "Color");
    let color_children = color.children.as_ref().unwrap();
    assert_eq!(
        slice(KITCHEN_SINK, &color_children[1].range),
        "RED(0xFF0000)"
    );
    assert_eq!(
        slice(KITCHEN_SINK, &color_children[1].selection_range),
        "RED"
    );
    assert_eq!(
        slice(KITCHEN_SINK, &color_children[2].range),
        "GREEN(0x00FF00)"
    );
}

#[test]
fn every_selection_range_is_the_symbol_name() {
    let symbols = kotlin_symbols(KITCHEN_SINK);
    let mut checked = 0usize;
    check_selection_ranges(&symbols, KITCHEN_SINK, &mut checked);
    assert!(checked >= 30, "expected a rich outline, checked {checked}");
}

fn check_selection_ranges(symbols: &[DocumentSymbol], source: &str, checked: &mut usize) {
    for symbol in symbols {
        let text = slice(source, &symbol.selection_range);
        // Anonymous companion objects and constructors are named after their
        // container / keyword rather than a name token.
        if symbol.kind != SymbolKind::CONSTRUCTOR && symbol.name != "Companion" {
            assert_eq!(
                text, symbol.name,
                "selection_range of {:?} points at {:?}",
                symbol.name, text
            );
        }
        // The declaration range must contain the selection range.
        let key = |p: &tower_lsp::lsp_types::Position| (p.line, p.character);
        assert!(
            key(&symbol.range.start) <= key(&symbol.selection_range.start)
                && key(&symbol.selection_range.end) <= key(&symbol.range.end),
            "selection_range of {:?} escapes its range",
            symbol.name
        );
        *checked += 1;
        if let Some(children) = &symbol.children {
            check_selection_ranges(children, source, checked);
        }
    }
}

// ---------------------------------------------------------------------------
// Regression: Kotlin buffers must not be walked with the Java SyntaxKind enum
// ---------------------------------------------------------------------------

/// The two parsers number their `SyntaxKind` enums independently, so a Kotlin
/// green tree read through `hyperion_parser::SyntaxKind` is nonsense. This
/// documents the mismatch that made the old shared code path wrong.
#[test]
fn kotlin_and_java_syntax_kinds_do_not_agree() {
    let kotlin_class = hyperion_kotlin::SyntaxKind::CLASS_DECL as u16;
    let as_java = hyperion_parser::SyntaxKind::from(kotlin_class);
    assert_ne!(
        as_java,
        hyperion_parser::SyntaxKind::CLASS_DECL,
        "if the numbering ever coincides this regression test stops proving anything"
    );

    let kotlin_fun = hyperion_kotlin::SyntaxKind::FUNCTION_DECL as u16;
    assert_ne!(
        hyperion_parser::SyntaxKind::from(kotlin_fun),
        hyperion_parser::SyntaxKind::METHOD_DECL
    );
}

/// A Kotlin buffer must produce Kotlin symbols — not the empty/garbage outline
/// the Java walker yields when it mis-decodes Kotlin node kinds.
#[test]
fn kotlin_buffer_does_not_use_the_java_symbol_path() {
    let source = "class Widget {\n    fun render(): String = \"\"\n}\n";
    let symbols = kotlin_symbols(source);

    assert_eq!(symbols.len(), 1, "got: {:?}", outline(&symbols));
    assert_eq!(symbols[0].name, "Widget");
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);

    let children = symbols[0]
        .children
        .as_ref()
        .expect("Kotlin class members must be nested children");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "render");
    assert_eq!(children[0].kind, SymbolKind::METHOD);

    // The Java walker only knows FIELD_DECL/METHOD_DECL/... — it can never
    // produce Kotlin-specific kinds, so their presence proves the Kotlin path
    // ran. Check that on a file that exercises them.
    let kotlin_only = kotlin_symbols("enum class E { A }\nobject O\ntypealias T = Int\n");
    let kinds: Vec<SymbolKind> = kotlin_only.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            SymbolKind::ENUM,
            SymbolKind::OBJECT,
            SymbolKind::TYPE_PARAMETER
        ],
        "got: {:?}",
        outline(&kotlin_only)
    );
    assert_eq!(
        kotlin_only[0]
            .children
            .as_ref()
            .map(|c| c.iter().map(|s| s.name.clone()).collect::<Vec<_>>()),
        Some(vec!["A".to_string()])
    );
}

/// Java buffers keep using the Java walker — the new dispatch must not change
/// existing behaviour.
#[test]
fn java_buffers_still_use_the_java_walker() {
    let symbols = symbols_for(
        "class Foo {\n    private int bar;\n    void baz() {}\n}\n",
        Language::Java,
    );
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Foo");
    assert_eq!(symbols[0].kind, SymbolKind::CLASS);
    let children = symbols[0].children.as_ref().expect("class members");
    assert!(children.iter().any(|s| s.kind == SymbolKind::FIELD));
    assert!(children.iter().any(|s| s.kind == SymbolKind::METHOD));
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_and_trivial_kotlin_files() {
    assert!(kotlin_symbols("").is_empty());
    assert!(kotlin_symbols("// just a comment\n").is_empty());
    assert!(kotlin_symbols("package a.b\n\nimport c.d\n").is_empty());
}

#[test]
fn malformed_kotlin_does_not_panic_or_invent_symbols() {
    let symbols = kotlin_symbols("class {\n  fun (\n");
    // Names come from real identifier tokens only; a nameless class yields none.
    for symbol in &symbols {
        assert!(
            !symbol.name.is_empty(),
            "invented an empty symbol name: {symbol:?}"
        );
    }
}

#[test]
fn nested_and_inner_classes_nest() {
    let symbols = kotlin_symbols("class A {\n  inner class B {\n    class C\n  }\n}\n");
    let a = &symbols[0];
    let b = &a.children.as_ref().unwrap()[0];
    assert_eq!(b.name, "B");
    assert_eq!(b.detail.as_deref(), Some("inner class"));
    let c = &b.children.as_ref().unwrap()[0];
    assert_eq!(c.name, "C");
    assert_eq!(c.kind, SymbolKind::CLASS);
}

#[test]
fn suspend_and_generic_functions_are_recognised() {
    let symbols = kotlin_symbols("suspend fun <T> load(id: Long): T? = null\n");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "load");
    assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
    assert_eq!(
        slice(
            "suspend fun <T> load(id: Long): T? = null\n",
            &symbols[0].range
        ),
        "suspend fun <T> load(id: Long): T? = null"
    );
}
