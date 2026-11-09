# hyperion-lsp

The Language Server Protocol server for [Hyperion](../README.md), plus the
`hyperion-lsp` binary that ships it.

**Start with the [root README](../README.md)** for what Hyperion is, what
actually works per language, how to install it, and the license. This file
covers only what a contributor working inside this crate needs.

## The binary

```bash
hyperion-lsp                    # LSP server over stdio (no arguments)
hyperion-lsp format <files>     # format Java files; --check, --list
hyperion-lsp dap                # Debug Adapter Protocol server over stdio
```

Logs go to `$TMPDIR/hyperion-lsp.log`. Set `RUST_LOG=debug` for verbose output.

## Layout

| Path | Responsibility |
|---|---|
| `src/server/` | Protocol surface: lifecycle, capabilities, request routing, document sync |
| `src/language_handler.rs` | Per-language dispatch for shared providers. An optional handler returning `None` declines the request; the caller still owns the language gate and must not assume a Java fallback is safe |
| `src/navigation/`, `src/hover/`, `src/completion/` | Providers, with per-language modules alongside the Java implementation |
| `src/diagnostic_engine.rs` | Diagnostic orchestration and the per-language gates for type checking, nullability, inspections and security |
| `src/inspections/` | Java inspection registry, plus a smaller Kotlin-native one |
| `src/code_actions/`, `src/refactor*` | Java-tree-based code actions and refactorings; request routing gates these through `Backend::java_document` |
| `src/gradle_integration.rs`, `src/maven_integration.rs` | Build-system project models |
| `src/jdk_indexer.rs` | Background JDK `src.zip` and bytecode indexing |

## Things worth knowing before you change something here

- **Positions are UTF-16.** The server advertises `positionEncoding: utf-16`
  and converts to UTF-8 byte offsets internally. Get this wrong and non-ASCII
  files break in ways tests on ASCII fixtures will not catch.
- **Text sync is incremental.** `apply_incremental_change` applies ranges in
  place; it does not re-read the file.
- **Do not hold a `DashMap` guard across an `await`.** Diagnostic publishing
  clones the fields it needs and drops the guard before computing, so
  `didChange` is never blocked behind a slow analysis. Reintroducing the guard
  reintroduces editor lag.
- **`parse_tree.syntax()` yields a *Java* syntax tree.** Java-only protocol
  routes use `Backend::java_document` to reject other languages before calling
  those providers. New Java-tree features must use the same boundary.
- **Unsupported file extensions are ignored at `didOpen`.** Do not introduce a
  Java fallback for unknown paths; the client-provided language ID is not a
  trusted dispatch key.

## Tests and benchmarks

```bash
cargo test -p hyperion-lsp
cargo bench -p hyperion-lsp --bench lsp_operations
cargo bench -p hyperion-lsp --bench cold_start
```

End-to-end latency and memory against Eclipse jdtls are measured by the
separate harness in [`tools/bench`](../tools/bench/README.md), not by the
Criterion benches here.

## License

PolyForm Shield License 1.0.0. See the repository [`LICENSE`](../LICENSE).
