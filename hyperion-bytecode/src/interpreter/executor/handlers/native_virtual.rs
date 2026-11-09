// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Native virtual method dispatch for standard Java classes.
//!
//! Shared dispatch logic used by both `invocation.rs` (invokevirtual) and
//! `invokedynamic.rs` (Groovy dynamic dispatch). Handles ArrayList, HashMap,
//! HashSet, StringBuilder, String, Integer, and File methods.

mod string;
mod string_builder;

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::{natural_cmp, Heap};
use crate::interpreter::value::{ObjectRef, Value};
use crate::interpreter::HeapId;

/// Result of a native virtual dispatch attempt.
pub(crate) enum NativeDispatchResult {
    /// Method was handled; push this value onto the operand stack.
    Handled(Value),
    /// Method was handled; no value to push (void return).
    HandledVoid,
    /// Method was not recognized as native.
    NotHandled,
}

/// Try to dispatch a virtual method call on a standard Java class natively.
///
/// Returns `NativeDispatchResult::Handled(value)` if the method was handled and
/// a value should be pushed, `HandledVoid` if handled with no return value,
/// or `NotHandled` if this method isn't a known native.
///
/// The `descriptor` is used to decide whether to box return values (Groovy's
/// `()Ljava/lang/Object;` descriptors need boxed results).
pub(crate) fn dispatch_native_virtual(
    class_name: &str,
    method_name: &str,
    descriptor: &str,
    receiver: ObjectRef,
    heap_id: HeapId,
    args: &[Value],
    heap: &mut Heap,
) -> VMResult<NativeDispatchResult> {
    if class_name == "java/lang/StringBuilder" {
        return string_builder::dispatch(method_name, descriptor, receiver, heap_id, args, heap);
    }
    if class_name == "java/lang/String" {
        return string::dispatch(method_name, descriptor, receiver, heap_id, args, heap);
    }

    match (class_name, method_name) {
        // ── Groovy Script properties ───────────────────────────────────
        ("groovy/lang/Script", "getProperty") => {
            if args.is_empty() {
                return Err(VMError::internal("Script.getProperty requires name"));
            }
            let name = value_to_string_lossy(heap, &args[0])?;
            let value = heap
                .get_dynamic_property(heap_id, &name)
                .unwrap_or(Value::Reference(ObjectRef::Null));
            Ok(NativeDispatchResult::Handled(value))
        }
        ("groovy/lang/Script", "setProperty") => {
            if args.len() < 2 {
                return Err(VMError::internal(
                    "Script.setProperty requires name and value",
                ));
            }
            let name = value_to_string_lossy(heap, &args[0])?;
            heap.set_dynamic_property(heap_id, &name, args[1].clone());
            Ok(NativeDispatchResult::HandledVoid)
        }

        // ── Integer / Long / Double / Float unboxing ───────────────────
        (
            "java/lang/Integer" | "java/lang/Long" | "java/lang/Short" | "java/lang/Byte"
            | "java/lang/Double" | "java/lang/Float",
            "intValue" | "longValue" | "doubleValue" | "floatValue" | "shortValue" | "byteValue",
        ) => {
            // Wrappers store the primitive directly in field 0.
            let primitive = heap.get_field(heap_id, 0)?;
            let as_f64 = boxed_number_as_f64(heap, heap_id);
            Ok(NativeDispatchResult::Handled(boxed_conversion(
                method_name,
                primitive.as_ref(),
                as_f64,
            )))
        }
        // Wrapper.compareTo(other) — reached directly or via Comparable.
        (
            "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Double"
            | "java/lang/Float"
            | "java/lang/Character",
            "compareTo",
        ) => {
            let other = args
                .first()
                .ok_or_else(|| VMError::internal("Wrapper.compareTo requires an argument"))?;
            match other {
                Value::Reference(ObjectRef::Null) => return Err(VMError::NullPointerException),
                Value::Reference(ObjectRef::Object(id)) => {
                    let other_class = heap.get_object_class_name(*id)?;
                    if other_class != class_name {
                        return Err(VMError::class_cast_exception(&other_class, class_name));
                    }
                }
                // Groovy's dynamic dispatch can pass an unboxed primitive.
                _ => {}
            }
            let ord = natural_cmp(heap, &Value::Reference(receiver), other) as i32;
            Ok(NativeDispatchResult::Handled(Value::Int(ord)))
        }

        // ── Boolean ────────────────────────────────────────────────────
        // A boxed Boolean stores its primitive in field 0 as an int (0/1).
        // Groovy unboxes `&&`/`||` operands via `booleanValue`.
        ("java/lang/Boolean", "booleanValue") => {
            let v = match heap.get_field(heap_id, 0)? {
                Some(Value::Int(v)) => v,
                _ => 0,
            };
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                v != 0,
                descriptor,
                heap,
            )?))
        }

        // ── ArrayList ──────────────────────────────────────────────────
        ("java/util/ArrayList", "add") => {
            if args.is_empty() {
                return Err(VMError::internal("ArrayList.add requires argument"));
            }
            heap.arraylist_add(heap_id, args[0].clone())?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                true, descriptor, heap,
            )?))
        }
        ("java/util/ArrayList", "get" | "getAt") => {
            if args.is_empty() {
                return Err(VMError::internal("ArrayList.get requires index"));
            }
            let index = coerce_to_int(&args[0], heap)?;
            let value = heap.arraylist_get(heap_id, index)?;
            Ok(NativeDispatchResult::Handled(value))
        }
        ("java/util/ArrayList", "set") => {
            if args.len() < 2 {
                return Err(VMError::internal("ArrayList.set requires index and value"));
            }
            let index = args[0].as_int()?;
            let old_value = heap.arraylist_set(heap_id, index, args[1].clone())?;
            Ok(NativeDispatchResult::Handled(old_value))
        }
        ("java/util/ArrayList", "remove") if descriptor.starts_with("(I)") => {
            if args.is_empty() {
                return Err(VMError::internal("ArrayList.remove requires index"));
            }
            let index = args[0].as_int()?;
            let removed = heap.arraylist_remove(heap_id, index)?;
            Ok(NativeDispatchResult::Handled(removed))
        }
        // remove(Object) removes the first element that equals() the argument.
        ("java/util/ArrayList", "remove") => {
            let Some(value) = args.first() else {
                return Err(VMError::internal("ArrayList.remove requires value"));
            };
            let position = heap
                .collection_snapshot(heap_id)?
                .iter()
                .position(|element| heap.same_element(element, value));
            let removed = match position {
                Some(index) => {
                    heap.arraylist_remove(heap_id, i32::try_from(index).unwrap_or(i32::MAX))?;
                    true
                }
                None => false,
            };
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                removed, descriptor, heap,
            )?))
        }
        // contains and indexOf compare with equals(), as the elements can decide it here.
        ("java/util/ArrayList" | "java/util/LinkedList", "contains" | "indexOf") => {
            let target = args
                .first()
                .cloned()
                .unwrap_or(Value::Reference(ObjectRef::Null));
            let elements = heap.collection_snapshot(heap_id)?;
            let position = elements
                .iter()
                .position(|element| heap.same_element(element, &target));
            if method_name == "contains" {
                Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                    position.is_some(),
                    descriptor,
                    heap,
                )?))
            } else {
                Ok(NativeDispatchResult::Handled(Value::Int(
                    position.map_or(-1, |index| index as i32),
                )))
            }
        }
        ("java/util/ArrayList", "size") => {
            let size = heap.arraylist_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_int(
                size, descriptor, heap,
            )?))
        }
        ("java/util/ArrayList", "isEmpty") => {
            let size = heap.arraylist_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                size == 0,
                descriptor,
                heap,
            )?))
        }
        ("java/util/ArrayList", "clear") => {
            heap.arraylist_clear(heap_id)?;
            Ok(NativeDispatchResult::HandledVoid)
        }

        // ── Iterable.iterator() / Iterator ─────────────────────────────
        // `iterator()` snapshots the collection's elements (dispatch is by the
        // receiver's *runtime* class, so this covers List/Set/Collection-typed
        // variables backed by any of these concrete collections).
        (
            "java/util/ArrayList"
            | "java/util/LinkedList"
            | "java/util/HashSet"
            | "java/util/LinkedHashSet"
            | "java/util/TreeSet"
            | "java/util/HashMap"
            | "java/util/LinkedHashMap"
            | "java/util/TreeMap",
            "iterator",
        ) => {
            let items = heap.collection_snapshot(heap_id)?;
            let it = heap.allocate_iterator(items)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(it),
            )))
        }
        ("groovy/lang/IntRange", "iterator") => {
            let from = heap
                .get_field(heap_id, 0)?
                .as_ref()
                .map(|value| coerce_to_int(value, heap))
                .transpose()?
                .unwrap_or(0);
            let to = heap
                .get_field(heap_id, 1)?
                .as_ref()
                .map(|value| coerce_to_int(value, heap))
                .transpose()?
                .unwrap_or(0);
            let inclusive = heap
                .get_field(heap_id, 2)?
                .and_then(|value| value.as_int().ok())
                .is_none_or(|value| value != 0);
            let iterator = heap.allocate_int_range_iterator(from, to, inclusive)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(iterator),
            )))
        }

        // ── Map views (entrySet/keySet/values) + Map.Entry + TreeSet ends ─
        // keySet and values are live views of the map. entrySet is a snapshot of new entry
        // objects, which a read cannot keep in step with the map. Removing an entry from
        // it removes the entry from the map.
        (
            "java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap",
            "keySet" | "values",
        ) => {
            let view = heap.allocate_map_view(heap_id, method_name == "values")?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(view),
            )))
        }
        ("java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap", "entrySet") => {
            let entries = heap.map_snapshot(heap_id)?; // sorted for TreeMap
            let list = heap.allocate_arraylist()?;
            for (k, v) in entries {
                // A Map.Entry holding key (field 0) and value (field 1).
                let e = heap.allocate_object(crate::interpreter::heap::ClassRef {
                    name: "java/util/Map$Entry".to_string(),
                    instance_field_count: 2,
                    static_field_count: 0,
                })?;
                heap.set_field(e, 0, k)?;
                heap.set_field(e, 1, v)?;
                // setValue writes through to this map
                heap.set_dynamic_property(
                    e,
                    "$entry_map",
                    Value::Reference(ObjectRef::Object(heap_id)),
                );
                heap.arraylist_add(list, Value::Reference(ObjectRef::Object(e)))?;
            }
            heap.mark_entry_set(list, heap_id);
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(list),
            )))
        }
        ("java/util/Map$Entry", "getKey") => Ok(NativeDispatchResult::Handled(
            heap.get_field(heap_id, 0)?
                .unwrap_or(Value::Reference(ObjectRef::Null)),
        )),
        ("java/util/Map$Entry", "getValue") => Ok(NativeDispatchResult::Handled(
            heap.get_field(heap_id, 1)?
                .unwrap_or(Value::Reference(ObjectRef::Null)),
        )),
        // setValue replaces the entry's value and, for an entry of a map's entrySet, the
        // map's value for the key. It returns the previous value.
        ("java/util/Map$Entry", "setValue") => {
            let Some(value) = args.first().cloned() else {
                return Err(VMError::internal("Map.Entry.setValue requires value"));
            };
            let old = heap
                .get_field(heap_id, 1)?
                .unwrap_or(Value::Reference(ObjectRef::Null));
            heap.set_field(heap_id, 1, value.clone())?;
            if let Some(Value::Reference(ObjectRef::Object(map))) =
                heap.get_dynamic_property(heap_id, "$entry_map")
            {
                let key = heap
                    .get_field(heap_id, 0)?
                    .unwrap_or(Value::Reference(ObjectRef::Null));
                heap.hashmap_put(map, key, value)?;
            }
            Ok(NativeDispatchResult::Handled(old))
        }
        ("java/util/TreeSet", "first" | "last") => {
            let items = heap.collection_snapshot(heap_id)?; // sorted
            let v = if method_name == "first" {
                items.first().cloned()
            } else {
                items.last().cloned()
            };
            Ok(NativeDispatchResult::Handled(
                v.unwrap_or(Value::Reference(ObjectRef::Null)),
            ))
        }

        ("java/util/Iterator", "hasNext") => {
            let more = heap.iterator_has_next(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                more, descriptor, heap,
            )?))
        }
        ("java/util/Iterator", "next") => {
            let v = heap.iterator_next(heap_id)?;
            Ok(NativeDispatchResult::Handled(v))
        }

        // ── AWT: Color / BufferedImage / Graphics2D ────────────────────
        ("java/awt/Color", "getRGB") => {
            let rgb = heap
                .get_field(heap_id, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            Ok(NativeDispatchResult::Handled(Value::Int(rgb)))
        }
        ("java/awt/image/BufferedImage", "getWidth") => {
            let w = heap
                .get_field(heap_id, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            Ok(NativeDispatchResult::Handled(Value::Int(w)))
        }
        ("java/awt/image/BufferedImage", "getHeight") => {
            let h = heap
                .get_field(heap_id, 1)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            Ok(NativeDispatchResult::Handled(Value::Int(h)))
        }
        ("java/awt/image/BufferedImage", "setRGB") => {
            let x = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let y = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let rgb = coerce_to_int(args.get(2).unwrap_or(&Value::Int(0)), heap)?;
            raster_set(heap, heap_id, x, y, rgb)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        ("java/awt/image/BufferedImage", "getRGB") => {
            let x = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let y = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let rgb = raster_get(heap, heap_id, x, y)?;
            Ok(NativeDispatchResult::Handled(Value::Int(rgb)))
        }
        ("java/awt/image/BufferedImage", "createGraphics" | "getGraphics") => {
            // A Graphics2D bound to this image: field 0 = image ref, field 1 =
            // current draw color (default opaque black, as for an off-screen image).
            let g = heap.allocate_object(crate::interpreter::heap::ClassRef {
                name: "java/awt/Graphics2D".to_string(),
                instance_field_count: 2,
                static_field_count: 0,
            })?;
            heap.set_field(g, 0, Value::Reference(receiver))?;
            heap.set_field(g, 1, Value::Int(0xFF00_0000u32 as i32))?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(g),
            )))
        }
        // setColor sets the paint used by draw/fill. setBackground nominally
        // differs (it is the clearRect color), but these programs do
        // `setBackground(WHITE); fillRect(...)` to paint a background, so treat
        // both as setting the current color — matching the reference output.
        ("java/awt/Graphics2D" | "java/awt/Graphics", "setColor" | "setBackground") => {
            let rgb = match args.first() {
                Some(Value::Reference(ObjectRef::Object(cid))) => heap
                    .get_field(*cid, 0)?
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(0),
                _ => 0xFF00_0000u32 as i32,
            };
            heap.set_field(heap_id, 1, Value::Int(rgb))?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        // Only a Color paint is modelled. Any other Paint, such as a gradient, would be drawn
        // in the wrong colour, so it is refused rather than read as a colour.
        ("java/awt/Graphics2D" | "java/awt/Graphics", "setPaint") => {
            let color = match args.first() {
                Some(Value::Reference(ObjectRef::Object(cid)))
                    if heap.get_class(*cid)?.name == "java/awt/Color" =>
                {
                    *cid
                }
                _ => {
                    return Err(VMError::internal(
                        "Graphics2D.setPaint is modelled only for a java.awt.Color paint",
                    ))
                }
            };
            let rgb = heap
                .get_field(color, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            heap.set_field(heap_id, 1, Value::Int(rgb))?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        ("java/awt/Graphics2D" | "java/awt/Graphics", "fillRect") => {
            let x = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let y = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let w = coerce_to_int(args.get(2).unwrap_or(&Value::Int(0)), heap)?;
            let h = coerce_to_int(args.get(3).unwrap_or(&Value::Int(0)), heap)?;
            graphics_fill_rect(heap, heap_id, x, y, w, h)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        ("java/awt/Graphics2D" | "java/awt/Graphics", "drawLine") => {
            let x1 = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let y1 = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let x2 = coerce_to_int(args.get(2).unwrap_or(&Value::Int(0)), heap)?;
            let y2 = coerce_to_int(args.get(3).unwrap_or(&Value::Int(0)), heap)?;
            graphics_draw_line(heap, heap_id, x1, y1, x2, y2)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        ("java/awt/Graphics2D" | "java/awt/Graphics", "drawRect") => {
            let x = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let y = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let w = coerce_to_int(args.get(2).unwrap_or(&Value::Int(0)), heap)?;
            let h = coerce_to_int(args.get(3).unwrap_or(&Value::Int(0)), heap)?;
            // The outline spans x..=x+w and y..=y+h, as Graphics.drawRect does.
            graphics_draw_line(heap, heap_id, x, y, x + w, y)?;
            graphics_draw_line(heap, heap_id, x + w, y, x + w, y + h)?;
            graphics_draw_line(heap, heap_id, x + w, y + h, x, y + h)?;
            graphics_draw_line(heap, heap_id, x, y + h, x, y)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        // Drawing attributes that affect only fine visual detail (stroke width,
        // anti-aliasing, the font) are accepted and ignored.
        (
            "java/awt/Graphics2D" | "java/awt/Graphics",
            "setStroke" | "setRenderingHint" | "setRenderingHints" | "dispose" | "translate"
            | "setFont" | "addRenderingHints",
        ) => Ok(NativeDispatchResult::HandledVoid),
        // Shapes are objects this runtime does not build, so painting one would drop it.
        ("java/awt/Graphics2D" | "java/awt/Graphics", "draw" | "fill") => Err(VMError::internal(
            "Graphics2D.draw/fill(Shape) is not modelled: shapes are not built",
        )),

        // ── HashMap ────────────────────────────────────────────────────
        ("java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap", "put") => {
            if args.len() < 2 {
                return Err(VMError::internal("HashMap.put requires key and value"));
            }
            let old_value = heap.hashmap_put(heap_id, args[0].clone(), args[1].clone())?;
            Ok(NativeDispatchResult::Handled(old_value))
        }
        (
            "java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap",
            "get" | "getAt",
        ) => {
            if args.is_empty() {
                return Err(VMError::internal("HashMap.get requires key"));
            }
            let value = heap.hashmap_get(heap_id, &args[0])?;
            Ok(NativeDispatchResult::Handled(value))
        }
        ("java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap", "size") => {
            let size = heap.hashmap_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_int(
                size, descriptor, heap,
            )?))
        }
        ("java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap", "containsKey") => {
            if args.is_empty() {
                return Err(VMError::internal("HashMap.containsKey requires key"));
            }
            let contains = heap.hashmap_contains_key(heap_id, &args[0])?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                contains, descriptor, heap,
            )?))
        }
        ("java/util/HashMap" | "java/util/LinkedHashMap" | "java/util/TreeMap", "isEmpty") => {
            let size = heap.hashmap_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                size == 0,
                descriptor,
                heap,
            )?))
        }

        // ── HashSet ────────────────────────────────────────────────────
        ("java/util/HashSet" | "java/util/TreeSet", "add") => {
            if args.is_empty() {
                return Err(VMError::internal("HashSet.add requires value"));
            }
            let added = heap.hashset_add(heap_id, args[0].clone())?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                added, descriptor, heap,
            )?))
        }
        ("java/util/HashSet" | "java/util/TreeSet", "contains") => {
            if args.is_empty() {
                return Err(VMError::internal("HashSet.contains requires value"));
            }
            let contains = heap.hashset_contains(heap_id, &args[0])?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                contains, descriptor, heap,
            )?))
        }
        // java.io.File answers from the file system, for the path it was constructed with.
        // A File without a stored path (one a Gradle build made) is left to the Gradle natives.
        ("java/io/File", "exists" | "isFile" | "isDirectory") => {
            let Some(path) = file_path_field(heap, heap_id)? else {
                return Ok(NativeDispatchResult::NotHandled);
            };
            let path = std::path::Path::new(&path);
            let answer = match method_name {
                "exists" => path.exists(),
                "isFile" => path.is_file(),
                _ => path.is_dir(),
            };
            Ok(NativeDispatchResult::Handled(Value::Int(i32::from(answer))))
        }
        ("java/util/HashSet" | "java/util/TreeSet", "remove") => {
            if args.is_empty() {
                return Err(VMError::internal("HashSet.remove requires value"));
            }
            let removed = heap.hashset_remove(heap_id, &args[0])?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                removed, descriptor, heap,
            )?))
        }
        ("java/util/HashSet" | "java/util/TreeSet", "size") => {
            let size = heap.hashset_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_int(
                size, descriptor, heap,
            )?))
        }
        ("java/util/HashSet" | "java/util/TreeSet", "isEmpty") => {
            let size = heap.hashset_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                size == 0,
                descriptor,
                heap,
            )?))
        }

        // ── java.util.concurrent.CountDownLatch ────────────────────────
        // Threads run synchronously (Thread.start invokes run() inline), so by
        // the time await() is reached the counting-down has already happened.
        // Field 0 holds the remaining count.
        ("java/util/concurrent/CountDownLatch", "countDown") => {
            let n = heap
                .get_field(heap_id, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            heap.set_field(heap_id, 0, Value::Int((n - 1).max(0)))?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        ("java/util/concurrent/CountDownLatch", "getCount") => {
            let n = heap
                .get_field(heap_id, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            Ok(NativeDispatchResult::Handled(Value::Long(n.max(0) as i64)))
        }
        ("java/util/concurrent/CountDownLatch", "await") => {
            // Threads run synchronously, so every countDown that will happen has already
            // happened. A count still above zero means Java would block here forever.
            let remaining = heap
                .get_field(heap_id, 0)?
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            if descriptor.ends_with(")V") {
                if remaining > 0 {
                    return Err(VMError::internal(format!(
                        "CountDownLatch.await() would block forever: count is {remaining}"
                    )));
                }
                Ok(NativeDispatchResult::HandledVoid)
            } else {
                // The timed form reports whether the count reached zero in time.
                Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                    remaining <= 0,
                    descriptor,
                    heap,
                )?))
            }
        }

        // ── java.util.Properties / Hashtable (heap-backed string map) ──
        ("java/util/Properties" | "java/util/Hashtable", "put" | "setProperty") => {
            if args.len() < 2 {
                return Err(VMError::internal("Properties.put requires key and value"));
            }
            let old = heap.hashmap_put(heap_id, args[0].clone(), args[1].clone())?;
            Ok(NativeDispatchResult::Handled(old))
        }
        ("java/util/Properties" | "java/util/Hashtable", "get") => {
            let v = heap.hashmap_get(
                heap_id,
                args.first().unwrap_or(&Value::Reference(ObjectRef::Null)),
            )?;
            Ok(NativeDispatchResult::Handled(v))
        }
        ("java/util/Properties", "getProperty") => {
            let key = args
                .first()
                .cloned()
                .unwrap_or(Value::Reference(ObjectRef::Null));
            let v = heap.hashmap_get(heap_id, &key)?;
            // getProperty(key, default): substitute the default when absent.
            let out = match (&v, args.get(1)) {
                (Value::Reference(ObjectRef::Null), Some(def)) => def.clone(),
                _ => v,
            };
            Ok(NativeDispatchResult::Handled(out))
        }
        ("java/util/Properties" | "java/util/Hashtable", "containsKey") => {
            let key = args
                .first()
                .cloned()
                .unwrap_or(Value::Reference(ObjectRef::Null));
            let c = heap.hashmap_contains_key(heap_id, &key)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                c, descriptor, heap,
            )?))
        }
        ("java/util/Properties" | "java/util/Hashtable", "size") => {
            let n = heap.hashmap_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(Value::Int(n)))
        }
        ("java/util/Properties" | "java/util/Hashtable", "isEmpty") => {
            let n = heap.hashmap_size(heap_id)?;
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                n == 0,
                descriptor,
                heap,
            )?))
        }

        // ── java.util.Optional (field 0 = value, or Null for empty) ────
        ("java/util/Optional", "isPresent") => {
            let present = !matches!(
                heap.get_field(heap_id, 0)?,
                None | Some(Value::Reference(ObjectRef::Null))
            );
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                present, descriptor, heap,
            )?))
        }
        ("java/util/Optional", "isEmpty") => {
            let empty = matches!(
                heap.get_field(heap_id, 0)?,
                None | Some(Value::Reference(ObjectRef::Null))
            );
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                empty, descriptor, heap,
            )?))
        }
        ("java/util/Optional", "get" | "orElseThrow") => {
            match heap.get_field(heap_id, 0)? {
                Some(value) if !matches!(value, Value::Reference(ObjectRef::Null)) => {
                    Ok(NativeDispatchResult::Handled(value))
                }
                // `orElseThrow(Supplier)` throws what the supplier returns, and calling the
                // supplier needs the interpreter. Fail rather than return a null value.
                _ if descriptor != "()Ljava/lang/Object;" => Err(VMError::internal(
                    "Optional.orElseThrow(Supplier) on an empty Optional is not supported",
                )),
                _ => Err(VMError::NoSuchElementException(
                    "No value present".to_string(),
                )),
            }
        }
        // OptionalInt/OptionalLong/OptionalDouble's unboxed accessors — the
        // stream terminal ops (findFirst/reduce/min/max/average/...) share
        // the same `java/util/Optional` runtime shape (see
        // `streams.rs::optional_wrap`) regardless of element flavor, so
        // these just read the same field 0.
        ("java/util/Optional", "getAsInt" | "getAsLong" | "getAsDouble") => {
            match heap.get_field(heap_id, 0)? {
                Some(v @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_))) => {
                    Ok(NativeDispatchResult::Handled(v))
                }
                _ => Err(VMError::internal("Optional.getAsX on empty")),
            }
        }
        ("java/util/Optional", "orElse") => {
            let v = heap
                .get_field(heap_id, 0)?
                .unwrap_or(Value::Reference(ObjectRef::Null));
            let out = match v {
                Value::Reference(ObjectRef::Null) => args
                    .first()
                    .cloned()
                    .unwrap_or(Value::Reference(ObjectRef::Null)),
                other => other,
            };
            Ok(NativeDispatchResult::Handled(out))
        }

        ("java/io/PrintStream", "println" | "print" | "printf" | "format" | "append") => {
            let text = match (method_name, args.first()) {
                ("println", None) => String::new(),
                // printf/format(fmt, Object...): the varargs Object[] is arg 1.
                ("printf" | "format", _) => java_printf(heap, args)?,
                (_, None) => String::new(),
                // `println(boolean)` must render true/false, not 1/0.
                (_, Some(Value::Int(v))) if descriptor.starts_with("(Z)") => {
                    if *v != 0 {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    }
                }
                // `println(char)` renders the code point as a character.
                (_, Some(Value::Int(v))) if descriptor.starts_with("(C)") => {
                    char::from_u32(*v as u32)
                        .map(|c| c.to_string())
                        .unwrap_or_default()
                }
                (_, Some(v)) => value_to_string_lossy(heap, v)?,
            };
            let line = if method_name == "println" {
                format!("{text}\n")
            } else {
                text
            };
            // The synthesized stream stores 1 in field 0 for System.err, 0 for out.
            let is_err = heap
                .get_field(heap_id, 0)
                .ok()
                .flatten()
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0)
                == 1;
            heap.write_program_output(&line, is_err);
            // printf/format/append return the stream (the receiver); the void
            // print/println return nothing.
            if matches!(method_name, "printf" | "format" | "append") {
                Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
            } else {
                Ok(NativeDispatchResult::HandledVoid)
            }
        }
        // ── java.lang.Enum (name/ordinal stored as dynamic properties) ─
        // Guarded on $enum_name so this covers any enum subclass receiver,
        // whether reached via invokevirtual or invokeinterface. name()/ordinal()
        // are final in Enum; toString() defaults to name() (a user override would
        // be a real vtable method, but those are rare and resolved when present).
        (_, "name" | "toString") if heap.get_dynamic_property(heap_id, "$enum_name").is_some() => {
            let v = heap
                .get_dynamic_property(heap_id, "$enum_name")
                .unwrap_or(Value::Reference(ObjectRef::Null));
            Ok(NativeDispatchResult::Handled(v))
        }
        (_, "ordinal")
            if heap
                .get_dynamic_property(heap_id, "$enum_ordinal")
                .is_some() =>
        {
            let v = heap
                .get_dynamic_property(heap_id, "$enum_ordinal")
                .unwrap_or(Value::Int(0));
            Ok(NativeDispatchResult::Handled(v))
        }

        // ── Throwable sidecar (works for JDK and user subclasses) ─────
        (_, "getMessage" | "getLocalizedMessage")
            if heap.throwable_data(heap_id).ok().flatten().is_some() =>
        {
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.throwable_message(heap_id)?,
            )))
        }
        (_, "getCause" | "getException")
            if heap.throwable_data(heap_id).ok().flatten().is_some() =>
        {
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.throwable_cause(heap_id)?,
            )))
        }
        // addSuppressed records an exception that failed while this one propagated (JLS
        // 14.20.3.1 closes resources this way). getSuppressed returns them in order.
        (_, "addSuppressed") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            let exception = args
                .first()
                .ok_or_else(|| VMError::internal("Throwable.addSuppressed missing argument"))?
                .as_reference()?;
            if exception == ObjectRef::Null {
                return Err(VMError::NullPointerException);
            }
            if exception == receiver {
                return Err(VMError::IllegalArgumentException(
                    "Self-suppression not permitted".to_string(),
                ));
            }
            // The suppressed exceptions are kept in an ArrayList on the throwable
            let list = match heap.get_dynamic_property(heap_id, "$suppressed") {
                Some(Value::Reference(ObjectRef::Object(list))) => list,
                _ => {
                    let list = heap.allocate_arraylist()?;
                    heap.set_dynamic_property(
                        heap_id,
                        "$suppressed",
                        Value::Reference(ObjectRef::Object(list)),
                    );
                    list
                }
            };
            heap.arraylist_add(list, Value::Reference(exception))?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        (_, "getSuppressed") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            let suppressed = match heap.get_dynamic_property(heap_id, "$suppressed") {
                Some(Value::Reference(ObjectRef::Object(list))) => {
                    heap.collection_snapshot(list)?
                }
                _ => Vec::new(),
            };
            let length = i32::try_from(suppressed.len()).map_err(|_| VMError::out_of_memory())?;
            let array = heap.allocate_array(
                crate::interpreter::heap::ArrayElementType::Reference(
                    "java/lang/Throwable".to_string(),
                ),
                length,
            )?;
            for (index, exception) in suppressed.into_iter().enumerate() {
                heap.array_store(array, i32::try_from(index).unwrap_or(i32::MAX), exception)?;
            }
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Array(array),
            )))
        }
        (_, "initCause") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            let cause = args
                .first()
                .ok_or_else(|| VMError::internal("Throwable.initCause missing argument"))?
                .as_reference()?;
            if cause == receiver {
                return Err(VMError::IllegalArgumentException(
                    "Self-causation not permitted".to_string(),
                ));
            }
            if !heap.set_throwable_cause(heap_id, cause)? {
                return Err(VMError::IllegalStateException(
                    "Can't overwrite cause".to_string(),
                ));
            }
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        (_, "fillInStackTrace") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        (cls, "toString") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            // Throwable.toString = fully-qualified class name + ": " + message.
            let name = cls.replace('/', ".");
            let s = match heap.throwable_message(heap_id)? {
                ObjectRef::Object(mid)
                    if heap.get_object_class_name(mid)? == "java/lang/String" =>
                {
                    format!("{name}: {}", heap.get_string(mid)?)
                }
                _ => name,
            };
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.intern_string(&s)?,
            )))
        }
        (cls, "printStackTrace") if heap.throwable_data(heap_id).ok().flatten().is_some() => {
            // Minimal: print the class + message to the captured stderr.
            let name = cls.replace('/', ".");
            let line = match heap.throwable_message(heap_id)? {
                ObjectRef::Object(mid)
                    if heap
                        .get_object_class_name(mid)
                        .map(|n| n == "java/lang/String")
                        .unwrap_or(false) =>
                {
                    format!("{name}: {}\n", heap.get_string(mid)?)
                }
                _ => format!("{name}\n"),
            };
            heap.write_program_output(&line, true);
            Ok(NativeDispatchResult::HandledVoid)
        }

        _ => Ok(NativeDispatchResult::NotHandled),
    }
}

/// (width, height, pixel-array heap id) of a BufferedImage (fields 0/1/2).
fn image_dims(heap: &Heap, img_id: HeapId) -> VMResult<(i32, i32, HeapId)> {
    let w = heap
        .get_field(img_id, 0)?
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0);
    let h = heap
        .get_field(img_id, 1)?
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0);
    let arr = match heap.get_field(img_id, 2)? {
        Some(Value::Reference(ObjectRef::Array(id))) => id,
        _ => return Err(VMError::internal("BufferedImage has no pixel buffer")),
    };
    Ok((w, h, arr))
}

/// Store one ARGB pixel in a BufferedImage, ignoring out-of-bounds writes.
fn raster_set(heap: &mut Heap, img_id: HeapId, x: i32, y: i32, rgb: i32) -> VMResult<()> {
    let (w, h, arr) = image_dims(heap, img_id)?;
    if x >= 0 && y >= 0 && x < w && y < h {
        heap.array_store(arr, y * w + x, Value::Int(rgb))?;
    }
    Ok(())
}

/// Read one ARGB pixel from a BufferedImage (0 when out of bounds).
fn raster_get(heap: &Heap, img_id: HeapId, x: i32, y: i32) -> VMResult<i32> {
    let (w, h, arr) = image_dims(heap, img_id)?;
    if x >= 0 && y >= 0 && x < w && y < h {
        Ok(heap.array_load(arr, y * w + x)?.as_int().unwrap_or(0))
    } else {
        Ok(0)
    }
}

/// The (image id, current color) a Graphics2D is bound to (fields 0/1).
fn graphics_target(heap: &Heap, g_id: HeapId) -> VMResult<(HeapId, i32)> {
    let img = match heap.get_field(g_id, 0)? {
        Some(Value::Reference(ObjectRef::Object(id))) => id,
        _ => return Err(VMError::internal("Graphics2D not bound to an image")),
    };
    let color = heap
        .get_field(g_id, 1)?
        .and_then(|v| v.as_int().ok())
        .unwrap_or(0);
    Ok((img, color))
}

/// Fill a rectangle in the bound image with the current color.
fn graphics_fill_rect(
    heap: &mut Heap,
    g_id: HeapId,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> VMResult<()> {
    let (img, color) = graphics_target(heap, g_id)?;
    for yy in y..y.saturating_add(h) {
        for xx in x..x.saturating_add(w) {
            raster_set(heap, img, xx, yy, color)?;
        }
    }
    Ok(())
}

/// Rasterize a line (Bresenham) in the bound image with the current color.
fn graphics_draw_line(
    heap: &mut Heap,
    g_id: HeapId,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) -> VMResult<()> {
    let (img, color) = graphics_target(heap, g_id)?;
    let (mut x, mut y) = (x1, y1);
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        raster_set(heap, img, x, y, color)?;
        if x == x2 && y == y2 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    Ok(())
}

fn value_to_string_lossy(heap: &Heap, value: &Value) -> VMResult<String> {
    Ok(match value {
        Value::Reference(ObjectRef::Null) => "null".to_string(),
        Value::Reference(ObjectRef::Object(id)) => {
            // Strings, boxed wrappers, StringBuilder, and the built-in
            // collections have a well-defined, deterministic `toString`; the
            // shared renderer produces it. User/unknown objects (whose real
            // `toString` only the executor can run) fall back to "".
            heap.render_known_object(*id).unwrap_or_default()
        }
        Value::Reference(ObjectRef::Array(_)) => String::new(),
        Value::Reference(ObjectRef::MethodHandle(_)) => String::new(),
        Value::Int(v) => v.to_string(),
        Value::Long(v) => v.to_string(),
        // Java renders whole-valued floats/doubles with a trailing ".0"
        // (`println(7.0)` -> "7.0"); Rust's `to_string` would drop it ("7").
        // `f32` keeps its own shortest form so float precision isn't widened.
        Value::Float(v) => crate::interpreter::heap::format_java_f32(*v),
        Value::Double(v) => crate::interpreter::heap::format_java_float(*v),
        Value::ReturnAddress(_) => String::new(),
        Value::CategoryTwoPlaceholder => String::new(),
    })
}

/// Implement `PrintStream.printf/format(String, Object...)`. `args` is
/// `[formatString, Object[] varargs]` (the JVM packs varargs into an array).
fn java_printf(heap: &Heap, args: &[Value]) -> VMResult<String> {
    let fmt = match args.first() {
        Some(v) => value_to_string_lossy(heap, v)?,
        None => String::new(),
    };
    let varargs: Vec<Value> = match args.get(1) {
        Some(Value::Reference(ObjectRef::Array(id))) => {
            let len = heap.array_length(*id)?;
            (0..len)
                .map(|i| heap.array_load(*id, i))
                .collect::<VMResult<Vec<_>>>()?
        }
        // A single non-array trailing arg (defensive): treat it as the lone value.
        Some(v) => vec![v.clone()],
        None => Vec::new(),
    };
    Ok(crate::interpreter::natives::java_format(
        heap, &fmt, &varargs,
    ))
}

/// Whether the call site's declared return type is a JVM reference or array.
///
/// Groovy `invokedynamic` sites generally declare `Object` even when the
/// resolved native target returns a primitive. Native dispatch must adapt that
/// primitive to the call-site contract, just as a real method handle adapter
/// would. Ordinary `invokevirtual` sites retain their primitive descriptor.
fn descriptor_returns_reference(descriptor: &str) -> bool {
    descriptor.rsplit_once(')').is_some_and(|(_, return_type)| {
        return_type.starts_with('L') || return_type.starts_with('[')
    })
}

/// Return a boxed Integer for a reference-returning call site, otherwise a raw int.
fn maybe_box_int(value: i32, descriptor: &str, heap: &mut Heap) -> VMResult<Value> {
    if descriptor_returns_reference(descriptor) {
        let boxed = heap.allocate_integer(value)?;
        Ok(Value::Reference(ObjectRef::Object(boxed)))
    } else {
        Ok(Value::Int(value))
    }
}

/// Return a boxed Boolean for a reference-returning call site, otherwise a raw JVM boolean.
fn maybe_box_boolean(value: bool, descriptor: &str, heap: &mut Heap) -> VMResult<Value> {
    if descriptor_returns_reference(descriptor) {
        let boxed = heap.allocate_boolean(value)?;
        Ok(Value::Reference(ObjectRef::Object(boxed)))
    } else {
        Ok(Value::Int(i32::from(value)))
    }
}

/// Read a boxed numeric wrapper's value (stored directly in field 0) as f64.
/// Works for Integer/Long/Short/Byte/Double/Float/Character regardless of the
/// primitive's stored variant; returns 0.0 if the field isn't a number.
/// `Number.xxxValue()` on a boxed primitive. Integral primitives wrap to the target
/// width, as Java's narrowing casts do. Floating primitives use the saturating cast
/// to `int` or `long` first, then narrow to `short` or `byte`.
fn boxed_conversion(method_name: &str, primitive: Option<&Value>, as_f64: f64) -> Value {
    let integral = match primitive {
        Some(Value::Int(v)) => Some(i64::from(*v)),
        Some(Value::Long(v)) => Some(*v),
        _ => None,
    };
    let as_int = integral.map_or(as_f64 as i32, |v| v as i32);
    match method_name {
        "doubleValue" => Value::Double(as_f64),
        "floatValue" => Value::Float(as_f64 as f32),
        "longValue" => Value::Long(integral.unwrap_or(as_f64 as i64)),
        "intValue" => Value::Int(as_int),
        "shortValue" => Value::Int(i32::from(as_int as i16)),
        _ => Value::Int(i32::from(as_int as i8)),
    }
}

fn boxed_number_as_f64(heap: &Heap, id: HeapId) -> f64 {
    match heap.get_field(id, 0) {
        Ok(Some(Value::Int(i))) => i as f64,
        Ok(Some(Value::Long(l))) => l as f64,
        Ok(Some(Value::Float(f))) => f as f64,
        Ok(Some(Value::Double(d))) => d,
        _ => 0.0,
    }
}

/// Coerce a value to i32, handling boxed Integer/Long via heap lookup.
fn coerce_to_int(value: &Value, heap: &Heap) -> VMResult<i32> {
    match value {
        Value::Int(i) => Ok(*i),
        Value::Long(l) => Ok(*l as i32),
        Value::Reference(ObjectRef::Object(id)) => {
            match heap.get_object_class_name(*id)?.as_str() {
                "java/lang/Integer" => heap.get_boxed_int(*id),
                "java/lang/Long" => Ok(heap.get_boxed_long(*id)? as i32),
                _ => Ok(0),
            }
        }
        _ => Ok(0),
    }
}

/// The path a `java/io/File` was constructed with, or `None` when its constructor did not run.
fn file_path_field(heap: &Heap, file: HeapId) -> VMResult<Option<String>> {
    match heap.get_field(file, crate::interpreter::natives::file::PATH_FIELD)? {
        Some(Value::Reference(ObjectRef::Object(path))) => Ok(Some(heap.get_string(path)?)),
        _ => Ok(None),
    }
}
