// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion LSP Server Entry Point
//!
//! This is the main entry point for the Hyperion Language Server Protocol (LSP) server.
//! It initializes the async runtime, sets up logging/tracing, and starts the LSP server
//! on standard input/output.
//!
//! # Architecture
//!
//! The server uses the following architecture:
//! - **Runtime**: Tokio async runtime for non-blocking I/O
//! - **Protocol**: Tower-LSP for LSP protocol handling
//! - **Transport**: Stdio (stdin/stdout) for client communication
//! - **Logging**: Tracing framework for structured logging
//!
//! # Communication Protocol
//!
//! The LSP uses JSON-RPC 2.0 over stdio:
//! - **Input**: The client sends requests/notifications on stdin
//! - **Output**: The server sends responses/notifications on stdout
//! - **Logging**: Diagnostic logs are sent via LSP `window/logMessage` notifications
//!
//! # Usage
//!
//! The server is designed to be started by an LSP client (editor/IDE):
//!
//! ```bash
//! hyperion-lsp
//! ```
//!
//! The server will:
//! 1. Initialize logging to a file (not stdout, to avoid interfering with LSP protocol)
//! 2. Create the LSP service with the Backend implementation
//! 3. Listen for LSP messages on stdin
//! 4. Send responses and notifications on stdout
//! 5. Run until the client sends a shutdown request

use clap::{Parser, Subcommand};
use glob::glob;
use hyperion_fmt::{format_source, FormatConfig};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::io::{stdin, stdout};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use hyperion_lsp::server::create_lsp_server;

#[derive(Parser)]
#[command(name = "hyperion")]
#[command(version)]
#[command(about = "Hyperion Language Server for Java and Kotlin", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Format Java source files (palantir-java-format style)
    Format {
        /// Files to format (glob patterns supported)
        #[arg(required = true)]
        files: Vec<String>,
        /// Check if files are formatted (exit 1 if not, don't modify)
        #[arg(long, short)]
        check: bool,
        /// Print files that would be changed
        #[arg(long, short)]
        list: bool,
    },
    /// Run as a Debug Adapter Protocol (DAP) server for JVM debugging
    Dap,
    /// List declarations matching a simple name (e.g. `hyperion sym LOGIN`)
    Sym {
        /// Simple name to look up
        name: String,
        #[command(flatten)]
        search: SearchArgs,
    },
    /// Find references to a symbol (e.g. `hyperion refs CoreEvent.LOGIN`)
    Refs {
        /// Simple name, `Type.member`, or fully qualified name
        name: String,
        #[command(flatten)]
        search: SearchArgs,
    },
    /// Show where a symbol is declared
    Def {
        /// Simple name, `Type.member`, or fully qualified name
        name: String,
        #[command(flatten)]
        search: SearchArgs,
    },
    /// List implementations/subtypes of an interface or class
    Impls {
        /// Simple name or fully qualified name of the type
        name: String,
        #[command(flatten)]
        search: SearchArgs,
    },
    /// Regex search with enclosing-declaration annotation (`Class#method`)
    Grep {
        /// Regex pattern (Rust regex syntax)
        pattern: String,
        /// Skip enclosing-declaration annotation (plain text search)
        #[arg(long)]
        no_enclosing: bool,
        #[command(flatten)]
        search: SearchArgs,
    },
}

#[derive(clap::Args)]
struct SearchArgs {
    /// Workspace root (defaults to the current directory)
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit JSON lines instead of text
    #[arg(long)]
    json: bool,
}

/// Which index-backed search operation to run.
#[derive(Clone, Copy)]
enum SearchOp {
    Sym,
    Refs,
    Def,
    Impls,
}

fn search_root(root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn finish_search(hits: &[hyperion_lsp::search_cli::Hit], json: bool) -> ExitCode {
    use hyperion_lsp::search_cli::{render_json, render_text};
    print!(
        "{}",
        if json {
            render_json(hits)
        } else {
            render_text(hits)
        }
    );
    if hits.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Exit codes for search subcommands: 0 = hits, 1 = no hits, 2 = ambiguous
/// query or error (grep-compatible).
async fn run_search_command(op: SearchOp, name: &str, search: SearchArgs) -> ExitCode {
    use hyperion_lsp::search_cli::{Outcome, SearchSession};

    let session = match SearchSession::open(search_root(search.root)).await {
        Ok(session) => session,
        Err(error) => {
            eprintln!("error: {error:#}");
            return ExitCode::from(2);
        }
    };

    let outcome = match op {
        SearchOp::Sym => Outcome::Hits(session.sym(name)),
        SearchOp::Refs => session.refs(name),
        SearchOp::Def => session.def(name),
        SearchOp::Impls => session.impls(name),
    };

    match outcome {
        Outcome::Hits(hits) => finish_search(&hits, search.json),
        Outcome::Ambiguous(candidates) => {
            eprintln!("ambiguous name '{name}' - candidates:");
            print!("{}", hyperion_lsp::search_cli::render_text(&candidates));
            eprintln!("re-run with a fully qualified name");
            ExitCode::from(2)
        }
        Outcome::Unavailable(reason) => {
            eprintln!("references unavailable: {reason}");
            ExitCode::from(2)
        }
    }
}

fn run_grep_command(pattern: &str, no_enclosing: bool, search: SearchArgs) -> ExitCode {
    match hyperion_lsp::search_cli::grep(&search_root(search.root), pattern, !no_enclosing) {
        Ok(hits) => finish_search(&hits, search.json),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

/// Format Java source files
///
/// Returns true if all files are properly formatted (or were formatted successfully)
fn format_files(patterns: Vec<String>, check: bool, list: bool) -> bool {
    let config = FormatConfig::default();
    let mut files: Vec<PathBuf> = Vec::new();

    // Expand glob patterns
    for pattern in &patterns {
        match glob(pattern) {
            Ok(paths) => {
                for entry in paths.flatten() {
                    // Only process .java files
                    if entry.extension().is_some_and(|ext| ext == "java") {
                        files.push(entry);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: Invalid glob pattern '{pattern}': {e}");
                return false;
            }
        }
    }

    if files.is_empty() {
        eprintln!("No Java files found matching: {}", patterns.join(", "));
        return true; // Not an error, just nothing to do
    }

    let mut unformatted_count = 0;
    let mut error_count = 0;
    let total = files.len();

    for file in &files {
        let source = match hyperion_workspace::read_source_file(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file.display(), e);
                error_count += 1;
                continue;
            }
        };

        let formatted = match format_source(&source, &config) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error formatting {}: {}", file.display(), e);
                error_count += 1;
                continue;
            }
        };

        if source != formatted {
            unformatted_count += 1;

            if list || check {
                println!("{}", file.display());
            }

            if !check && !list {
                // Write formatted content
                if let Err(e) = fs::write(file, &formatted) {
                    eprintln!("Error writing {}: {}", file.display(), e);
                    error_count += 1;
                }
            }
        }
    }

    // Print summary
    if check {
        if unformatted_count > 0 {
            eprintln!("\n{unformatted_count} of {total} files need formatting");
            return false;
        }
        println!("All {total} files are properly formatted");
    } else if !list {
        if unformatted_count > 0 {
            println!("Formatted {unformatted_count} of {total} files");
        } else {
            println!("All {total} files are properly formatted");
        }
    }

    error_count == 0 && (unformatted_count == 0 || !check)
}

/// Main entry point for the Hyperion LSP server.
///
/// # Process Flow
///
/// 1. Initialize the tracing/logging infrastructure
/// 2. Create the LSP service and server
/// 3. Run the server on stdin/stdout
/// 4. Exit when the client disconnects
///
/// # Error Handling
///
/// The server uses `Result` types throughout. Any errors during initialization
/// or runtime are logged and cause the process to exit with a non-zero status code.
///
/// # Panics
///
/// The server should not panic under normal circumstances. All errors are handled
/// gracefully and reported to the client via LSP error responses.
fn main() -> ExitCode {
    hyperion_lsp::allocator::tune();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Failed to create Tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    let exit_code = runtime.block_on(run());

    // Tokio's stdin adapter uses a blocking worker that cannot be cancelled while the editor
    // keeps its pipe open. A bounded runtime shutdown ensures a finished/fatal connection loop
    // cannot leave the process stuck waiting for that worker forever.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    exit_code
}

async fn run() -> ExitCode {
    let cli = Cli::parse();

    // Handle CLI commands
    match cli.command {
        Some(Commands::Format { files, check, list }) => {
            let success = format_files(files, check, list);
            return if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Some(Commands::Dap) => {
            // The protocol owns stdout and editors rarely surface stderr, so
            // the adapter logs to $LOG_DIR (default: temp dir)/hyperion-dap.log.
            hyperion_dap::logging::init_file_logging();
            let reader = tokio::io::BufReader::new(stdin());
            let writer = stdout();
            if let Err(e) = hyperion_dap::server::run(reader, writer).await {
                tracing::error!("DAP server error: {e}");
                eprintln!("DAP server error: {e}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        Some(Commands::Sym { name, search }) => {
            return run_search_command(SearchOp::Sym, &name, search).await;
        }
        Some(Commands::Refs { name, search }) => {
            return run_search_command(SearchOp::Refs, &name, search).await;
        }
        Some(Commands::Def { name, search }) => {
            return run_search_command(SearchOp::Def, &name, search).await;
        }
        Some(Commands::Impls { name, search }) => {
            return run_search_command(SearchOp::Impls, &name, search).await;
        }
        Some(Commands::Grep {
            pattern,
            no_enclosing,
            search,
        }) => {
            return run_grep_command(&pattern, no_enclosing, search);
        }
        None => {
            // No subcommand - run LSP server
        }
    }
    // Initialize tracing/logging
    //
    // We configure tracing to write to a file, falling back to stderr so
    // stdout remains reserved for LSP. The log directory can be
    // specified via LOG_FILE env var, otherwise defaults to the system's temp directory.
    //
    // Log levels:
    // - TRACE: Very detailed information (disabled by default)
    // - DEBUG: Detailed information for debugging
    // - INFO: General informational messages
    // - WARN: Warning messages for unexpected situations
    // - ERROR: Error messages for failures
    let log_dir =
        std::env::var("LOG_FILE").map_or_else(|_| std::env::temp_dir(), std::path::PathBuf::from);
    let log_filename = format!("hyperion-lsp-{}.log", std::process::id());
    let log_file = log_dir.join(&log_filename);
    let log_writer = match tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::NEVER)
        .filename_prefix(log_filename)
        .build(&log_dir)
    {
        Ok(file) => {
            eprintln!("LOG_FILE: {}", log_file.display());
            tracing_subscriber::fmt::writer::BoxMakeWriter::new(file)
        }
        Err(error) => {
            eprintln!(
                "Cannot open log file {}: {error}; falling back to stderr",
                log_file.display()
            );
            tracing_subscriber::fmt::writer::BoxMakeWriter::new(std::io::stderr)
        }
    };

    // Log level: INFO by default, DEBUG if HYPERION_DEBUG=1
    let log_level = if std::env::var("HYPERION_DEBUG").is_ok() {
        eprintln!("DEBUG logging enabled (HYPERION_DEBUG=1)");
        Level::DEBUG
    } else {
        Level::INFO
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(log_writer)
        .with_ansi(false) // No ANSI colors in log file
        .with_target(true) // Include module paths
        .with_thread_ids(true) // Include thread IDs for debugging
        .with_line_number(true) // Include line numbers
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    info!(
        "Starting Hyperion LSP Server v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Create the LSP service
    //
    // The `create_lsp_server` function returns:
    // 1. The LspService configured with our Backend
    // 2. The Server socket for running the service
    let (service, socket) = create_lsp_server();

    info!("LSP service created, listening on stdin/stdout");

    // Run the server
    //
    // The server will:
    // - Read JSON-RPC messages from stdin
    // - Route them to the appropriate handler methods
    // - Write JSON-RPC responses to stdout
    // - Continue until the client sends a shutdown request
    tower_lsp::Server::new(stdin(), stdout(), socket)
        .serve(service)
        .await;

    info!("Hyperion LSP Server stopped");
    ExitCode::SUCCESS
}
