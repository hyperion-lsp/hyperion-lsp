// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end tests that drive the real `hyperion-lsp` executable over stdio.
//!
//! Deliberately kept as its own test binary. Unlike every other
//! group, these tests spawn the shipped server as a child process and wait on
//! its replies against wall-clock deadlines (`wait_for_response`). Co-scheduling
//! them in a shared binary with CPU-heavy in-process tests — the javac/java
//! spawning run-executor tests in particular — makes the spawned server miss
//! those deadlines and answer before it has finished indexing. Its own binary
//! preserves the scheduling isolation it had before the consolidation.

#[macro_use]
#[path = "../support/test_macros.rs"]
mod test_macros;

#[macro_use]
#[path = "../support/lsp_test_framework.rs"]
mod lsp_test_framework;

mod integration_test;
