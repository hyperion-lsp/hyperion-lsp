// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared diagnostics pipeline for LSP and CLI tooling.
//!
//! Diagnostics are collected in phases so the LSP can publish incrementally:
//!
//! ```text
//! Phase 1 (fast, ~1ms):   Parse errors
//! Phase 2 (medium):       + Symbols + Type checking + Nullability + Inspections
//! Phase 3 (slow):         + Security scanning (uses pre-parsed syntax tree)
//! ```
//!
//! Each publish replaces the previous set for that URI (LSP protocol behavior).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyperion_semantics::db::{Database, FileId, ParseTree};
use hyperion_semantics::{ParseDatabase, SemanticDatabase};
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::Diagnostic;
use tracing::{debug, warn};

use crate::diagnostics::{
    nullability_diagnostic_to_lsp, parse_error_to_diagnostic, semantic_error_to_diagnostic,
};
use crate::semantic_host::SemanticHost;

#[derive(Clone, Copy, Debug)]
pub struct DiagnosticOptions {
    pub run_type_check: bool,
    pub run_nullability: bool,
    pub run_inspections: bool,
    pub run_security: bool,
    /// Run the inspections IntelliJ ships disabled (magic numbers, raw types).
    pub opt_in_inspections: bool,
    pub nullability_unknown_severity: NullabilityUnknownSeverity,
}

/// Controls severity of null-dereference warnings when nullability is Unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NullabilityUnknownSeverity {
    Warning,
    Info,
    Hint,
    #[default]
    Off,
}

impl NullabilityUnknownSeverity {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "warning" => Self::Warning,
            "info" => Self::Info,
            "hint" => Self::Hint,
            "off" => Self::Off,
            _ => Self::default(),
        }
    }
}

impl Default for DiagnosticOptions {
    fn default() -> Self {
        Self {
            run_type_check: true,
            run_nullability: true,
            run_inspections: true,
            run_security: true,
            opt_in_inspections: false,
            nullability_unknown_severity: NullabilityUnknownSeverity::default(),
        }
    }
}

async fn hydrate_java_nullability_dependencies(host: &Arc<SemanticHost>, file_id: FileId) -> usize {
    let started = Instant::now();
    debug!(?file_id, "Starting diagnostic dependency hydration");
    let hydrated =
        crate::source_publication::hydrate_dependency_method_signatures(host, file_id).await;
    log_diagnostic_stage(file_id, "dependency hydration", started.elapsed());
    hydrated
}

const SLOW_DIAGNOSTIC_STAGE: Duration = Duration::from_millis(250);

fn log_diagnostic_stage(file_id: FileId, stage: &'static str, elapsed: Duration) {
    if elapsed >= SLOW_DIAGNOSTIC_STAGE {
        warn!(?file_id, stage, ?elapsed, "Slow diagnostic stage completed");
    } else {
        debug!(?file_id, stage, ?elapsed, "Diagnostic stage completed");
    }
}

/// Collect all diagnostics for a file in one shot (non-phased).
///
/// This is the original API used by callers that don't need incremental publishing.
pub async fn collect_diagnostics_for_file(
    file_id: FileId,
    language_id: &str,
    source_text: &str,
    host: &Arc<SemanticHost>,
    security_provider: Option<&Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>>,
    workspace_ready: bool,
    options: DiagnosticOptions,
) -> Vec<Diagnostic> {
    let workspace_index = host.index();
    // The one-shot CLI/API path needs the same exact indexed call bindings as
    // phased LSP diagnostics. Publish them before entering the read
    // transaction (hydration writes; a reader cannot upgrade); otherwise a
    // fully indexed workspace can still analyze dependency calls through only
    // the conservative same-arity fallback.
    if workspace_ready && options.run_nullability && language_id == "java" {
        let hydrated = hydrate_java_nullability_dependencies(host, file_id).await;
        if hydrated > 0 {
            debug!(
                "Hydrated nullability contracts from {} cold dependency files",
                hydrated
            );
        }
    }

    let txn = host.read().await;
    let parse_tree = txn.parse_file(file_id);

    // Phase 1: Parse errors
    let mut diagnostics = collect_parse_diagnostics(&parse_tree, source_text);

    let run_semantic_phase =
        options.run_type_check || options.run_nullability || options.run_inspections;
    if workspace_ready && run_semantic_phase {
        // Phase 2: Semantic diagnostics (non-phased path — no cancellation)
        diagnostics.extend(collect_semantic_diagnostics(
            file_id,
            language_id,
            source_text,
            txn.snapshot(),
            &parse_tree,
            workspace_index,
            options,
            &|| false,
        ));
    } else if !workspace_ready {
        debug!(
            "Type check deferred (workspace not ready): file_id={:?}",
            file_id
        );
    }
    // Security scanning needs no snapshot; release the gate first.
    drop(txn);

    // Phase 3: Security scanning (uses pre-parsed syntax tree)
    diagnostics.extend(
        collect_security_diagnostics(
            file_id,
            language_id,
            source_text,
            parse_tree,
            workspace_index,
            security_provider,
            options,
        )
        .await,
    );

    deduplicate_diagnostics(&mut diagnostics);
    diagnostics
}

/// Collect diagnostics in phases, calling `publish` between each phase.
///
/// Each phase replaces the previous diagnostic set (LSP protocol behavior).
/// This gives users instant feedback on parse errors (~1ms) while heavier
/// analyses (type checking, security scanning) run in the background.
///
/// `is_current` is the exact analysis-publication fence. It is checked before
/// and after blocking work and between semantic sub-steps; `publish` is awaited
/// so phase order cannot be inverted by detached client tasks.
pub async fn collect_and_publish_phased<Current, Publish, PublishFuture>(
    file_id: FileId,
    language_id: &str,
    source_text: &str,
    host: &Arc<SemanticHost>,
    security_provider: Option<&Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>>,
    workspace_ready: bool,
    options: DiagnosticOptions,
    is_current: Current,
    mut publish: Publish,
) where
    Current: Fn() -> bool + Send + Sync + 'static,
    Publish: FnMut(Vec<Diagnostic>) -> PublishFuture,
    PublishFuture: std::future::Future<Output = bool>,
{
    let workspace_index = host.index();
    let is_current: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(is_current);

    // Check before entering the gate: a superseded analysis must not add a
    // reader that a queued writer would then have to wait for.
    if !is_current() {
        debug!("Diagnostics superseded before snapshot acquisition");
        return;
    }

    let txn = host.read().await;

    if !is_current() {
        debug!("Diagnostics superseded after semaphore acquisition");
        return;
    }

    let parse_tree = txn.parse_file(file_id);
    if !is_current() {
        debug!("Diagnostics superseded after parsing");
        return;
    }

    // ── Phase 1: Parse errors (fast, ~1ms) ──
    let mut diagnostics = collect_parse_diagnostics(&parse_tree, source_text);
    deduplicate_diagnostics(&mut diagnostics);

    // Client I/O is an unbounded async boundary. Never retain a read
    // transaction while awaiting it: a queued writer would otherwise wait for
    // this client, and every later reader would queue behind that writer.
    drop(txn);
    if !publish(diagnostics.clone()).await {
        return;
    }

    // Nullability contracts for cold dependency files are loaded on demand.
    // Phase 1 released its snapshot before client I/O, so hydration can publish
    // inputs without waiting behind this diagnostic request.
    if workspace_ready && options.run_nullability && language_id == "java" {
        let hydrated = hydrate_java_nullability_dependencies(host, file_id).await;
        if hydrated > 0 {
            debug!(
                "Hydrated nullability contracts from {} cold dependency files",
                hydrated
            );
        }
        if !is_current() {
            debug!("Diagnostics superseded after dependency contract hydration");
            return;
        }
    }

    let run_semantic_phase =
        options.run_type_check || options.run_nullability || options.run_inspections;
    let parse_tree = if workspace_ready && run_semantic_phase {
        // Check cancellation before expensive Phase 2.
        if !is_current() {
            debug!("Diagnostics superseded before Phase 2 (snapshot dropped)");
            return;
        }

        let txn = host.read().await;
        if !is_current() {
            debug!("Diagnostics superseded after Phase 2 snapshot acquisition");
            return;
        }

        // ── Phase 2: Semantic diagnostics (type check + nullability + inspections) ──
        //
        // Run on spawn_blocking so CPU-heavy work doesn't starve the tokio runtime.
        // The read transaction moves into the blocking task, so cancellation
        // of this parent future cannot make a still-live snapshot invisible to
        // the gate.
        let language_id_owned = language_id.to_owned();
        let source_text_owned = source_text.to_owned();
        let workspace_index_clone = Arc::clone(workspace_index);
        let phase_is_current = Arc::clone(&is_current);
        let phase2_result = txn
            .run_blocking(move |txn| {
                let is_superseded = || !phase_is_current();

                if is_superseded() {
                    debug!("Diagnostics superseded at start of Phase 2 spawn_blocking");
                    return None;
                }

                let semantic_diags = collect_semantic_diagnostics(
                    file_id,
                    &language_id_owned,
                    &source_text_owned,
                    txn.snapshot(),
                    &parse_tree,
                    &workspace_index_clone,
                    options,
                    &is_superseded,
                );

                Some((semantic_diags, parse_tree))
            })
            .await;

        let Some((semantic_diags, parse_tree)) = phase2_result.ok().flatten() else {
            // Task was cancelled or panicked — snapshot already dropped inside
            return;
        };

        if !is_current() {
            debug!("Diagnostics superseded after Phase 2 spawn_blocking");
            return;
        }

        diagnostics.extend(semantic_diags);
        deduplicate_diagnostics(&mut diagnostics);
        if !publish(diagnostics.clone()).await {
            return;
        }
        parse_tree
    } else {
        if !workspace_ready {
            debug!(
                "Type check deferred (workspace not ready): file_id={:?}",
                file_id
            );
        }
        parse_tree
    };

    // ── Phase 3: Security scanning (slow, uses pre-parsed syntax tree) ──
    // No Salsa snapshot is needed, but supersession still gates the CPU work
    // and its resulting client notification.
    if !is_current() {
        return;
    }
    let security_diags = collect_security_diagnostics(
        file_id,
        language_id,
        source_text,
        parse_tree,
        workspace_index,
        security_provider,
        options,
    )
    .await;
    if !security_diags.is_empty() && is_current() {
        diagnostics.extend(security_diags);
        deduplicate_diagnostics(&mut diagnostics);
        let _ = publish(diagnostics).await;
    }
}

// ── Phase functions ──

/// Phase 1: Collect parse errors from the syntax tree.
fn collect_parse_diagnostics(parse_tree: &ParseTree, source_text: &str) -> Vec<Diagnostic> {
    debug!("Phase 1: {} parse errors", parse_tree.errors.len());

    parse_tree
        .errors
        .iter()
        .map(|error| parse_error_to_diagnostic(error, source_text))
        .collect()
}

/// Phase 2: Collect semantic diagnostics (symbols, type checking, nullability, inspections).
///
/// Checks `is_cancelled` between each expensive sub-step so we can bail out early
/// and drop the Salsa snapshot, unblocking pending `set_*()` write locks.
fn collect_semantic_diagnostics(
    file_id: FileId,
    language_id: &str,
    source_text: &str,
    snapshot: &salsa::Snapshot<Database>,
    parse_tree: &ParseTree,
    workspace_index: &Arc<WorkspaceIndex>,
    options: DiagnosticOptions,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // 1. Unresolved symbol diagnostics from FileSemantics (Salsa-cached)
    if options.run_type_check && matches!(language_id, "java" | "kotlin") {
        let started = Instant::now();
        debug!(?file_id, "Starting file-semantics diagnostics");
        let file_sem = snapshot.file_semantics(file_id);

        let unresolved_diags: Vec<Diagnostic> = file_sem
            .diagnostics()
            .iter()
            .filter(|error| {
                // Error recovery can relocate otherwise-valid identifiers. A
                // Kotlin file with any parse error therefore suppresses only
                // unresolved-reference diagnostics, preventing cascades while
                // retaining independent semantic checks.
                !(language_id == "kotlin"
                    && !parse_tree.errors.is_empty()
                    && matches!(
                        error.kind,
                        hyperion_semantics::SemanticErrorKind::UnresolvedReference { .. }
                    ))
            })
            .map(|error| semantic_error_to_diagnostic(error, source_text))
            .collect();

        debug!(
            "FileSemantics for file_id={:?}: {} unresolved-symbol diagnostics",
            file_id,
            unresolved_diags.len()
        );

        diagnostics.extend(unresolved_diags);
        log_diagnostic_stage(file_id, "file semantics", started.elapsed());
    }

    // Bail early — drop snapshot so set_*() can proceed
    if is_cancelled() {
        debug!("Diagnostics cancelled after file_semantics");
        return diagnostics;
    }

    // 2. Type mismatch diagnostics from TypeChecker. Nullability and
    // inspections are independently configurable, so disabling type checking
    // must not skip the remainder of this semantic phase.
    let started = Instant::now();
    debug!(?file_id, "Starting type-check diagnostics");
    let semantic_errors = if !options.run_type_check {
        Vec::new()
    } else if language_id == "kotlin" {
        use hyperion_kotlin::SyntaxNode as KotlinSyntaxNode;
        use hyperion_semantics::KotlinTypeChecker;

        let kotlin_syntax = KotlinSyntaxNode::new_root(parse_tree.green.clone());
        let symbol_table = snapshot.file_symbol_table(file_id);
        let mut kotlin_checker = KotlinTypeChecker::new();
        kotlin_checker.check_with_context_and_symbols(
            &kotlin_syntax,
            file_id,
            Arc::clone(workspace_index),
            symbol_table,
        )
    } else {
        use hyperion_semantics::TypeChecker;

        let symbol_table = snapshot.file_symbol_table(file_id);

        let syntax = parse_tree.syntax();
        let symbols = symbol_table.all_symbols();

        debug!(
            "Resolved {} symbols for type checking in file_id={:?}",
            symbols.len(),
            file_id
        );

        let mut type_checker = TypeChecker::new();

        // Reuse file_semantics from snapshot (Salsa memoizes within same snapshot)
        let file_sem = snapshot.file_semantics(file_id);
        type_checker.set_file_semantics(Arc::clone(&file_sem));

        let mut errors = type_checker.check_with_context(
            &syntax,
            &symbols,
            file_id,
            Arc::clone(workspace_index),
        );
        let package = hyperion_semantics::utils::build_imports_from_syntax(&syntax).current_package;
        errors.extend(hyperion_semantics::compile_checks::check_file(
            &syntax,
            &file_sem,
            workspace_index,
            package.as_deref(),
        ));
        errors
    };

    debug!(
        "Type checking file_id={:?} found {} semantic errors",
        file_id,
        semantic_errors.len()
    );

    let semantic_diagnostics: Vec<Diagnostic> = semantic_errors
        .iter()
        .map(|error| semantic_error_to_diagnostic(error, source_text))
        .collect();

    diagnostics.extend(semantic_diagnostics);
    log_diagnostic_stage(file_id, "type checking", started.elapsed());

    // Bail early — don't start nullability if we're about to be superseded
    if is_cancelled() {
        debug!("Diagnostics cancelled after type checking");
        return diagnostics;
    }

    // 3. Nullability analysis
    if language_id == "java" && options.run_nullability {
        let started = Instant::now();
        debug!(?file_id, "Starting nullability diagnostics");
        let null_analysis = snapshot.file_nullability(file_id);

        for diag in &null_analysis.diagnostics {
            if let Some(lsp_diag) = nullability_diagnostic_to_lsp(
                diag,
                source_text,
                options.nullability_unknown_severity,
            ) {
                diagnostics.push(lsp_diag);
            }
        }

        debug!(
            "Nullability analysis for file_id={:?} found {} diagnostics",
            file_id,
            null_analysis.diagnostics.len()
        );
        log_diagnostic_stage(file_id, "nullability", started.elapsed());
    }

    // Bail early before inspections
    if is_cancelled() {
        debug!("Diagnostics cancelled after nullability");
        return diagnostics;
    }

    // 4. Inspections
    //
    // The inspection frameworks are language-typed and stay that way. The Java
    // inspections compare `hyperion_parser::SyntaxKind` values; `ParseTree::syntax()`
    // would reinterpret a Kotlin green tree's raw kind numbers as unrelated Java
    // variants. The Kotlin inspections are a separate, natively-typed registry that
    // walks `hyperion_kotlin::SyntaxNode` with Kotlin semantics.
    if options.run_inspections {
        let started = Instant::now();
        debug!(?file_id, "Starting inspection diagnostics");
        match language_id {
            "java" => {
                let syntax = parse_tree.syntax();
                diagnostics.extend(crate::inspections::run_inspections_with_opt_in(
                    &syntax,
                    source_text,
                    workspace_index,
                    file_id,
                    options.opt_in_inspections,
                ));
            }
            "kotlin" => {
                use hyperion_kotlin::SyntaxNode as KotlinSyntaxNode;

                let kotlin_syntax = KotlinSyntaxNode::new_root(parse_tree.green.clone());
                diagnostics.extend(crate::inspections::run_kotlin_inspections(
                    &kotlin_syntax,
                    source_text,
                    !parse_tree.errors.is_empty(),
                ));
            }
            _ => {}
        }
        log_diagnostic_stage(file_id, "inspections", started.elapsed());
    }

    diagnostics
}

/// Phase 3: Collect security diagnostics using the pre-parsed syntax tree.
async fn collect_security_diagnostics(
    file_id: FileId,
    language_id: &str,
    source_text: &str,
    parse_tree: Arc<ParseTree>,
    workspace_index: &Arc<WorkspaceIndex>,
    security_provider: Option<&Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>>,
    options: DiagnosticOptions,
) -> Vec<Diagnostic> {
    if language_id != "java" || !options.run_security {
        return vec![];
    }

    let Some(security_prov) = security_provider.cloned() else {
        return vec![];
    };
    let Some(file_path) = workspace_index.get_file_path(file_id) else {
        return vec![];
    };

    // The scan is CPU-bound, so it must not run on a runtime worker. A task
    // this worker woke, such as a writer waiting for the gate the caller just
    // released, sits in the worker's LIFO slot until the worker yields. Other
    // workers cannot steal from that slot.
    let source_text = source_text.to_owned();
    let scan = tokio::task::spawn_blocking(move || {
        // Use pre-parsed syntax tree to avoid redundant parsing
        let syntax = parse_tree.syntax();
        security_prov.scan_file_with_syntax(&file_path, &source_text, &syntax)
    });
    let security_diagnostics = match scan.await {
        Ok(diagnostics) => diagnostics,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(_) => return vec![],
    };
    if !security_diagnostics.is_empty() {
        debug!(
            "Found {} security issues in file_id={:?}",
            security_diagnostics.len(),
            file_id
        );
    }

    security_diagnostics
}

/// Collapse repeated reports of the same user-facing issue while preserving provider order.
///
/// A range alone is not an identity: independent providers and rules can legitimately report
/// different problems at the same source span. Severity and ancillary metadata are intentionally
/// excluded because they describe an issue rather than identify it.
fn deduplicate_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        seen.insert((
            diagnostic.range,
            diagnostic.source.clone(),
            diagnostic.code.clone(),
            diagnostic.message.clone(),
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::{
        collect_and_publish_phased, collect_diagnostics_for_file, deduplicate_diagnostics,
        DiagnosticOptions,
    };
    use crate::semantic_host::SemanticHost;
    use hyperion_workspace::{Language, WorkspaceIndex};

    use std::sync::Arc;
    use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

    fn diagnostic(source: &str, code: &str, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(3, 4), Position::new(3, 9)),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String(code.to_owned())),
            code_description: None,
            source: Some(source.to_owned()),
            message: message.to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    #[test]
    fn deduplicates_exact_diagnostics_but_preserves_distinct_same_range_issues() {
        let duplicate = diagnostic("hyperion-types", "type-mismatch", "Expected String");
        let mut diagnostics = vec![
            duplicate.clone(),
            duplicate,
            diagnostic("hyperion-nullability", "type-mismatch", "Expected String"),
            diagnostic("hyperion-types", "other-rule", "Expected String"),
            diagnostic("hyperion-types", "type-mismatch", "Possibly null"),
        ];

        deduplicate_diagnostics(&mut diagnostics);

        assert_eq!(diagnostics.len(), 4);
        assert_eq!(diagnostics[0].source.as_deref(), Some("hyperion-types"));
        assert_eq!(
            diagnostics[1].source.as_deref(),
            Some("hyperion-nullability")
        );
        assert_eq!(
            diagnostics[2].code,
            Some(NumberOrString::String("other-rule".to_owned()))
        );
        assert_eq!(diagnostics[3].message, "Possibly null");
    }

    fn indexed_overload_fixture() -> (
        tempfile::TempDir,
        Arc<SemanticHost>,
        hyperion_semantics::db::FileId,
        String,
    ) {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let provider_path = temp.path().join("Dependency.java");
        let caller_path = temp.path().join("Client.java");
        let provider = concat!(
            "package api; public class Dependency { ",
            "public static void accept(@Nonnull String selected, Object marker) {} ",
            "public static void accept(Object selected, String marker) {} }",
        );
        let caller = concat!(
            "package app; import api.Dependency; class Client { ",
            "void run(@Nullable String selected) { ",
            "Dependency.accept(selected, new Object()); } }",
        )
        .to_owned();
        index
            .index_file_with_source(&provider_path, provider)
            .expect("index provider");
        index
            .index_file_with_source(&caller_path, &caller)
            .expect("index caller");
        let caller_id = index.get_file_id(&caller_path).expect("caller id");
        let host = SemanticHost::new(index);
        host.with_db(|db| {
            db.register_file(caller_id, Arc::new(caller.clone()), Language::Java);
        });
        (temp, host, caller_id, caller)
    }

    fn nullability_only_options() -> DiagnosticOptions {
        DiagnosticOptions {
            run_type_check: false,
            run_nullability: true,
            run_inspections: false,
            run_security: false,
            ..DiagnosticOptions::default()
        }
    }

    #[tokio::test]
    async fn one_shot_diagnostics_publish_exact_indexed_call_bindings() {
        let (_temp, host, caller_id, caller) = indexed_overload_fixture();
        let diagnostics = collect_diagnostics_for_file(
            caller_id,
            "java",
            &caller,
            &host,
            None,
            true,
            nullability_only_options(),
        )
        .await;

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("selected")),
            "the exact String/Object overload must retain its non-null contract: {diagnostics:?}"
        );
    }

    #[tokio::test]
    async fn phased_nullability_runs_and_hydrates_when_type_check_is_disabled() {
        let (_temp, host, caller_id, caller) = indexed_overload_fixture();
        let publications = parking_lot::Mutex::new(Vec::new());
        collect_and_publish_phased(
            caller_id,
            "java",
            &caller,
            &host,
            None,
            true,
            nullability_only_options(),
            || true,
            |diagnostics| {
                publications.lock().push(diagnostics);
                std::future::ready(true)
            },
        )
        .await;

        let publications = publications.into_inner();
        assert!(
            publications
                .iter()
                .flatten()
                .any(|diagnostic| diagnostic.message.contains("selected")),
            "nullability-only phase must publish exact-overload diagnostics: {publications:?}"
        );
    }

    #[tokio::test]
    async fn phased_client_publication_never_retains_a_salsa_snapshot() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let path = temp.path().join("Client.java");
        let source = "class Client {}".to_owned();
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, &source)
            .expect("index client");
        let file_id = index.get_file_id(&path).expect("client file id");
        let host = SemanticHost::new(index);
        host.with_db(|db| {
            db.register_file(file_id, Arc::new(source.clone()), Language::Java);
        });
        let publication_host = Arc::clone(&host);
        let options = DiagnosticOptions {
            run_type_check: false,
            run_nullability: false,
            run_inspections: false,
            run_security: false,
            ..DiagnosticOptions::default()
        };

        collect_and_publish_phased(
            file_id,
            "java",
            &source,
            &host,
            None,
            true,
            options,
            || true,
            move |_| {
                let host = Arc::clone(&publication_host);
                async move {
                    // A writer can only enter the gate if no read transaction
                    // (and so no Salsa snapshot) is retained across client I/O.
                    let mut writer =
                        tokio::time::timeout(std::time::Duration::from_secs(2), host.write())
                            .await
                            .expect("a source writer must not wait behind client publication");
                    writer.db().set_file_text_tracked(
                        file_id,
                        Arc::new("class Client { int changed; }".to_owned()),
                    );
                    true
                }
            },
        )
        .await;
    }
}
