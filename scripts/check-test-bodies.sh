#!/usr/bin/env bash
# Reject Rust tests whose bodies contain only whitespace or comments.
#
# A comment-only `#[test]` is reported as passing by Cargo without executing a
# contract. Strip comments before matching so placeholders cannot inflate the
# suite's apparent coverage. This is intentionally narrow: semantic review is
# still required for tests that contain executable but low-value assertions.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

offenders=()

list_files() {
    if [[ $# -gt 0 ]]; then
        printf '%s\0' "$@"
    else
        git ls-files --cached --others --exclude-standard -z -- '*.rs' ':(exclude)vendor/**'
    fi
}

# Materialize the file list so Git failures reach set -e instead of being lost
# in a process substitution. Keep NUL delimiters for paths containing whitespace.
file_list=$(mktemp)
trap 'rm -f "$file_list"' EXIT
list_files "$@" > "$file_list"

while IFS= read -r -d '' file; do
    if [[ ! -f "$file" ]]; then
        # A tracked file deleted in the working tree is no longer a test input.
        [[ $# -eq 0 ]] && continue
        printf 'Cannot inspect Rust test file: %s\n' "$file" >&2
        exit 2
    fi
    # Use the portable C locale: inherited locale names may not exist on macOS.
    # Each analysis stage must succeed; rg exit 1 alone means "no matches".
    if ! stripped=$(LC_ALL=C perl -0pe 's{/\*.*?\*/}{}gs; s{//[^\n]*}{}g' "$file"); then
        printf 'Failed to strip comments from %s\n' "$file" >&2
        exit 2
    fi
    if matches=$(printf '%s' "$stripped" |
        rg --pcre2 --multiline --only-matching --replace '$1' \
            '#\[(?:tokio::)?test\]\s*(?:#\[[^\]]+\]\s*)*(?:async\s+)?fn\s+([A-Za-z0-9_]+)[^{]*\{\s*\}'); then
        while IFS= read -r test_name; do
            [[ -n "$test_name" ]] && offenders+=("$file:$test_name")
        done <<< "$matches"
    else
        status=$?
        if [[ "$status" -ne 1 ]]; then
            printf 'Failed to inspect test bodies in %s (status %s)\n' "$file" "$status" >&2
            exit 2
        fi
    fi
done < "$file_list"

if [[ ${#offenders[@]} -eq 0 ]]; then
    printf '✓ No empty or comment-only Rust tests\n'
    exit 0
fi

printf '✗ Empty or comment-only Rust tests:\n' >&2
for offender in "${offenders[@]}"; do
    printf '  - %s\n' "$offender" >&2
done
printf 'Replace each placeholder with an executable contract or remove it.\n' >&2
exit 1
