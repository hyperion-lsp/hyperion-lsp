// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin signature help (`textDocument/signatureHelp`).
//!
//! Kotlin has call shapes Java does not — named arguments, trailing lambdas
//! outside the parentheses, calls with no parentheses at all — so these tests
//! pin the argument-shape handling as much as the signature rendering.
//!
//! The cursor is written as `<|>` and stripped before indexing.

use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::signature_help_kotlin::KotlinSignatureHelpProvider;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{ParameterLabel, SignatureHelp};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

const CURSOR: &str = "<|>";

struct KotlinWorkspace {
    _temp: TempDir,
    root: PathBuf,
    index: Arc<WorkspaceIndex>,
    host: Arc<SemanticHost>,
}

/// Index `files` (relative paths, Kotlin source) into a throwaway workspace and
/// register each one with the Salsa DB, exactly as `Backend::new` does.
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

    fn signature_help(&self, rel_path: &str, source: &str) -> Option<SignatureHelp> {
        let (clean, offset) = strip_cursor(source);
        let file_id = self.file_id(rel_path);
        let position = hyperion_lsp::position_utils::offset_to_position(&clean, offset);
        KotlinSignatureHelpProvider::new(&self.host.read_now(), Arc::clone(&self.index))
            .signature_help(file_id, position, &clean)
            .expect("signature help must not error")
    }
}

/// A one-file workspace whose single source carries the cursor marker.
fn help_for(source: &str) -> Option<SignatureHelp> {
    let (clean, _) = strip_cursor(source);
    let ws = workspace(&[("Main.kt", &clean)]);
    ws.signature_help("Main.kt", source)
}

fn strip_cursor(source: &str) -> (String, usize) {
    match source.find(CURSOR) {
        Some(offset) => (source.replace(CURSOR, ""), offset),
        None => (source.to_string(), source.len()),
    }
}

/// `(label, active_signature, active_parameter)` for the response.
fn summary(help: &SignatureHelp) -> (String, u32, u32) {
    let active = help.active_signature.unwrap_or(0);
    (
        help.signatures[active as usize].label.clone(),
        active,
        help.active_parameter.unwrap_or(0),
    )
}

fn labels(help: &SignatureHelp) -> Vec<String> {
    help.signatures.iter().map(|s| s.label.clone()).collect()
}

// ---------------------------------------------------------------------------
// Positional arguments
// ---------------------------------------------------------------------------

#[test]
fn top_level_function_first_parameter() {
    let help = help_for(
        r#"
fun greet(name: String, times: Int): String = ""

fun main() {
    greet(<|>)
}
"#,
    )
    .expect("signature help inside the argument list");

    assert_eq!(
        summary(&help),
        ("greet(name: String, times: Int): String".to_string(), 0, 0)
    );
}

#[test]
fn active_parameter_advances_past_comma() {
    let help = help_for(
        r#"
fun greet(name: String, times: Int): String = ""

fun main() {
    greet("bob", <|>)
}
"#,
    )
    .expect("signature help after the comma");

    assert_eq!(summary(&help).2, 1, "cursor is on the second parameter");
}

#[test]
fn parameter_labels_are_offsets_into_the_signature_label() {
    let help = help_for(
        r#"
fun greet(name: String, times: Int): String = ""

fun main() {
    greet(<|>)
}
"#,
    )
    .expect("signature help");

    let signature = &help.signatures[0];
    let parameters = signature.parameters.as_ref().expect("parameters");
    let spans: Vec<&str> = parameters
        .iter()
        .map(|p| match &p.label {
            ParameterLabel::LabelOffsets([start, end]) => {
                &signature.label[*start as usize..*end as usize]
            }
            ParameterLabel::Simple(_) => panic!("expected offset labels"),
        })
        .collect();

    assert_eq!(spans, vec!["name: String", "times: Int"]);
}

#[test]
fn no_help_outside_the_argument_list() {
    assert!(help_for(
        r#"
fun greet(name: String): String = ""

fun main() {
    greet("bob")<|>
}
"#,
    )
    .is_none());
}

#[test]
fn innermost_call_wins() {
    let help = help_for(
        r#"
fun outer(value: String) {}
fun inner(count: Int): String = ""

fun main() {
    outer(inner(<|>))
}
"#,
    )
    .expect("signature help for the nested call");

    assert_eq!(summary(&help).0, "inner(count: Int): String");
}

#[test]
fn unclosed_call_still_resolves() {
    let help = help_for(
        r"
fun greet(name: String) {}

fun main() {
    greet(<|>
}
",
    )
    .expect("an unterminated call is the common case while typing");

    assert_eq!(summary(&help).0, "greet(name: String)");
}

// ---------------------------------------------------------------------------
// Named arguments
// ---------------------------------------------------------------------------

#[test]
fn named_argument_selects_parameter_by_name() {
    let help = help_for(
        r"
fun greet(name: String, times: Int, loud: Boolean) {}

fun main() {
    greet(loud = <|>)
}
",
    )
    .expect("signature help");

    assert_eq!(
        summary(&help).2,
        2,
        "`loud =` points at the third parameter regardless of slot"
    );
}

#[test]
fn named_argument_out_of_order_after_positional() {
    let help = help_for(
        r#"
fun greet(name: String, times: Int, loud: Boolean) {}

fun main() {
    greet("bob", loud = <|>)
}
"#,
    )
    .expect("signature help");

    assert_eq!(summary(&help).2, 2);
}

#[test]
fn unknown_argument_name_falls_back_to_the_slot() {
    let help = help_for(
        r#"
fun greet(name: String, times: Int) {}

fun main() {
    greet("bob", nope = <|>)
}
"#,
    )
    .expect("signature help");

    assert_eq!(summary(&help).2, 1);
}

// ---------------------------------------------------------------------------
// Trailing lambdas
// ---------------------------------------------------------------------------

#[test]
fn trailing_lambda_outside_parens_is_the_last_parameter() {
    let help = help_for(
        r"
fun runIt(times: Int, body: () -> Unit) {}

fun main() {
    runIt(3) { <|> }
}
",
    )
    .expect("signature help inside the trailing lambda");

    let (label, _, active) = summary(&help);
    assert_eq!(label, "runIt(times: Int, body: () -> Unit)");
    assert_eq!(active, 1, "the trailing lambda binds the last parameter");
}

#[test]
fn parenthesis_free_trailing_lambda_call() {
    let help = help_for(
        r"
fun applyIt(body: () -> Unit) {}

fun main() {
    applyIt { <|> }
}
",
    )
    .expect("a call written with no parentheses at all");

    let (label, _, active) = summary(&help);
    assert_eq!(label, "applyIt(body: () -> Unit)");
    assert_eq!(active, 0);
}

#[test]
fn lambda_argument_inside_parens_is_positional() {
    let help = help_for(
        r"
fun runIt(body: () -> Unit, label: String) {}

fun main() {
    runIt({ }, <|>)
}
",
    )
    .expect("signature help");

    assert_eq!(summary(&help).2, 1);
}

// ---------------------------------------------------------------------------
// Overloads
// ---------------------------------------------------------------------------

#[test]
fn overloads_are_all_reported() {
    let help = help_for(
        r"
fun send(body: String) {}
fun send(body: String, retries: Int) {}

fun main() {
    send(<|>)
}
",
    )
    .expect("signature help");

    assert_eq!(
        labels(&help),
        vec!["send(body: String)", "send(body: String, retries: Int)"]
    );
}

#[test]
fn overload_arity_drives_the_active_signature() {
    let help = help_for(
        r#"
fun send(body: String) {}
fun send(body: String, retries: Int) {}

fun main() {
    send("hi", <|>)
}
"#,
    )
    .expect("signature help");

    assert_eq!(
        summary(&help).0,
        "send(body: String, retries: Int)",
        "two typed arguments can only fit the two-parameter overload"
    );
}

#[test]
fn named_argument_disambiguates_overloads() {
    let help = help_for(
        r"
fun send(body: String) {}
fun send(payload: String, retries: Int) {}

fun main() {
    send(payload = <|>)
}
",
    )
    .expect("signature help");

    assert_eq!(
        summary(&help).0,
        "send(payload: String, retries: Int)",
        "only one overload has a parameter called `payload`"
    );
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

#[test]
fn primary_constructor() {
    let help = help_for(
        r"
class Person(val name: String, val age: Int)

fun main() {
    Person(<|>)
}
",
    )
    .expect("signature help for a constructor call");

    assert_eq!(summary(&help).0, "Person(name: String, age: Int)");
}

#[test]
fn secondary_constructors_are_offered_alongside_the_primary() {
    let help = help_for(
        r"
class Person(val name: String) {
    constructor(name: String, age: Int) : this(name)
}

fun main() {
    Person(<|>)
}
",
    )
    .expect("signature help");

    assert_eq!(
        labels(&help),
        vec!["Person(name: String)", "Person(name: String, age: Int)"],
        "the workspace index skips CONSTRUCTOR_DECL entirely, so the CST must supply it"
    );
}

// ---------------------------------------------------------------------------
// Receivers and scoping
// ---------------------------------------------------------------------------

#[test]
fn member_call_on_an_inferred_receiver() {
    let help = help_for(
        r#"
class Person(val name: String) {
    fun describe(prefix: String): String = prefix + name
}

fun main() {
    val p = Person("bob")
    p.describe(<|>)
}
"#,
    )
    .expect("signature help for a qualified call");

    assert_eq!(summary(&help).0, "describe(prefix: String): String");
}

#[test]
fn local_function_is_visible_only_inside_its_block() {
    let inside = help_for(
        r"
fun outer() {
    fun helper(count: Int) {}
    helper(<|>)
}
",
    )
    .expect("a local fun is callable from its own block");
    assert_eq!(summary(&inside).0, "helper(count: Int)");

    assert!(
        help_for(
            r"
fun outer() {
    fun helper(count: Int) {}
}

fun other() {
    helper(<|>)
}
",
        )
        .is_none(),
        "the same local fun is out of scope from a sibling function"
    );
}

#[test]
fn vararg_and_default_parameters_render_as_written() {
    let help = help_for(
        r#"
fun log(level: String = "info", vararg values: Int) {}

fun main() {
    log(<|>)
}
"#,
    )
    .expect("signature help");

    assert_eq!(summary(&help).0, "log(level: String, vararg values: Int)");
}

#[test]
fn cross_file_top_level_function() {
    let ws = workspace(&[
        (
            "util/Greeting.kt",
            "package util\n\nfun greet(name: String, times: Int) {}\n",
        ),
        (
            "Main.kt",
            "import util.greet\n\nfun main() {\n    greet()\n}\n",
        ),
    ]);

    let help = ws
        .signature_help(
            "Main.kt",
            "import util.greet\n\nfun main() {\n    greet(<|>)\n}\n",
        )
        .expect("signature help resolved through the workspace index");

    assert_eq!(summary(&help).0, "greet(name: String, times: Int)");
}

#[test]
fn unimported_unique_function_is_not_adopted_from_another_package() {
    let ws = workspace(&[
        (
            "other/Greeting.kt",
            "package other\n\nfun greet(wrong: Int) {}\n",
        ),
        ("Main.kt", "fun main() {\n    greet()\n}\n"),
    ]);

    assert!(
        ws.signature_help("Main.kt", "fun main() {\n    greet(<|>)\n}\n")
            .is_none(),
        "a globally unique spelling is not a Kotlin scope binding"
    );
}

#[test]
fn explicit_import_selects_one_of_duplicate_top_level_functions() {
    let ws = workspace(&[
        (
            "left/Send.kt",
            "package left\n\nfun send(left: String) {}\n",
        ),
        (
            "right/Send.kt",
            "package right\n\nfun send(right: Int) {}\n",
        ),
        (
            "Main.kt",
            "import right.send\n\nfun main() {\n    send()\n}\n",
        ),
    ]);

    let help = ws
        .signature_help(
            "Main.kt",
            "import right.send\n\nfun main() {\n    send(<|>)\n}\n",
        )
        .expect("the explicit import is an exact binding");
    assert_eq!(summary(&help).0, "send(right: Int)");
}

#[test]
fn ambiguous_wildcard_imports_do_not_form_a_signature_set() {
    let ws = workspace(&[
        (
            "left/Send.kt",
            "package left\n\nfun send(left: String) {}\n",
        ),
        (
            "right/Send.kt",
            "package right\n\nfun send(right: Int) {}\n",
        ),
        (
            "Main.kt",
            "import left.*\nimport right.*\n\nfun main() {\n    send()\n}\n",
        ),
    ]);

    assert!(
        ws.signature_help(
            "Main.kt",
            "import left.*\nimport right.*\n\nfun main() {\n    send(<|>)\n}\n",
        )
        .is_none(),
        "two wildcard owners are ambiguous, not overloads"
    );
}

#[test]
fn unknown_qualified_receiver_does_not_fall_back_to_global_name() {
    let ws = workspace(&[
        (
            "other/Sender.kt",
            "package other\n\nclass Sender { fun send(wrong: Int) {} }\n",
        ),
        ("Main.kt", "fun main() {\n    mystery.send()\n}\n"),
    ]);

    assert!(
        ws.signature_help("Main.kt", "fun main() {\n    mystery.send(<|>)\n}\n")
            .is_none(),
        "an unknown receiver cannot authorize a same-named workspace member"
    );
}

#[test]
fn unresolvable_callee_yields_nothing() {
    assert!(
        help_for(
            r"
fun main() {
    thereIsNoSuchFunction(<|>)
}
",
        )
        .is_none(),
        "showing an empty parameter list would be worse than showing nothing"
    );
}
