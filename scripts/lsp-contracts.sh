#!/usr/bin/env bash
# Fast contracts for editor behavior at the production-facing LSP boundary.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

nice -n 10 cargo test -p hyperion-lsp -j 4 --test protocol lsp_contract_tests::
nice -n 10 cargo test -p hyperion-lsp -j 4 --test server_e2e
nice -n 10 cargo test -p hyperion-lsp -j 4 --test protocol java_only_request_gates::
nice -n 10 cargo test -p hyperion-lsp -j 4 --test kotlin kotlin_semantic_completion_tests::
nice -n 10 cargo test -p hyperion-lsp -j 4 --test diagnostics security_diagnostic_pipeline_tests::
