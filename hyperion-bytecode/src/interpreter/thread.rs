// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM thread execution context

use super::errors::{VMError, VMResult};
use super::frame::Frame;
use super::monitor::{MonitorTable, ThreadId};

/// JVM thread execution context
pub struct Thread {
    /// Unique thread identifier
    pub id: ThreadId,

    /// Call stack (frames)
    pub frames: Vec<Frame>,

    /// Maximum call depth
    pub max_depth: usize,

    /// Execution step limit (for sandboxing)
    pub step_limit: Option<u64>,

    /// Steps executed so far
    pub steps_executed: u64,

    /// Set only by a return from the outermost frame. Reaching the end of an
    /// owned method body is invalid bytecode, not successful completion.
    completed: bool,
}

impl Thread {
    /// Create a new thread
    pub fn new(max_depth: usize, step_limit: Option<u64>) -> Self {
        Thread {
            id: ThreadId::MAIN,
            frames: Vec::new(),
            max_depth,
            step_limit,
            steps_executed: 0,
            completed: false,
        }
    }

    /// Get the current (top) frame
    pub fn current_frame(&self) -> Option<&Frame> {
        self.frames.last()
    }

    /// Get mutable reference to the current frame
    pub fn current_frame_mut(&mut self) -> Option<&mut Frame> {
        self.frames.last_mut()
    }

    /// Push a new frame onto the call stack
    pub fn push_frame(&mut self, frame: Frame) -> VMResult<()> {
        let effective_depth = if self.completed { 0 } else { self.frames.len() };
        if effective_depth >= self.max_depth {
            return Err(VMError::StackOverflow);
        }
        // A completed top-level frame is retained briefly so legacy callers can
        // read its return value. Starting a new top-level invocation consumes
        // that completion and begins with a fresh stack.
        if self.completed {
            self.frames.clear();
            self.completed = false;
        }
        self.frames.push(frame);
        Ok(())
    }

    /// Pop the current frame and release all its acquired monitors
    pub fn pop_frame(&mut self) -> Option<Frame> {
        self.frames.pop()
    }

    /// Release all monitors acquired by a frame in reverse order (JVMS §2.11.10)
    ///
    /// This should be called before popping a frame to ensure proper cleanup.
    /// Called from:
    /// - Return opcode handlers (normal method exit)
    /// - exception_handler::handle_exception() (exceptional method exit)
    pub fn release_frame_monitors(&mut self, monitors: &mut MonitorTable) -> VMResult<()> {
        let thread_id = self.id;
        let mut first_error = None;
        if let Some(frame) = self.current_frame_mut() {
            // Release every monitor in reverse order (LIFO). A corrupt/stale
            // bookkeeping entry must not strand otherwise valid locks.
            while let Some(heap_id) = frame.acquired_monitors.pop() {
                if monitors.exit(heap_id, thread_id).is_err() && first_error.is_none() {
                    first_error = Some(VMError::internal(
                        "Failed to release monitor during frame cleanup",
                    ));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Pop every frame above `target_depth`, attempting monitor cleanup for all
    /// of them even if one frame's bookkeeping is corrupt.
    pub(crate) fn unwind_to_depth(
        &mut self,
        target_depth: usize,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        if target_depth > self.depth() {
            return Err(VMError::internal(format!(
                "cannot unwind from depth {} to deeper depth {target_depth}",
                self.depth()
            )));
        }

        let mut first_error = None;
        while self.depth() > target_depth {
            if let Err(error) = self.release_frame_monitors(monitors) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
            self.pop_frame();
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Unwind to `target_depth` while another failure is already being reported.
    ///
    /// A cleanup error must not replace the exception that caused the unwind, so it
    /// is logged rather than returned. Frames are popped either way.
    pub(crate) fn unwind_after_failure(
        &mut self,
        target_depth: usize,
        monitors: &mut MonitorTable,
    ) {
        if let Err(error) = self.unwind_to_depth(target_depth, monitors) {
            tracing::warn!(
                "monitor cleanup failed while unwinding to depth {target_depth}: {error:?}"
            );
        }
    }

    /// Get the call stack depth
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Check if there are any frames on the stack
    pub fn has_frames(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Check and increment step counter (for sandboxing)
    pub fn increment_step(&mut self) -> VMResult<()> {
        self.steps_executed += 1;
        if let Some(limit) = self.step_limit {
            if self.steps_executed > limit {
                return Err(VMError::ExecutionLimitExceeded);
            }
        }
        Ok(())
    }

    /// Get the number of steps executed
    pub fn steps_executed(&self) -> u64 {
        self.steps_executed
    }

    /// Reset step counter
    pub fn reset_steps(&mut self) {
        self.steps_executed = 0;
    }

    pub(crate) fn mark_completed(&mut self) {
        self.completed = true;
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.completed
    }

    /// Get all frames (for GC root collection)
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Add all stack values as GC roots
    ///
    /// Called during garbage collection to mark all objects referenced
    /// from the stack as live. Traverses all frames and adds:
    /// - All local variables
    /// - All operand stack values
    pub fn add_gc_roots(&self, roots: &mut super::gc::GCRoots) {
        for frame in &self.frames {
            // Add all local variables that contain values
            for local in frame.locals.iter().flatten() {
                roots.add_value(local);
            }

            // Add all operand stack values
            for value in &frame.operand_stack {
                roots.add_value(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::HeapId;

    #[test]
    fn test_thread_creation() {
        let thread = Thread::new(1024, Some(1_000_000));
        assert_eq!(thread.depth(), 0);
        assert_eq!(thread.steps_executed(), 0);
    }

    #[test]
    fn test_push_pop_frames() {
        let mut thread = Thread::new(10, None);

        let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();
        thread.push_frame(frame).unwrap();

        assert_eq!(thread.depth(), 1);
        assert!(thread.current_frame().is_some());

        thread.pop_frame();
        assert_eq!(thread.depth(), 0);
    }

    #[test]
    fn test_max_depth_exceeded() {
        let mut thread = Thread::new(2, None);

        let frame1 = Frame::new("Test".to_string(), "test1".to_string(), 5, 10, vec![]).unwrap();
        let frame2 = Frame::new("Test".to_string(), "test2".to_string(), 5, 10, vec![]).unwrap();
        let frame3 = Frame::new("Test".to_string(), "test3".to_string(), 5, 10, vec![]).unwrap();

        thread.push_frame(frame1).unwrap();
        thread.push_frame(frame2).unwrap();
        assert!(thread.push_frame(frame3).is_err());
    }

    #[test]
    fn test_step_limit() {
        let mut thread = Thread::new(10, Some(5));

        for _ in 0..5 {
            thread.increment_step().unwrap();
        }
        assert!(thread.increment_step().is_err());
    }

    #[test]
    fn monitor_cleanup_drains_valid_entries_after_a_stale_one() {
        let mut thread = Thread::new(4, None);
        let mut monitors = MonitorTable::new();
        let valid = HeapId(10);
        let stale = HeapId(11);
        monitors.enter(valid, thread.id).unwrap();

        let mut frame = Frame::new("Test".into(), "run".into(), 0, 0, vec![]).unwrap();
        frame.acquired_monitors.push(valid);
        frame.acquired_monitors.push(stale);
        thread.push_frame(frame).unwrap();

        assert!(thread.release_frame_monitors(&mut monitors).is_err());
        assert!(!monitors.is_owned_by(valid, thread.id));
        assert!(thread.current_frame().unwrap().acquired_monitors.is_empty());
    }

    #[test]
    fn unwind_to_depth_pops_all_frames_despite_cleanup_error() {
        let mut thread = Thread::new(4, None);
        let mut monitors = MonitorTable::new();
        let valid = HeapId(20);
        monitors.enter(valid, thread.id).unwrap();

        let base = Frame::new("Test".into(), "base".into(), 0, 0, vec![]).unwrap();
        thread.push_frame(base).unwrap();
        let mut child = Frame::new("Test".into(), "child".into(), 0, 0, vec![]).unwrap();
        child.acquired_monitors.push(valid);
        child.acquired_monitors.push(HeapId(21));
        thread.push_frame(child).unwrap();

        assert!(thread.unwind_to_depth(1, &mut monitors).is_err());
        assert_eq!(thread.depth(), 1);
        assert!(!monitors.is_owned_by(valid, thread.id));
    }
}
