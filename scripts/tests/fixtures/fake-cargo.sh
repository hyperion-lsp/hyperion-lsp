#!/usr/bin/env sh

set -eu

: "${CARGO_TARGET_DIR:?test must set CARGO_TARGET_DIR}"
: "${HYPERION_FAKE_CARGO_ARGS:?test must set HYPERION_FAKE_CARGO_ARGS}"

printf '%s\n' "$*" > "$HYPERION_FAKE_CARGO_ARGS"

test -f hyperion-lsp/Cargo.toml

PROFILE=debug
for argument in "$@"; do
  if [ "$argument" = "--release" ]; then
    PROFILE=release
  fi
done

mkdir -p "$CARGO_TARGET_DIR/$PROFILE"
printf 'fake hyperion-lsp %s\n' "$PROFILE" > "$CARGO_TARGET_DIR/$PROFILE/hyperion-lsp"
chmod 755 "$CARGO_TARGET_DIR/$PROFILE/hyperion-lsp"
