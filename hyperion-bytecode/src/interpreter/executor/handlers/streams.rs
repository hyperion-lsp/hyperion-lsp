// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util.stream support (eager model)
//!
//! A stream is a heap collection (class `java/util/stream/Stream`) holding a
//! snapshot of its elements; every operation evaluates immediately over that
//! snapshot. Lambdas and method references passed to stream operations run
//! synchronously through the executor (`call_sam_sync`), the same machinery
//! comparator-driven sort uses. This trades laziness (short-circuiting,
//! infinite streams) for simplicity — fine for the finite, side-effect-light
//! pipelines the interpreter targets.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::Executor;
use crate::interpreter::heap::Heap;
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};
use crate::interpreter::HeapId;

use super::invocation::natural_compare;

/// Unwrap a boxed numeric wrapper to its primitive (field 0); primitives pass
/// through. Used where a stream op's result must be numeric (mapToInt/sum).
fn unwrap_numeric(heap: &Heap, v: Value) -> Value {
    if let Value::Reference(ObjectRef::Object(id)) = &v {
        if let Ok(Some(
            inner @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_)),
        )) = heap.get_field(*id, 0)
        {
            return inner;
        }
    }
    v
}

fn as_i64(heap: &Heap, v: &Value) -> i64 {
    match unwrap_numeric(heap, v.clone()) {
        Value::Int(n) => n as i64,
        Value::Long(n) => n,
        Value::Float(f) => f as i64,
        Value::Double(d) => d as i64,
        _ => 0,
    }
}

fn as_f64(heap: &Heap, v: &Value) -> f64 {
    match unwrap_numeric(heap, v.clone()) {
        Value::Int(n) => n as f64,
        Value::Long(n) => n as f64,
        Value::Float(f) => f as f64,
        Value::Double(d) => d,
        _ => 0.0,
    }
}

/// Wrap a terminal stream result in an `Optional`-shaped object (field 0 =
/// the value, or `Null` when absent). Real Java has distinct `Optional` /
/// `OptionalInt` / `OptionalLong` / `OptionalDouble` types, but dispatch here
/// is keyed purely on the *runtime* class name the interpreter itself
/// assigns (see `dispatch_native_virtual`'s `java/util/Optional` arm), not on
/// anything resolved from the constant pool — so a single shared class works
/// for every stream flavor; `get`/`getAsInt`/`getAsLong`/`getAsDouble`/
/// `orElse`/`isPresent` all key off this same shape.
fn optional_wrap(heap: &mut Heap, value: Option<Value>) -> VMResult<Value> {
    let id = heap.allocate_object(crate::interpreter::heap::ClassRef {
        name: "java/util/Optional".to_string(),
        instance_field_count: 1,
        static_field_count: 0,
    })?;
    heap.set_field(id, 0, value.unwrap_or(Value::Reference(ObjectRef::Null)))?;
    Ok(Value::Reference(ObjectRef::Object(id)))
}

impl Executor {
    /// Dispatch a method call on a stream object. `stream_id` is the heap
    /// collection backing the stream; `args` are the call's arguments
    /// (receiver excluded). Pushes the result for non-void operations.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dispatch_stream_method(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        stream_id: HeapId,
        method_name: &str,
        descriptor: &str,
        args: Vec<Value>,
    ) -> VMResult<()> {
        let items = heap.collection_snapshot(stream_id)?;
        // The functional argument (predicate/function/consumer/collector).
        let sam = |args: &[Value]| -> VMResult<HeapId> {
            match args.first() {
                Some(Value::Reference(ObjectRef::Object(id))) => Ok(*id),
                _ => Err(VMError::internal(format!(
                    "Stream.{method_name} expects an object argument"
                ))),
            }
        };
        // Same extraction as `sam`, but for a single already-selected value —
        // needed by `reduce`, where the functional argument isn't always at
        // index 0 (its position shifts with the identity/combiner arity).
        let sam_id = |method_name: &str, v: &Value| -> VMResult<HeapId> {
            match v {
                Value::Reference(ObjectRef::Object(id)) => Ok(*id),
                _ => Err(VMError::internal(format!(
                    "Stream.{method_name} expects an object argument"
                ))),
            }
        };

        let result: Option<Value> = match method_name {
            "filter" => {
                let f = sam(&args)?;
                let mut kept = Vec::new();
                for it in items {
                    let keep = self
                        .call_sam_sync(thread, heap, monitors, f, vec![it.clone()])?
                        .as_int()
                        .unwrap_or(0)
                        != 0;
                    if keep {
                        kept.push(it);
                    }
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(kept)?,
                )))
            }
            "map" | "mapToObj" | "boxed" => {
                let mapped = match method_name {
                    // boxed(): the elements are already Values; identity here.
                    "boxed" => items,
                    _ => {
                        let f = sam(&args)?;
                        let mut out = Vec::with_capacity(items.len());
                        for it in items {
                            out.push(self.call_sam_sync(thread, heap, monitors, f, vec![it])?);
                        }
                        out
                    }
                };
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(mapped)?,
                )))
            }
            "mapToInt" | "mapToLong" | "mapToDouble" => {
                let f = sam(&args)?;
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    let v = self.call_sam_sync(thread, heap, monitors, f, vec![it])?;
                    out.push(unwrap_numeric(heap, v));
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "sorted" => {
                let comparator = match args.first() {
                    Some(Value::Reference(ObjectRef::Object(id))) => Some(*id),
                    _ => None,
                };
                // Insertion sort (stable), comparisons through the executor —
                // mirrors `sort_list`.
                let mut items = items;
                for i in 1..items.len() {
                    let mut j = i;
                    while j > 0 {
                        let ord = match comparator {
                            Some(c) => self
                                .call_sam_sync(
                                    thread,
                                    heap,
                                    monitors,
                                    c,
                                    vec![items[j - 1].clone(), items[j].clone()],
                                )?
                                .as_int()
                                .unwrap_or(0),
                            None => natural_compare(heap, &items[j - 1], &items[j]),
                        };
                        if ord > 0 {
                            items.swap(j - 1, j);
                            j -= 1;
                        } else {
                            break;
                        }
                    }
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(items)?,
                )))
            }
            "distinct" => {
                let mut seen = std::collections::HashSet::new();
                let mut out = Vec::new();
                for it in items {
                    if seen.insert(heap.render_value(&it)) {
                        out.push(it);
                    }
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "limit" | "skip" => {
                let n = args.first().map(|v| as_i64(heap, v)).unwrap_or(0).max(0) as usize;
                let out: Vec<Value> = if method_name == "limit" {
                    items.into_iter().take(n).collect()
                } else {
                    items.into_iter().skip(n).collect()
                };
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "count" => Some(Value::Long(items.len() as i64)),
            "sum" => Some(match descriptor {
                d if d.ends_with(")J") => Value::Long(
                    items
                        .iter()
                        .fold(0i64, |a, v| a.wrapping_add(as_i64(heap, v))),
                ),
                d if d.ends_with(")D") => {
                    Value::Double(items.iter().map(|v| as_f64(heap, v)).sum())
                }
                _ => Value::Int(
                    items
                        .iter()
                        .fold(0i32, |a, v| a.wrapping_add(as_i64(heap, v) as i32)),
                ),
            }),
            "anyMatch" | "allMatch" | "noneMatch" => {
                let f = sam(&args)?;
                let mut any = false;
                let mut all = true;
                for it in items {
                    let m = self
                        .call_sam_sync(thread, heap, monitors, f, vec![it])?
                        .as_int()
                        .unwrap_or(0)
                        != 0;
                    any |= m;
                    all &= m;
                }
                let out = match method_name {
                    "anyMatch" => any,
                    "allMatch" => all,
                    _ => !any,
                };
                Some(Value::Int(out as i32))
            }
            "forEach" | "forEachOrdered" => {
                let f = sam(&args)?;
                for it in items {
                    self.call_sam_sync(thread, heap, monitors, f, vec![it])?;
                }
                None
            }
            "toList" => {
                let id = heap.allocate_arraylist()?;
                for it in items {
                    heap.arraylist_add(id, it)?;
                }
                Some(Value::Reference(ObjectRef::Object(id)))
            }
            "collect" => {
                let collector = sam(&args)?;
                Some(self.stream_collect(thread, heap, monitors, collector, items)?)
            }
            "peek" => {
                // Side-effecting pass-through: run the consumer over every
                // element (in order) but hand the *original* snapshot on to
                // the next stage, unchanged.
                let f = sam(&args)?;
                for it in &items {
                    self.call_sam_sync(thread, heap, monitors, f, vec![it.clone()])?;
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(items)?,
                )))
            }
            "takeWhile" => {
                let f = sam(&args)?;
                let mut out = Vec::new();
                for it in items {
                    let keep = self
                        .call_sam_sync(thread, heap, monitors, f, vec![it.clone()])?
                        .as_int()
                        .unwrap_or(0)
                        != 0;
                    if !keep {
                        break;
                    }
                    out.push(it);
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "dropWhile" => {
                let f = sam(&args)?;
                let mut out = Vec::new();
                let mut dropping = true;
                for it in items {
                    if dropping {
                        let keep = self
                            .call_sam_sync(thread, heap, monitors, f, vec![it.clone()])?
                            .as_int()
                            .unwrap_or(0)
                            != 0;
                        if keep {
                            continue;
                        }
                        dropping = false;
                    }
                    out.push(it);
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "flatMap" | "flatMapToInt" | "flatMapToLong" | "flatMapToDouble" => {
                // The mapper returns a nested stream per element; flatten by
                // reading each nested stream's own snapshot and concatenating.
                let f = sam(&args)?;
                let mut out = Vec::new();
                for it in items {
                    let mapped = self.call_sam_sync(thread, heap, monitors, f, vec![it])?;
                    if let Value::Reference(ObjectRef::Object(id)) = mapped {
                        if let Ok(sub) = heap.collection_snapshot(id) {
                            out.extend(sub);
                        }
                    }
                }
                Some(Value::Reference(ObjectRef::Object(
                    heap.allocate_stream(out)?,
                )))
            }
            "findFirst" | "findAny" => {
                // Eager model has no laziness to exploit for "any" vs "first",
                // so both return the first surviving element.
                Some(optional_wrap(heap, items.into_iter().next())?)
            }
            "min" | "max" => {
                // Object streams pass an explicit Comparator; primitive streams
                // (IntStream/LongStream/DoubleStream) call with no arguments
                // and rely on natural numeric order.
                let comparator = match args.first() {
                    Some(Value::Reference(ObjectRef::Object(id))) => Some(*id),
                    _ => None,
                };
                let mut best: Option<Value> = None;
                for it in items {
                    best = Some(match best {
                        None => it,
                        Some(cur) => {
                            let ord = match comparator {
                                Some(c) => self
                                    .call_sam_sync(
                                        thread,
                                        heap,
                                        monitors,
                                        c,
                                        vec![cur.clone(), it.clone()],
                                    )?
                                    .as_int()
                                    .unwrap_or(0),
                                None => natural_compare(heap, &cur, &it),
                            };
                            let take_new = if method_name == "min" {
                                ord > 0
                            } else {
                                ord < 0
                            };
                            if take_new {
                                it
                            } else {
                                cur
                            }
                        }
                    });
                }
                Some(optional_wrap(heap, best)?)
            }
            "average" => {
                let avg = if items.is_empty() {
                    None
                } else {
                    let sum: f64 = items.iter().map(|v| as_f64(heap, v)).sum();
                    Some(Value::Double(sum / items.len() as f64))
                };
                Some(optional_wrap(heap, avg)?)
            }
            "reduce" => match args.len() {
                // reduce(BinaryOperator) -> Optional<T> (or OptionalInt/Long/Double)
                1 => {
                    let f = sam_id(method_name, &args[0])?;
                    let mut acc: Option<Value> = None;
                    for it in items {
                        acc = Some(match acc {
                            None => it,
                            Some(a) => {
                                self.call_sam_sync(thread, heap, monitors, f, vec![a, it])?
                            }
                        });
                    }
                    Some(optional_wrap(heap, acc)?)
                }
                // reduce(identity, accumulator) -> T. The 3-arg overload
                // (identity, accumulator, combiner) only matters for
                // parallel streams; sequentially it reduces the same way and
                // the combiner is never invoked.
                2 | 3 => {
                    let identity = args[0].clone();
                    let f = sam_id(method_name, &args[1])?;
                    let mut acc = identity;
                    for it in items {
                        acc = self.call_sam_sync(thread, heap, monitors, f, vec![acc, it])?;
                    }
                    Some(acc)
                }
                n => {
                    return Err(VMError::internal(format!(
                        "Stream.reduce: unexpected arity {n}"
                    )))
                }
            },
            other => {
                return Err(VMError::NoSuchMethod(format!(
                    "java/util/stream/Stream.{other}{descriptor}"
                )))
            }
        };

        if let Some(v) = result {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            frame.push_raw(v);
        }
        Ok(())
    }

    /// Terminal `collect(collector)`: interpret the marker created by the
    /// `Collectors` factory (see `execute_invokestatic`).
    fn stream_collect(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        collector: HeapId,
        items: Vec<Value>,
    ) -> VMResult<Value> {
        let prop = |heap: &Heap, key: &str| -> VMResult<String> {
            match heap.get_dynamic_property(collector, key) {
                Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(id),
                _ => Ok(String::new()),
            }
        };
        let kind = prop(heap, "$collector")?;
        match kind.as_str() {
            "joining" => {
                let (delim, prefix, suffix) = (
                    prop(heap, "$delimiter")?,
                    prop(heap, "$prefix")?,
                    prop(heap, "$suffix")?,
                );
                let parts: Vec<String> = items.iter().map(|v| heap.render_value(v)).collect();
                let joined = format!("{prefix}{}{suffix}", parts.join(&delim));
                Ok(Value::Reference(heap.intern_string(&joined)?))
            }
            "toList" => {
                let id = heap.allocate_arraylist()?;
                for it in items {
                    heap.arraylist_add(id, it)?;
                }
                Ok(Value::Reference(ObjectRef::Object(id)))
            }
            "toSet" => {
                // Elements are keyed as a HashSet keys them: equal user objects are one element
                let id = heap.allocate_hashset()?;
                for it in items {
                    let key = self.canonical_container_key(thread, heap, monitors, id, it, true)?;
                    heap.hashset_add(id, key)?;
                }
                Ok(Value::Reference(ObjectRef::Object(id)))
            }
            other => Err(VMError::internal(format!(
                "unsupported Collector: Collectors.{other}"
            ))),
        }
    }
}
