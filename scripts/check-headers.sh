#!/usr/bin/env bash
# Check that changed Rust files have copyright headers.
#
# vendor/ is excluded deliberately. Those files are third-party code (tower-lsp,
# MIT/Apache-2.0) carrying upstream attribution; stamping Hyperion's copyright on
# them is a licensing defect, not a fix. An earlier version of this script had no
# such carve-out, which is how all 22 vendored files came to assert sole Hyperion
# copyright over someone else's work. Do not remove this exclusion.

set -euo pipefail

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

# Get the repository root (parent of scripts/)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$REPO_ROOT"

CHECK_ALL=0
if [[ "${1:-}" == "--all" ]]; then
    CHECK_ALL=1
elif [[ $# -ne 0 ]]; then
    printf 'Usage: %s [--all]\n' "$0" >&2
    exit 2
fi

echo "Checking copyright headers in: $REPO_ROOT"
echo ""

# Array to collect files missing headers
missing_headers=()
files=()

file_list=$(mktemp)
trap 'rm -f "$file_list"' EXIT
if [[ "$CHECK_ALL" -eq 1 ]]; then
    git ls-files --cached --others --exclude-standard -z -- '*.rs' ':(exclude)vendor/**' > "$file_list"
else
    # Include the full branch before its first push, rather than only HEAD^.
    if upstream=$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}' 2>/dev/null); then
        base=$(git merge-base HEAD "$upstream")
    elif git rev-parse --verify origin/main >/dev/null 2>&1; then
        base=$(git merge-base HEAD origin/main)
    elif git rev-parse HEAD^ >/dev/null 2>&1; then
        base=HEAD^
    else
        base=
    fi
    if [[ -n "$base" ]]; then
        git diff --name-only --diff-filter=ACMR -z "$base" -- '*.rs' ':(exclude)vendor/**' > "$file_list"
    else
        # A root commit has no parent: inspect the complete tracked snapshot.
        git ls-files --cached -z -- '*.rs' ':(exclude)vendor/**' > "$file_list"
    fi
    git ls-files --others --exclude-standard -z -- '*.rs' ':(exclude)vendor/**' >> "$file_list"
fi

while IFS= read -r -d '' file; do
    [[ -f "$file" ]] || continue
    files+=("$file")
done < "$file_list"

for file in "${files[@]}"; do
    # Check if file has the header
    if ! head -n 40 "$file" | grep -q "SPDX-License-Identifier: .*PolyForm-Shield-1.0.0" \
        || ! head -n 1 "$file" | grep -Eq "Copyright [0-9]{4} Adrian Helvik"; then
        missing_headers+=("$file")
    fi
done

# Report results
if [ ${#missing_headers[@]} -eq 0 ]; then
    if [[ "$CHECK_ALL" -eq 1 ]]; then
        echo -e "${GREEN}✓ All Rust files have copyright headers${NC}"
    else
        echo -e "${GREEN}✓ All changed Rust files have copyright headers${NC}"
    fi
    exit 0
else
    echo -e "${RED}✗ Found ${#missing_headers[@]} file(s) missing copyright headers:${NC}"
    echo ""
    for file in "${missing_headers[@]}"; do
        echo "  - $file"
    done
    echo ""
    echo "Add the project header to the files listed above."
    exit 1
fi
