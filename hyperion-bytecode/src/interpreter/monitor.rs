// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM Monitor synchronization support
//!
//! Implements per-object intrinsic locks (monitors) as specified in JVMS §2.11.10.
//! Monitors are reentrant - same thread can acquire same lock multiple times.

use super::errors::{VMError, VMResult};
use super::executor::access_flags::ACC_SYNCHRONIZED;
use super::frame::Frame;
use super::heap::Heap;
use super::thread::Thread;
use super::value::{HeapId, ObjectRef};
use std::collections::HashMap;

/// Thread identity for monitor ownership tracking.
/// In single-threaded mode, use ThreadId::MAIN as the dummy thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThreadId(pub u32);

impl ThreadId {
    /// Default thread ID for single-threaded execution
    pub const MAIN: ThreadId = ThreadId(0);
}

/// State of an object's intrinsic lock (monitor)
#[derive(Debug, Clone)]
struct MonitorState {
    /// Thread that currently owns this monitor (None = unowned)
    owner: Option<ThreadId>,
    /// Entry count for reentrant locking
    entry_count: u32,
}

impl MonitorState {
    fn new() -> Self {
        MonitorState {
            owner: None,
            entry_count: 0,
        }
    }
}

/// Monitor operation errors
#[derive(Debug, Clone)]
pub enum MonitorError {
    /// Thread doesn't own the monitor (IllegalMonitorStateException)
    NotOwned,
    /// Reentrant lock count overflow
    Overflow,
}

/// Table of all active object monitors
#[derive(Debug)]
pub struct MonitorTable {
    monitors: HashMap<HeapId, MonitorState>,
}

impl MonitorTable {
    /// Create a new monitor table
    pub fn new() -> Self {
        MonitorTable {
            monitors: HashMap::new(),
        }
    }

    /// Acquire monitor for object (MONITORENTER semantics)
    ///
    /// - If unowned: acquire with count=1
    /// - If owned by same thread: increment count (reentrant)
    /// - If owned by different thread: would block (error in single-threaded)
    pub fn enter(&mut self, object: HeapId, thread: ThreadId) -> Result<(), MonitorError> {
        let monitor = self
            .monitors
            .entry(object)
            .or_insert_with(MonitorState::new);

        match monitor.owner {
            None => {
                // Unowned - acquire
                monitor.owner = Some(thread);
                monitor.entry_count = 1;
                Ok(())
            }
            Some(owner) if owner == thread => {
                // Same thread - reentrant, increment count
                monitor.entry_count = monitor
                    .entry_count
                    .checked_add(1)
                    .ok_or(MonitorError::Overflow)?;
                Ok(())
            }
            Some(_other) => {
                // Different thread owns it
                // In single-threaded mode, this shouldn't happen, but return NotOwned for consistency
                Err(MonitorError::NotOwned)
            }
        }
    }

    /// Release monitor for object (MONITOREXIT semantics)
    ///
    /// - If not owned by this thread: IllegalMonitorStateException
    /// - If entry_count > 1: decrement count
    /// - If entry_count == 1: release monitor
    pub fn exit(&mut self, object: HeapId, thread: ThreadId) -> Result<(), MonitorError> {
        let monitor = self
            .monitors
            .get_mut(&object)
            .ok_or(MonitorError::NotOwned)?;

        if monitor.owner != Some(thread) {
            return Err(MonitorError::NotOwned);
        }

        monitor.entry_count = monitor
            .entry_count
            .checked_sub(1)
            .ok_or(MonitorError::NotOwned)?; // Underflow safety
        if monitor.entry_count == 0 {
            // Remove monitor from HashMap when fully released
            self.monitors.remove(&object);
        }

        Ok(())
    }

    /// Check if thread owns the monitor (for wait/notify validation)
    pub fn is_owned_by(&self, object: HeapId, thread: ThreadId) -> bool {
        self.monitors
            .get(&object)
            .map(|m| m.owner == Some(thread))
            .unwrap_or(false)
    }

    /// Get the number of active monitors (for testing/diagnostics)
    pub fn active_monitors(&self) -> usize {
        self.monitors.len()
    }

    /// Add all monitored objects as GC roots
    ///
    /// Called during garbage collection to mark all objects that have
    /// active monitors as live. Any object with a monitor must be kept
    /// alive as long as threads may be synchronized on it.
    pub fn add_gc_roots(&self, roots: &mut super::gc::GCRoots) {
        for heap_id in self.monitors.keys() {
            roots.add_heap_id(*heap_id);
        }
    }
}

impl Default for MonitorTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Acquire the class monitor for a static `synchronized` method (JVMS §2.11.10).
///
/// The lock is the `Class` object of the declaring class. It is interned, so it is
/// the same object that `X.class` yields. The monitor is recorded in `frame`, so
/// returning from or unwinding the frame releases it. Other methods are unchanged.
pub(crate) fn enter_static_synchronized(
    access_flags: u16,
    class_name: &str,
    frame: &mut Frame,
    thread: &Thread,
    heap: &mut Heap,
    monitors: &mut MonitorTable,
) -> VMResult<()> {
    if access_flags & ACC_SYNCHRONIZED == 0 {
        return Ok(());
    }
    let ObjectRef::Object(lock) = heap.intern_class(class_name)? else {
        return Ok(());
    };
    monitors
        .enter(lock, thread.id)
        .map_err(|_| VMError::internal("Monitor deadlock in single-threaded execution"))?;
    frame.acquired_monitors.push(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_enter_unowned() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread = ThreadId::MAIN;

        assert!(table.enter(heap_id, thread).is_ok());
        assert!(table.is_owned_by(heap_id, thread));
    }

    #[test]
    fn test_monitor_exit_unowned_fails() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread = ThreadId::MAIN;

        assert!(matches!(
            table.exit(heap_id, thread),
            Err(MonitorError::NotOwned)
        ));
    }

    #[test]
    fn test_monitor_reentrant() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread = ThreadId::MAIN;

        assert!(table.enter(heap_id, thread).is_ok());
        assert!(table.enter(heap_id, thread).is_ok()); // Reentrant acquire
        assert!(table.is_owned_by(heap_id, thread));

        // Need two exits to fully release
        assert!(table.exit(heap_id, thread).is_ok());
        assert!(table.is_owned_by(heap_id, thread)); // Still owned

        assert!(table.exit(heap_id, thread).is_ok());
        assert!(!table.is_owned_by(heap_id, thread)); // Now released
    }

    #[test]
    fn test_monitor_wrong_thread() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread1 = ThreadId(1);
        let thread2 = ThreadId(2);

        assert!(table.enter(heap_id, thread1).is_ok());
        assert!(matches!(
            table.exit(heap_id, thread2),
            Err(MonitorError::NotOwned)
        ));
    }

    #[test]
    fn test_monitor_multiple_objects() {
        let mut table = MonitorTable::new();
        let heap_id1 = HeapId(1);
        let heap_id2 = HeapId(2);
        let thread = ThreadId::MAIN;

        // Acquire two different monitors
        assert!(table.enter(heap_id1, thread).is_ok());
        assert!(table.enter(heap_id2, thread).is_ok());

        // Both should be owned by same thread
        assert!(table.is_owned_by(heap_id1, thread));
        assert!(table.is_owned_by(heap_id2, thread));

        // Release in any order
        assert!(table.exit(heap_id2, thread).is_ok());
        assert!(table.exit(heap_id1, thread).is_ok());

        // Both should be released
        assert!(!table.is_owned_by(heap_id1, thread));
        assert!(!table.is_owned_by(heap_id2, thread));
    }

    #[test]
    fn test_monitor_deep_reentrant() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread = ThreadId::MAIN;

        // Acquire 5 times
        for _ in 0..5 {
            assert!(table.enter(heap_id, thread).is_ok());
        }

        assert!(table.is_owned_by(heap_id, thread));

        // Release 4 times - should still be owned
        for _ in 0..4 {
            assert!(table.exit(heap_id, thread).is_ok());
            assert!(table.is_owned_by(heap_id, thread));
        }

        // Final release
        assert!(table.exit(heap_id, thread).is_ok());
        assert!(!table.is_owned_by(heap_id, thread));
    }

    #[test]
    fn test_monitor_reentrant_overflow_returns_error() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(99);
        let thread = ThreadId::MAIN;

        table.monitors.insert(
            heap_id,
            MonitorState {
                owner: Some(thread),
                entry_count: u32::MAX,
            },
        );

        assert!(matches!(
            table.enter(heap_id, thread),
            Err(MonitorError::Overflow)
        ));
    }

    #[test]
    fn test_monitor_cleanup_after_release() {
        let mut table = MonitorTable::new();
        let heap_id = HeapId(0);
        let thread = ThreadId::MAIN;

        // Initially no monitors
        assert_eq!(table.active_monitors(), 0);

        // Acquire monitor
        assert!(table.enter(heap_id, thread).is_ok());
        assert_eq!(table.active_monitors(), 1);

        // Reentrant acquire
        assert!(table.enter(heap_id, thread).is_ok());
        assert_eq!(table.active_monitors(), 1); // Still just one monitor

        // First release - still one monitor (entry_count > 0)
        assert!(table.exit(heap_id, thread).is_ok());
        assert_eq!(table.active_monitors(), 1);

        // Full release - monitor removed from HashMap
        assert!(table.exit(heap_id, thread).is_ok());
        assert_eq!(table.active_monitors(), 0); // Monitor cleaned up!
        assert!(!table.is_owned_by(heap_id, thread));
    }

    #[test]
    fn test_monitor_cleanup_multiple_objects() {
        let mut table = MonitorTable::new();
        let obj1 = HeapId(1);
        let obj2 = HeapId(2);
        let obj3 = HeapId(3);
        let thread = ThreadId::MAIN;

        // Acquire three monitors
        assert!(table.enter(obj1, thread).is_ok());
        assert!(table.enter(obj2, thread).is_ok());
        assert!(table.enter(obj3, thread).is_ok());
        assert_eq!(table.active_monitors(), 3);

        // Release obj1 completely
        assert!(table.exit(obj1, thread).is_ok());
        assert_eq!(table.active_monitors(), 2);

        // Release obj2 completely
        assert!(table.exit(obj2, thread).is_ok());
        assert_eq!(table.active_monitors(), 1);

        // Release obj3 completely
        assert!(table.exit(obj3, thread).is_ok());
        assert_eq!(table.active_monitors(), 0);
    }

    #[test]
    fn static_synchronized_locks_the_class_object_until_the_frame_returns() -> VMResult<()> {
        let mut heap = Heap::new(64 * 1024);
        let mut monitors = MonitorTable::new();
        let mut thread = Thread::new(16, None);
        let mut frame = Frame::new("p.C".into(), "m".into(), 0, 0, Vec::new())?;

        enter_static_synchronized(
            ACC_SYNCHRONIZED,
            "p.C",
            &mut frame,
            &thread,
            &mut heap,
            &mut monitors,
        )?;
        let ObjectRef::Object(lock) = heap.intern_class("p.C")? else {
            panic!("a class lock is a heap object");
        };
        assert!(monitors.is_owned_by(lock, thread.id));

        thread.push_frame(frame)?;
        thread.release_frame_monitors(&mut monitors)?;
        assert!(!monitors.is_owned_by(lock, thread.id));
        Ok(())
    }

    #[test]
    fn methods_without_synchronized_take_no_class_lock() -> VMResult<()> {
        let mut heap = Heap::new(64 * 1024);
        let mut monitors = MonitorTable::new();
        let thread = Thread::new(16, None);
        let mut frame = Frame::new("p.C".into(), "m".into(), 0, 0, Vec::new())?;

        enter_static_synchronized(0, "p.C", &mut frame, &thread, &mut heap, &mut monitors)?;

        assert!(frame.acquired_monitors.is_empty());
        Ok(())
    }
}
