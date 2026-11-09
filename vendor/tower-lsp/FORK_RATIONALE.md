# Why tower-lsp is vendored

## Licensing and attribution

This is a fork of [ebkalderon/tower-lsp](https://github.com/ebkalderon/tower-lsp),
Copyright (c) 2023 Eyal Kalderon and the tower-lsp contributors, dual-licensed
under MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0 ([LICENSE-APACHE](LICENSE-APACHE)).
The entire vendored fork, including Hyperion's modifications and added files,
is available under those same terms. See [MODIFICATIONS.md](MODIFICATIONS.md)
for the complete list of what changed and when.

## Rationale

The upstream `tower-lsp` (ebkalderon/tower-lsp) is effectively abandoned:

- Last commit: January 5, 2024
- No releases since 0.20.0
- Minimal activity in 2023-2024, none in 2025

## Why this matters

- LSP is core infrastructure for Hyperion - we need control over bug fixes
- The LSP protocol evolves; upstream may not add features we need
- Waiting on unresponsive maintainers blocks development

## Alternatives considered

- `async-lsp` - Less mature, smaller ecosystem
- `lspower` - Fork of tower-lsp, also stale
- Raw `lsp-types` - Too low-level, reinventing the wheel

## Maintenance approach

- Track upstream changes if activity resumes
- Apply security patches as needed
- Document any local modifications below

## Local modifications

Tracking upstream 0.20.0 with local changes. See [MODIFICATIONS.md](MODIFICATIONS.md)
for the per-file list.
