// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gate that holds back symbol-dependent diagnostics until the libraries
//! loaded at startup (the JDK and the build dependencies) have settled.
//!
//! The question answered here is "has every initial library load finished,
//! whatever its outcome", not "is the index complete". A failed, partial,
//! cancelled, superseded or panicking load settles exactly like a successful
//! one; the diagnostics published afterwards are then as good as the index
//! will get without user action. That is why this is not
//! [`DestructiveRefactoringReadiness`](super::DestructiveRefactoringReadiness),
//! which stays false after any partial result.
//!
//! Each startup task owns a [`StartupLibraryHold`]. The gate settles when the
//! last hold is dropped, so a task cannot forget to settle it: an early
//! return, a panic (Tokio drops the future while unwinding) and a dropped
//! future all release the hold. Settlement is latched; holds taken afterwards
//! are not handed out, so a later build refresh never gates diagnostics again.
//!
//! The gate starts closed with no holds. Startup takes a registration hold
//! before it spawns any task and drops it after every task has taken its own,
//! so the count cannot reach zero between two registrations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

/// Upper bound on how long startup library loading may hold diagnostics
/// back. A build tool that hangs (a Gradle daemon stuck on the network, say)
/// must not leave the editor without semantic diagnostics for the session.
/// Past this point the gate settles anyway and the late load republishes
/// diagnostics through its own revision publication when it finishes.
pub(crate) const STARTUP_LIBRARY_DEADLINE: Duration = Duration::from_secs(180);

#[derive(Debug)]
pub(crate) struct StartupLibraryGate {
    pending: Mutex<usize>,
    settled: AtomicBool,
    changed: tokio::sync::watch::Sender<bool>,
}

/// One unit of startup library work. Dropping it settles that unit.
#[derive(Debug)]
#[must_use = "dropping a startup library hold settles it immediately"]
pub(crate) struct StartupLibraryHold {
    gate: Arc<StartupLibraryGate>,
}

impl StartupLibraryGate {
    /// A closed gate, waiting for startup to register its library loads.
    pub(crate) fn new() -> Self {
        let (changed, _) = tokio::sync::watch::channel(false);
        Self {
            pending: Mutex::new(0),
            settled: AtomicBool::new(false),
            changed,
        }
    }

    /// An open gate, for hosts that run no startup library loading.
    pub(crate) fn settled() -> Self {
        let gate = Self::new();
        gate.settle();
        gate
    }

    pub(crate) fn is_settled(&self) -> bool {
        self.settled.load(Ordering::SeqCst)
    }

    /// Register one startup load. Returns `None` once the gate has settled:
    /// work started after that point does not hold diagnostics back.
    pub(crate) fn hold(self: &Arc<Self>) -> Option<StartupLibraryHold> {
        let mut pending = self.pending.lock();
        if self.is_settled() {
            return None;
        }
        *pending += 1;
        Some(StartupLibraryHold {
            gate: Arc::clone(self),
        })
    }

    /// Register a load that continues the initial one (a build refresh that
    /// supersedes the startup refresh). Only joins while some initial load is
    /// still held, so it can neither reopen a settled gate nor settle it
    /// before startup has registered its own loads.
    pub(crate) fn join_if_pending(self: &Arc<Self>) -> Option<StartupLibraryHold> {
        let mut pending = self.pending.lock();
        if self.is_settled() || *pending == 0 {
            return None;
        }
        *pending += 1;
        Some(StartupLibraryHold {
            gate: Arc::clone(self),
        })
    }

    /// Open the gate regardless of outstanding holds (deadline fallback).
    /// Returns whether this call was the one that settled it.
    pub(crate) fn settle(&self) -> bool {
        let _pending = self.pending.lock();
        let first = !self.settled.swap(true, Ordering::SeqCst);
        if first {
            self.changed.send_replace(true);
        }
        first
    }

    /// Resolve once the gate has settled.
    pub(crate) async fn wait_settled(&self) {
        let mut changed = self.changed.subscribe();
        // `wait_for` checks the current value first, so a settlement that
        // happened before the subscription is not missed.
        let _ = changed.wait_for(|settled| *settled).await;
    }

    fn release(&self) {
        let mut pending = self.pending.lock();
        debug_assert!(*pending > 0);
        *pending = pending.saturating_sub(1);
        if *pending == 0 && !self.settled.swap(true, Ordering::SeqCst) {
            self.changed.send_replace(true);
        }
    }
}

impl Default for StartupLibraryGate {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StartupLibraryHold {
    fn drop(&mut self) {
        self.gate.release();
    }
}

/// Wait for the gate to settle (or for [`STARTUP_LIBRARY_DEADLINE`]), then
/// re-analyse every open document so the full diagnostic set replaces the
/// parse-only set published while the gate was closed.
pub(crate) fn spawn_settlement_republish(
    publication: crate::server::diagnostics::InvalidationRuntime,
    deadline: Duration,
) {
    tokio::spawn(async move {
        let gate = Arc::clone(&publication.startup_libraries);
        if tokio::time::timeout(deadline, gate.wait_settled())
            .await
            .is_err()
            && gate.settle()
        {
            tracing::warn!(
                "Startup library loading still running after {}s; publishing semantic diagnostics without waiting",
                deadline.as_secs()
            );
        }
        tracing::debug!("Startup libraries settled; republishing open documents");
        publication
            .publish_workspace_diagnostics("startup libraries")
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::StartupLibraryGate;
    use std::sync::Arc;
    use std::time::Duration;

    async fn settles_within(gate: &StartupLibraryGate, within: Duration) -> bool {
        tokio::time::timeout(within, gate.wait_settled())
            .await
            .is_ok()
    }

    #[test]
    fn a_new_gate_is_closed_and_a_standalone_gate_is_open() {
        assert!(!StartupLibraryGate::new().is_settled());
        assert!(StartupLibraryGate::settled().is_settled());
    }

    #[test]
    fn settles_only_when_the_last_hold_drops() {
        let gate = Arc::new(StartupLibraryGate::new());
        let registration = gate.hold().unwrap();
        let jdk = gate.hold().unwrap();
        let build = gate.hold().unwrap();
        drop(registration);
        assert!(!gate.is_settled());
        drop(build);
        assert!(!gate.is_settled(), "JDK still loading");
        drop(jdk);
        assert!(gate.is_settled());
    }

    #[test]
    fn a_settled_gate_is_latched_and_hands_out_no_more_holds() {
        let gate = Arc::new(StartupLibraryGate::new());
        drop(gate.hold().unwrap());
        assert!(gate.is_settled());
        assert!(gate.hold().is_none());
        assert!(gate.join_if_pending().is_none());
        assert!(gate.is_settled());
    }

    #[test]
    fn join_does_not_arm_an_unregistered_gate() {
        let gate = Arc::new(StartupLibraryGate::new());
        assert!(gate.join_if_pending().is_none());
        assert!(!gate.is_settled());
    }

    /// No workspace root: only the JDK load is registered.
    #[test]
    fn single_file_session_settles_when_the_jdk_load_ends() {
        let gate = Arc::new(StartupLibraryGate::new());
        let registration = gate.hold().unwrap();
        let jdk = gate.hold().unwrap();
        drop(registration);
        assert!(!gate.is_settled());
        drop(jdk);
        assert!(gate.is_settled());
    }

    /// A refresh that supersedes the startup refresh joins before the old
    /// one is cancelled, so the gate stays closed until the newer one ends.
    #[test]
    fn superseding_refresh_keeps_the_gate_closed_until_it_ends() {
        let gate = Arc::new(StartupLibraryGate::new());
        let jdk = gate.hold().unwrap();
        let startup_refresh = gate.hold().unwrap();
        drop(jdk);
        let watcher_refresh = gate.join_if_pending().unwrap();
        drop(startup_refresh);
        assert!(!gate.is_settled());
        drop(watcher_refresh);
        assert!(gate.is_settled());
    }

    /// Failure, an early return and a panic all reduce to dropping the hold.
    #[tokio::test]
    async fn a_panicking_or_failing_task_settles_its_hold() {
        let gate = Arc::new(StartupLibraryGate::new());
        let registration = gate.hold().unwrap();
        let panicking = gate.hold().unwrap();
        let failing = gate.hold().unwrap();
        drop(registration);

        let panicked = tokio::spawn(async move {
            let _hold = panicking;
            panic!("injected startup task panic");
        })
        .await;
        assert!(panicked.unwrap_err().is_panic());
        assert!(!gate.is_settled());

        let failed: Result<(), &str> = tokio::spawn(async move {
            let _hold = failing;
            Err("JDK not found")
        })
        .await
        .unwrap();
        assert!(failed.is_err());
        assert!(settles_within(&gate, Duration::from_secs(1)).await);
    }

    /// A task aborted before it ever ran still drops the hold it owns.
    #[tokio::test]
    async fn an_aborted_task_settles_its_hold() {
        let gate = Arc::new(StartupLibraryGate::new());
        let hold = gate.hold().unwrap();
        let task = tokio::spawn(async move {
            let _hold = hold;
            std::future::pending::<()>().await;
        });
        task.abort();
        let _ = task.await;
        assert!(gate.is_settled());
    }

    #[tokio::test]
    async fn waiting_after_settlement_returns_at_once() {
        let gate = Arc::new(StartupLibraryGate::new());
        drop(gate.hold().unwrap());
        assert!(settles_within(&gate, Duration::from_millis(100)).await);
    }

    #[tokio::test]
    async fn forced_settlement_opens_the_gate_and_later_drops_are_harmless() {
        let gate = Arc::new(StartupLibraryGate::new());
        let hung = gate.hold().unwrap();
        assert!(!settles_within(&gate, Duration::from_millis(20)).await);
        assert!(gate.settle());
        assert!(!gate.settle(), "only the first settlement reports itself");
        assert!(gate.is_settled());
        drop(hung);
        assert!(gate.is_settled());
    }
}
