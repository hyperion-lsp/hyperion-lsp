# Semantics architecture

This document describes the current `hyperion-semantics` data flow. The source
of truth for the exact query signatures is `src/db.rs`.

## Query dependencies

- `parse_file`, `file_symbols`, `file_symbol_table`, and `scope_graph` depend on
  file text and language, directly or through `parse_file`.
- Java and Kotlin `file_semantics` depend on the parse tree, language, workspace
  index, and `workspace_version`. Groovy and Scala return an empty result based
  only on language.
- Java `file_nullability` depends on the parse tree, language, and immutable
  `method_signatures` snapshot. Other languages return an empty result based
  only on language.

The database computes at file granularity. Expression inference is performed by
stateless engines over cached parse and symbol data rather than being a Salsa
query for every expression offset. This avoids caches keyed by offsets that
become unstable after edits.

## Query groups

### SourceDatabase

`file_text` and `file_language` are caller-owned inputs. Changing either
invalidates dependent parse and semantic queries for that file.

### ParseDatabase

`parse_file` dispatches by `Language`:

- Kotlin uses `hyperion-kotlin`.
- Java and the current fallback path use `hyperion-parser`.

The result preserves syntax errors alongside the tree so editor features can
continue operating on incomplete source.

### SemanticDatabase

- `file_semantics` caches resolved names and semantic diagnostics.
- `file_symbols` extracts declarations used by outlines and indexing.
- `file_symbol_table` builds the scope-aware table used by resolution and
  inference.
- `scope_graph` is currently Kotlin-specific; Java returns an empty graph.
- `file_nullability` runs the Java CFG/dataflow analysis. Kotlin is not routed
  through this analysis.

`workspace_index_handle` points at the cross-file index. Because the handle can
remain pointer-stable while its contents change, `workspace_version` is the
explicit Salsa invalidation input for Java and Kotlin `file_semantics` queries.
Java nullability instead reads an immutable `method_signatures` snapshot;
non-Java nullability queries return before reading that snapshot. The signature
snapshot is synchronized with `method_signature_version`.

## Main modules

| Area | Modules |
|---|---|
| Database and cached file results | `db`, `file_semantics` |
| Symbols and scopes | `symbol`, `semantic_walker`, `kotlin_semantic_walker` |
| Resolution | `resolver`, `kotlin_resolver`, `unified_resolver`, `resolved_name` |
| Inference and substitution | `inference`, `kotlin_inference`, `type_substitution`, `type_parser`, `types` |
| Checking and diagnostics | `type_checker`, `kotlin_type_checker`, `semantic_error` |
| Java nullability | `nullability` |

## Boundaries

- `hyperion-parser` and `hyperion-kotlin` own syntax construction.
- `hyperion-workspace` owns files, JDK/JAR symbols and cross-file indexes.
- `hyperion-semantics` owns meaning within and across the cached symbol data.
- `hyperion-lsp` translates protocol requests into these operations and maps
  byte offsets to LSP positions.

Keeping those boundaries matters: protocol types should not drive inference.
External index publications use the workspace epoch because additions can
resolve previously missing names and removals can change shadowing or
ambiguity. Ordinary source edits use the transitive dependent-file queue, which
re-publishes only affected Salsa text/language inputs instead of bumping the
global epoch. The language guards above keep empty Groovy/Scala semantic and
non-Java nullability results independent of unrelated workspace/signature
changes.

## Concurrency

`Database::snapshot()` creates the Salsa snapshot used by concurrent readers.
Mutable workspace state is accessed through the shared workspace-index handle.
Callers must not assume that cloning the handle creates an independent index.

## Known constraints

- Java and Kotlin semantic queries still depend on one broad workspace epoch.
  Narrower invalidation needs tracked lookup dependencies that include missing
  names, wildcard-package additions, ambiguity changes, and inherited members.
- Query invalidation is file-grained, not expression-grained.
- Java and Kotlin do not share every analysis implementation.
- Complex inferred types may require a resolver constructed with workspace
  context rather than only `file_symbol_table`.
- Unsupported or incomplete source should normally produce unknown/error
  results, not panic.
