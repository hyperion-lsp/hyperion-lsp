// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use glob::Pattern;
use hyperion_lsp::config::load_config;
use hyperion_lsp::diagnostic_engine::collect_diagnostics_for_file;
use hyperion_lsp::security_diagnostics::SecurityDiagnosticsProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::FileId;
use hyperion_semantics::nullability::{
    extract_declared_type_names, extract_field_signatures, extract_method_signatures,
    TypeHierarchyFact, WorkspaceMemberFactRevision,
};
use hyperion_workspace::scanner::ScannerBuilder;
use hyperion_workspace::{Language, WorkspaceIndex};
use serde::Serialize;
use tower_lsp::lsp_types::DiagnosticSeverity;

#[derive(Parser)]
#[command(name = "hyperion-check")]
#[command(about = "Run Hyperion diagnostics in CI or from the CLI")]
#[command(version)]
struct Cli {
    /// Files or directories to scan (defaults to current directory)
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Root directory for workspace indexing (defaults to inferred root)
    #[arg(long)]
    root: Option<PathBuf>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Write output to a file instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,

    /// Path to a .hyperion.toml config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Fail when diagnostics at or above this severity exist
    #[arg(long, value_enum, default_value_t = FailOn::Error)]
    fail_on: FailOn,

    /// Comma-separated glob patterns to include
    #[arg(long, value_delimiter = ',')]
    include: Vec<String>,

    /// Comma-separated glob patterns to exclude
    #[arg(long, value_delimiter = ',')]
    exclude: Vec<String>,

    /// Do not respect .gitignore rules
    #[arg(long)]
    no_gitignore: bool,

    /// Follow symlinks while scanning
    #[arg(long)]
    follow_links: bool,

    /// Maximum directory traversal depth
    #[arg(long)]
    max_depth: Option<usize>,

    /// Disable type checking diagnostics
    #[arg(long)]
    no_typecheck: bool,

    /// Disable nullability diagnostics
    #[arg(long)]
    no_nullability: bool,

    /// Disable inspections (unused imports, dead code, etc.)
    #[arg(long)]
    no_inspections: bool,

    /// Disable security diagnostics
    #[arg(long)]
    no_security: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum FailOn {
    Error,
    Warning,
    Info,
    Hint,
    Never,
}

#[derive(Serialize)]
struct FileDiagnostics {
    path: String,
    diagnostics: Vec<tower_lsp::lsp_types::Diagnostic>,
}

struct PreparedFile {
    path: PathBuf,
    file_id: FileId,
    language: Language,
    source: Arc<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let root = determine_root(&cli.paths, cli.root.as_ref());
    let files = match collect_files(
        &cli.paths,
        cli.no_gitignore,
        cli.follow_links,
        cli.max_depth,
        &cli.include,
        &cli.exclude,
    ) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("Failed to scan files: {err}");
            return ExitCode::from(2);
        }
    };

    if files.is_empty() {
        println!("No source files found.");
        return ExitCode::SUCCESS;
    }

    let config = match load_config(&root, cli.config.as_deref()) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };

    let workspace_index = Arc::new(WorkspaceIndex::new(root.clone()));
    workspace_index.set_root(root);
    let host = SemanticHost::new(Arc::clone(&workspace_index));
    let security_provider = Arc::new(SecurityDiagnosticsProvider::new());

    let mut options = config.diagnostic_options();
    if cli.no_typecheck {
        options.run_type_check = false;
    }
    if cli.no_nullability {
        options.run_nullability = false;
    }
    if cli.no_inspections {
        options.run_inspections = false;
    }
    if cli.no_security {
        options.run_security = false;
    }

    let mut prepared_files = Vec::new();

    // The whole publication phase is one write transaction (this CLI is the
    // only user of the host, so it is uncontended); diagnostics run under read
    // transactions afterwards.
    let mut txn = host.write().await;

    // Publish the complete workspace revision before asking Salsa for any
    // diagnostics. In particular, a file may refer to a declaration that is
    // later in the sorted input list.
    for path in &files {
        let source = match hyperion_workspace::read_source_file(path) {
            Ok(text) => Arc::new(text),
            Err(err) => {
                eprintln!("Failed to read {}: {err}", path.display());
                continue;
            }
        };

        if let Err(err) = workspace_index.index_file_with_source(path, source.as_str()) {
            eprintln!("Failed to index {}: {err}", path.display());
            continue;
        }

        let Some(file_id) = workspace_index.get_file_id(path) else {
            eprintln!("Failed to resolve FileId for {}", path.display());
            continue;
        };

        let language = Language::from_path(path).unwrap_or(Language::Java);
        txn.db()
            .register_file(file_id, Arc::clone(&source), language);

        prepared_files.push(PreparedFile {
            path: path.clone(),
            file_id,
            language,
            source,
        });
    }

    // `index_file_with_source` resolves usages against declarations currently
    // present in the index. Repeat only the indexing transaction after every
    // declaration is known so files sorted before their dependencies receive
    // the same exact overload/member bindings as files sorted after them.
    for prepared in &prepared_files {
        if let Err(err) =
            workspace_index.index_file_with_source(&prepared.path, prepared.source.as_str())
        {
            eprintln!(
                "Failed to finalize index bindings for {}: {err}",
                prepared.path.display()
            );
        }
    }

    // Publish the immutable declaration revision only after every source file
    // is present in the index. Canonical direct supertypes then include Java's
    // full lexical/import/static-import resolution rather than a syntax-local
    // approximation, including files sorted before their dependencies.
    for prepared in &prepared_files {
        if prepared.language != Language::Java {
            continue;
        }
        let syntax = workspace_index
            .cached_java_parse(prepared.file_id, prepared.source.as_str())
            .map_or_else(
                || {
                    let tokens = hyperion_parser::Lexer::new(prepared.source.as_str()).collect();
                    let (green, _errors) = hyperion_parser::Parser::parse(tokens);
                    hyperion_parser::SyntaxNode::new_root(green)
                },
                |green| hyperion_parser::SyntaxNode::new_root(green.as_ref().clone()),
            );
        let signatures = extract_method_signatures(&syntax, prepared.file_id);
        let fields = extract_field_signatures(&syntax, prepared.file_id);
        let declared_types = extract_declared_type_names(&syntax);
        let Some(resolved) = workspace_index
            .file_supertypes_for_revision(prepared.file_id, prepared.source.as_str())
        else {
            eprintln!(
                "Skipped semantic facts without an exact hierarchy revision for {}",
                prepared.path.display()
            );
            continue;
        };
        let mut supertypes_by_owner = resolved
            .types
            .iter()
            .map(|resolved| (resolved.type_fqn.as_str(), resolved))
            .collect::<std::collections::HashMap<_, _>>();
        if supertypes_by_owner.len() != declared_types.len() {
            eprintln!(
                "Skipped inconsistent hierarchy facts for {}",
                prepared.path.display()
            );
            continue;
        }
        let type_hierarchies = declared_types
            .iter()
            .map(|owner| {
                supertypes_by_owner.remove(owner.as_str()).map(|resolved| {
                    TypeHierarchyFact::from_resolved_supertype_roles(
                        owner.clone(),
                        resolved.superclass.clone(),
                        resolved.interfaces.clone(),
                    )
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(type_hierarchies) = type_hierarchies else {
            eprintln!(
                "Skipped mismatched declaration/hierarchy facts for {}",
                prepared.path.display()
            );
            continue;
        };
        let mut db_guard = txn.db();
        let revision = WorkspaceMemberFactRevision::new(
            resolved.file_revision,
            resolved.workspace_revision,
            resolved.fingerprint,
        );
        let changed = workspace_index.with_current_resolved_file_supertypes(&resolved, || {
            let changed = db_guard
                .method_signature_cache()
                .update_workspace_file_member_facts(
                    prepared.file_id,
                    revision,
                    signatures,
                    fields,
                    declared_types,
                    type_hierarchies,
                );
            if changed {
                db_guard.bump_method_signature_version();
            }
            changed
        });
        if changed.is_none() {
            eprintln!(
                "Skipped concurrently retired hierarchy facts for {}",
                prepared.path.display()
            );
            continue;
        }
    }

    if !prepared_files.is_empty() {
        txn.bump_workspace_epoch();
    }
    drop(txn);

    let mut file_diagnostics = Vec::with_capacity(prepared_files.len());
    for prepared in prepared_files {
        let diagnostics = collect_diagnostics_for_file(
            prepared.file_id,
            language_id_for(prepared.language),
            prepared.source.as_str(),
            &host,
            Some(&security_provider),
            true,
            options,
        )
        .await;

        file_diagnostics.push(FileDiagnostics {
            path: prepared.path.display().to_string(),
            diagnostics,
        });
    }

    let exit_code = report_diagnostics(&cli, &file_diagnostics);
    ExitCode::from(exit_code)
}

fn determine_root(paths: &[PathBuf], override_root: Option<&PathBuf>) -> PathBuf {
    if let Some(root) = override_root {
        return root.clone();
    }

    let Some(first) = paths.first() else {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    };

    let mut root = if first.is_dir() {
        first.clone()
    } else {
        first
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };

    for path in paths.iter().skip(1) {
        let path_root = if path.is_dir() {
            path.clone()
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };
        while !path_root.starts_with(&root) {
            if let Some(parent) = root.parent() {
                root = parent.to_path_buf();
            } else {
                return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            }
        }
    }

    root
}

fn collect_files(
    paths: &[PathBuf],
    no_gitignore: bool,
    follow_links: bool,
    max_depth: Option<usize>,
    include_patterns: &[String],
    exclude_patterns: &[String],
) -> Result<Vec<PathBuf>, String> {
    let mut files = HashSet::new();

    for path in paths {
        if path.is_dir() {
            let mut builder = ScannerBuilder::new(path.clone())
                .respect_gitignore(!no_gitignore)
                .follow_links(follow_links);
            if let Some(depth) = max_depth {
                builder = builder.max_depth(depth);
            }
            let scanned = builder.scan().map_err(|err| err.to_string())?;
            for file in scanned {
                files.insert(file);
            }
        } else {
            files.insert(path.clone());
        }
    }

    let include = compile_patterns(include_patterns)?;
    let exclude = compile_patterns(exclude_patterns)?;

    let mut filtered: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| matches_filters(path, &include, &exclude))
        .collect();

    filtered.sort();
    Ok(filtered)
}

fn compile_patterns(patterns: &[String]) -> Result<Vec<Pattern>, String> {
    let mut compiled = Vec::new();
    for pattern in patterns {
        compiled.push(Pattern::new(pattern).map_err(|err| err.to_string())?);
    }
    Ok(compiled)
}

fn matches_filters(path: &Path, include: &[Pattern], exclude: &[Pattern]) -> bool {
    if !include.is_empty() && !include.iter().any(|pattern| pattern.matches_path(path)) {
        return false;
    }

    if !exclude.is_empty() && exclude.iter().any(|pattern| pattern.matches_path(path)) {
        return false;
    }

    true
}

const fn language_id_for(language: Language) -> &'static str {
    match language {
        Language::Java => "java",
        Language::Kotlin => "kotlin",
        Language::Groovy => "groovy",
        Language::Scala => "scala",
    }
}

fn report_diagnostics(cli: &Cli, diagnostics: &[FileDiagnostics]) -> u8 {
    let mut has_failures = false;

    match cli.format {
        OutputFormat::Text => {
            let mut output = String::new();
            for entry in diagnostics {
                for diag in &entry.diagnostics {
                    let severity = diag.severity.unwrap_or(DiagnosticSeverity::ERROR);
                    if severity_matches_fail_on(severity, cli.fail_on) {
                        has_failures = true;
                    }
                    let line = diag.range.start.line + 1;
                    let col = diag.range.start.character + 1;
                    let severity_label = severity_label(severity);
                    output.push_str(&format!(
                        "{}:{}:{}: {}: {}\n",
                        entry.path, line, col, severity_label, diag.message
                    ));
                }
            }
            write_output(cli.output.as_ref(), &output);
        }
        OutputFormat::Json => {
            let mut serialized = Vec::new();
            for entry in diagnostics {
                if entry.diagnostics.iter().any(|diag| {
                    severity_matches_fail_on(
                        diag.severity.unwrap_or(DiagnosticSeverity::ERROR),
                        cli.fail_on,
                    )
                }) {
                    has_failures = true;
                }
                serialized.push(entry);
            }
            let json =
                serde_json::to_string_pretty(&serialized).unwrap_or_else(|_| "[]".to_string());
            write_output(cli.output.as_ref(), &json);
        }
    }

    u8::from(has_failures)
}

fn write_output(path: Option<&PathBuf>, content: &str) {
    if let Some(path) = path {
        if let Err(err) = fs::write(path, content) {
            eprintln!("Failed to write output to {}: {err}", path.display());
        }
    } else {
        print!("{content}");
    }
}

fn severity_matches_fail_on(severity: DiagnosticSeverity, fail_on: FailOn) -> bool {
    match fail_on {
        FailOn::Never => false,
        FailOn::Error => severity == DiagnosticSeverity::ERROR,
        FailOn::Warning => {
            severity == DiagnosticSeverity::ERROR || severity == DiagnosticSeverity::WARNING
        }
        FailOn::Info => {
            severity == DiagnosticSeverity::ERROR
                || severity == DiagnosticSeverity::WARNING
                || severity == DiagnosticSeverity::INFORMATION
        }
        FailOn::Hint => true,
    }
}

const fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "error",
        DiagnosticSeverity::WARNING => "warning",
        DiagnosticSeverity::INFORMATION => "info",
        DiagnosticSeverity::HINT => "hint",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tower_lsp::lsp_types::{Diagnostic, Position, Range};

    fn diag_with_severity(severity: DiagnosticSeverity) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(severity),
            code: None,
            code_description: None,
            source: None,
            message: "test".to_string(),
            related_information: None,
            tags: None,
            data: None,
        }
    }

    #[test]
    fn report_diagnostics_sets_failure_on_error() {
        let temp = TempDir::new().unwrap();
        let output = temp.path().join("out.txt");
        let cli = Cli {
            paths: vec![],
            root: None,
            format: OutputFormat::Text,
            output: Some(output),
            config: None,
            fail_on: FailOn::Error,
            include: Vec::new(),
            exclude: Vec::new(),
            no_gitignore: false,
            follow_links: false,
            max_depth: None,
            no_typecheck: false,
            no_nullability: false,
            no_inspections: false,
            no_security: false,
        };

        let diagnostics = vec![FileDiagnostics {
            path: "Test.java".to_string(),
            diagnostics: vec![diag_with_severity(DiagnosticSeverity::ERROR)],
        }];

        let exit_code = report_diagnostics(&cli, &diagnostics);
        assert_eq!(exit_code, 1);
    }
}
