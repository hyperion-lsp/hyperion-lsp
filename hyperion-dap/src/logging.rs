// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! File logging for the DAP adapter.
//!
//! stdout carries the protocol and stderr is usually swallowed by the editor,
//! so the adapter logs to `hyperion-dap.log` in `$LOG_DIR` (default: the
//! system temp directory). `HYPERION_DEBUG=1` raises the level to DEBUG.

use std::path::PathBuf;

/// Start logging to the adapter's log file and return its path. Returns
/// `None` (and logs nothing) if a subscriber is already installed or the file
/// cannot be created; the adapter must work either way.
pub fn init_file_logging() -> Option<PathBuf> {
    let log_dir = std::env::var("LOG_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    std::fs::create_dir_all(&log_dir).ok()?;
    let log_file = log_dir.join("hyperion-dap.log");

    let appender = tracing_appender::rolling::never(&log_dir, "hyperion-dap.log");
    let level = if std::env::var("HYPERION_DEBUG").is_ok() {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(level)
        .with_writer(appender)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok()?;

    tracing::info!(
        "Starting Hyperion DAP adapter v{} (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    eprintln!("hyperion-dap log: {}", log_file.display());
    Some(log_file)
}
