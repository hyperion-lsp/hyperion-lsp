#!/usr/bin/env bash
# Add the project header to first-party Rust files that lack one.
#
# Header:
#   // Copyright <year> Adrian Helvik
#   // SPDX-License-Identifier: PolyForm-Shield-1.0.0
#
# vendor/ is excluded: it is third-party code carrying its own attribution.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Materialize the list so a Git failure cannot silently produce a successful empty run.
file_list=$(mktemp)
trap 'rm -f "$file_list"' EXIT
git ls-files --cached --others --exclude-standard -z -- '*.rs' ':(exclude)vendor/**' > "$file_list"

while IFS= read -r -d '' file; do
    [[ -f "$file" ]] || continue
    LC_ALL=C perl -i -0777 -pe '
        next if m{\A// Copyright [0-9]{4} Adrian Helvik\n(?:.*\n)*?// SPDX-License-Identifier: }m;
        if (!s{\A(// Copyright [0-9]{4} Adrian Helvik\n)}{$1// SPDX-License-Identifier: PolyForm-Shield-1.0.0\n}) {
            $_ = "// Copyright " . (1900 + (localtime)[5]) . " Adrian Helvik\n"
                . "// SPDX-License-Identifier: PolyForm-Shield-1.0.0\n\n" . $_;
        }
    ' "$file"
done < "$file_list"

printf 'Headers synchronized. Review git diff before committing.\n'
