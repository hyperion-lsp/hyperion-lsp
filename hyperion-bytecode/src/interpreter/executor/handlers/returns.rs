// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Return instruction handlers for the Executor
//!
//! Handles Ireturn, Lreturn, Freturn, Dreturn, Areturn (value returns)
//! and Return (void return) opcodes.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::Executor;
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::Value;
use tracing::debug;

impl Executor {
    /// Execute a value return (ireturn, lreturn, freturn, dreturn, areturn).
    ///
    /// The return value should already be popped from the frame before calling.
    /// The caller must also extract class_name and method_name before dropping frame.
    pub(crate) fn execute_return_value(
        &mut self,
        thread: &mut Thread,
        monitors: &mut MonitorTable,
        return_value: Value,
        class_name: String,
        method_name: String,
    ) -> VMResult<()> {
        debug!(
            "RETURN: from {}.{}, value={:?}, stack_depth={}",
            class_name,
            method_name,
            return_value,
            thread.depth()
        );

        // Release all monitors acquired by this frame
        thread.release_frame_monitors(monitors)?;

        // Check if there's a caller frame before popping
        let has_caller = thread.depth() > 1;

        if has_caller {
            // There's a caller frame - pop this frame and push result to caller
            thread.pop_frame().ok_or(VMError::NoActiveFrame)?;

            // Push return value to caller's frame
            if let Some(caller_frame) = thread.current_frame_mut() {
                caller_frame.push(return_value)?;
            }
        } else {
            // Top-level method return - push result back for execute_with_methods() to get
            if let Some(f) = thread.current_frame_mut() {
                f.push(return_value)?;
            }
            thread.mark_completed();
        }
        Ok(())
    }

    /// Execute a void return.
    ///
    /// Class initialization state is committed by the initialization coordinator
    /// only after this frame has returned and monitor cleanup has succeeded.
    pub(crate) fn execute_return_void(
        &mut self,
        thread: &mut Thread,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Release all monitors acquired by this frame
        thread.release_frame_monitors(monitors)?;

        if thread.depth() > 1 {
            thread.pop_frame().ok_or(VMError::NoActiveFrame)?;
            // Caller frame continues execution (no return value pushed)
        } else {
            thread.mark_completed();
        }
        Ok(())
    }
}
