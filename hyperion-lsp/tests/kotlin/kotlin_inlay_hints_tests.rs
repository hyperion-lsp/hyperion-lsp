// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin inlay hints (`textDocument/inlayHint`).
//!
//! Two families are pinned here: `: Type` hints on `val`/`var` declarations
//! written without an annotation, and `name = ` parameter hints at call sites.
//! The load-bearing negative cases are "the declaration already says the type"
//! and "inference returned `Unknown`" — a `?` placeholder must never reach the
//! editor.

use hyperion_lsp::inlay_hints_kotlin::KotlinInlayHintsProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct KotlinWorkspace {
    _temp: TempDir,
    root: PathBuf,
    index: Arc<WorkspaceIndex>,
    host: Arc<SemanticHost>,
}

fn workspace(files: &[(&str, &str)]) -> KotlinWorkspace {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(&root).expect("workspace dir");

    let index = WorkspaceIndex::new(root.clone());
    for (rel_path, content) in files {
        let full = root.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent dir");
        }
        std::fs::write(&full, content).expect("write source");
        index
            .index_file_with_source(&full, content)
            .expect("index source");
    }

    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|guard| {
        for (rel_path, content) in files {
            let full = root.join(rel_path);
            let file_id = index.get_file_id(&full).expect("file indexed");
            guard.set_file_text(file_id, Arc::new((*content).to_string()));
            guard.set_file_language(file_id, hyperion_workspace::Language::Kotlin);
        }
    });

    KotlinWorkspace {
        _temp: temp,
        root,
        index,
        host,
    }
}

impl KotlinWorkspace {
    fn file_id(&self, rel_path: &str) -> FileId {
        self.index
            .get_file_id(&self.root.join(rel_path))
            .expect("file indexed")
    }

    fn hints(&self, rel_path: &str, source: &str) -> Vec<InlayHint> {
        let range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: hyperion_lsp::position_utils::offset_to_position(source, source.len()),
        };
        KotlinInlayHintsProvider::new(&self.host.read_now(), Arc::clone(&self.index))
            .inlay_hints(self.file_id(rel_path), range, source)
            .expect("inlay hints must not error")
    }
}

fn hints_for(source: &str) -> Vec<InlayHint> {
    let ws = workspace(&[("Main.kt", source)]);
    ws.hints("Main.kt", source)
}

fn label_of(hint: &InlayHint) -> String {
    match &hint.label {
        InlayHintLabel::String(text) => text.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.clone()).collect(),
    }
}

fn labels(source: &str, kind: InlayHintKind) -> Vec<String> {
    hints_for(source)
        .iter()
        .filter(|hint| hint.kind == Some(kind))
        .map(label_of)
        .collect()
}

fn type_hints(source: &str) -> Vec<String> {
    labels(source, InlayHintKind::TYPE)
}

fn parameter_hints(source: &str) -> Vec<String> {
    labels(source, InlayHintKind::PARAMETER)
}

// ---------------------------------------------------------------------------
// Type hints
// ---------------------------------------------------------------------------

#[test]
fn literal_initializers() {
    assert_eq!(
        type_hints(
            r#"
fun main() {
    val count = 42
    val name = "bob"
    val flag = true
}
"#,
        ),
        vec![": Int", ": String", ": Boolean"]
    );
}

#[test]
fn constructor_initializer() {
    assert_eq!(
        type_hints(
            r#"
class Person(val name: String)

fun main() {
    val p = Person("bob")
}
"#,
        ),
        vec![": Person"]
    );
}

#[test]
fn no_hint_when_the_type_is_written_out() {
    assert!(
        type_hints(
            r#"
fun main() {
    val name: String = "bob"
    val count: Int = 42
}
"#,
        )
        .is_empty(),
        "the annotation already shows the type"
    );
}

#[test]
fn no_hint_for_a_function_typed_annotation() {
    assert!(
        type_hints(
            r"
fun main() {
    val f: (Int) -> Int = { x -> x }
}
",
        )
        .is_empty(),
        "FUNCTION_TYPE is an annotation too, not just TYPE_REF"
    );
}

#[test]
fn when_initializer_uses_the_inference_engine() {
    // The token-stream heuristic would read the *condition* `1` and answer
    // `Int`; only real least-upper-bound inference gets `String`.
    assert_eq!(
        type_hints(
            r#"
fun main() {
    val code = 1
    val label = when (code) {
        1 -> "one"
        else -> "many"
    }
}
"#,
        ),
        vec![": Int", ": String"]
    );
}

#[test]
fn lambda_initializer_is_typed_as_a_function() {
    let hints = type_hints(
        r"
fun main() {
    val double = { x: Int -> x * 2 }
}
",
    );

    assert_eq!(hints.len(), 1, "the lambda declaration gets one hint");
    assert!(
        hints[0].contains("->"),
        "a lambda should render as a function type, got {:?}",
        hints[0]
    );
}

#[test]
fn no_hint_when_inference_returns_unknown() {
    let hints = type_hints(
        r"
fun main() {
    val mystery = someUnknownThing()
}
",
    );

    assert!(
        hints.is_empty(),
        "an uninferable declaration must produce no hint, never `: ?`; got {hints:?}"
    );
}

#[test]
fn no_type_hint_ever_renders_a_placeholder() {
    let source = r"
fun main() {
    val a = whoKnows()
    val b = whoKnows().alsoUnknown
    val c = 1
}
";
    for hint in type_hints(source) {
        assert!(
            !hint.contains('?') && !hint.contains("Unknown"),
            "placeholder leaked into a type hint: {hint:?}"
        );
    }
}

#[test]
fn destructuring_declaration_gets_no_hint() {
    assert!(
        type_hints(
            r"
fun main() {
    val pair = Pair(1, 2)
    val (first, second) = pair
}
",
        )
        .iter()
        .all(|hint| hint != ": Pair"),
        "a destructuring declaration has no single place to hang a type"
    );
}

#[test]
fn property_without_an_initializer_gets_no_hint() {
    assert!(type_hints(
        r"
class Holder {
    val computed get() = 3
}
",
    )
    .is_empty());
}

#[test]
fn type_hint_is_anchored_after_the_declared_name() {
    let source = "fun main() {\n    val count = 42\n}\n";
    let hints = hints_for(source);
    let hint = hints
        .iter()
        .find(|h| h.kind == Some(InlayHintKind::TYPE))
        .expect("type hint");

    assert_eq!(hint.position.line, 1);
    assert_eq!(
        hint.position.character, 13,
        "immediately after `count`, before the ` =`"
    );
}

// ---------------------------------------------------------------------------
// Parameter hints
// ---------------------------------------------------------------------------

#[test]
fn literal_arguments_are_named() {
    assert_eq!(
        parameter_hints(
            r#"
fun greet(name: String, times: Int) {}

fun main() {
    greet("bob", 3)
}
"#,
        ),
        vec!["name = ", "times = "]
    );
}

#[test]
fn argument_matching_the_parameter_name_is_skipped() {
    assert_eq!(
        parameter_hints(
            r#"
fun greet(name: String, times: Int) {}

fun main() {
    val name = "bob"
    greet(name, 3)
}
"#,
        ),
        vec!["times = "],
        "`greet(name, …)` already reads as the parameter"
    );
}

#[test]
fn named_arguments_are_left_alone() {
    assert!(
        parameter_hints(
            r#"
fun greet(name: String, times: Int) {}

fun main() {
    greet(name = "bob", times = 3)
}
"#,
        )
        .is_empty(),
        "an explicit named argument needs no hint"
    );
}

#[test]
fn lambda_arguments_are_skipped() {
    assert_eq!(
        parameter_hints(
            r"
fun runIt(times: Int, body: () -> Unit) {}

fun main() {
    runIt(3, { })
}
",
        ),
        vec!["times = "],
        "a lambda is long enough to read on its own"
    );
}

#[test]
fn trailing_lambda_gets_no_hint() {
    assert!(parameter_hints(
        r"
fun applyIt(body: () -> Unit) {}

fun main() {
    applyIt { }
}
",
    )
    .is_empty());
}

#[test]
fn negated_literals_are_named() {
    assert_eq!(
        parameter_hints(
            r"
fun offset(delta: Int, enabled: Boolean) {}

fun main() {
    offset(-1, !false)
}
",
        ),
        vec!["delta = ", "enabled = "]
    );
}

#[test]
fn long_expressions_are_skipped() {
    assert!(
        parameter_hints(
            r#"
fun wrap(value: String) {}
fun compute(): String = ""

fun main() {
    wrap(compute())
}
"#,
        )
        .is_empty(),
        "a nested call already says what it is"
    );
}

#[test]
fn vararg_parameters_get_no_hint() {
    assert_eq!(
        parameter_hints(
            r#"
fun log(level: String, vararg values: Int) {}

fun main() {
    log("info", 1, 2, 3)
}
"#,
        ),
        vec!["level = "],
        "a vararg slot swallows several arguments; naming the first would be wrong"
    );
}

#[test]
fn constructor_arguments_are_named() {
    assert_eq!(
        parameter_hints(
            r#"
class Person(val name: String, val age: Int)

fun main() {
    val p = Person("bob", 42)
}
"#,
        ),
        vec!["name = ", "age = "]
    );
}

#[test]
fn member_call_arguments_are_named() {
    assert_eq!(
        parameter_hints(
            r#"
class Greeter {
    fun greet(name: String, times: Int) {}
}

fun main() {
    val g = Greeter()
    g.greet("bob", 3)
}
"#,
        ),
        vec!["name = ", "times = "]
    );
}

#[test]
fn unresolvable_callee_gets_no_parameter_hints() {
    assert!(parameter_hints(
        r#"
fun main() {
    thereIsNoSuchFunction("bob", 3)
}
"#,
    )
    .is_empty());
}

#[test]
fn parameter_hint_is_anchored_at_the_argument() {
    let source = "fun greet(name: String) {}\n\nfun main() {\n    greet(\"bob\")\n}\n";
    let hints = hints_for(source);
    let hint = hints
        .iter()
        .find(|h| h.kind == Some(InlayHintKind::PARAMETER))
        .expect("parameter hint");

    assert_eq!(hint.position.line, 3);
    assert_eq!(hint.position.character, 10, "at the start of `\"bob\"`");
}

// ---------------------------------------------------------------------------
// Range filtering
// ---------------------------------------------------------------------------

#[test]
fn hints_outside_the_requested_range_are_dropped() {
    let source = "fun main() {\n    val a = 1\n    val b = 2\n}\n";
    let ws = workspace(&[("Main.kt", source)]);

    let first_line_only = Range {
        start: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 1,
            character: 13,
        },
    };
    let hints = KotlinInlayHintsProvider::new(&ws.host.read_now(), Arc::clone(&ws.index))
        .inlay_hints(ws.file_id("Main.kt"), first_line_only, source)
        .expect("inlay hints");

    assert_eq!(
        hints.iter().map(label_of).collect::<Vec<_>>(),
        vec![": Int"],
        "only the declaration on the requested line should be hinted"
    );
}
