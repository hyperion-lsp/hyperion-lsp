#!/usr/bin/env bash
# Fast contracts for local installation and editor packaging.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

bash scripts/tests/build-hyperion-ls-test.sh
bash scripts/tests/git-hooks-test.sh
python3 scripts/tests/check-regression-test.py
nice -n 10 cargo test -p hyperion-lsp -j 4 --test protocol cli_integration_tests::

if command -v npm >/dev/null 2>&1; then
    (
        cd editors/vscode
        npm ci --ignore-scripts
        npm test
        npm audit --audit-level=high
    )
else
    printf 'npm unavailable; skipping VS Code extension contracts\n'
fi
