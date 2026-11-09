// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::process::ExitCode;

use tokio::io::{stdin, stdout, BufReader};

#[tokio::main]
async fn main() -> ExitCode {
    // Log to a file (not stderr/stdout to avoid protocol interference).
    hyperion_dap::logging::init_file_logging();

    let reader = BufReader::new(stdin());
    let writer = stdout();

    if let Err(e) = hyperion_dap::server::run(reader, writer).await {
        tracing::error!("DAP server error: {e}");
        return ExitCode::FAILURE;
    }

    tracing::info!("Hyperion DAP Server stopped");
    ExitCode::SUCCESS
}
