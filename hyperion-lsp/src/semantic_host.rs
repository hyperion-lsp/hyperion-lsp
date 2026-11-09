// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The one owner of the semantic database and the one read/write gate in front
//! of it.
//!
//! # Model
//!
//! Every reader of semantic state holds a [`ReadTxn`]; every writer holds the
//! single [`WriteTxn`]. The gate is a fair FIFO reader/writer lock:
//!
//! * a [`ReadTxn`] is the *only* way to obtain a Salsa `Snapshot`;
//! * a [`WriteTxn`] is the *only* way to reach `&mut Database` (every Salsa
//!   `set_*`) and the only context in which the workspace index may be mutated;
//! * the `Database` itself is private to this module, so no other code can
//!   create a snapshot or set an input without going through a transaction.
//!
//! Salsa 0.16 blocks `set_*` until every live snapshot is dropped. Because a
//! writer holds the gate exclusively, no snapshot exists while it writes, so the
//! Salsa write can never wait on a reader. Because a reader never acquires
//! anything while it holds its transaction, no reader can wait on a writer
//! either. The wait-for graph therefore has only the edge "queued writer waits
//! for the readers that entered before it" and is acyclic.
//!
//! # Lock order (the only order in the crate)
//!
//! 1. per-URI publication mutex (`tokio::sync::Mutex`, document handlers only);
//! 2. the gate: [`SemanticHost::read`] or [`SemanticHost::write`], **at most one
//!    transaction per task and never a second one while one is held**
//!    (upgrading a read to a write, or a write to a read, self-deadlocks);
//! 3. index path lock, then index file lock (only in code that already holds a
//!    [`WriteTxn`]: those functions take `&mut WriteTxn`);
//! 4. leaf locks (the database mutex below, `WorkspaceIndex` internals, `DashMap`
//!    shards, caches): held for a bounded critical section, never across an
//!    `.await`, another lock acquisition, or a Salsa write.
//!
//! What the types enforce: 2 (nothing outside this module can build a snapshot
//! or write an input) and the `&mut WriteTxn` requirement of 3. What debug
//! builds additionally assert: no transaction is acquired from a thread that is
//! running inside one ([`ReadTxn::run_blocking`] / [`WriteTxn::run_blocking`]),
//! and the workspace index is only mutated while a [`WriteTxn`] is live.
//!
//! # Timeouts
//!
//! Waiting for the gate is unbounded for background work, because progress is
//! guaranteed. The only bounded wait is [`SemanticHost::read_interactive`]: a
//! user-facing request budget after which the handler answers "retry" instead of
//! leaving the editor waiting behind a long writer queue. It is not a deadlock
//! breaker and nothing depends on it for liveness.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hyperion_semantics::db::{Database, FileId};
use hyperion_workspace::{ExclusiveIndexWrites, WorkspaceIndex};
use parking_lot::{Mutex, MutexGuard};
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio::task::JoinHandle;

/// How long an interactive request (completion, hover, ...) waits for the gate
/// before answering "retry". See the module docs: this is a latency budget for
/// the user, not a liveness mechanism.
pub const INTERACTIVE_READ_BUDGET: Duration = Duration::from_secs(2);

/// The interactive read budget elapsed while writers were ahead in the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("semantic state is being updated")
    }
}

impl std::error::Error for Busy {}

thread_local! {
    /// Depth of transaction scopes running on this thread (debug assertion only).
    static TXN_SCOPE_DEPTH: Cell<u32> = const { Cell::new(0) };
}

struct ThreadTxnScope;

impl ThreadTxnScope {
    fn enter() -> Self {
        TXN_SCOPE_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for ThreadTxnScope {
    fn drop(&mut self) {
        TXN_SCOPE_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

fn debug_assert_no_txn_on_thread(what: &str) {
    debug_assert_eq!(
        TXN_SCOPE_DEPTH.with(Cell::get),
        0,
        "lock order violation: {what} requested from a thread already running inside a \
         semantic transaction (a nested acquisition can deadlock behind a queued writer)"
    );
}

/// Owns the semantic database and the gate in front of it.
pub struct SemanticHost {
    gate: Arc<RwLock<()>>,
    /// Private leaf lock. Writers hold the gate exclusively and readers only
    /// lock it to create their snapshot, so it is never contended for longer
    /// than a snapshot creation.
    db: Arc<Mutex<Database>>,
    index: Arc<WorkspaceIndex>,
    /// Write transactions currently holding the gate (0 or 1).
    writers: AtomicUsize,
}

impl SemanticHost {
    /// A host with an empty database bound to `index`.
    pub fn new(index: Arc<WorkspaceIndex>) -> Arc<Self> {
        Self::with_database(Database::new(), index)
    }

    /// A host owning `db`, which is bound to `index` here.
    pub fn with_database(mut db: Database, index: Arc<WorkspaceIndex>) -> Arc<Self> {
        use hyperion_semantics::db::SemanticDatabase as _;
        db.set_workspace_index_handle(hyperion_semantics::WorkspaceIndexHandle(Arc::clone(&index)));
        Arc::new(Self {
            gate: Arc::new(RwLock::new(())),
            db: Arc::new(Mutex::new(db)),
            index,
            writers: AtomicUsize::new(0),
        })
    }

    /// Turn on the debug-build check that the workspace index is only mutated
    /// inside a [`WriteTxn`]. The server enables it; unit tests that prepare an
    /// index directly do not.
    pub fn enforce_exclusive_index_writes(&self) {
        self.index.require_exclusive_writes();
    }

    /// The workspace index this host publishes. Mutating it requires a [`WriteTxn`].
    pub const fn index(&self) -> &Arc<WorkspaceIndex> {
        &self.index
    }

    /// Whether a write transaction currently holds the gate.
    pub fn write_in_progress(&self) -> bool {
        self.writers.load(Ordering::Acquire) != 0
    }

    /// Enter the gate as a reader. Never call while holding another transaction.
    pub async fn read(&self) -> ReadTxn {
        debug_assert_no_txn_on_thread("read()");
        let guard = Arc::clone(&self.gate).read_owned().await;
        self.begin_read(guard)
    }

    /// [`Self::read`] with the user-facing latency budget.
    pub async fn read_interactive(&self) -> Result<ReadTxn, Busy> {
        tokio::time::timeout(INTERACTIVE_READ_BUDGET, self.read())
            .await
            .map_err(|_| Busy)
    }

    /// Enter the gate as the single writer. Never call while holding another
    /// transaction.
    pub async fn write(self: &Arc<Self>) -> WriteTxn {
        debug_assert_no_txn_on_thread("write()");
        let guard = Arc::clone(&self.gate).write_owned().await;
        self.begin_write(guard)
    }

    /// Run one bounded publication step as a write transaction from a blocking
    /// thread (`spawn_blocking`, or any thread outside an async task) that holds
    /// no transaction. Long index builds
    /// and external-index refreshes call this once per bounded step (a chunk of
    /// files, one archive, one final install) so interactive requests and
    /// editor notifications interleave between steps instead of waiting for the
    /// whole operation.
    pub fn publish_blocking<R>(self: &Arc<Self>, step: impl FnOnce(&mut WriteTxn) -> R) -> R {
        let mut txn = futures::executor::block_on(self.write());
        let _scope = ThreadTxnScope::enter();
        step(&mut txn)
    }

    /// Non-waiting [`Self::read`] for synchronous callers that know no writer
    /// exists (one-shot tools and tests).
    pub fn read_now(&self) -> ReadTxn {
        let guard = Arc::clone(&self.gate)
            .try_read_owned()
            .expect("read_now(): a writer holds the semantic gate");
        self.begin_read(guard)
    }

    /// Non-waiting [`Self::write`] for synchronous callers that know the gate is
    /// free (one-shot tools and tests).
    pub fn write_now(self: &Arc<Self>) -> WriteTxn {
        let guard = Arc::clone(&self.gate)
            .try_write_owned()
            .expect("write_now(): a transaction holds the semantic gate");
        self.begin_write(guard)
    }

    fn begin_read(&self, guard: OwnedRwLockReadGuard<()>) -> ReadTxn {
        let snapshot = self.db.lock().snapshot();
        ReadTxn {
            snapshot,
            _guard: guard,
        }
    }

    fn begin_write(self: &Arc<Self>, guard: OwnedRwLockWriteGuard<()>) -> WriteTxn {
        WriteTxn {
            db: Arc::clone(&self.db),
            index: Arc::clone(&self.index),
            _exclusive_index_writes: self.index.begin_exclusive_writes(),
            _guard: guard,
            _active: WriterRegistration::new(Arc::clone(self)),
        }
    }

    /// A host over a fresh database and an empty index, for tests and tools.
    pub fn for_tests() -> Arc<Self> {
        Self::new(Arc::new(WorkspaceIndex::new(std::env::temp_dir())))
    }

    /// Run `f` on the database in a short write transaction. Non-waiting: for
    /// tests and one-shot tools that know no other transaction is live.
    pub fn with_db<R>(self: &Arc<Self>, f: impl FnOnce(&mut Database) -> R) -> R {
        let mut txn = self.write_now();
        let mut db = txn.db();
        f(&mut db)
    }

    /// The editor-registered text of `file`, without entering the gate.
    ///
    /// Dependency hydration prepares its member facts outside any transaction
    /// (the expensive part must not block writers) and revalidates under a
    /// [`WriteTxn`] before installing, so a stale answer here only costs a retry.
    pub fn registered_text(&self, file: FileId) -> Option<Arc<String>> {
        self.db.lock().try_file_text(file)
    }

    /// The Java member-fact cache shared with every snapshot. Interior-locked and
    /// independent of Salsa's revision lock, so reading it needs no transaction.
    pub fn member_fact_cache(&self) -> Arc<hyperion_semantics::nullability::MethodSignatureCache> {
        self.db.lock().method_signature_cache_handle()
    }
}

/// Marks the gate as write-held from acquisition until its transaction ends.
struct WriterRegistration(Arc<SemanticHost>);

impl WriterRegistration {
    fn new(host: Arc<SemanticHost>) -> Self {
        host.writers.fetch_add(1, Ordering::AcqRel);
        Self(host)
    }
}

impl Drop for WriterRegistration {
    fn drop(&mut self) {
        self.0.writers.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Shared access to one consistent Salsa snapshot. Dropping it releases the gate.
pub struct ReadTxn {
    // Field order matters: the snapshot (a Salsa read lock) must drop before the
    // gate is released so a waiting writer never observes a live snapshot.
    snapshot: salsa::Snapshot<Database>,
    _guard: OwnedRwLockReadGuard<()>,
}

impl ReadTxn {
    /// A transaction over a private, single-use database that nothing else can
    /// write, for computations over exact source text that must not see (or
    /// disturb) the server's live state, such as rename's identity proofs.
    /// `configure` sets its inputs. Not gated: no writer can exist.
    pub fn scratch(configure: impl FnOnce(&mut Database)) -> Self {
        let mut db = Database::new();
        configure(&mut db);
        let guard = Arc::new(RwLock::new(()))
            .try_read_owned()
            .expect("a fresh gate is free");
        Self {
            snapshot: db.snapshot(),
            _guard: guard,
        }
    }

    /// The snapshot every query in this transaction must use.
    pub const fn snapshot(&self) -> &salsa::Snapshot<Database> {
        &self.snapshot
    }

    /// Run `work` on a blocking thread with this transaction and release it
    /// there. Starts immediately; awaiting the handle is optional.
    pub fn run_blocking<R: Send + 'static>(
        self,
        work: impl FnOnce(&Self) -> R + Send + 'static,
    ) -> JoinHandle<R> {
        tokio::task::spawn_blocking(move || {
            let _scope = ThreadTxnScope::enter();
            work(&self)
        })
    }
}

impl std::ops::Deref for ReadTxn {
    type Target = salsa::Snapshot<Database>;

    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

/// Exclusive access to the semantic database and the workspace index.
pub struct WriteTxn {
    db: Arc<Mutex<Database>>,
    index: Arc<WorkspaceIndex>,
    _exclusive_index_writes: ExclusiveIndexWrites,
    _guard: OwnedRwLockWriteGuard<()>,
    _active: WriterRegistration,
}

impl WriteTxn {
    /// The database, for Salsa input writes and member-fact cache updates.
    ///
    /// Never create a snapshot from this guard: Salsa's next `set_*` would wait
    /// for it forever. Snapshots come from [`ReadTxn`] only.
    pub fn db(&mut self) -> MutexGuard<'_, Database> {
        self.db.lock()
    }

    /// The workspace index. It may be mutated only while this transaction lives.
    pub const fn index(&self) -> &Arc<WorkspaceIndex> {
        &self.index
    }

    /// Make an already-completed external index mutation visible to Salsa and to
    /// every revision-keyed cache: bump the global workspace epoch.
    pub fn bump_workspace_epoch(&mut self) {
        self.db().bump_workspace_version();
    }

    /// Run `work` on a blocking thread with this transaction and release it
    /// there. Starts immediately; awaiting the handle is optional.
    pub fn run_blocking<R: Send + 'static>(
        mut self,
        work: impl FnOnce(&mut Self) -> R + Send + 'static,
    ) -> JoinHandle<R> {
        tokio::task::spawn_blocking(move || {
            let _scope = ThreadTxnScope::enter();
            work(&mut self)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::db::{Language, SourceDatabase};
    use std::path::PathBuf;

    fn host() -> Arc<SemanticHost> {
        SemanticHost::new(Arc::new(WorkspaceIndex::new(PathBuf::from("."))))
    }

    #[tokio::test]
    async fn readers_share_the_gate_and_a_writer_excludes_them() {
        let host = host();
        let first = host.read().await;
        let second = host.read().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), host.write())
                .await
                .is_err(),
            "a writer must wait for live readers"
        );
        drop((first, second));
        let writer = host.write().await;
        assert!(host.write_in_progress());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), host.read())
                .await
                .is_err(),
            "a reader must wait for the writer"
        );
        drop(writer);
        assert!(!host.write_in_progress());
        drop(host.read().await);
    }

    #[tokio::test]
    async fn a_queued_writer_is_not_starved_by_later_readers() {
        let host = host();
        let reader = host.read().await;
        let writer_host = Arc::clone(&host);
        let writer = tokio::spawn(async move { writer_host.write().await });
        tokio::task::yield_now().await;
        // The writer is queued: a new reader must not overtake it.
        assert!(tokio::time::timeout(Duration::from_millis(50), host.read())
            .await
            .is_err());
        drop(reader);
        drop(writer.await.expect("writer task"));
    }

    #[tokio::test]
    async fn interactive_reads_give_up_after_the_budget_without_holding_the_queue() {
        tokio::time::pause();
        let host = host();
        let writer = host.write().await;
        let result = host.read_interactive().await;
        assert_eq!(result.err(), Some(Busy));
        drop(writer);
        assert!(host.read_interactive().await.is_ok());
    }

    #[tokio::test]
    async fn a_write_is_visible_to_the_next_snapshot_and_not_to_an_older_one() {
        let host = host();
        let file = FileId::new(1);
        host.write().await.db().register_file(
            file,
            Arc::new("class A {}".to_owned()),
            Language::Java,
        );
        let before = host.read().await;
        drop(before);
        let mut writer = host.write().await;
        writer
            .db()
            .register_file(file, Arc::new("class B {}".to_owned()), Language::Java);
        drop(writer);
        let after = host.read().await;
        assert_eq!(after.file_text(file).as_str(), "class B {}");
    }

    #[tokio::test]
    async fn blocking_work_runs_inside_the_transaction() {
        let host = host();
        let file = FileId::new(7);
        let text = host
            .write()
            .await
            .run_blocking(move |txn| {
                txn.db()
                    .register_file(file, Arc::new("class C {}".to_owned()), Language::Java);
                txn.bump_workspace_epoch();
                "done"
            })
            .await
            .expect("blocking write");
        assert_eq!(text, "done");
        let seen = host
            .read()
            .await
            .run_blocking(move |txn| txn.file_text(file).to_string())
            .await
            .expect("blocking read");
        assert_eq!(seen, "class C {}");
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn acquiring_a_transaction_from_inside_one_is_a_lock_order_violation() {
        let host = host();
        let inner_host = Arc::clone(&host);
        let outcome = host
            .read()
            .await
            .run_blocking(move |_txn| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tokio::runtime::Handle::current().block_on(inner_host.write())
                }))
                .is_err()
            })
            .await
            .expect("blocking read");
        assert!(outcome, "nested acquisition must trip the debug assertion");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "outside a semantic write transaction")]
    fn index_mutation_outside_a_write_transaction_is_rejected() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let host = SemanticHost::new(Arc::clone(&index));
        host.enforce_exclusive_index_writes();
        let _ = index.index_file_with_source(&temp.path().join("A.java"), "class A {}");
    }

    #[test]
    fn index_mutation_inside_a_write_transaction_is_allowed() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let host = SemanticHost::new(Arc::clone(&index));
        host.enforce_exclusive_index_writes();
        let txn = host.write_now();
        txn.index()
            .index_file_with_source(&temp.path().join("A.java"), "class A {}")
            .expect("indexed inside the transaction");
    }
}
