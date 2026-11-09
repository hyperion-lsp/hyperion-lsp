// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Exception handling with frame unwinding
//!
//! Implements proper exception handling including:
//! - Frame unwinding when no handler found in current frame
//! - Exception type matching with class hierarchy
//! - Stack trace building for uncaught exceptions
//! - Monitor cleanup during frame unwinding (per JVMS §2.11.10)

use super::errors::{VMError, VMResult};
use super::method_area::MethodArea;
use super::monitor::MonitorTable;
use super::thread::Thread;
use super::value::Value;
use std::sync::{Arc, RwLock};

#[cfg(test)]
use super::heap::Heap;
#[cfg(test)]
use crate::producer::ConstantPoolItem;

/// Handle an exception with proper frame unwinding
///
/// Searches for an exception handler starting from the current frame.
/// If no handler is found, unwinds the call stack frame by frame until
/// a handler is found or the stack is exhausted.
///
/// Releases all monitors acquired by unwound frames to prevent deadlocks.
///
/// Returns Ok(()) if a handler was found and execution should continue.
/// Returns Err(VMError::UncaughtException) if no handler exists.
pub fn handle_exception(
    thread: &mut Thread,
    exception_class: &str,
    exception_value: Value,
    monitors: &mut MonitorTable,
    method_area: Option<&Arc<RwLock<MethodArea>>>,
) -> VMResult<()> {
    handle_exception_with_unwind_floor(
        thread,
        exception_class,
        exception_value,
        monitors,
        method_area,
        None,
    )
}

/// Handle an exception without allowing it to escape into frames below
/// `unwind_floor`. Synchronous nested execution uses this boundary so the Rust
/// driver regains control and can apply call-site semantics (notably
/// ExceptionInInitializerError wrapping) before outer Java handlers run.
pub(crate) fn handle_exception_with_unwind_floor(
    thread: &mut Thread,
    exception_class: &str,
    exception_value: Value,
    monitors: &mut MonitorTable,
    method_area: Option<&Arc<RwLock<MethodArea>>>,
    unwind_floor: Option<usize>,
) -> VMResult<()> {
    let exception_reference = match &exception_value {
        Value::Reference(reference) if !reference.is_null() => *reference,
        _ => {
            return Err(VMError::internal(
                "exception dispatch requires a non-null Throwable object",
            ));
        }
    };
    if unwind_floor.is_some_and(|floor| thread.depth() < floor) {
        return Err(VMError::UncaughtException {
            exception_class: exception_class.to_string(),
            stack_trace: String::new(),
            exception: exception_reference,
        });
    }
    // Build initial stack trace for error reporting
    let mut stack_trace_frames = Vec::new();
    for frame in thread.frames.iter().rev() {
        stack_trace_frames.push(format!("  at {}.{}", frame.class_name, frame.method_name));
    }

    // Search for handler, unwinding frames as needed
    loop {
        // Get current frame
        let frame = match thread.current_frame_mut() {
            Some(f) => f,
            None => {
                // Stack exhausted - uncaught exception
                let stack_trace = stack_trace_frames.join("\n");
                return Err(VMError::UncaughtException {
                    exception_class: exception_class.to_string(),
                    stack_trace,
                    exception: exception_reference,
                });
            }
        };

        // Use usize for PC comparison - do NOT truncate to u16!
        // Methods with >65KB bytecode (e.g., Groovy with 200 closures) would fail
        // if we truncate PC=70000 to 4464 via `as u16`
        let current_pc = frame.exception_pc();

        // Clone exception table to avoid borrow conflicts
        let exception_table = frame.exception_table.clone();

        // Search exception table in this frame
        for entry in &exception_table {
            // Check if PC is in protected region [start_pc, end_pc)
            // Widen exception table entries to usize for comparison (not narrow PC to u16!)
            let start = entry.start_pc as usize;
            let end = entry.end_pc as usize;
            if current_pc >= start && current_pc < end {
                // Check if handler matches exception type
                let matches = match &entry.catch_type {
                    None => true, // catch-all (finally block)
                    Some(catch_class) => {
                        // Check if exception is assignable to catch type
                        match is_assignable(exception_class, catch_class, method_area) {
                            Ok(matches) => matches,
                            Err(error) => {
                                // Resolving an exception-table catch type can
                                // itself throw a linkage error. That error
                                // replaces the original exception and escapes
                                // this method; it must not be matched against
                                // the same unresolved table entry again.
                                let target_depth = thread.depth().saturating_sub(1);
                                thread.unwind_after_failure(target_depth, monitors);
                                return Err(error);
                            }
                        }
                    }
                };

                if matches {
                    // Found matching handler!
                    // Clear operand stack and push exception value
                    let frame = thread.current_frame_mut().unwrap();
                    frame.clear_stack();
                    frame.push_raw(exception_value);

                    // Jump to handler
                    frame.pc = entry.handler_pc as usize;
                    return Ok(());
                }
            }
        }

        // No handler in current frame - unwind to parent frame. A synchronous
        // nested runner stops after unwinding its boundary frame, before the
        // outer caller's exception table is considered.
        let reached_unwind_floor = unwind_floor.is_some_and(|floor| thread.depth() <= floor);
        if thread.depth() <= 1 || reached_unwind_floor {
            // An uncaught frame is terminated, not resumable. Drain all of its
            // monitor bookkeeping and pop it even when one entry is stale.
            let target_depth = thread.depth().saturating_sub(1);
            thread.unwind_after_failure(target_depth, monitors);
            let stack_trace = stack_trace_frames.join("\n");
            return Err(VMError::UncaughtException {
                exception_class: exception_class.to_string(),
                stack_trace,
                exception: exception_reference,
            });
        }

        // Preserve the Java exception as the primary failure while cleanup
        // best-effort drains every lock held by the discarded frame.
        let target_depth = thread.depth() - 1;
        thread.unwind_after_failure(target_depth, monitors);
    }
}

/// Walk class hierarchy to check if exception is assignable to catch type
///
/// This replaces hardcoded exception lists with proper hierarchy walking.
/// Returns true if exception_class is assignable to catch_class per JVM spec.
fn is_assignable(
    exception_class: &str,
    catch_class: &str,
    method_area: Option<&Arc<RwLock<MethodArea>>>,
) -> VMResult<bool> {
    // Fast path: exact match
    if exception_class == catch_class {
        return Ok(true);
    }

    // If no MethodArea, fall back to well-known hierarchy
    let Some(ma) = method_area else {
        return Ok(check_wellknown_hierarchy(exception_class, catch_class));
    };

    // Hold the outer MethodArea guard once. Its named subtype query holds the
    // inner metadata lock for the complete steady-state traversal and fails
    // closed when no authoritative loaded/stub metadata exists.
    let ma_read = ma
        .read()
        .map_err(|_| VMError::internal("MethodArea lock poisoned"))?;
    ma_read
        .get_class(catch_class)
        .map_err(|error| match error {
            VMError::ClassNotFound(name) => VMError::NoClassDefFoundError(format!(
                "exception handler catch type {name} could not be resolved"
            )),
            other => other,
        })?;
    ma_read.is_subtype(exception_class, catch_class)
}

/// Fallback for classes not in MethodArea (bootstrap classes)
///
/// Uses hardcoded knowledge of well-known JDK exception hierarchy.
/// This is needed because bootstrap classes (java.lang.*) aren't always in MethodArea.
fn check_wellknown_hierarchy(exception_class: &str, catch_class: &str) -> bool {
    if exception_class == catch_class {
        return true;
    }

    // Unknown application exceptions can still be caught at the root when
    // their defining class is unavailable during bootstrap fallback.
    if catch_class == "java/lang/Throwable" {
        return true;
    }

    let mut current = exception_class;
    while let Some(parent) = well_known_superclass(current) {
        if parent == catch_class {
            return true;
        }
        current = parent;
    }
    false
}

/// Direct parents for bootstrap exception classes that may not be loaded into
/// the method area. Keeping parent links (rather than broad family lists)
/// preserves transitivity without making sibling exceptions assignable.
fn well_known_superclass(class: &str) -> Option<&'static str> {
    match class {
        "java/lang/Throwable" => Some("java/lang/Object"),
        "java/lang/Exception" | "java/lang/Error" => Some("java/lang/Throwable"),
        "java/lang/RuntimeException"
        | "java/lang/ClassNotFoundException"
        | "java/lang/NoSuchMethodException"
        | "java/lang/NoSuchFieldException"
        | "java/lang/InterruptedException"
        | "java/io/IOException" => Some("java/lang/Exception"),
        "java/lang/IndexOutOfBoundsException"
        | "java/lang/NullPointerException"
        | "java/lang/ArithmeticException"
        | "java/lang/ClassCastException"
        | "java/lang/IllegalArgumentException"
        | "java/lang/IllegalStateException"
        | "java/lang/UnsupportedOperationException"
        | "java/lang/SecurityException"
        | "java/lang/NegativeArraySizeException"
        | "java/lang/ArrayStoreException"
        | "java/lang/IllegalMonitorStateException"
        | "java/util/NoSuchElementException"
        | "java/util/ConcurrentModificationException" => Some("java/lang/RuntimeException"),
        "java/lang/ArrayIndexOutOfBoundsException"
        | "java/lang/StringIndexOutOfBoundsException" => {
            Some("java/lang/IndexOutOfBoundsException")
        }
        "java/lang/NumberFormatException" => Some("java/lang/IllegalArgumentException"),
        "java/util/regex/PatternSyntaxException" => Some("java/lang/IllegalArgumentException"),
        "java/io/FileNotFoundException" => Some("java/io/IOException"),
        "java/lang/VirtualMachineError"
        | "java/lang/LinkageError"
        | "java/lang/AssertionError"
        | "java/lang/ThreadDeath" => Some("java/lang/Error"),
        "java/lang/StackOverflowError"
        | "java/lang/OutOfMemoryError"
        | "java/lang/InternalError"
        | "java/lang/UnknownError" => Some("java/lang/VirtualMachineError"),
        "java/lang/NoClassDefFoundError"
        | "java/lang/UnsatisfiedLinkError"
        | "java/lang/ClassFormatError"
        | "java/lang/VerifyError"
        | "java/lang/ExceptionInInitializerError"
        | "java/lang/BootstrapMethodError"
        | "java/lang/IncompatibleClassChangeError" => Some("java/lang/LinkageError"),
        "java/lang/AbstractMethodError"
        | "java/lang/NoSuchFieldError"
        | "java/lang/NoSuchMethodError"
        | "java/lang/IllegalAccessError"
        | "java/lang/InstantiationError" => Some("java/lang/IncompatibleClassChangeError"),
        _ => None,
    }
}

/// Check if exception type matches catch type
///
/// Returns true if the thrown exception class is assignable to the catch type.
#[cfg(test)]
fn matches_exception_type(
    exception_class: &str,
    catch_type_index: u16,
    _heap: &Heap,
    constant_pool: Option<&Vec<ConstantPoolItem>>,
    method_area: Option<&Arc<RwLock<MethodArea>>>,
) -> VMResult<bool> {
    // Get catch type from constant pool
    let catch_class = match constant_pool {
        Some(cp) => {
            if catch_type_index as usize >= cp.len() {
                return Err(VMError::InvalidConstantPoolIndex(catch_type_index));
            }

            // Get the class name from the constant pool
            // ConstantPoolItem::Class contains a name_index pointing to a UTF-8 entry
            let name_index = match &cp[catch_type_index as usize] {
                ConstantPoolItem::Class { name_index } => *name_index,
                _ => {
                    return Err(VMError::ClassFormatError(format!(
                        "Invalid catch type at CP index {}",
                        catch_type_index
                    )));
                }
            };

            // Resolve the actual class name from the UTF-8 entry
            if name_index == 0 || name_index as usize >= cp.len() {
                return Err(VMError::InvalidConstantPoolIndex(name_index));
            }
            match &cp[name_index as usize] {
                ConstantPoolItem::Utf8(name) => name,
                _ => {
                    return Err(VMError::ClassFormatError(format!(
                        "Invalid class name at CP index {}",
                        name_index
                    )));
                }
            }
        }
        None => {
            // No constant pool - can't resolve type, assume match
            return Ok(true);
        }
    };

    // Check if exception_class is assignable to catch_class
    // Object catch-all
    if catch_class == "java/lang/Object" {
        return Ok(true);
    }

    // Use hierarchy walk instead of hardcoded lists
    is_assignable(exception_class, catch_class, method_area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::frame::{ExceptionTableEntry, Frame};
    use crate::interpreter::heap::Heap;
    use crate::interpreter::method_area::{ClassInitState, FieldLayout, LoadedClass, MethodArea};
    use crate::interpreter::monitor::MonitorTable;
    use crate::interpreter::value::ObjectRef;
    use crate::producer::ConstantPoolItem;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_exception_handler_in_current_frame() {
        let mut thread = Thread::new(10, None);

        // Create a frame with exception table
        let exception_table = vec![ExceptionTableEntry {
            start_pc: 0,
            end_pc: 10,
            handler_pc: 20,
            catch_type: None, // catch-all
        }];

        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();
        frame.exception_table = exception_table;
        frame.pc = 5; // Within protected region

        thread.push_frame(frame).unwrap();

        let exception = Value::Reference(ObjectRef::Object(crate::interpreter::HeapId(99)));
        let mut monitors = MonitorTable::new();
        let result = handle_exception(
            &mut thread,
            "java/lang/Exception",
            exception,
            &mut monitors,
            None,
        );

        assert!(result.is_ok());
        assert_eq!(thread.current_frame().unwrap().pc, 20); // Jumped to handler
    }

    #[test]
    fn test_exception_unwinding_to_parent() {
        let mut thread = Thread::new(10, None);

        // Parent frame with handler
        let parent_exception_table = vec![ExceptionTableEntry {
            start_pc: 0,
            end_pc: 100,
            handler_pc: 50,
            catch_type: None, // catch-all
        }];

        let mut parent_frame =
            Frame::new("Parent".to_string(), "parent".to_string(), 5, 10, vec![]).unwrap();
        parent_frame.exception_table = parent_exception_table;
        parent_frame.pc = 10;

        // Child frame with no handler
        let child_frame =
            Frame::new("Child".to_string(), "child".to_string(), 5, 10, vec![]).unwrap();

        thread.push_frame(parent_frame).unwrap();
        thread.push_frame(child_frame).unwrap();

        assert_eq!(thread.depth(), 2);

        let exception = Value::Reference(ObjectRef::Object(crate::interpreter::HeapId(99)));
        let mut monitors = MonitorTable::new();
        let result = handle_exception(
            &mut thread,
            "java/lang/Exception",
            exception,
            &mut monitors,
            None,
        );

        assert!(result.is_ok());
        // Should have unwound to parent frame
        assert_eq!(thread.depth(), 1);
        assert_eq!(thread.current_frame().unwrap().pc, 50); // Jumped to parent's handler
    }

    #[test]
    fn test_uncaught_exception() {
        let mut thread = Thread::new(10, None);

        // Frames with no matching handler, caller first and throwing callee last.
        let caller = Frame::new("Caller".to_string(), "run".to_string(), 5, 10, vec![]).unwrap();
        let callee =
            Frame::new("Callee".to_string(), "explode".to_string(), 5, 10, vec![]).unwrap();
        thread.push_frame(caller).unwrap();
        thread.push_frame(callee).unwrap();

        let exception = Value::Reference(ObjectRef::Object(crate::interpreter::HeapId(99)));
        let mut monitors = MonitorTable::new();
        let result = handle_exception(
            &mut thread,
            "java/lang/Exception",
            exception,
            &mut monitors,
            None,
        );

        assert!(result.is_err());
        match result {
            Err(VMError::UncaughtException {
                exception_class,
                stack_trace,
                ..
            }) => {
                assert_eq!(exception_class, "java/lang/Exception");
                let callee = stack_trace.find("Callee.explode").unwrap();
                let caller = stack_trace.find("Caller.run").unwrap();
                assert!(callee < caller, "{stack_trace}");
            }
            _ => panic!("Expected UncaughtException"),
        }
    }

    #[test]
    fn test_exception_type_matching() {
        let heap = Heap::new(1024 * 1024);

        // Test exact match
        // CP layout: [0: Utf8("") (placeholder), 1: Utf8("java/lang/RuntimeException"), 2: Class{name_index:1}]
        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/RuntimeException".to_string()),
            ConstantPoolItem::Class { name_index: 1 }, // Points to CP[1]
        ];
        let result =
            matches_exception_type("java/lang/RuntimeException", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(result);

        // Test inheritance (NullPointerException extends RuntimeException)
        let result =
            matches_exception_type("java/lang/NullPointerException", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(result);

        // Test catch-all with Object
        let cp_object = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Object".to_string()),
            ConstantPoolItem::Class { name_index: 1 }, // Points to CP[1]
        ];
        let result =
            matches_exception_type("java/lang/Exception", 2, &heap, Some(&cp_object), None)
                .unwrap();
        assert!(result);

        // Test Throwable catches all
        let cp_throwable = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Throwable".to_string()),
            ConstantPoolItem::Class { name_index: 1 }, // Points to CP[1]
        ];
        let result =
            matches_exception_type("java/lang/Exception", 2, &heap, Some(&cp_throwable), None)
                .unwrap();
        assert!(result);
    }

    #[test]
    fn bootstrap_exception_matching_follows_real_parent_links() {
        assert!(check_wellknown_hierarchy(
            "java/lang/StringIndexOutOfBoundsException",
            "java/lang/IndexOutOfBoundsException"
        ));
        assert!(check_wellknown_hierarchy(
            "java/lang/NumberFormatException",
            "java/lang/IllegalArgumentException"
        ));
        assert!(check_wellknown_hierarchy(
            "java/lang/StackOverflowError",
            "java/lang/Error"
        ));
        assert!(!check_wellknown_hierarchy(
            "java/lang/OutOfMemoryError",
            "java/lang/StackOverflowError"
        ));
        assert!(!check_wellknown_hierarchy(
            "java/lang/LinkageError",
            "java/lang/VirtualMachineError"
        ));
    }

    #[test]
    fn test_monitor_cleanup_on_exception_unwinding() {
        let mut thread = Thread::new(10, None);
        let mut monitors = MonitorTable::new();

        // Create a parent frame with a handler
        let parent_exception_table = vec![ExceptionTableEntry {
            start_pc: 0,
            end_pc: 100,
            handler_pc: 50,
            catch_type: None, // catch-all
        }];

        let mut parent_frame =
            Frame::new("Parent".to_string(), "parent".to_string(), 5, 10, vec![]).unwrap();
        parent_frame.exception_table = parent_exception_table;
        parent_frame.pc = 10;

        // Create a child frame that acquired monitors
        let mut child_frame =
            Frame::new("Child".to_string(), "child".to_string(), 5, 10, vec![]).unwrap();

        // Simulate the child frame acquiring two monitors
        let object1 = crate::interpreter::value::HeapId(1);
        let object2 = crate::interpreter::value::HeapId(2);
        child_frame.acquired_monitors.push(object1);
        child_frame.acquired_monitors.push(object2);

        // Actually acquire the monitors in the monitor table
        monitors.enter(object1, thread.id).unwrap();
        monitors.enter(object2, thread.id).unwrap();

        thread.push_frame(parent_frame).unwrap();
        thread.push_frame(child_frame).unwrap();

        assert_eq!(thread.depth(), 2);
        assert!(monitors.is_owned_by(object1, thread.id));
        assert!(monitors.is_owned_by(object2, thread.id));

        // Trigger exception handling
        let exception = Value::Reference(ObjectRef::Object(crate::interpreter::HeapId(99)));
        let result = handle_exception(
            &mut thread,
            "java/lang/Exception",
            exception,
            &mut monitors,
            None,
        );

        assert!(result.is_ok());
        // Should have unwound to parent frame
        assert_eq!(thread.depth(), 1);
        assert_eq!(thread.current_frame().unwrap().pc, 50); // Jumped to parent's handler

        // Verify that monitors were released during unwinding
        assert!(
            !monitors.is_owned_by(object1, thread.id),
            "Monitor 1 should be released"
        );
        assert!(
            !monitors.is_owned_by(object2, thread.id),
            "Monitor 2 should be released"
        );
    }

    #[test]
    fn test_monitor_cleanup_on_uncaught_exception() {
        let mut thread = Thread::new(10, None);
        let mut monitors = MonitorTable::new();

        // Create a frame with monitors but no handler
        let mut frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![]).unwrap();

        // Simulate acquiring monitors
        let object1 = crate::interpreter::value::HeapId(1);
        frame.acquired_monitors.push(object1);

        // Actually acquire the monitor
        monitors.enter(object1, thread.id).unwrap();

        thread.push_frame(frame).unwrap();

        assert!(monitors.is_owned_by(object1, thread.id));

        // Trigger uncaught exception
        let exception = Value::Reference(ObjectRef::Object(crate::interpreter::HeapId(99)));
        let result = handle_exception(
            &mut thread,
            "java/lang/Exception",
            exception,
            &mut monitors,
            None,
        );

        assert!(result.is_err());
        assert_eq!(thread.depth(), 0, "uncaught frame must be popped");
        // Verify that monitor was released even with uncaught exception
        assert!(
            !monitors.is_owned_by(object1, thread.id),
            "Monitor should be released even on uncaught exception"
        );
    }

    #[test]
    fn test_custom_exception_hierarchy_matching() {
        let heap = Heap::new(1024 * 1024);

        // Create a MethodArea with custom exception classes
        let method_area = Arc::new(RwLock::new(MethodArea::new()));

        // Add custom exception: MyException extends RuntimeException
        {
            let ma = method_area.write().unwrap();
            let my_exception = LoadedClass {
                name: "com/example/MyException".to_string(),
                superclass: Some("java/lang/RuntimeException".to_string()),
                interfaces: vec![],
                vtable: vec![],
                resolved_virtual_methods: Vec::new(),
                non_virtual_methods: vec![],
                itable: vec![],
                field_layout: FieldLayout::new(),
                init_state: ClassInitState::NotInitialized,
                declared_fields: Arc::from([]),
                constant_field_values: Arc::from([]),
                modifiers: 0,
                is_interface: false,
                is_native_stub: false,
                default_conflict_count: 0,
                source_file: None,
                nest: None,
                bootstrap_methods: vec![],
            };
            ma.register_class(my_exception).unwrap();

            // Add another custom exception: MyIOException extends IOException
            let my_io_exception = LoadedClass {
                name: "com/example/MyIOException".to_string(),
                superclass: Some("java/io/IOException".to_string()),
                interfaces: vec![],
                vtable: vec![],
                resolved_virtual_methods: Vec::new(),
                non_virtual_methods: vec![],
                itable: vec![],
                field_layout: FieldLayout::new(),
                init_state: ClassInitState::NotInitialized,
                declared_fields: Arc::from([]),
                constant_field_values: Arc::from([]),
                modifiers: 0,
                is_interface: false,
                is_native_stub: false,
                default_conflict_count: 0,
                source_file: None,
                nest: None,
                bootstrap_methods: vec![],
            };
            ma.register_class(my_io_exception).unwrap();
        }

        // Test 1: Custom exception caught by exact match
        let cp1 = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("com/example/MyException".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];
        let result = matches_exception_type(
            "com/example/MyException",
            2,
            &heap,
            Some(&cp1),
            Some(&method_area),
        )
        .unwrap();
        assert!(result, "Custom exception should be caught by exact match");

        // Test 2: Custom exception caught by superclass (RuntimeException)
        let cp2 = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/RuntimeException".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];
        let result = matches_exception_type(
            "com/example/MyException",
            2,
            &heap,
            Some(&cp2),
            Some(&method_area),
        )
        .unwrap();
        assert!(
            result,
            "Custom exception should be caught by RuntimeException catch block"
        );

        // Test 3: Custom exception caught by Exception (grandparent)
        let cp3 = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];
        let result = matches_exception_type(
            "com/example/MyException",
            2,
            &heap,
            Some(&cp3),
            Some(&method_area),
        )
        .unwrap();
        assert!(
            result,
            "Custom exception should be caught by Exception catch block"
        );

        // Test 4: Custom exception NOT caught by unrelated exception type
        let cp4 = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/io/IOException".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];
        let result = matches_exception_type(
            "com/example/MyException",
            2,
            &heap,
            Some(&cp4),
            Some(&method_area),
        )
        .unwrap();
        assert!(
            !result,
            "Custom exception should NOT be caught by unrelated IOException"
        );

        // Test 5: MyIOException caught by IOException
        let result = matches_exception_type(
            "com/example/MyIOException",
            2,
            &heap,
            Some(&cp4),
            Some(&method_area),
        )
        .unwrap();
        assert!(
            result,
            "MyIOException should be caught by IOException catch block"
        );

        // Test 6: MyIOException caught by Exception (grandparent)
        let result = matches_exception_type(
            "com/example/MyIOException",
            2,
            &heap,
            Some(&cp3),
            Some(&method_area),
        )
        .unwrap();
        assert!(
            result,
            "MyIOException should be caught by Exception catch block"
        );
    }

    // ============================================================
    // Regression tests: Error/Exception hierarchy separation
    // ============================================================
    // These tests verify that Error and its subclasses are NOT caught
    // by catch(Exception), which was incorrectly happening before the fix.

    #[test]
    fn test_error_not_caught_by_exception() {
        let heap = Heap::new(1024 * 1024);

        // CP with Exception as catch type
        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // Error should NOT be caught by Exception
        let result = matches_exception_type("java/lang/Error", 2, &heap, Some(&cp), None).unwrap();
        assert!(
            !result,
            "java.lang.Error must NOT be caught by catch(Exception)"
        );
    }

    #[test]
    fn test_stackoverflow_not_caught_by_exception() {
        let heap = Heap::new(1024 * 1024);

        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // StackOverflowError should NOT be caught by Exception
        let result =
            matches_exception_type("java/lang/StackOverflowError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            !result,
            "StackOverflowError must NOT be caught by catch(Exception)"
        );
    }

    #[test]
    fn test_outofmemory_not_caught_by_exception() {
        let heap = Heap::new(1024 * 1024);

        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // OutOfMemoryError should NOT be caught by Exception
        let result =
            matches_exception_type("java/lang/OutOfMemoryError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            !result,
            "OutOfMemoryError must NOT be caught by catch(Exception)"
        );
    }

    #[test]
    fn test_linkage_error_not_caught_by_exception() {
        let heap = Heap::new(1024 * 1024);

        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // LinkageError should NOT be caught by Exception
        let result =
            matches_exception_type("java/lang/LinkageError", 2, &heap, Some(&cp), None).unwrap();
        assert!(
            !result,
            "LinkageError must NOT be caught by catch(Exception)"
        );

        // NoClassDefFoundError (subclass of LinkageError) should NOT be caught by Exception
        let result =
            matches_exception_type("java/lang/NoClassDefFoundError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            !result,
            "NoClassDefFoundError must NOT be caught by catch(Exception)"
        );
    }

    #[test]
    fn test_error_caught_by_throwable() {
        let heap = Heap::new(1024 * 1024);

        // CP with Throwable as catch type
        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Throwable".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // Error SHOULD be caught by Throwable
        let result = matches_exception_type("java/lang/Error", 2, &heap, Some(&cp), None).unwrap();
        assert!(result, "Error should be caught by catch(Throwable)");

        // StackOverflowError SHOULD be caught by Throwable
        let result =
            matches_exception_type("java/lang/StackOverflowError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            result,
            "StackOverflowError should be caught by catch(Throwable)"
        );
    }

    #[test]
    fn test_error_caught_by_error() {
        let heap = Heap::new(1024 * 1024);

        // CP with Error as catch type
        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Error".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // Error SHOULD be caught by Error
        let result = matches_exception_type("java/lang/Error", 2, &heap, Some(&cp), None).unwrap();
        assert!(result, "Error should be caught by catch(Error)");

        // StackOverflowError SHOULD be caught by Error
        let result =
            matches_exception_type("java/lang/StackOverflowError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            result,
            "StackOverflowError should be caught by catch(Error)"
        );

        // OutOfMemoryError SHOULD be caught by Error
        let result =
            matches_exception_type("java/lang/OutOfMemoryError", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(result, "OutOfMemoryError should be caught by catch(Error)");
    }

    #[test]
    fn test_exception_still_caught_by_exception() {
        let heap = Heap::new(1024 * 1024);

        let cp = vec![
            ConstantPoolItem::Utf8(String::new()), // Index 0 placeholder
            ConstantPoolItem::Utf8("java/lang/Exception".to_string()),
            ConstantPoolItem::Class { name_index: 1 },
        ];

        // Regular exceptions SHOULD still be caught by Exception
        let result =
            matches_exception_type("java/lang/Exception", 2, &heap, Some(&cp), None).unwrap();
        assert!(result, "Exception should be caught by catch(Exception)");

        let result =
            matches_exception_type("java/lang/RuntimeException", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            result,
            "RuntimeException should be caught by catch(Exception)"
        );

        let result =
            matches_exception_type("java/lang/NullPointerException", 2, &heap, Some(&cp), None)
                .unwrap();
        assert!(
            result,
            "NullPointerException should be caught by catch(Exception)"
        );

        let result =
            matches_exception_type("java/io/IOException", 2, &heap, Some(&cp), None).unwrap();
        assert!(result, "IOException should be caught by catch(Exception)");
    }
}
