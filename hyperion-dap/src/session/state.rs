// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! State boundaries for the DAP session orchestrator.

use crate::types::{Source, Variable};
use hyperion_jdwp::replies::{FieldInfo, LineTableEntry, LoadedClass, MethodInfo};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy)]
pub(super) struct ScopeInfo {
    pub thread_id: u64,
    pub frame_id: u64,
    pub class_id: u64,
    pub method_id: u64,
    pub code_index: u64,
    /// Position in the thread's stack (0 = top). Frame IDs die whenever a
    /// thread resumes (including an expression's method invocation), so the
    /// depth is what lets `refresh_frames` re-point every scope afterwards.
    pub depth: i32,
}

/// How an expandable object's children are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectKind {
    Plain,
    Array,
    /// `java.util.Collection`: children are its elements (via `toArray()`).
    Collection,
    /// `java.util.Map`: children are its entries (via `entrySet().toArray()`).
    Map,
}

#[derive(Debug, Clone)]
pub(super) struct ObjectRefInfo {
    pub object_id: u64,
    pub thread_id: u64,
    pub kind: ObjectKind,
}

/// One installed JDWP breakpoint location.
pub(super) type BoundLocation = (u64, u64, u64);

/// One breakpoint the client asked for in a source file.
#[derive(Debug, Clone)]
pub(super) struct BreakpointEntry {
    pub id: u64,
    /// Line the client asked for.
    pub line: u64,
    /// Line the breakpoint actually sits on once bound.
    pub actual_line: Option<u64>,
    pub condition: Option<String>,
    /// DAP `hitCondition`: `N`, `==N`, `>N`, `>=N`, `<N`, `<=N` or `%N`.
    pub hit_condition: Option<String>,
    /// DAP `logMessage`: makes this a logpoint (`{expr}` is interpolated).
    pub log_message: Option<String>,
    /// How many times the breakpoint was reached with its condition true.
    pub hits: u64,
    /// JDWP breakpoint requests installed for this entry (one per matching
    /// location across every class compiled from the source file).
    pub request_ids: Vec<u32>,
    pub bound: HashSet<BoundLocation>,
}

/// Breakpoints of one source file. Classes are matched to the file through
/// their `SourceFile` attribute, not through any path-to-class-name rule, so
/// Kotlin facades, nested, anonymous and local classes all resolve.
#[derive(Debug, Clone)]
pub(super) struct SourceBreakpoints {
    pub file_name: String,
    /// JVM package of the file in internal form (`com/example`, empty for the
    /// default package); `None` when the file could not be read.
    pub package: Option<String>,
    /// Line ranges of class-like bodies in the file (see `type_body_ranges`).
    pub type_ranges: Vec<(u64, u64)>,
    pub source: Source,
    pub entries: Vec<BreakpointEntry>,
}

/// A step in progress: what the client asked for and how often the adapter
/// re-issued it because the step landed in a frame the user does not want.
#[derive(Debug, Clone, Copy)]
pub(super) struct StepIntent {
    pub thread_id: u64,
    pub depth: i32,
    pub hops: u32,
    /// False when the step started inside a filtered frame.
    pub filtering: bool,
    /// Step-into: the last hop was a step-out of a filtered callee.
    pub leaving_filtered: bool,
}

pub(super) struct BreakpointState {
    pub sources: HashMap<PathBuf, SourceBreakpoints>,
    pub conditions: HashMap<u32, String>,
    pub next_id: u64,
    pub active_step_request: Option<u32>,
    /// The step the client asked for while it is being carried out; filtered
    /// frames (JDK internals, synthetic frames) re-issue it instead of stopping.
    pub step: Option<StepIntent>,
    /// ClassPrepare requests filtered by `SourceFile` name, keyed by that name.
    pub class_prepare_requests: HashMap<String, u32>,
}

impl Default for BreakpointState {
    fn default() -> Self {
        Self {
            sources: HashMap::new(),
            conditions: HashMap::new(),
            next_id: 1,
            active_step_request: None,
            step: None,
            class_prepare_requests: HashMap::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct TypeCache {
    pub by_signature: HashMap<String, u64>,
    pub signature_by_id: HashMap<u64, String>,
    pub line_tables: HashMap<(u64, u64), Vec<LineTableEntry>>,
    pub methods: HashMap<u64, Vec<MethodInfo>>,
    pub fields: HashMap<u64, Vec<FieldInfo>>,
    /// Every loaded class as of the last scan (refreshed on demand).
    pub all_classes: Option<Vec<LoadedClass>>,
    /// `SourceFile` attribute per class (`None` = the class has none).
    pub source_files: HashMap<u64, Option<String>>,
    /// Direct superclass and interfaces per class, for subtype checks.
    pub supertypes: HashMap<u64, Vec<u64>>,
    /// Reference-type tag (class/interface) of classes with breakpoints.
    pub type_tags: HashMap<u64, u8>,
}

pub(super) struct VariableState {
    next_reference: AtomicU64,
    pub scopes: HashMap<u64, ScopeInfo>,
    pub cached: HashMap<u64, Vec<Variable>>,
    pub objects: HashMap<u64, ObjectRefInfo>,
}

impl Default for VariableState {
    fn default() -> Self {
        Self {
            next_reference: AtomicU64::new(1),
            scopes: HashMap::new(),
            cached: HashMap::new(),
            objects: HashMap::new(),
        }
    }
}

impl VariableState {
    pub fn allocate_reference(&self) -> u64 {
        self.next_reference.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Default)]
pub(super) struct ExceptionState {
    pub requests: Vec<u32>,
    /// `(caught, uncaught)` last requested by the client, kept so the request
    /// can be re-installed after being suspended for an evaluation.
    pub active: Option<(bool, bool)>,
    pub by_thread: HashMap<u64, (u64, bool)>,
}
