#!/usr/bin/env sh
set -eu

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "$REPO_ROOT"

MODE=${1:-debug}
case "$MODE" in
  debug) PROFILE=debug ;;
  release) PROFILE=release ;;
  *)
    printf 'Usage: %s [debug|release]\n' "$0" >&2
    exit 2
    ;;
esac

BIN_DIR=${HYPERION_BIN_DIR:-"$HOME/.local/bin"}
TARGET_ROOT=${CARGO_TARGET_DIR:-target}
CARGO_COMMAND=${HYPERION_CARGO:-cargo}

if [ "$PROFILE" = "release" ]; then
  nice -n 10 "$CARGO_COMMAND" build --release -p hyperion-lsp --bin hyperion-lsp -j 4
else
  nice -n 10 "$CARGO_COMMAND" build -p hyperion-lsp --bin hyperion-lsp -j 4
fi

mkdir -p "$BIN_DIR"
install -m 755 "$TARGET_ROOT/$PROFILE/hyperion-lsp" "$BIN_DIR/hyperion-lsp"
# Compatibility alias is an explicit opt-in.
if [ "${HYPERION_INSTALL_LEGACY_ALIAS:-0}" = 1 ]; then
  install -m 755 "$TARGET_ROOT/$PROFILE/hyperion-lsp" "$BIN_DIR/hyperion-ls"
fi

printf "Installed hyperion-lsp to %s\n" "$BIN_DIR"
