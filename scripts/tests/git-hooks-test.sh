#!/usr/bin/env bash
# Formatting checks must never expand a partially staged commit.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/repo/scripts/hooks" "$TEST_ROOT/tools"
cp "$REPO_ROOT/scripts/setup-hooks.sh" "$TEST_ROOT/repo/scripts/"
cp "$REPO_ROOT/scripts/hooks/pre-commit" "$TEST_ROOT/repo/scripts/hooks/"
cd "$TEST_ROOT/repo"
git init -q
git config user.name Test
git config user.email test@example.org
printf 'staged\n' > example.rs
git add example.rs
git -c core.hooksPath=/dev/null commit -qm initial
git worktree add -qb linked "$TEST_ROOT/linked"
# Installation works when .git is a file, as it is in a linked worktree.
mkdir -p "$TEST_ROOT/linked/scripts/hooks"
cp scripts/setup-hooks.sh "$TEST_ROOT/linked/scripts/"
cp scripts/hooks/pre-commit "$TEST_ROOT/linked/scripts/hooks/"
sh "$TEST_ROOT/linked/scripts/setup-hooks.sh"
HOOK=$(git rev-parse --path-format=absolute --git-path hooks)/pre-commit
[[ -x "$HOOK" ]]
printf 'partial\n' > example.rs
git add example.rs
printf 'unstaged\n' >> example.rs
git diff --cached --binary > "$TEST_ROOT/index-before"
cp example.rs "$TEST_ROOT/file-before"
cat > "$TEST_ROOT/tools/cargo" <<'CARGO'
#!/bin/sh
printf '%s\n' "$*" > "$HOOK_TEST_ARGS"
exit 1
CARGO
chmod +x "$TEST_ROOT/tools/cargo"
export HOOK_TEST_ARGS="$TEST_ROOT/args"
if PATH="$TEST_ROOT/tools:$PATH" sh "$HOOK"; then
    printf 'Formatting failure did not block the commit\n' >&2
    exit 1
fi
grep -Fx 'fmt --all -- --check' "$TEST_ROOT/args"
git diff --cached --binary > "$TEST_ROOT/index-after"
cmp "$TEST_ROOT/index-before" "$TEST_ROOT/index-after"
cmp example.rs "$TEST_ROOT/file-before"
printf '✓ Hooks support worktrees and preserve partially staged edits\n'
