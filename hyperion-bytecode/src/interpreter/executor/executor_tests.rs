// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Executor tests are organized into logical modules in the tests/ subdirectory
// This file serves as the entry point for all test modules

#[path = "tests/mod.rs"]
mod cases;

use super::{Executor, NestedExecutionState, GRADLE_SCRIPT_METHODS, GRADLE_VOID_DSL_METHODS};
use crate::interpreter::method_area::{ClassInitState, LoadedClass};
use crate::interpreter::{
    Frame, Heap, HeapId, MethodArea, MonitorTable, Thread, ThreadId, VMError,
};
use std::sync::{Arc, RwLock};

#[test]
fn void_dsl_methods_are_subset_of_script_methods() {
    for method in GRADLE_VOID_DSL_METHODS {
        assert!(
            GRADLE_SCRIPT_METHODS.contains(method),
            "GRADLE_VOID_DSL_METHODS contains {:?} which is not in GRADLE_SCRIPT_METHODS",
            method
        );
    }
}

#[test]
fn nested_fallthrough_is_rejected_and_unwinds_frames_and_monitors() {
    let mut executor = Executor::new(vec![0x03]);
    let mut thread = Thread::new(4, None);
    let mut heap = Heap::new(1024 * 1024);
    let mut monitors = MonitorTable::new();
    thread
        .push_frame(Frame::new("Caller".into(), "run".into(), 0, 1, vec![]).unwrap())
        .unwrap();

    let state = NestedExecutionState::capture(&executor, &thread, "test method");
    let lock = HeapId(42);
    monitors.enter(lock, thread.id).unwrap();
    let mut nested = Frame::with_bytecode(
        "Broken".into(),
        "fallThrough".into(),
        0,
        0,
        vec![],
        vec![0x00], // nop, then reachable end-of-code without return
        "()V".into(),
        vec![],
    )
    .unwrap();
    nested.acquired_monitors.push(lock);
    thread.push_frame(nested).unwrap();

    let result = executor.finish_nested_execution(&mut thread, &mut heap, &mut monitors, state);
    assert!(matches!(result, Err(VMError::InvalidBytecode)));
    assert_eq!(thread.depth(), 1);
    assert!(!monitors.is_owned_by(lock, thread.id));
    assert_eq!(executor.pc(), 0);
}

#[test]
fn empty_owned_method_is_invalid_instead_of_completing_the_thread() {
    let mut executor = Executor::new(vec![0x00]);
    let mut thread = Thread::new(2, None);
    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();
    thread
        .push_frame(
            Frame::with_bytecode(
                "Broken".into(),
                "empty".into(),
                0,
                0,
                vec![],
                Vec::new(),
                "()V".into(),
                vec![],
            )
            .unwrap(),
        )
        .unwrap();

    assert!(matches!(
        executor.step(&mut thread, &mut heap, &mut monitors),
        Err(VMError::InvalidBytecode)
    ));
    assert_eq!(thread.depth(), 1);
}

#[test]
fn clinit_return_does_not_publish_success_before_frame_cleanup() {
    let method_area = Arc::new(RwLock::new(MethodArea::new()));
    {
        let area = method_area.write().unwrap();
        area.register_class(LoadedClass::new("Broken".into()))
            .unwrap();
        area.set_init_state("Broken", ClassInitState::Initializing(ThreadId::MAIN))
            .unwrap();
    }
    let mut executor = Executor::new(Vec::new()).with_method_area(method_area.clone());
    let mut thread = Thread::new(2, None);
    let mut monitors = MonitorTable::new();
    let mut frame = Frame::new("Broken".into(), "<clinit>".into(), 0, 0, vec![]).unwrap();
    frame.acquired_monitors.push(HeapId(99));
    thread.push_frame(frame).unwrap();

    assert!(executor
        .execute_return_void(&mut thread, &mut monitors)
        .is_err());
    assert_eq!(
        method_area
            .read()
            .unwrap()
            .get_class("Broken")
            .unwrap()
            .init_state,
        ClassInitState::Initializing(ThreadId::MAIN)
    );
}

#[test]
fn secondary_executor_gc_roots_active_monitors() {
    let executor = Executor::new(Vec::new());
    let thread = Thread::new(2, None);
    let mut monitors = MonitorTable::new();
    let locked = HeapId(77);
    monitors.enter(locked, thread.id).unwrap();

    let roots = executor.collect_gc_roots(&thread, &monitors);
    assert!(roots.heap_ids.contains(&locked));
}
