# Hyperion Semantics

`hyperion-semantics` turns parsed Java and Kotlin files into the scope, symbol,
type, name-resolution and nullability data consumed by the language server.
It uses Salsa to memoize file-level queries and invalidate them when source or
workspace inputs change.

Start with the repository [README](../README.md) for product capabilities and
limitations. This document describes the crate boundary.

## Responsibilities

- Parse dispatch for Java and Kotlin through `parse_file`.
- Per-file symbol extraction and scope-aware symbol tables.
- Java and Kotlin name resolution, type inference and type checking.
- Cached `FileSemantics` results for resolved names and diagnostics.
- Java nullability analysis.
- Coordination with `hyperion-workspace` for cross-file symbols.

Workspace scanning, JAR indexing and persistence belong to
`hyperion-workspace`. LSP request routing and position conversion belong to
`hyperion-lsp`.

## Query surface

The database is split into three Salsa query groups:

| Group | Inputs and derived results |
|---|---|
| `SourceDatabase` | `file_text`, `file_language` |
| `ParseDatabase` | `parse_file` |
| `SemanticDatabase` | `file_semantics`, `file_symbols`, `file_symbol_table`, `scope_graph`, `file_nullability`, plus workspace/signature version inputs |

`Database::new()` initializes the non-file inputs. A caller registering a file
must provide both its text and language before asking for derived results.

```rust
use hyperion_semantics::{Database, FileId, SemanticDatabase, SourceDatabase};
use hyperion_workspace::Language;
use std::sync::Arc;

let mut db = Database::new();
let file = FileId::new(0);
db.set_file_text(file, Arc::new("class Example {}".to_owned()));
db.set_file_language(file, Language::Java);

let symbols = db.file_symbols(file);
assert!(symbols.symbols.iter().any(|symbol| symbol.name == "Example"));
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for invalidation and dependency
boundaries, and [TESTS.md](TESTS.md) for the testing strategy.

## Development

```bash
cargo test -p hyperion-semantics
cargo clippy -p hyperion-semantics --all-targets
cargo bench -p hyperion-semantics
```

The crate is licensed under the repository's PolyForm Shield License 1.0.0.
