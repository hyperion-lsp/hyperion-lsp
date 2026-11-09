#!/usr/bin/env bash
# Kept as a compatibility entry point for existing local workflows.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/lsp-contracts.sh"
