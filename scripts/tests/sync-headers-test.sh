#!/usr/bin/env bash
# Verify scope, idempotence, attribution, and path handling in an isolated repo.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/scripts" "$TEST_ROOT/vendor" "$TEST_ROOT/ignored"
cp "$REPO_ROOT/scripts/sync-headers.sh" "$TEST_ROOT/scripts/"
cd "$TEST_ROOT"
git init -q
printf '/ignored/\n' > .gitignore
printf 'fn main() {}\n' > 'new file.rs'
printf 'third-party content\n' | tee vendor/upstream.rs > ignored/example.rs
head -n 10 "$REPO_ROOT/hyperion-parser/src/lib.rs" | sed 's/Copyright 2025/Copyright 2026/' > existing.rs
printf '\nfn existing() {}\n' >> existing.rs
cp existing.rs expected.rs
chmod +x 'new file.rs'
bash scripts/sync-headers.sh
cmp existing.rs expected.rs
[[ $(head -n 1 'new file.rs') == "// Copyright $(date +%Y) Adrian Helvik" ]]
[[ -x 'new file.rs' ]]
grep -Fx 'fn main() {}' 'new file.rs'
grep -Fx 'third-party content' vendor/upstream.rs
grep -Fx 'third-party content' ignored/example.rs
cp 'new file.rs' before
bash scripts/sync-headers.sh
cmp before 'new file.rs'
HYPERION_REPO_URL='https://example.org/project?q=$literal&x=1' bash scripts/sync-headers.sh
# SPDX headers are independent of repository URLs, including shell metacharacters.
cmp before 'new file.rs'
grep -Fx '// SPDX-License-Identifier: PolyForm-Shield-1.0.0' 'new file.rs'
[[ $(rg -c '^// Copyright' existing.rs) == 1 ]]
# A failed file listing must fail the operation.
mkdir tools
printf '#!/bin/sh\nexit 42\n' > tools/git
chmod +x tools/git
status=0
PATH="$TEST_ROOT/tools:$PATH" bash scripts/sync-headers.sh > /dev/null 2>&1 || status=$?
[[ "$status" == 42 ]]
printf '✓ Header synchronization preserves attribution and first-party scope\n'
