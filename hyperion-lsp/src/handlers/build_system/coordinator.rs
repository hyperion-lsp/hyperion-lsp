// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Single-flight ordering for startup and watcher-driven build refreshes.

use std::num::NonZeroU64;
use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Clone)]
pub struct BuildRefreshCoordinator {
    inner: Arc<BuildRefreshCoordinatorInner>,
}

struct BuildRefreshCoordinatorInner {
    state: Mutex<BuildRefreshCoordinatorState>,
    changed: tokio::sync::watch::Sender<Option<NonZeroU64>>,
}

#[derive(Default)]
struct BuildRefreshCoordinatorState {
    next_generation: u64,
    active: Option<NonZeroU64>,
}

#[derive(Clone)]
#[must_use = "a build-refresh ticket must guard work and finalization"]
pub(crate) struct BuildRefreshTicket {
    coordinator: BuildRefreshCoordinator,
    generation: NonZeroU64,
}

struct BuildRefreshTaskGuard(BuildRefreshTicket);

struct BuildRefreshCancellation {
    generation: NonZeroU64,
    changed: tokio::sync::watch::Receiver<Option<NonZeroU64>>,
}

pub(crate) struct BuildReadinessCompletion {
    readiness: Arc<crate::server::DestructiveRefactoringReadiness>,
    gradle: Option<crate::server::RefactoringReadinessTicket>,
    maven: Option<crate::server::RefactoringReadinessTicket>,
}

impl Default for BuildRefreshCoordinator {
    fn default() -> Self {
        let (changed, _) = tokio::sync::watch::channel(None);
        Self {
            inner: Arc::new(BuildRefreshCoordinatorInner {
                state: Mutex::new(BuildRefreshCoordinatorState::default()),
                changed,
            }),
        }
    }
}

impl Drop for BuildRefreshTaskGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl BuildReadinessCompletion {
    pub(crate) fn reserve(readiness: Arc<crate::server::DestructiveRefactoringReadiness>) -> Self {
        let gradle = readiness.begin(crate::server::RefactoringIndexComponent::Gradle);
        let maven = readiness.begin(crate::server::RefactoringIndexComponent::Maven);
        Self {
            readiness,
            gradle: Some(gradle),
            maven: Some(maven),
        }
    }

    pub(crate) fn finish(&mut self, gradle_complete: bool, maven_complete: bool) {
        if let Some(ticket) = self.gradle.take() {
            let _ = self.readiness.finish(ticket, gradle_complete);
        }
        if let Some(ticket) = self.maven.take() {
            let _ = self.readiness.finish(ticket, maven_complete);
        }
    }
}

impl Drop for BuildReadinessCompletion {
    fn drop(&mut self) {
        self.finish(false, false);
    }
}

impl BuildRefreshCoordinator {
    fn begin_with<R, P>(&self, reserve: R) -> (BuildRefreshTicket, P)
    where
        R: FnOnce() -> P,
    {
        let mut state = self.inner.state.lock();
        let next_generation = state
            .next_generation
            .checked_add(1)
            .expect("build-refresh generation overflowed");
        let generation = NonZeroU64::new(next_generation)
            .expect("incrementing a build-refresh generation from zero cannot produce zero");

        // Reserve every side authority while this ordering lock is held. Two
        // concurrent watcher callbacks therefore cannot crown opposite
        // coordinator/readiness generations under opposite arrival orders. Do
        // this before mutating coordinator state so a panic in reservation
        // leaves the prior refresh current and observable.
        let payload = reserve();
        state.next_generation = next_generation;
        state.active = Some(generation);
        self.inner.changed.send_replace(Some(generation));
        (
            BuildRefreshTicket {
                coordinator: self.clone(),
                generation,
            },
            payload,
        )
    }

    /// Start one refresh under the session's single-flight authority.
    ///
    /// A newer startup/watcher request changes the shared generation. Both
    /// entry points run their work through [`run_build_refresh`], which drops
    /// the older future and its exact registry guards before it can finalize.
    pub(crate) fn spawn_with<R, P, F, Fut>(&self, reserve: R, task: F)
    where
        R: FnOnce() -> P,
        P: Send + 'static,
        F: FnOnce(BuildRefreshTicket, P) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (ticket, payload) = self.begin_with(reserve);
        tokio::spawn(async move {
            let _guard = BuildRefreshTaskGuard(ticket.clone());
            task(ticket, payload).await;
        });
    }

    #[cfg(test)]
    fn begin(&self) -> BuildRefreshTicket {
        self.begin_with(|| ()).0
    }

    #[cfg(test)]
    fn has_active_refresh(&self) -> bool {
        self.inner.state.lock().active.is_some()
    }
}

impl BuildRefreshTicket {
    fn cancellation(&self) -> BuildRefreshCancellation {
        BuildRefreshCancellation {
            generation: self.generation,
            changed: self.coordinator.inner.changed.subscribe(),
        }
    }

    fn finish(&self) -> bool {
        let mut state = self.coordinator.inner.state.lock();
        if state.active == Some(self.generation) {
            state.active = None;
            self.coordinator.inner.changed.send_replace(None);
            true
        } else {
            false
        }
    }
}

impl BuildRefreshCancellation {
    async fn cancelled(&mut self) {
        loop {
            if *self.changed.borrow() != Some(self.generation) {
                return;
            }
            if self.changed.changed().await.is_err() {
                return;
            }
        }
    }
}

pub(crate) async fn run_build_refresh<T, W, F, FF>(
    ticket: BuildRefreshTicket,
    work: W,
    finalize: F,
) -> bool
where
    W: std::future::Future<Output = T>,
    F: FnOnce(T) -> FF,
    FF: std::future::Future<Output = ()>,
{
    let mut cancellation = ticket.cancellation();
    let outcome = tokio::select! {
        biased;
        () = cancellation.cancelled() => return false,
        outcome = work => outcome,
    };

    let mut cancellation = ticket.cancellation();
    tokio::select! {
        biased;
        () = cancellation.cancelled() => false,
        () = finalize(outcome) => finish_completed_finalization(&ticket),
    }
}

fn finish_completed_finalization(ticket: &BuildRefreshTicket) -> bool {
    // Finalization effects are already complete. A newer generation may have
    // started between the final effect and this exact cleanup; that makes the
    // cleanup a no-op, not retroactive cancellation of completed work.
    let _ = ticket.finish();
    true
}

#[cfg(test)]
mod tests {
    use super::{
        finish_completed_finalization, run_build_refresh, BuildReadinessCompletion,
        BuildRefreshCoordinator,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(signal) = self.0.take() {
                let _ = signal.send(());
            }
        }
    }

    #[test]
    fn stale_completion_cannot_clear_newer_readiness_reservation() {
        let coordinator = BuildRefreshCoordinator::default();
        let readiness = Arc::new(crate::server::DestructiveRefactoringReadiness::new());
        for component in [
            crate::server::RefactoringIndexComponent::Sources,
            crate::server::RefactoringIndexComponent::Jdk,
        ] {
            let ticket = readiness.begin(component);
            assert!(readiness.finish(ticket, true));
        }
        let old_readiness = Arc::clone(&readiness);
        let (_old_ticket, old_completion) =
            coordinator.begin_with(move || BuildReadinessCompletion::reserve(old_readiness));
        let new_readiness = Arc::clone(&readiness);
        let (new_ticket, mut new_completion) =
            coordinator.begin_with(move || BuildReadinessCompletion::reserve(new_readiness));

        drop(old_completion);
        assert!(!readiness.is_ready());
        new_completion.finish(true, true);
        assert!(readiness.is_ready());
        assert!(new_ticket.finish());
    }

    #[test]
    fn completed_finalization_is_not_relabelled_when_cleanup_is_superseded() {
        let coordinator = BuildRefreshCoordinator::default();
        let old_ticket = coordinator.begin();
        let new_ticket = coordinator.begin();

        assert!(finish_completed_finalization(&old_ticket));
        assert!(new_ticket.finish());
        assert!(!coordinator.has_active_refresh());
    }

    #[test]
    fn panicking_side_reservation_leaves_prior_generation_current() {
        let coordinator = BuildRefreshCoordinator::default();
        let old_ticket = coordinator.begin();

        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = coordinator.begin_with(|| panic!("injected reservation panic"));
        }));

        assert!(panic.is_err());
        assert!(old_ticket.finish());
        assert!(!coordinator.has_active_refresh());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn watcher_generation_cancels_blocked_startup_before_finalization() {
        let coordinator = BuildRefreshCoordinator::default();
        let finalizations = Arc::new(AtomicUsize::new(0));
        let (old_entered_tx, old_entered_rx) = tokio::sync::oneshot::channel();
        let (old_dropped_tx, old_dropped_rx) = tokio::sync::oneshot::channel();

        let old_ticket = coordinator.begin();
        let old_finalizations = Arc::clone(&finalizations);
        let old = tokio::spawn(async move {
            let work = async move {
                let _drop_signal = DropSignal(Some(old_dropped_tx));
                let _ = old_entered_tx.send(());
                std::future::pending::<()>().await;
            };
            run_build_refresh(old_ticket, work, move |()| async move {
                old_finalizations.fetch_add(1, Ordering::SeqCst);
            })
            .await
        });
        old_entered_rx.await.unwrap();

        let watcher_ticket = coordinator.begin();
        let watcher_finalizations = Arc::clone(&finalizations);
        assert!(
            run_build_refresh(
                watcher_ticket,
                std::future::ready(()),
                move |()| async move {
                    watcher_finalizations.fetch_add(1, Ordering::SeqCst);
                }
            )
            .await
        );

        tokio::time::timeout(std::time::Duration::from_secs(1), old_dropped_rx)
            .await
            .expect("cancelled startup work must be dropped")
            .unwrap();
        assert!(!old.await.unwrap());
        assert_eq!(finalizations.load(Ordering::SeqCst), 1);
        assert!(!coordinator.has_active_refresh());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn newer_generation_cancels_blocked_outer_finalization() {
        let coordinator = BuildRefreshCoordinator::default();
        let finalizations = Arc::new(AtomicUsize::new(0));
        let (finalizer_entered_tx, finalizer_entered_rx) = tokio::sync::oneshot::channel();
        let (finalizer_dropped_tx, finalizer_dropped_rx) = tokio::sync::oneshot::channel();

        let old_ticket = coordinator.begin();
        let old = tokio::spawn(async move {
            run_build_refresh(old_ticket, std::future::ready(()), move |()| async move {
                let _drop_signal = DropSignal(Some(finalizer_dropped_tx));
                let _ = finalizer_entered_tx.send(());
                // Never resolves: the superseded finalization is dropped instead of
                // reaching the increment below, which is why `finalizations` stays 0.
                std::future::pending::<()>().await;
            })
            .await
        });
        finalizer_entered_rx.await.unwrap();

        let new_ticket = coordinator.begin();
        tokio::time::timeout(std::time::Duration::from_secs(1), finalizer_dropped_rx)
            .await
            .expect("superseded outer finalization must be dropped")
            .unwrap();
        assert!(!old.await.unwrap());
        assert_eq!(finalizations.load(Ordering::SeqCst), 0);

        let new_finalizations = Arc::clone(&finalizations);
        assert!(
            run_build_refresh(new_ticket, std::future::ready(()), move |()| async move {
                new_finalizations.fetch_add(1, Ordering::SeqCst);
            })
            .await
        );
        assert_eq!(finalizations.load(Ordering::SeqCst), 1);
        assert!(!coordinator.has_active_refresh());
    }

    #[tokio::test]
    async fn cancellation_wins_when_old_work_becomes_ready_simultaneously() {
        let coordinator = BuildRefreshCoordinator::default();
        let finalizations = Arc::new(AtomicUsize::new(0));
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let old_ticket = coordinator.begin();
        let old_finalizations = Arc::clone(&finalizations);
        let old = tokio::spawn(async move {
            let work = async move {
                let _ = entered_tx.send(());
                let _ = release_rx.await;
            };
            run_build_refresh(old_ticket, work, move |()| async move {
                old_finalizations.fetch_add(1, Ordering::SeqCst);
            })
            .await
        });
        entered_rx.await.unwrap();

        let new_ticket = coordinator.begin();
        let _ = release_tx.send(());
        assert!(!old.await.unwrap());
        assert_eq!(finalizations.load(Ordering::SeqCst), 0);
        assert!(new_ticket.finish());
    }
}
