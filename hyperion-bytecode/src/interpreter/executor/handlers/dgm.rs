// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Default Groovy Methods (DGM) — collection/iterable extension methods.
//!
//! Groovy compiles calls like `list.each { ... }`, `list.collect { ... }`,
//! `list.sum()` to `invokedynamic` sites that, when no instance method matches,
//! resolve through the runtime MOP to `DefaultGroovyMethods.<m>(Collection, …)`.
//! Those statics have no bytecode body, so we intercept the common ones here and
//! implement them natively against the heap.
//!
//! Closure-taking methods (`each`/`collect`/`find`/…) drive the executor
//! re-entrantly via [`Executor::run_closure_to_completion`]: the closure frame is
//! pushed and the step loop is run until it returns, yielding the closure's value
//! inline so we can fold it into the result.

use super::super::{Executor, NestedExecutionState};
use super::groovy_truth::is_truthy as is_groovy_truthy;
use crate::interpreter::closure_runtime::ClosureRuntime;
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::Heap;
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};
use crate::interpreter::HeapId;

/// Materialise the elements of a Groovy-iterable receiver as a snapshot vector.
///
/// Returns `None` when the receiver is not a kind we treat as a DGM collection,
/// signalling the caller to fall through to ordinary MOP resolution.
fn dgm_elements(heap: &Heap, class_name: &str, heap_id: HeapId) -> Option<Vec<Value>> {
    match class_name {
        "java/util/ArrayList"
        | "java/util/LinkedList"
        | "java/util/HashSet"
        | "java/util/LinkedHashSet"
        | "java/util/TreeSet" => heap.collection_snapshot(heap_id).ok(),
        "groovy/lang/IntRange" => int_range_elements(heap, heap_id),
        _ => heap.collection_snapshot(heap_id).ok(),
    }
}

/// Expand an `IntRange` object (fields: 0=from, 1=to, 2=inclusive) into its
/// integer elements, honouring reverse ranges (`5..1`).
fn int_range_elements(heap: &Heap, heap_id: HeapId) -> Option<Vec<Value>> {
    let from = unbox_i64(heap, &heap.get_field(heap_id, 0).ok()??)?;
    let to = unbox_i64(heap, &heap.get_field(heap_id, 1).ok()??)?;
    let inclusive = match heap.get_field(heap_id, 2) {
        Ok(Some(v)) => unbox_i64(heap, &v).unwrap_or(1) != 0,
        _ => true,
    };
    let mut out = Vec::new();
    if from <= to {
        let end = if inclusive { to } else { to - 1 };
        for i in from..=end {
            out.push(Value::Int(i as i32));
        }
    } else {
        let end = if inclusive { to } else { to + 1 };
        let mut i = from;
        while i >= end {
            out.push(Value::Int(i as i32));
            i -= 1;
        }
    }
    Some(out)
}

/// Unwrap a numeric `Value` (primitive or boxed wrapper) to `i64`.
fn unbox_i64(heap: &Heap, value: &Value) -> Option<i64> {
    match value {
        Value::Int(i) => Some(i64::from(*i)),
        Value::Long(l) => Some(*l),
        Value::Reference(ObjectRef::Object(id)) => {
            if let Ok(i) = heap.get_boxed_int(*id) {
                return Some(i64::from(i));
            }
            heap.get_boxed_long(*id).ok()
        }
        _ => None,
    }
}

/// Unwrap a numeric `Value` (primitive or boxed wrapper) to `f64`.
fn unbox_f64(heap: &Heap, value: &Value) -> Option<f64> {
    match value {
        Value::Int(i) => Some(f64::from(*i)),
        Value::Long(l) => Some(*l as f64),
        Value::Float(f) => Some(f64::from(*f)),
        Value::Double(d) => Some(*d),
        Value::Reference(ObjectRef::Object(id)) => {
            let class = heap.get_object_class_name(*id).ok()?;
            match class.as_str() {
                "java/lang/Integer"
                | "java/lang/Short"
                | "java/lang/Byte"
                | "java/lang/Character" => heap.get_boxed_int(*id).ok().map(f64::from),
                "java/lang/Long" => heap.get_boxed_long(*id).ok().map(|v| v as f64),
                "java/lang/Double" | "java/lang/Float" => match heap.get_field(*id, 0) {
                    Ok(Some(Value::Double(d))) => Some(d),
                    Ok(Some(Value::Float(f))) => Some(f64::from(f)),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// Box a numeric sum result the way Groovy would: integral sums stay `Integer`
/// (or `Long` when out of `int` range); any floating element promotes to `Double`.
fn box_number(heap: &mut Heap, is_float: bool, int_sum: i64, float_sum: f64) -> VMResult<Value> {
    if is_float {
        let id = heap.allocate_double(float_sum)?;
        Ok(Value::Reference(ObjectRef::Object(id)))
    } else if i32::try_from(int_sum).is_ok() {
        let id = heap.allocate_integer(int_sum as i32)?;
        Ok(Value::Reference(ObjectRef::Object(id)))
    } else {
        let id = heap.allocate_long(int_sum)?;
        Ok(Value::Reference(ObjectRef::Object(id)))
    }
}

/// Box a primitive `Value` so it can be passed to a closure's `doCall`, whose
/// parameters are `Object` references loaded with `aload`. References (already
/// boxed, including `null`) pass through unchanged.
fn box_for_closure(heap: &mut Heap, value: Value) -> VMResult<Value> {
    let id = match value {
        Value::Int(i) => heap.allocate_integer(i)?,
        Value::Long(l) => heap.allocate_long(l)?,
        Value::Float(f) => heap.allocate_float(f)?,
        Value::Double(d) => heap.allocate_double(d)?,
        other => return Ok(other),
    };
    Ok(Value::Reference(ObjectRef::Object(id)))
}

/// Detect a closure argument and return its heap id.
fn closure_id_of(heap: &Heap, value: &Value) -> Option<HeapId> {
    if let Value::Reference(ObjectRef::Object(id)) = value {
        let class = heap.get_object_class_name(*id).ok()?;
        if class.contains("$_closure_")
            || class.contains("$_nested_")
            || class == "groovy/lang/Closure"
        {
            return Some(*id);
        }
    }
    None
}

impl Executor {
    /// Push a closure's `doCall` frame and run the step loop until it returns,
    /// yielding the closure's (boxed) result value inline.
    ///
    /// This makes native code able to call Groovy closures synchronously, which
    /// is what the iterating DGM methods need. The executor's `pc`/`bytecode` are
    /// saved and restored around the nested run so the calling `invokedynamic`
    /// frame resumes at its continuation.
    pub(crate) fn run_closure_to_completion(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        closure_id: HeapId,
        args: Vec<Value>,
    ) -> VMResult<Value> {
        let state = NestedExecutionState::capture(self, thread, "DGM closure");
        if let Err(error) =
            ClosureRuntime::invoke_do_call(heap, &self.method_registry, thread, closure_id, args)
        {
            thread.unwind_after_failure(state.target_depth(), monitors);
            state.restore(self);
            return Err(error);
        }

        // GC is intentionally not run here: in-progress native result
        // collections are held only in Rust locals and would not be roots.
        self.finish_nested_execution(thread, heap, monitors, state)?;
        Ok(match thread.current_frame_mut() {
            Some(frame) => frame.pop_raw().unwrap_or(Value::Reference(ObjectRef::Null)),
            None => Value::Reference(ObjectRef::Null),
        })
    }

    /// Try to handle an `invokedynamic` virtual call as a Default Groovy Method.
    ///
    /// Returns `Ok(Some(()))` when handled (result already pushed onto the current
    /// frame), `Ok(None)` to fall through to ordinary MOP resolution.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_dgm_dispatch(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        class_name: &str,
        method_name: &str,
        receiver: ObjectRef,
        heap_id: HeapId,
        args: &[Value],
    ) -> VMResult<Option<()>> {
        // Only intercept methods we know; otherwise let MOP resolution proceed so
        // user-defined methods that happen to share a name still win.
        let is_known = matches!(
            method_name,
            "each"
                | "collect"
                | "find"
                | "findAll"
                | "any"
                | "every"
                | "sum"
                | "max"
                | "min"
                | "join"
                | "first"
                | "last"
                | "reverse"
                | "sort"
                | "unique"
                | "contains"
                | "take"
                | "drop"
                | "indexOf"
                | "flatten"
                | "count"
                | "inject"
                | "collectMany"
        );
        if !is_known {
            return Ok(None);
        }

        let Some(elements) = dgm_elements(heap, class_name, heap_id) else {
            return Ok(None);
        };

        let result: Value = match method_name {
            "each" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                for el in elements {
                    let arg = box_for_closure(heap, el)?;
                    self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                }
                Value::Reference(receiver)
            }
            "collect" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let out = heap.allocate_arraylist()?;
                for el in elements {
                    let arg = box_for_closure(heap, el)?;
                    let mapped =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    heap.arraylist_add(out, mapped)?;
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "findAll" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let out = heap.allocate_arraylist()?;
                for el in elements {
                    let arg = box_for_closure(heap, el.clone())?;
                    let keep =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    if is_groovy_truthy(heap, &keep)? {
                        heap.arraylist_add(out, el)?;
                    }
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "find" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let mut found = Value::Reference(ObjectRef::Null);
                for el in elements {
                    let arg = box_for_closure(heap, el.clone())?;
                    let matches =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    if is_groovy_truthy(heap, &matches)? {
                        found = el;
                        break;
                    }
                }
                found
            }
            "any" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let mut any = false;
                for el in elements {
                    let arg = box_for_closure(heap, el)?;
                    let r =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    if is_groovy_truthy(heap, &r)? {
                        any = true;
                        break;
                    }
                }
                let id = heap.allocate_boolean(any)?;
                Value::Reference(ObjectRef::Object(id))
            }
            "every" => {
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let mut every = true;
                for el in elements {
                    let arg = box_for_closure(heap, el)?;
                    let r =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    if !is_groovy_truthy(heap, &r)? {
                        every = false;
                        break;
                    }
                }
                let id = heap.allocate_boolean(every)?;
                Value::Reference(ObjectRef::Object(id))
            }
            // ── Aggregates ─────────────────────────────────────────────────
            "sum" => {
                // `sum { … }` maps each element through the closure before adding.
                let mapped = match args.first().and_then(|a| closure_id_of(heap, a)) {
                    Some(cid) => {
                        let mut out = Vec::with_capacity(elements.len());
                        for el in elements {
                            let arg = box_for_closure(heap, el)?;
                            out.push(self.run_closure_to_completion(
                                thread,
                                heap,
                                monitors,
                                cid,
                                vec![arg],
                            )?);
                        }
                        out
                    }
                    None => elements,
                };
                match sum_values(heap, &mapped)? {
                    Some(v) => v,
                    None => return Ok(None),
                }
            }
            "max" | "min" => {
                let want_max = method_name == "max";
                if elements.is_empty() {
                    Value::Reference(ObjectRef::Null)
                } else if let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) {
                    // `max { key }` / `min { key }`: pick the element whose closure
                    // result is numerically extreme.
                    let mut best = elements[0].clone();
                    let arg = box_for_closure(heap, best.clone())?;
                    let mut best_key =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    for el in &elements[1..] {
                        let arg = box_for_closure(heap, el.clone())?;
                        let key =
                            self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                        let (k, bk) = (unbox_f64(heap, &key), unbox_f64(heap, &best_key));
                        if let (Some(k), Some(bk)) = (k, bk) {
                            if (want_max && k > bk) || (!want_max && k < bk) {
                                best = el.clone();
                                best_key = key;
                            }
                        }
                    }
                    best
                } else if let Some(best) = extremum(heap, &elements, want_max) {
                    best
                } else {
                    return Ok(None);
                }
            }
            "count" => {
                // `count { pred }` counts matches; `count(x)` counts equal elements.
                let n = if let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) {
                    let mut n = 0i32;
                    for el in elements {
                        let arg = box_for_closure(heap, el)?;
                        let r =
                            self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                        if is_groovy_truthy(heap, &r)? {
                            n += 1;
                        }
                    }
                    n
                } else if let Some(target) = args.first() {
                    elements
                        .iter()
                        .filter(|el| values_equal(heap, el, target))
                        .count() as i32
                } else {
                    return Ok(None);
                };
                let id = heap.allocate_integer(n)?;
                Value::Reference(ObjectRef::Object(id))
            }
            "inject" => {
                // `inject(initial) { acc, el -> … }` — left fold.
                let Some(cid) = args.get(1).and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let mut acc = args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Reference(ObjectRef::Null));
                for el in elements {
                    // Both args reach the closure as `Object`s (loaded via aload),
                    // so re-box the accumulator each round — after the first call
                    // it is a raw primitive returned by the closure body.
                    let acc_arg = box_for_closure(heap, acc.clone())?;
                    let el_arg = box_for_closure(heap, el)?;
                    acc = self.run_closure_to_completion(
                        thread,
                        heap,
                        monitors,
                        cid,
                        vec![acc_arg, el_arg],
                    )?;
                }
                acc
            }
            "collectMany" => {
                // flatMap: the closure returns a collection per element.
                let Some(cid) = args.first().and_then(|a| closure_id_of(heap, a)) else {
                    return Ok(None);
                };
                let out = heap.allocate_arraylist()?;
                for el in elements {
                    let arg = box_for_closure(heap, el)?;
                    let sub =
                        self.run_closure_to_completion(thread, heap, monitors, cid, vec![arg])?;
                    if let Value::Reference(ObjectRef::Object(id)) = sub {
                        if let Ok(items) = heap.collection_snapshot(id) {
                            for it in items {
                                heap.arraylist_add(out, it)?;
                            }
                            continue;
                        }
                    }
                    heap.arraylist_add(out, sub)?;
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "sort" => {
                let mut sorted = elements;
                sort_values(heap, &mut sorted);
                let out = heap.allocate_arraylist()?;
                for el in sorted {
                    heap.arraylist_add(out, el)?;
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "unique" => {
                let out = heap.allocate_arraylist()?;
                let mut seen: Vec<Value> = Vec::new();
                for el in elements {
                    if !seen.iter().any(|s| values_equal(heap, s, &el)) {
                        seen.push(el.clone());
                        heap.arraylist_add(out, el)?;
                    }
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "contains" => {
                let Some(target) = args.first() else {
                    return Ok(None);
                };
                let found = elements.iter().any(|el| values_equal(heap, el, target));
                let id = heap.allocate_boolean(found)?;
                Value::Reference(ObjectRef::Object(id))
            }
            "indexOf" => {
                let Some(target) = args.first() else {
                    return Ok(None);
                };
                let idx = elements
                    .iter()
                    .position(|el| values_equal(heap, el, target))
                    .map_or(-1, |i| i as i32);
                let id = heap.allocate_integer(idx)?;
                Value::Reference(ObjectRef::Object(id))
            }
            "take" | "drop" => {
                let Some(n) = args.first().and_then(|a| unbox_i64(heap, a)) else {
                    return Ok(None);
                };
                let n = n.max(0) as usize;
                let out = heap.allocate_arraylist()?;
                let slice: Vec<Value> = if method_name == "take" {
                    elements.into_iter().take(n).collect()
                } else {
                    elements.into_iter().skip(n).collect()
                };
                for el in slice {
                    heap.arraylist_add(out, el)?;
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "flatten" => {
                let out = heap.allocate_arraylist()?;
                flatten_into(heap, &elements, out)?;
                Value::Reference(ObjectRef::Object(out))
            }
            "first" => elements
                .first()
                .cloned()
                .unwrap_or(Value::Reference(ObjectRef::Null)),
            "last" => elements
                .last()
                .cloned()
                .unwrap_or(Value::Reference(ObjectRef::Null)),
            "reverse" => {
                let out = heap.allocate_arraylist()?;
                for el in elements.into_iter().rev() {
                    heap.arraylist_add(out, el)?;
                }
                Value::Reference(ObjectRef::Object(out))
            }
            "join" => {
                let sep = match args.first() {
                    Some(v) => heap.render_value(v),
                    None => String::new(),
                };
                let joined = elements
                    .iter()
                    .map(|v| heap.render_value(v))
                    .collect::<Vec<_>>()
                    .join(&sep);
                Value::Reference(heap.intern_string(&joined)?)
            }
            _ => return Ok(None),
        };

        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
        frame.push_raw(result);
        Ok(Some(()))
    }
}

/// Sum a slice of values the way Groovy would: integral sums stay `Integer`/
/// `Long`, any floating element promotes to `Double`. Returns `None` when an
/// element is not numeric (signals the caller to fall through to MOP).
fn sum_values(heap: &mut Heap, elements: &[Value]) -> VMResult<Option<Value>> {
    let mut int_sum: i64 = 0;
    let mut float_sum: f64 = 0.0;
    let mut is_float = false;
    for el in elements {
        let Some(n) = unbox_f64(heap, el) else {
            return Ok(None);
        };
        if matches!(el, Value::Float(_) | Value::Double(_)) || unbox_i64(heap, el).is_none() {
            is_float = true;
        }
        float_sum += n;
        if let Some(i) = unbox_i64(heap, el) {
            int_sum += i;
        }
    }
    Ok(Some(box_number(heap, is_float, int_sum, float_sum)?))
}

/// Value equality for DGM membership ops (`contains`/`indexOf`/`unique`/`count`).
/// Numbers compare by value (across boxing); everything else by rendered form.
fn values_equal(heap: &Heap, a: &Value, b: &Value) -> bool {
    if let (Some(x), Some(y)) = (unbox_f64(heap, a), unbox_f64(heap, b)) {
        return (x - y).abs() < f64::EPSILON;
    }
    heap.render_value(a) == heap.render_value(b)
}

/// Sort in place: numerically when every element is numeric, else by rendered
/// form (matches the common `[3,1,2].sort()` and `["b","a"].sort()` cases).
fn sort_values(heap: &Heap, elements: &mut [Value]) {
    let all_numeric = elements.iter().all(|v| unbox_f64(heap, v).is_some());
    if all_numeric {
        elements.sort_by(|a, b| {
            let (x, y) = (
                unbox_f64(heap, a).unwrap_or(0.0),
                unbox_f64(heap, b).unwrap_or(0.0),
            );
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        elements.sort_by_key(|a| heap.render_value(a));
    }
}

/// Recursively flatten nested collections into `out` (Groovy `flatten()`).
fn flatten_into(heap: &mut Heap, elements: &[Value], out: HeapId) -> VMResult<()> {
    for el in elements {
        if let Value::Reference(ObjectRef::Object(id)) = el {
            if let Ok(items) = heap.collection_snapshot(*id) {
                flatten_into(heap, &items, out)?;
                continue;
            }
        }
        heap.arraylist_add(out, el.clone())?;
    }
    Ok(())
}

/// Pick the numerically (or, failing that, lexically) extreme element.
fn extremum(heap: &Heap, elements: &[Value], want_max: bool) -> Option<Value> {
    let all_numeric = elements.iter().all(|v| unbox_f64(heap, v).is_some());
    if all_numeric {
        let mut best = elements[0].clone();
        let mut best_key = unbox_f64(heap, &best)?;
        for el in &elements[1..] {
            let key = unbox_f64(heap, el)?;
            if (want_max && key > best_key) || (!want_max && key < best_key) {
                best = el.clone();
                best_key = key;
            }
        }
        Some(best)
    } else {
        let mut best = elements[0].clone();
        let mut best_key = heap.render_value(&best);
        for el in &elements[1..] {
            let key = heap.render_value(el);
            if (want_max && key > best_key) || (!want_max && key < best_key) {
                best = el.clone();
                best_key = key;
            }
        }
        Some(best)
    }
}
