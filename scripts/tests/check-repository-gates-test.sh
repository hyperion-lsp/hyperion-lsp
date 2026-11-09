#!/usr/bin/env bash
# Discovery/inspection failures must not be reported as clean repository checks.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/scripts" "$TEST_ROOT/hyperion-build/tests" "$TEST_ROOT/tools"
cp "$REPO_ROOT/scripts/check-headers.sh" "$REPO_ROOT/scripts/check-build-test-relevance.sh" "$TEST_ROOT/scripts/"
cd "$TEST_ROOT"
git init -q
git config user.name Test
git config user.email test@example.org
printf 'ignored/\n' > .gitignore
git add .gitignore
git commit -qm initial
git update-ref refs/remotes/origin/main HEAD
printf '#[test]\nfn product() { hyperion_build::run(); }\n' > 'hyperion-build/tests/product test.rs'
bash scripts/check-build-test-relevance.sh
if bash scripts/check-headers.sh; then exit 1; fi
printf '// Copyright 2026 Adrian Helvik\n// SPDX-License-Identifier: PolyForm-Shield-1.0.0\n' > valid.rs
git add valid.rs 'hyperion-build/tests/product test.rs'
git commit -qm fixture
printf 'another commit\n' > unrelated
git add unrelated
git commit -qm unrelated
# The default gate must still find the first commit's missing header.
if bash scripts/check-headers.sh; then exit 1; fi
sed -i '1i// Copyright 2026 Adrian Helvik\n// SPDX-License-Identifier: PolyForm-Shield-1.0.0' 'hyperion-build/tests/product test.rs'
mkdir ignored
printf 'external code\n' > ignored/example.rs
bash scripts/check-headers.sh --all
for pair in git:check-headers.sh find:check-build-test-relevance.sh rg:check-build-test-relevance.sh; do
    tool=${pair%%:*}
    checker=${pair#*:}
    printf '#!/bin/sh\nexit 42\n' > "tools/$tool"
    chmod +x "tools/$tool"
    status=0
    PATH="$TEST_ROOT/tools:$PATH" bash "scripts/$checker" > output 2>&1 || status=$?
    [[ "$status" == 42 ]]
    rm "tools/$tool"
done
printf '✓ Repository gates reject tool failures and inspect the whole branch\n'

# A public root commit must inspect committed files even without a base ref.
mkdir "$TEST_ROOT/root-commit"
cd "$TEST_ROOT/root-commit"
git init -q
git config user.name Test
git config user.email test@example.org
mkdir scripts
cp "$REPO_ROOT/scripts/check-headers.sh" scripts/
printf 'fn main() {}\n' > missing.rs
git add .
git commit -qm initial
if bash scripts/check-headers.sh; then
    printf 'root commit missing header was accepted\n' >&2
    exit 1
fi
printf '// Copyright 2026 Adrian Helvik\n// SPDX-License-Identifier: PolyForm-Shield-1.0.0\nfn main() {}\n' > missing.rs
bash scripts/check-headers.sh
