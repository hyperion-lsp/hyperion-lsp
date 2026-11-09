#!/usr/bin/env bash
# Exercise the quality gate itself, including failures of its analysis tools.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
CHECKER="$REPO_ROOT/scripts/check-test-bodies.sh"

expect_status() {
    local expected="$1"; shift
    local actual=0
    "$@" > "$TEST_ROOT/output" 2>&1 || actual=$?
    if [[ "$actual" -ne "$expected" ]]; then
        cat "$TEST_ROOT/output" >&2
        printf 'Expected status %s, got %s\n' "$expected" "$actual" >&2
        exit 1
    fi
}

printf '#[test]\nfn real_contract() { assert_eq!(2 + 2, 4); }\n' > "$TEST_ROOT/valid test.rs"
printf '#[tokio::test]\nasync fn placeholder() { /* nothing */ }\n' > "$TEST_ROOT/empty test.rs"
expect_status 0 bash "$CHECKER" "$TEST_ROOT/valid test.rs"
expect_status 1 bash "$CHECKER" "$TEST_ROOT/empty test.rs"
grep -F 'empty test.rs:placeholder' "$TEST_ROOT/output" > /dev/null
expect_status 2 bash "$CHECKER" "$TEST_ROOT/missing.rs"
# The caller's unsupported locale must not break comment stripping.
expect_status 0 env LC_ALL=not_a_real_locale bash "$CHECKER" "$TEST_ROOT/valid test.rs"

# Tool failures must not be mistaken for an empty list of offending tests.
mkdir "$TEST_ROOT/tools"
for tool in perl rg git; do
    printf '#!/bin/sh\nexit 42\n' > "$TEST_ROOT/tools/$tool"
    chmod +x "$TEST_ROOT/tools/$tool"
    if [[ "$tool" == git ]]; then
        expect_status 42 env PATH="$TEST_ROOT/tools:$PATH" bash "$CHECKER"
    else
        expect_status 2 env PATH="$TEST_ROOT/tools:$PATH" bash "$CHECKER" "$TEST_ROOT/valid test.rs"
    fi
    rm "$TEST_ROOT/tools/$tool"
done

printf '✓ Test-body checker rejects placeholders and tool failures\n'
