#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

export CARGO_TARGET_DIR="$TEST_ROOT/target"
export HYPERION_BIN_DIR="$TEST_ROOT/bin"
export HYPERION_CARGO="$REPO_ROOT/scripts/tests/fixtures/fake-cargo.sh"
export HYPERION_FAKE_CARGO_ARGS="$TEST_ROOT/cargo-args"

# The installed script must build this repository even from another directory.
cd "$TEST_ROOT"
sh "$REPO_ROOT/scripts/build-hyperion-ls.sh"
grep -Fx 'build -p hyperion-lsp --bin hyperion-lsp -j 4' "$HYPERION_FAKE_CARGO_ARGS"
cmp "$CARGO_TARGET_DIR/debug/hyperion-lsp" "$HYPERION_BIN_DIR/hyperion-lsp"
test ! -e "$HYPERION_BIN_DIR/hyperion-ls"
test -x "$HYPERION_BIN_DIR/hyperion-lsp"

HYPERION_INSTALL_LEGACY_ALIAS=1 "$REPO_ROOT/scripts/build-hyperion-ls.sh" release
grep -Fx 'build --release -p hyperion-lsp --bin hyperion-lsp -j 4' "$HYPERION_FAKE_CARGO_ARGS"
cmp "$CARGO_TARGET_DIR/release/hyperion-lsp" "$HYPERION_BIN_DIR/hyperion-lsp"
cmp "$HYPERION_BIN_DIR/hyperion-lsp" "$HYPERION_BIN_DIR/hyperion-ls"
test -x "$HYPERION_BIN_DIR/hyperion-lsp"

if "$REPO_ROOT/scripts/build-hyperion-ls.sh" optimized >"$TEST_ROOT/invalid-out" 2>"$TEST_ROOT/invalid-err"; then
    printf 'installer accepted an unsupported build mode\n' >&2
    exit 1
fi
grep -F '[debug|release]' "$TEST_ROOT/invalid-err"

# The default destination is editor-neutral and works without an override.
unset HYPERION_BIN_DIR
HOME="$TEST_ROOT/default-home" sh "$REPO_ROOT/scripts/build-hyperion-ls.sh"
cmp "$CARGO_TARGET_DIR/debug/hyperion-lsp" "$TEST_ROOT/default-home/.local/bin/hyperion-lsp"
test ! -e "$TEST_ROOT/default-home/.local/bin/hyperion-ls"
