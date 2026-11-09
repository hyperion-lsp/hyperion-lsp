// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Monitor operation handlers
//!
//! Handles Monitorenter and Monitorexit opcodes for JVM synchronized blocks.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};

/// Execute Monitorenter: Acquire the monitor associated with an object.
/// The `obj_ref` value should already be popped from the operand stack.
pub fn handle_monitorenter(
    obj_ref: Value,
    thread: &mut Thread,
    monitors: &mut MonitorTable,
) -> VMResult<()> {
    match obj_ref {
        Value::Reference(ObjectRef::Null) => {
            return Err(VMError::NullPointerException);
        }
        Value::Reference(ObjectRef::Object(heap_id))
        | Value::Reference(ObjectRef::Array(heap_id)) => {
            // Acquire monitor for this object
            monitors
                .enter(heap_id, thread.id)
                .map_err(|_| VMError::internal("Monitor deadlock in single-threaded execution"))?;
            // Track the acquired monitor in the current frame for cleanup
            if let Some(current_frame) = thread.current_frame_mut() {
                current_frame.acquired_monitors.push(heap_id);
            }
        }
        _ => {
            return Err(VMError::type_mismatch(
                "object reference",
                &format!("{:?}", obj_ref),
            ));
        }
    }
    Ok(())
}

/// Execute Monitorexit: Release the monitor associated with an object.
/// The `obj_ref` value should already be popped from the operand stack.
pub fn handle_monitorexit(
    obj_ref: Value,
    thread: &mut Thread,
    monitors: &mut MonitorTable,
) -> VMResult<()> {
    match obj_ref {
        Value::Reference(ObjectRef::Null) => {
            return Err(VMError::NullPointerException);
        }
        Value::Reference(ObjectRef::Object(heap_id))
        | Value::Reference(ObjectRef::Array(heap_id)) => {
            // Release monitor for this object
            monitors
                .exit(heap_id, thread.id)
                .map_err(|_| VMError::illegal_monitor_state("current thread is not owner"))?;
            // Remove from frame's monitor tracking (if it was tracked)
            if let Some(current_frame) = thread.current_frame_mut() {
                // Remove the first occurrence of this heap_id from acquired_monitors
                // (monitors can be released in different order than acquired)
                if let Some(pos) = current_frame
                    .acquired_monitors
                    .iter()
                    .position(|&id| id == heap_id)
                {
                    current_frame.acquired_monitors.remove(pos);
                }
            }
        }
        _ => {
            return Err(VMError::type_mismatch(
                "object reference",
                &format!("{:?}", obj_ref),
            ));
        }
    }
    Ok(())
}
