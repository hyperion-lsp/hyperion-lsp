#!/bin/sh
#
# Setup script for git hooks
# Run this after cloning the repository
#

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HOOKS_DIR="$(git -C "$REPO_ROOT" rev-parse --path-format=absolute --git-path hooks)"
mkdir -p "$HOOKS_DIR"
SOURCE_HOOKS="$SCRIPT_DIR/hooks"

echo "Setting up git hooks..."

# Copy hooks from scripts/hooks to .git/hooks
for hook in "$SOURCE_HOOKS"/*; do
    if [ -f "$hook" ]; then
        hook_name=$(basename "$hook")
        cp "$hook" "$HOOKS_DIR/$hook_name"
        chmod +x "$HOOKS_DIR/$hook_name"
        echo "  Installed: $hook_name"
    fi
done

echo "Git hooks installed successfully!"
echo ""
echo "Alternatively, you can configure git to use the hooks directory directly:"
echo "  git config core.hooksPath scripts/hooks"
