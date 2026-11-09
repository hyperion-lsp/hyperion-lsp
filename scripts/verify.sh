#!/usr/bin/env bash
# Local verification. --fast still runs formatting, linting, and focused contracts;
# the full mode additionally tests each first-party package sequentially.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

RED=$'\033[0;31m'; GREEN=$'\033[0;32m'; YELLOW=$'\033[1;33m'; NC=$'\033[0m'

case "${1:-}" in
    '') [[ $# -eq 0 ]] || exit 2; FAST=0 ;;
    --fast) [[ $# -eq 1 ]] || exit 2; FAST=1 ;;
    *) printf 'Usage: %s [--fast]\n' "$0" >&2; exit 2 ;;
esac

failures=()

step() {
    local name="$1"; shift
    printf '\n%s==> %s%s\n' "$YELLOW" "$name" "$NC"
    if "$@"; then
        printf '%s    ok%s\n' "$GREEN" "$NC"
    else
        printf '%s    FAILED%s\n' "$RED" "$NC"
        failures+=("$name")
    fi
}

# Third-party code must keep its own attribution. scripts/check-headers.sh
# excludes vendor/ for that reason; see the comment in that file before
# changing it.
step "copyright headers" bash scripts/check-headers.sh --all
step "build-test relevance" bash scripts/check-build-test-relevance.sh
step "test-body checker contracts" bash scripts/tests/check-test-bodies-test.sh
step "header synchronization contracts" bash scripts/tests/sync-headers-test.sh
step "repository gate contracts" bash scripts/tests/check-repository-gates-test.sh
step "verification orchestration contracts" bash scripts/tests/verify-test.sh
step "test bodies" bash scripts/check-test-bodies.sh

# Formatting and lint policy apply to first-party workspace members. The
# vendored tower-lsp fork is intentionally excluded so it keeps upstream style
# and attribution; its behavior is covered by the focused transport tests.
# Read the authoritative workspace members rather than maintaining a crate list.
# Python 3.11+ supplies tomllib.
packages_file=$(mktemp)
trap 'rm -f "$packages_file"' EXIT
python3 - <<'PACKAGES' > "$packages_file" || exit 1
import tomllib
from pathlib import Path
workspace = tomllib.loads(Path("Cargo.toml").read_text())["workspace"]
for member in workspace["members"]:
    print(tomllib.loads((Path(member) / "Cargo.toml").read_text())["package"]["name"])
PACKAGES
packages=()
while IFS= read -r package; do
    packages+=("$package")
done < "$packages_file"
for package in "${packages[@]}"; do
    step "rustfmt ($package)" cargo fmt -p "$package" -- --check
    step "clippy ($package, all targets)" nice -n 10 cargo clippy -p "$package" --all-targets -j 4 --no-deps --quiet -- -D warnings
done

step "tower-lsp transport contracts" \
    nice -n 10 cargo test -p tower-lsp --manifest-path vendor/tower-lsp/Cargo.toml --lib transport::tests -j 4

step "LSP contracts" bash scripts/lsp-contracts.sh
step "editor and operational contracts" bash scripts/ops-contracts.sh

if [[ "$FAST" -eq 0 ]]; then
    for package in "${packages[@]}"; do
        step "tests ($package)" nice -n 10 cargo test -p "$package" -j 4 --no-fail-fast -- --test-threads=4
    done
else
    printf '\n%s==> full test suite skipped (--fast)%s\n' "$YELLOW" "$NC"
fi

printf '\n'
if [[ ${#failures[@]} -eq 0 ]]; then
    printf '%s✓ all checks passed%s\n' "$GREEN" "$NC"
    exit 0
fi

printf '%s✗ %d check(s) failed:%s\n' "$RED" "${#failures[@]}" "$NC"
for f in "${failures[@]}"; do printf '  - %s\n' "$f"; done
exit 1
