#!/usr/bin/env bash
# Reject hyperion-build integration targets that cannot exercise the product.
#
# This is intentionally a smoke check, not a substitute for reviewing contract
# quality. It prevents the concrete failure mode where entire
# test binaries that only construct fixtures and assert their own source text.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

irrelevant=()
file_list=$(mktemp)
trap 'rm -f "$file_list"' EXIT
find hyperion-build/tests -maxdepth 1 -type f -name '*.rs' -print0 > "$file_list"

# rg status 1 means no match; execution errors must fail the gate.
contains() {
    local status=0
    rg -q "$1" "$2" || status=$?
    if [[ "$status" -gt 1 ]]; then
        printf 'Failed to inspect %s (rg status %s)\n' "$2" "$status" >&2
        exit "$status"
    fi
    return "$status"
}

while IFS= read -r -d '' file; do
    if ! contains '#\[(tokio::)?test\]' "$file"; then
        continue
    fi
    if ! contains '\bhyperion_build\b|CARGO_BIN_EXE_' "$file"; then
        irrelevant+=("$file")
    fi
done < "$file_list"

if [[ ${#irrelevant[@]} -eq 0 ]]; then
    printf '✓ Every hyperion-build integration target references the library or a built binary\n'
    exit 0
fi

printf '✗ Integration targets with tests but no hyperion-build product reference:\n' >&2
for file in "${irrelevant[@]}"; do
    printf '  - %s\n' "$file" >&2
done
printf 'Exercise hyperion_build or a CARGO_BIN_EXE_* binary, or remove the self-test target.\n' >&2
exit 1
