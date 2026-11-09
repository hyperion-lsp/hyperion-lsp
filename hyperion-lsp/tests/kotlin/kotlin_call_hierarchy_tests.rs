// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin call hierarchy, exercised through the real LSP request path.
//!
//! Every test drives a live `Backend`: `did_open` first, then
//! `prepareCallHierarchy` / `incomingCalls` / `outgoingCalls`, so the per-language
//! dispatch in `server::hierarchy` is part of what is under test.
//!
//! The cross-language pair (Java calling Kotlin, Kotlin calling Java) is the
//! point of the shared index: both directions read the same usage records, and
//! only the *caller-side* CST walk differs.

use crate::kotlin_hierarchy_harness::{callee_names, caller_names, text_at, Workspace};
use tower_lsp::lsp_types::SymbolKind;

// ---------------------------------------------------------------------------
// prepare
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prepare_on_a_top_level_function() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

fun gre<|>et(name: String) {
}
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("Main.kt").await;

    assert_eq!(item.name, "greet");
    assert_eq!(item.kind, SymbolKind::FUNCTION);
    assert_eq!(fqn_of(&item), "app.greet");
}

#[tokio::test]
async fn prepare_on_a_member_function_reports_its_class() {
    let workspace = Workspace::open(&[(
        "Service.kt",
        r"package app

class Service {
    fun proc<|>ess() {
    }
}
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("Service.kt").await;

    assert_eq!(item.name, "process");
    assert_eq!(item.kind, SymbolKind::METHOD);
    assert_eq!(item.detail.as_deref(), Some("Service"));
    assert_eq!(fqn_of(&item), "app.Service.process");
}

#[tokio::test]
async fn prepare_on_a_class_name_yields_its_primary_constructor() {
    let workspace = Workspace::open(&[(
        "User.kt",
        r"package app

class Us<|>er(val name: String)
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("User.kt").await;

    assert_eq!(item.name, "User");
    assert_eq!(
        item.kind,
        SymbolKind::CONSTRUCTOR,
        "in Kotlin the class name is the constructor"
    );
    assert_eq!(fqn_of(&item), "app.User");
}

#[tokio::test]
async fn prepare_on_an_object_name_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Registry.kt",
        r"package app

object Reg<|>istry
",
    )])
    .await;

    assert!(
        workspace
            .prepare_call_hierarchy("Registry.kt")
            .await
            .is_none(),
        "a Kotlin object has no callable constructor"
    );
}

#[tokio::test]
async fn prepare_on_a_secondary_constructor_keyword() {
    let workspace = Workspace::open(&[(
        "User.kt",
        r"package app

class User {
    con<|>structor(name: String) {
    }
}
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("User.kt").await;

    assert_eq!(item.name, "User");
    assert_eq!(item.kind, SymbolKind::CONSTRUCTOR);
    assert_eq!(fqn_of(&item), "app.User");
}

#[tokio::test]
async fn prepare_on_a_property_accessor_reports_the_property() {
    let workspace = Workspace::open(&[(
        "Config.kt",
        r#"package app

class Config {
    val label: String
        g<|>et() = compute()

    fun compute(): String = "x"
}
"#,
    )])
    .await;

    let item = workspace.call_hierarchy_item("Config.kt").await;

    assert_eq!(item.name, "label");
    assert_eq!(item.kind, SymbolKind::PROPERTY);
    assert_eq!(fqn_of(&item), "app.Config.label");
}

#[tokio::test]
async fn prepare_on_a_keyword_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

f<|>un greet(name: String) {
}
",
    )])
    .await;

    assert!(
        workspace.prepare_call_hierarchy("Main.kt").await.is_none(),
        "a cursor on `fun` is not asking about a callable"
    );
}

#[tokio::test]
async fn prepare_on_a_class_body_keyword_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

class Holder {
    va<|>l count: Int = 0
}
",
    )])
    .await;

    assert!(
        workspace.prepare_call_hierarchy("Main.kt").await.is_none(),
        "a cursor on `val` is not asking about a callable"
    );
}

#[tokio::test]
async fn prepare_on_a_function_body_identifier_returns_nothing() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r#"package app

fun greet() {
    val na<|>me = "x"
}
"#,
    )])
    .await;

    assert!(
        workspace.prepare_call_hierarchy("Main.kt").await.is_none(),
        "an identifier inside a function body is not the function declaration"
    );
}

// ---------------------------------------------------------------------------
// Kotlin calls Kotlin
// ---------------------------------------------------------------------------

#[tokio::test]
async fn incoming_calls_from_kotlin() {
    let workspace = Workspace::open(&[
        (
            "Service.kt",
            r"package app

class Service {
    fun proc<|>ess() {
    }
}
",
        ),
        (
            "Main.kt",
            r"package app

fun run() {
    val service: Service = Service()
    service.process()
}
",
        ),
    ])
    .await;

    let item = workspace.call_hierarchy_item("Service.kt").await;
    let calls = workspace.incoming_calls(item).await;

    assert_eq!(caller_names(&calls), vec!["run"]);

    let caller = &calls[0];
    assert_eq!(caller.from.kind, SymbolKind::FUNCTION);
    assert_eq!(
        caller.from_ranges.len(),
        1,
        "one call site, got {:?}",
        caller.from_ranges
    );
    assert_eq!(
        text_at(&workspace.file("Main.kt").text, caller.from_ranges[0]),
        "process",
        "the reported range must be the call site, not the caller's body"
    );
}

#[tokio::test]
async fn repeated_call_sites_merge_into_one_caller() {
    let workspace = Workspace::open(&[
        (
            "Service.kt",
            r"package app

class Service {
    fun proc<|>ess() {
    }
}
",
        ),
        (
            "Main.kt",
            r"package app

fun run() {
    val service: Service = Service()
    service.process()
    service.process()
}
",
        ),
    ])
    .await;

    let item = workspace.call_hierarchy_item("Service.kt").await;
    let calls = workspace.incoming_calls(item).await;

    assert_eq!(calls.len(), 1, "one caller, got {calls:?}");
    assert_eq!(
        calls[0].from_ranges.len(),
        2,
        "both call sites belong to the same caller"
    );
}

#[tokio::test]
async fn incoming_calls_on_a_constructor_are_the_construction_sites() {
    let workspace = Workspace::open(&[
        (
            "User.kt",
            r"package app

class Us<|>er(val name: String)
",
        ),
        (
            "Factory.kt",
            r#"package app

fun accept(user: User): User = user

fun build(): User {
    return User("bob")
}
"#,
        ),
    ])
    .await;

    let item = workspace.call_hierarchy_item("User.kt").await;
    let calls = workspace.incoming_calls(item).await;

    assert_eq!(caller_names(&calls), vec!["build"]);
    assert_eq!(
        text_at(&workspace.file("Factory.kt").text, calls[0].from_ranges[0]),
        "User"
    );
}

#[tokio::test]
async fn kotlin_constructor_incoming_keeps_java_new_expressions_only() {
    let workspace = Workspace::open(&[
        (
            "User.kt",
            r"package app

class User {
    con<|>structor()
    constructor(name: String)
}
",
        ),
        (
            "Client.java",
            r#"package app;

class Client {
    User field;
    User identity(User user) { return user; }
    User makeZero() { return new User(); }
    User makeOne() { return new User("one"); }
}
"#,
        ),
    ])
    .await;

    let zero = workspace.call_hierarchy_item("User.kt").await;
    let calls = workspace.incoming_calls(zero).await;
    assert_eq!(caller_names(&calls), vec!["makeZero"]);
    assert_eq!(
        text_at(&workspace.file("Client.java").text, calls[0].from_ranges[0]),
        "User"
    );
}

#[tokio::test]
async fn java_call_does_not_guess_between_same_arity_kotlin_constructors() {
    let workspace = Workspace::open(&[
        (
            "Choice.kt",
            r"package app

class Choice {
    con<|>structor(name: String)
    constructor(count: Int)
}
",
        ),
        (
            "Client.java",
            r#"package app;

class Client {
    Choice make() { return new Choice("one"); }
}
"#,
        ),
    ])
    .await;

    let string_constructor = workspace.call_hierarchy_item("Choice.kt").await;
    let int_constructor = workspace
        .backend_prepare_at("Choice.kt", 4, 8)
        .await
        .expect("second constructor prepare")
        .into_iter()
        .next()
        .expect("second constructor item");
    assert!(
        workspace
            .incoming_calls(string_constructor)
            .await
            .is_empty(),
        "arity alone cannot prove the String overload"
    );
    assert!(
        workspace.incoming_calls(int_constructor).await.is_empty(),
        "arity alone cannot prove the Int overload"
    );
}

#[tokio::test]
async fn secondary_constructor_items_are_exact_and_calls_are_overload_specific() {
    let workspace = Workspace::open(&[
        (
            "User.kt",
            r#"package app

fun zeroHelper() {}
fun oneHelper() {}

class User {
    con<|>structor() {
        zeroHelper()
    }

    constructor(name: String) {
        oneHelper()
    }
}
"#,
        ),
        (
            "Factory.kt",
            r#"package app

fun makeZero(): User = User()
fun makeOne(): User = User("one")
"#,
        ),
    ])
    .await;

    let zero = workspace.call_hierarchy_item("User.kt").await;
    let one = workspace
        .backend_prepare_at("User.kt", 10, 8)
        .await
        .expect("second constructor prepare")
        .into_iter()
        .next()
        .expect("second constructor item");

    assert_eq!(
        caller_names(&workspace.incoming_calls(zero.clone()).await),
        vec!["makeZero"]
    );
    assert_eq!(
        caller_names(&workspace.incoming_calls(one.clone()).await),
        vec!["makeOne"]
    );
    assert_eq!(
        callee_names(&workspace.outgoing_calls(zero.clone()).await),
        vec!["zeroHelper"]
    );
    assert_eq!(
        callee_names(&workspace.outgoing_calls(one.clone()).await),
        vec!["oneHelper"]
    );

    let mut retargeted = zero;
    retargeted.range = one.range;
    retargeted.selection_range = one.selection_range;
    assert!(
        workspace.outgoing_calls(retargeted).await.is_empty(),
        "visible ranges must not retarget opaque constructor identity"
    );
}

#[tokio::test]
async fn outgoing_calls_from_kotlin() {
    let workspace = Workspace::open(&[
        (
            "Service.kt",
            r"package app

class Service {
    fun process() {
    }
}
",
        ),
        (
            "Main.kt",
            r"package app

fun helper() {
}

fun r<|>un() {
    val service: Service = Service()
    service.process()
    helper()
}
",
        ),
    ])
    .await;

    let item = workspace.call_hierarchy_item("Main.kt").await;
    let calls = workspace.outgoing_calls(item).await;

    assert_eq!(callee_names(&calls), vec!["Service", "helper", "process"]);

    let process = calls
        .iter()
        .find(|call| call.to.name == "process")
        .expect("process among the outgoing calls");
    assert_eq!(
        text_at(&workspace.file("Main.kt").text, process.from_ranges[0]),
        "process",
        "outgoing ranges are the call sites inside the caller"
    );
}

#[tokio::test]
async fn outgoing_calls_are_scoped_to_the_declaration() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

fun helper() {
}

fun other() {
    helper()
}

fun r<|>un() {
}
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("Main.kt").await;
    let calls = workspace.outgoing_calls(item).await;

    assert!(
        calls.is_empty(),
        "`run` has an empty body; `other`'s call must not leak in: {calls:?}"
    );
}

#[tokio::test]
async fn outgoing_calls_exclude_calls_inside_nested_functions() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

fun helper() {
}

fun target() {
}

fun out<|>er() {
    helper()
    fun local() {
        target()
    }
}
",
    )])
    .await;

    let item = workspace.call_hierarchy_item("Main.kt").await;
    let calls = workspace.outgoing_calls(item).await;

    let names = callee_names(&calls);
    assert!(
        !names.iter().any(|name| name == "target"),
        "calls inside the nested function must not be attributed to outer: {names:?}"
    );
}

#[tokio::test]
async fn outgoing_calls_keep_same_named_targets_in_one_file_distinct() {
    let workspace = Workspace::open(&[(
        "Main.kt",
        r"package app

object Left {
    fun run() {}
}

object Right {
    fun run() {}
}

fun cal<|>ler() {
    Left.run()
    Right.run()
}
",
    )])
    .await;

    let caller = workspace.call_hierarchy_item("Main.kt").await;
    let calls = workspace.outgoing_calls(caller).await;
    let mut owners = calls
        .iter()
        .filter(|call| call.to.name == "run")
        .filter_map(|call| call.to.detail.clone())
        .collect::<Vec<_>>();
    owners.sort();
    assert_eq!(owners, vec!["Left", "Right"]);
}

#[tokio::test]
async fn unresolved_kotlin_receiver_does_not_bind_a_unique_workspace_name() {
    let workspace = Workspace::open(&[
        (
            "unrelated/Worker.kt",
            r"package unrelated

class Worker {
    fun run() {}
}
",
        ),
        (
            "app/Main.kt",
            r"package app

fun cal<|>ler() {
    missing.run()
}
",
        ),
    ])
    .await;

    let caller = workspace.call_hierarchy_item("app/Main.kt").await;
    let calls = workspace.outgoing_calls(caller).await;
    assert!(
        calls.is_empty(),
        "an unresolved Kotlin receiver must fail closed: {calls:?}"
    );
}

// ---------------------------------------------------------------------------
// Cross-language
// ---------------------------------------------------------------------------

#[tokio::test]
async fn java_caller_of_a_kotlin_function() {
    let workspace = Workspace::open(&[
        (
            "app/Greeter.kt",
            r"package app

class Greeter {
    fun gre<|>et() {
    }
}
",
        ),
        (
            "app/Client.java",
            r"package app;

public class Client {
    public void run() {
        Greeter greeter = new Greeter();
        greeter.greet();
    }
}
",
        ),
    ])
    .await;

    let item = workspace.call_hierarchy_item("app/Greeter.kt").await;
    let calls = workspace.incoming_calls(item).await;

    assert_eq!(
        caller_names(&calls),
        vec!["run"],
        "a Java method calling a Kotlin function must show up as a caller"
    );
    assert!(
        calls[0].from.uri.as_str().ends_with("Client.java"),
        "the caller item points into the Java file: {:?}",
        calls[0].from.uri
    );
    assert_eq!(
        text_at(
            &workspace.file("app/Client.java").text,
            calls[0].from_ranges[0]
        ),
        "greet"
    );
}

#[tokio::test]
async fn kotlin_caller_of_a_java_method() {
    let workspace = Workspace::open(&[
        (
            "app/Greeter.java",
            r"package app;

public class Greeter {
    public void greet() {
    }
}
",
        ),
        (
            "app/Main.kt",
            r"package app

fun run() {
    val greeter: Greeter = Greeter()
    greeter.greet()
}
",
        ),
    ])
    .await;

    // Prepared from the Java file, so this goes through the *Java* provider —
    // only the caller-side walk has to notice the caller is Kotlin.
    let items = workspace
        .backend_prepare_at("app/Greeter.java", 3, 16)
        .await
        .expect("prepare on the Java method");
    let calls = workspace.incoming_calls(items[0].clone()).await;

    assert_eq!(
        caller_names(&calls),
        vec!["run"],
        "a Kotlin function calling a Java method must show up as a caller"
    );
    assert!(
        calls[0].from.uri.as_str().ends_with("Main.kt"),
        "the caller item points into the Kotlin file: {:?}",
        calls[0].from.uri
    );
    assert_eq!(
        calls[0].from.kind,
        SymbolKind::FUNCTION,
        "a top-level Kotlin caller is a function, not a method"
    );
}

// ---------------------------------------------------------------------------
// Java is unchanged by the dispatch layer
// ---------------------------------------------------------------------------

/// A Java-only workspace must produce exactly what the Java provider produces on
/// its own — the opt-in `LanguageHandler` convention means Java never reaches a
/// Kotlin code path, and this pins that.
#[tokio::test]
async fn java_call_hierarchy_is_unchanged_by_dispatch() {
    let service = r"package app;

public class Service {
    public void process() {
        helper();
    }

    public void helper() {
    }
}
";
    let client = r"package app;

public class Client {
    public void run() {
        Service service = new Service();
        service.process();
    }
}
";

    let workspace =
        Workspace::open(&[("app/Service.java", service), ("app/Client.java", client)]).await;

    // `process` is on line 3.
    let through_server = workspace
        .backend_prepare_at("app/Service.java", 3, 18)
        .await
        .expect("java prepare");
    let server_caller = workspace
        .backend_prepare_at("app/Client.java", 3, 16)
        .await
        .expect("java prepare on the caller");

    let service_id = file_id_of(&through_server[0]);
    let client_id = file_id_of(&server_caller[0]);
    assert_ne!(service_id, client_id, "opened documents need distinct IDs");
    let reference = java_reference(
        &workspace,
        &[
            ("app/Service.java", service_id),
            ("app/Client.java", client_id),
        ],
    );

    let direct = reference
        .provider
        .prepare(
            service_id,
            tower_lsp::lsp_types::Position::new(3, 18),
            &workspace.file("app/Service.java").text,
            &workspace.file("app/Service.java").uri,
        )
        .expect("java prepare reference")
        .expect("java prepare reference is non-empty");

    assert_eq!(
        call_items_without_revision_tokens(through_server.clone()),
        call_items_without_revision_tokens(direct.clone()),
        "Java prepareCallHierarchy must preserve the Java provider's public result"
    );

    let server_incoming = workspace.incoming_calls(through_server[0].clone()).await;
    let direct_incoming = reference
        .provider
        .incoming_calls(&direct[0])
        .expect("java incoming reference");
    assert!(
        !direct_incoming.is_empty(),
        "the reference must be non-empty or this pin is vacuous"
    );
    assert_eq!(
        incoming_without_revision_tokens(server_incoming),
        incoming_without_revision_tokens(direct_incoming),
        "Java incomingCalls must preserve the Java provider's public result"
    );

    // Outgoing is pinned from the caller's side, where the calls are.
    let direct_caller = reference
        .provider
        .prepare(
            client_id,
            tower_lsp::lsp_types::Position::new(3, 16),
            &workspace.file("app/Client.java").text,
            &workspace.file("app/Client.java").uri,
        )
        .expect("java prepare reference")
        .expect("java prepare reference is non-empty");

    let server_outgoing = workspace.outgoing_calls(server_caller[0].clone()).await;
    let direct_outgoing = reference
        .provider
        .outgoing_calls(&direct_caller[0])
        .expect("java outgoing reference");
    assert!(
        !direct_outgoing.is_empty(),
        "the reference must be non-empty or this pin is vacuous"
    );
    assert_eq!(
        outgoing_without_revision_tokens(server_outgoing),
        outgoing_without_revision_tokens(direct_outgoing),
        "Java outgoingCalls must preserve the Java provider's public result"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fqn_of(item: &tower_lsp::lsp_types::CallHierarchyItem) -> String {
    item.data
        .as_ref()
        .and_then(|data| data.get("fqn"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn file_id_of(item: &tower_lsp::lsp_types::CallHierarchyItem) -> hyperion_semantics::db::FileId {
    let raw = item
        .data
        .as_ref()
        .and_then(|data| data.get("file_id"))
        .and_then(serde_json::Value::as_u64)
        .expect("file_id in item data");
    hyperion_semantics::db::FileId::new(raw as u32)
}

fn normalize_call_item_data(item: &mut tower_lsp::lsp_types::CallHierarchyItem) {
    let data = item
        .data
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .expect("call hierarchy items must retain opaque object data");
    assert!(data.get("fqn").is_some_and(serde_json::Value::is_string));
    assert!(data.get("file_id").is_some_and(serde_json::Value::is_u64));
    assert!(data
        .get("file_revision")
        .is_some_and(serde_json::Value::is_string));
    assert!(data
        .get("workspace_revision")
        .is_some_and(serde_json::Value::is_string));
    data.remove("file_revision");
    data.remove("workspace_revision");
}

fn call_items_without_revision_tokens(
    mut items: Vec<tower_lsp::lsp_types::CallHierarchyItem>,
) -> Vec<tower_lsp::lsp_types::CallHierarchyItem> {
    for item in &mut items {
        normalize_call_item_data(item);
    }
    items
}

fn incoming_without_revision_tokens(
    mut calls: Vec<tower_lsp::lsp_types::CallHierarchyIncomingCall>,
) -> Vec<tower_lsp::lsp_types::CallHierarchyIncomingCall> {
    for call in &mut calls {
        normalize_call_item_data(&mut call.from);
    }
    calls
}

fn outgoing_without_revision_tokens(
    mut calls: Vec<tower_lsp::lsp_types::CallHierarchyOutgoingCall>,
) -> Vec<tower_lsp::lsp_types::CallHierarchyOutgoingCall> {
    for call in &mut calls {
        normalize_call_item_data(&mut call.to);
    }
    calls
}

struct JavaReference {
    provider: hyperion_lsp::call_hierarchy::CallHierarchyProvider<'static>,
}

/// The Java answer, computed straight from the Java provider over the same files
/// on disk, with no dispatch layer in between.
///
/// The reference has to use the live server's FileIds so the items' `data`
/// payloads remain directly comparable even if allocation ranges change.
fn java_reference(
    workspace: &Workspace,
    files_with_ids: &[(&str, hyperion_semantics::db::FileId)],
) -> JavaReference {
    use hyperion_lsp::semantic_host::SemanticHost;
    use hyperion_semantics::db::{FileId, SourceDatabase};
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;

    let root = workspace
        .file(files_with_ids[0].0)
        .uri
        .to_file_path()
        .expect("fixture path")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root")
        .to_path_buf();

    let index = WorkspaceIndex::new(root);
    let files: Vec<(std::path::PathBuf, FileId, String)> = files_with_ids
        .iter()
        .map(|(name, file_id)| {
            let file = workspace.file(name);
            (
                file.uri.to_file_path().expect("fixture path"),
                *file_id,
                file.text.clone(),
            )
        })
        .collect();

    // Two passes: usages only resolve against symbols an earlier pass indexed.
    for _ in 0..2 {
        for (path, file_id, source) in &files {
            index
                .index_file_with_source_and_id(path, source, *file_id)
                .expect("index the reference file");
        }
    }

    let index = Arc::new(index);
    let host = SemanticHost::new(Arc::clone(&index));
    host.with_db(|guard| {
        for (_, file_id, source) in &files {
            guard.set_file_text(*file_id, Arc::new(source.clone()));
            guard.set_file_language(*file_id, hyperion_workspace::Language::Java);
        }
    });
    // The reference lives for the whole test, so its read transaction is
    // leaked with it (test-only).
    let txn: &'static hyperion_lsp::semantic_host::ReadTxn = Box::leak(Box::new(host.read_now()));

    JavaReference {
        provider: hyperion_lsp::call_hierarchy::CallHierarchyProvider::new(txn, index),
    }
}
