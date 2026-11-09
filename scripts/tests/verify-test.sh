#!/usr/bin/env bash
# Check the gate's package selection and failure aggregation without a build.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/scripts/tests" "$TEST_ROOT/bin" "$TEST_ROOT/first" "$TEST_ROOT/second"
cp "$REPO_ROOT/scripts/verify.sh" "$TEST_ROOT/scripts/"
cat > "$TEST_ROOT/Cargo.toml" <<'TOML'
[workspace]
members = ["first", "second"]
TOML
printf '[package]\nname = "first-package"\n' > "$TEST_ROOT/first/Cargo.toml"
printf '[package]\nname = "second-package"\n' > "$TEST_ROOT/second/Cargo.toml"
for name in check-headers.sh check-build-test-relevance.sh check-test-bodies.sh lsp-contracts.sh ops-contracts.sh tests/check-test-bodies-test.sh tests/sync-headers-test.sh tests/check-repository-gates-test.sh tests/verify-test.sh; do
    printf '#!/bin/sh\nexit 0\n' > "$TEST_ROOT/scripts/$name"
done
cat > "$TEST_ROOT/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$VERIFY_LOG"
if [[ "${FAIL_CLIPPY:-0}" == 1 && "$1" == clippy && "$3" == first-package ]]; then exit 42; fi
CARGO
chmod +x "$TEST_ROOT/bin/cargo"
export VERIFY_LOG="$TEST_ROOT/commands"
export PATH="$TEST_ROOT/bin:$PATH"
bash "$TEST_ROOT/scripts/verify.sh" --fast > "$TEST_ROOT/output"
grep -Fx 'clippy -p first-package --all-targets -j 4 --no-deps --quiet -- -D warnings' "$VERIFY_LOG"
grep -Fx 'clippy -p second-package --all-targets -j 4 --no-deps --quiet -- -D warnings' "$VERIFY_LOG"
if grep -E -- 'test -p (first|second)-package|--workspace' "$VERIFY_LOG"; then exit 1; fi
: > "$VERIFY_LOG"
bash "$TEST_ROOT/scripts/verify.sh" > "$TEST_ROOT/output"
grep -Fx 'test -p first-package -j 4 --no-fail-fast -- --test-threads=4' "$VERIFY_LOG"
grep -Fx 'test -p second-package -j 4 --no-fail-fast -- --test-threads=4' "$VERIFY_LOG"
: > "$VERIFY_LOG"
if FAIL_CLIPPY=1 bash "$TEST_ROOT/scripts/verify.sh" --fast > "$TEST_ROOT/output"; then exit 1; fi
grep -F '1 check(s) failed' "$TEST_ROOT/output"
# Later checks still run, but cannot erase the earlier failure.
grep -Fx 'clippy -p second-package --all-targets -j 4 --no-deps --quiet -- -D warnings' "$VERIFY_LOG"
if bash "$TEST_ROOT/scripts/verify.sh" --unknown > "$TEST_ROOT/output" 2>&1; then exit 1; fi
printf '✓ Verification selects authoritative packages, bounds builds, and retains failures\n'
