# Modifications to tower-lsp

This directory contains a fork of [tower-lsp](https://github.com/ebkalderon/tower-lsp)
by Eyal Kalderon and the tower-lsp contributors, vendored at upstream version
**0.20.0** (upstream commit as published on crates.io; upstream's last commit was
2024-01-05).

The upstream work is licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option, and remains available to you under those terms.

Modifications and files added for the Hyperion LSP project are licensed under
the same MIT-or-Apache-2.0 terms as the upstream work. The repository-level
PolyForm Shield 1.0.0 license does not apply to this vendored directory.

This file exists to satisfy the notice requirements of Apache License 2.0 §4(b)
("You must cause any modified files to carry prominent notices stating that You
changed the files") and to make the boundary between upstream code and Hyperion's
changes explicit.

## Files added by Hyperion (not present upstream)

| File | Added in | Purpose |
| --- | --- | --- |
| `src/cancellation.rs` | 2026-07-31 (`371520a8`) | Task-local `CancellationToken` used to implement cooperative `$/cancelRequest` handling. |
| `MODIFICATIONS.md` | this file | Attribution and change log. |
| `FORK_RATIONALE.md` | 2025-12-03 (`cedfdbc2`) | Why the crate is vendored. |

## Files modified by Hyperion

| File | Changed in | Change |
| --- | --- | --- |
| `Cargo.toml` | 2026-08-03 (`492d991c`, `b0700350`) | Declare the `tokio` `rt` feature needed by the changes below; retain the upstream dual-license metadata for the entire fork. |
| `src/lib.rs` | 2026-07-31 (`371520a8`), 2026-08-03 (`b0700350`) | Export the new `cancellation` module and the shutdown-signal types. |
| `src/jsonrpc.rs` | 2025-11-23 (`ba76fd24`), 2025-12-17 (`f72f1ff8`) | Clippy (pedantic/nursery) fixes and rustfmt. |
| `src/jsonrpc/request.rs` | 2025-11-23 (`ba76fd24`) | Clippy fix. |
| `src/jsonrpc/router.rs` | 2026-08-09 | Accept explicit JSON `null` for parameterless requests, as used by standard LSP clients for `shutdown`, while continuing to reject non-null values. |
| `src/service.rs` | 2026-07-31 (`371520a8`) | Request panic containment in `LspService`: synchronous dispatch panics are caught with `catch_unwind`, handler futures run in Tokio tasks and panics are mapped to JSON-RPC `InternalError` responses; `$/cancelRequest` is recognised before the transport concurrency queue. |
| `src/service/pending.rs` | 2026-07-31 (`371520a8`) | Pending-request tracking extended to carry cancellation tokens. |
| `src/service/layers.rs` | 2026-08-03 (`b0700350`) | `ExitService` fires the explicit shutdown trigger in addition to closing the client socket. |
| `src/service/client.rs` | 2025-11-15 (`61ecd445`), 2025-11-16 (`d9b9ca64`), 2026-08-03 (`b0700350`) | Test-compilation fixes after a telemetry-event change; documentation; shutdown-signal plumbing through `ClientSocket`. |
| `src/service/client/socket.rs` | 2026-08-03 (`b0700350`) | `ClientSocket` carries a `ShutdownSignal` and hands it to the transport via the `Loopback` trait. |
| `src/service/client/pending.rs` | 2025-12-22 (`ad37dfde`), 2026-11-06 | `#[allow(dead_code)]` to silence a warning under Hyperion's lint configuration; remove cancelled request waiters and discard late responses without panicking. |
| `src/transport.rs` | 2026-07-31 (`371520a8`), 2026-08-03 (`b0700350`), 2026-08-09 | Bounded, signal-driven shutdown: `shutdown_channel()` (`ShutdownTrigger`/`ShutdownSignal`, built on `futures::channel::oneshot` so the crate stays executor-agnostic), a supervisor that stops ingress, drains in-flight handlers under a deadline, then flushes and closes stdout; `Server::with_drain_deadline`. The drain bound requires a timer and therefore applies under `runtime-tokio` only. Document open/change/close notifications are state barriers so following semantic requests observe the accepted document version. |
| `tower-lsp-macros/src/lib.rs` | 2025-12-20 (`2e946e3a`), 2026-07-31 (`371520a8`) | Clippy auto-fix; generated dispatch updated for panic containment. |

## Files vendored unmodified

`examples/custom_notification.rs`, `examples/stdio.rs`, `examples/tcp.rs`,
`examples/websocket.rs`, `src/codec.rs`, `src/jsonrpc/error.rs`,
`src/jsonrpc/response.rs`,
`src/service/client/progress.rs`, `src/service/state.rs`, and all vendored
documentation files (`README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`,
`CODE_OF_CONDUCT.md`, `FEATURES.md`) are upstream's, unchanged.

Note that `README.md` and `CHANGELOG.md` are upstream's and describe the upstream
project, including its CI badges and crates.io links. They are not descriptions of
this fork.

## History

Between 2025-12-19 (`99bb90a4`) and this correction, every `.rs` file in this
directory carried a repository-level license header applied by an automated
script without carving out vendored code. Those generated license headers have
been removed. Modified and added files retain only a concise provenance notice.
License and attribution are carried by `LICENSE-MIT`, `LICENSE-APACHE`, the crate
metadata, and this modification notice; the entire fork remains dual-licensed
under MIT or Apache-2.0.
