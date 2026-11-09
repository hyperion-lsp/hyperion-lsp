// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `java.lang.Thread` native registration and single-threaded runtime model.

use super::{Heap, NativeRegistry, ObjectRef, VMError, VMResult, Value};

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toNanos",
        "(J)J",
        time_unit_to_nanos,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toMicros",
        "(J)J",
        time_unit_to_micros,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toMillis",
        "(J)J",
        time_unit_to_millis,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toSeconds",
        "(J)J",
        time_unit_to_seconds,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toMinutes",
        "(J)J",
        time_unit_to_minutes,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toHours",
        "(J)J",
        time_unit_to_hours,
    );
    registry.register(
        "java/util/concurrent/TimeUnit",
        "toDays",
        "(J)J",
        time_unit_to_days,
    );

    registry.register("java/lang/Thread", "<init>", "()V", thread_init);
    registry.register(
        "java/lang/Thread",
        "<init>",
        "(Ljava/lang/Runnable;)V",
        thread_init_with_runnable,
    );
    registry.register(
        "java/lang/Thread",
        "<init>",
        "(Ljava/lang/String;)V",
        thread_init_with_name,
    );
    registry.register("java/lang/Thread", "start", "()V", thread_start);
    registry.register("java/lang/Thread", "join", "()V", thread_join);
    registry.register("java/lang/Thread", "join", "(J)V", thread_join_timeout);
    registry.register("java/lang/Thread", "sleep", "(J)V", thread_sleep);
    registry.register(
        "java/lang/Thread",
        "currentThread",
        "()Ljava/lang/Thread;",
        thread_current_thread,
    );
    registry.register(
        "java/lang/Thread",
        "getName",
        "()Ljava/lang/String;",
        thread_get_name,
    );
    registry.register(
        "java/lang/Thread",
        "setName",
        "(Ljava/lang/String;)V",
        thread_set_name,
    );
    registry.register("java/lang/Thread", "getId", "()J", thread_get_id);
    registry.register("java/lang/Thread", "isAlive", "()Z", thread_is_alive);
    registry.register("java/lang/Thread", "interrupt", "()V", thread_interrupt);
    registry.register(
        "java/lang/Thread",
        "isInterrupted",
        "()Z",
        thread_is_interrupted,
    );
    registry.register(
        "java/lang/Thread",
        "interrupted",
        "()Z",
        thread_interrupted_static,
    );
    registry.register("java/lang/Thread", "yield", "()V", thread_yield);
    registry.register(
        "java/lang/Thread",
        "activeCount",
        "()I",
        thread_active_count,
    );
}

// ==== java.lang.Thread Natives ====

// Static storage for "main" thread and "current" thread reference
// For now we use thread-local storage for the current thread
thread_local! {
    static CURRENT_THREAD: std::cell::RefCell<Option<crate::interpreter::HeapId>> =
        const { std::cell::RefCell::new(None) };
}

/// java.lang.Thread.<init>()V - Default constructor
fn thread_init(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_thread()?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// java.lang.Thread.<init>(Ljava/lang/Runnable;)V - Constructor with Runnable
fn thread_init_with_runnable(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "Thread.<init>(Runnable) requires 'this' and runnable".to_string(),
        ));
    }

    let target = match &args[1] {
        Value::Reference(obj_ref) => *obj_ref,
        _ => ObjectRef::Null,
    };

    let id = heap.allocate_thread_with_runnable(target)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// java.lang.Thread.<init>(Ljava/lang/String;)V - Constructor with name
fn thread_init_with_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "Thread.<init>(String) requires 'this' and name".to_string(),
        ));
    }

    let name = match &args[1] {
        Value::Reference(ObjectRef::Object(id)) => heap.get_string(*id)?,
        _ => "Thread".to_string(),
    };

    let id = heap.allocate_thread_with_name(name)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// Nanoseconds in one unit, in `TimeUnit` ordinal order.
const TIME_UNIT_NANOS: [i128; 7] = [
    1,
    1_000,
    1_000_000,
    1_000_000_000,
    60_000_000_000,
    3_600_000_000_000,
    86_400_000_000_000,
];

/// `TimeUnit.toX(duration)`: the receiver's unit converted to unit `target`. A finer
/// target multiplies and saturates, and a coarser one truncates toward zero, as Java does.
fn convert_time_unit(heap: &mut Heap, args: &[Value], target: usize) -> VMResult<Option<Value>> {
    let this = match args.first() {
        Some(Value::Reference(ObjectRef::Object(id))) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let source = match heap.get_dynamic_property(this, "$enum_ordinal") {
        Some(Value::Int(ordinal)) => usize::try_from(ordinal)
            .map_err(|_| VMError::internal("TimeUnit ordinal out of range"))?,
        _ => return Err(VMError::internal("TimeUnit receiver has no unit")),
    };
    let amount = match args.get(1) {
        Some(Value::Long(amount)) => i128::from(*amount),
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "long".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let (from, to) = (TIME_UNIT_NANOS[source], TIME_UNIT_NANOS[target]);
    let converted = if from >= to {
        amount.saturating_mul(from / to)
    } else {
        amount / (to / from)
    };
    let clamped = converted.clamp(i128::from(i64::MIN), i128::from(i64::MAX));
    Ok(Some(Value::Long(clamped as i64)))
}

fn time_unit_to_nanos(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 0)
}

fn time_unit_to_micros(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 1)
}

fn time_unit_to_millis(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 2)
}

fn time_unit_to_seconds(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 3)
}

fn time_unit_to_minutes(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 4)
}

fn time_unit_to_hours(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 5)
}

fn time_unit_to_days(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    convert_time_unit(heap, args, 6)
}

/// java.lang.Thread.start()V - Start the thread
///
/// The interpreter runs `run()` inline for `Thread` receivers before the native
/// registry is consulted (`execute_invokevirtual`), so this native only sees calls
/// that path did not claim. A native gets the heap but not the interpreter, so it
/// cannot run the body. Fail, rather than mark a thread RUNNABLE that never runs.
fn thread_start(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Err(VMError::internal(
        "Thread.start() was not dispatched by the interpreter, so its run() body would not execute",
    ))
}

/// java.lang.Thread.join()V - Wait for thread to complete
fn thread_join(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    use crate::interpreter::heap::ThreadState;

    if args.is_empty() {
        return Err(VMError::NoSuchMethod("join requires 'this'".to_string()));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    // In single-threaded mode, if thread is TERMINATED, return immediately
    // Otherwise this would block forever, so we just return
    let state = heap.get_thread(this_id)?.state.clone();
    if state == ThreadState::Terminated {
        return Ok(None);
    }

    // In single-threaded simulation, we can't actually wait
    // Just return immediately (thread hasn't run yet or is "running")
    tracing::debug!(
        "Thread.join() called on thread {} (single-threaded mode, returning immediately)",
        this_id.0
    );

    Ok(None)
}

/// java.lang.Thread.join(J)V - Wait for thread with timeout
fn thread_join_timeout(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    // Same as join() in single-threaded mode - just return
    thread_join(heap, args)
}

/// java.lang.Thread.sleep(J)V - Sleep for specified milliseconds
fn thread_sleep(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "sleep requires milliseconds argument".to_string(),
        ));
    }

    let millis = args[0].as_long()?;
    if millis < 0 {
        return Err(VMError::InternalError(
            "IllegalArgumentException: timeout value is negative".to_string(),
        ));
    }

    // Actually sleep - this is safe in single-threaded mode
    std::thread::sleep(std::time::Duration::from_millis(millis as u64));

    Ok(None)
}

/// java.lang.Thread.currentThread()Ljava/lang/Thread; - Get current thread
fn thread_current_thread(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // Check if we have a current thread stored
    let thread_id = CURRENT_THREAD.with(|cell| *cell.borrow());

    if let Some(id) = thread_id {
        // Return existing current thread
        Ok(Some(Value::Reference(ObjectRef::Object(id))))
    } else {
        // Create a "main" thread if none exists
        let id = heap.allocate_thread_with_name("main".to_string())?;
        CURRENT_THREAD.with(|cell| *cell.borrow_mut() = Some(id));
        Ok(Some(Value::Reference(ObjectRef::Object(id))))
    }
}

/// java.lang.Thread.getName()Ljava/lang/String; - Get thread name
fn thread_get_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("getName requires 'this'".to_string()));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    let name = heap.get_thread_name(this_id)?;
    let name_ref = heap.intern_string(&name)?;
    Ok(Some(Value::Reference(name_ref)))
}

/// java.lang.Thread.setName(Ljava/lang/String;)V - Set thread name
fn thread_set_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "setName requires 'this' and name".to_string(),
        ));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    let name = match &args[1] {
        Value::Reference(ObjectRef::Object(id)) => heap.get_string(*id)?,
        Value::Reference(ObjectRef::Null) => {
            return Err(VMError::NullPointerException);
        }
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "String".to_string(),
                got: "primitive".to_string(),
            });
        }
    };

    heap.set_thread_name(this_id, name)?;
    Ok(None)
}

/// java.lang.Thread.getId()J - Get thread ID
fn thread_get_id(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("getId requires 'this'".to_string()));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    let thread_id = heap.get_thread_id(this_id)?;
    Ok(Some(Value::Long(thread_id)))
}

/// java.lang.Thread.isAlive()Z - Check if thread is alive
fn thread_is_alive(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("isAlive requires 'this'".to_string()));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    let alive = heap.is_thread_alive(this_id)?;
    Ok(Some(Value::Int(if alive { 1 } else { 0 })))
}

/// java.lang.Thread.interrupt()V - Interrupt the thread
fn thread_interrupt(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "interrupt requires 'this'".to_string(),
        ));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    heap.interrupt_thread(this_id)?;
    Ok(None)
}

/// java.lang.Thread.isInterrupted()Z - Check if thread is interrupted
fn thread_is_interrupted(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "isInterrupted requires 'this'".to_string(),
        ));
    }

    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };

    let interrupted = heap.is_thread_interrupted(this_id)?;
    Ok(Some(Value::Int(if interrupted { 1 } else { 0 })))
}

/// java.lang.Thread.interrupted()Z - Static: check and clear current thread's interrupted status
fn thread_interrupted_static(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // Get current thread
    let thread_id = CURRENT_THREAD.with(|cell| *cell.borrow());

    if let Some(id) = thread_id {
        let was_interrupted = heap.clear_thread_interrupted(id)?;
        Ok(Some(Value::Int(if was_interrupted { 1 } else { 0 })))
    } else {
        // No current thread, not interrupted
        Ok(Some(Value::Int(0)))
    }
}

/// java.lang.Thread.yield()V - Yield execution to other threads
fn thread_yield(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // In single-threaded mode, yield does nothing
    // In multi-threaded mode, this would call std::thread::yield_now()
    std::thread::yield_now();
    Ok(None)
}

/// java.lang.Thread.activeCount()I - Get number of active threads
fn thread_active_count(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // In single-threaded mode, always return 1
    Ok(Some(Value::Int(1)))
}
