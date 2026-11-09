// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `StringBuilder` virtual-method policy.

use super::{coerce_to_int, maybe_box_boolean, value_to_string_lossy, NativeDispatchResult};
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::Heap;
use crate::interpreter::value::{ObjectRef, Value};
use crate::interpreter::HeapId;

pub(super) fn dispatch(
    method_name: &str,
    descriptor: &str,
    receiver: ObjectRef,
    heap_id: HeapId,
    args: &[Value],
    heap: &mut Heap,
) -> VMResult<NativeDispatchResult> {
    match method_name {
        "append" => {
            if args.is_empty() {
                return Ok(NativeDispatchResult::Handled(Value::Reference(receiver)));
            }

            let current = get_content(heap, heap_id)?;
            let appended = append_text(heap, descriptor, args)?;
            set_content(heap, heap_id, &format!("{current}{appended}"))?;
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        "toString" => {
            // Each call returns a new String, as in Java, so the builder's own buffer
            // is never shared with the caller.
            let content = get_content(heap, heap_id)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&content)?,
            )))
        }
        "length" => {
            let length =
                crate::interpreter::natives::java_string_utf16_len(&get_content(heap, heap_id)?);
            Ok(NativeDispatchResult::Handled(Value::Int(length)))
        }
        "isEmpty" => {
            let empty = get_content(heap, heap_id)?.is_empty();
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                empty, descriptor, heap,
            )?))
        }
        "charAt" => {
            let index = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let content = get_content(heap, heap_id)?;
            let unit = crate::interpreter::natives::java_string_char_at(&content, index)?;
            Ok(NativeDispatchResult::Handled(Value::Int(i32::from(unit))))
        }
        "reverse" => {
            let reversed: String = get_content(heap, heap_id)?.chars().rev().collect();
            set_content(heap, heap_id, &reversed)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        "setLength" => {
            let requested = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let content = get_content(heap, heap_id)?;
            let length = crate::interpreter::natives::java_string_utf16_len(&content);
            if requested < 0 {
                return Err(VMError::StringIndexOutOfBounds {
                    index: requested,
                    length,
                });
            }
            let updated = if requested <= length {
                crate::interpreter::natives::java_string_replace_range(
                    &content,
                    requested,
                    length,
                    &[],
                )?
            } else {
                let padding = "\0".repeat((requested - length) as usize);
                format!("{content}{padding}")
            };
            set_content(heap, heap_id, &updated)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        "setCharAt" => {
            let index = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let character = coerce_to_int(args.get(1).unwrap_or(&Value::Int(0)), heap)?;
            let content = get_content(heap, heap_id)?;
            crate::interpreter::natives::java_string_char_at(&content, index)?;
            let updated = crate::interpreter::natives::java_string_replace_range(
                &content,
                index,
                index + 1,
                &[character as u16],
            )?;
            set_content(heap, heap_id, &updated)?;
            Ok(NativeDispatchResult::HandledVoid)
        }
        "deleteCharAt" => {
            let index = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let content = get_content(heap, heap_id)?;
            crate::interpreter::natives::java_string_char_at(&content, index)?;
            let updated = crate::interpreter::natives::java_string_replace_range(
                &content,
                index,
                index + 1,
                &[],
            )?;
            set_content(heap, heap_id, &updated)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        "insert" => {
            let offset = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let inserted = match args.get(1) {
                Some(Value::Int(value)) if descriptor.contains("IC)") => {
                    char::from_u32(*value as u32)
                        .map(|character| character.to_string())
                        .unwrap_or_default()
                }
                Some(value) => value_to_string_lossy(heap, value)?,
                None => String::new(),
            };
            let content = get_content(heap, heap_id)?;
            let replacement: Vec<u16> = inserted.encode_utf16().collect();
            let updated = crate::interpreter::natives::java_string_replace_range(
                &content,
                offset,
                offset,
                &replacement,
            )?;
            set_content(heap, heap_id, &updated)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(receiver)))
        }
        "substring" => {
            let content = get_content(heap, heap_id)?;
            let start = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            let end = match args.get(1) {
                Some(value) => coerce_to_int(value, heap)?,
                None => crate::interpreter::natives::java_string_utf16_len(&content),
            };
            let slice = crate::interpreter::natives::java_string_substring(&content, start, end)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&slice)?,
            )))
        }
        _ => Ok(NativeDispatchResult::NotHandled),
    }
}

/// Read a builder's backing content (stored as a `String` in field zero).
fn get_content(heap: &Heap, heap_id: HeapId) -> VMResult<String> {
    match heap.get_field(heap_id, 0)? {
        Some(Value::Reference(ObjectRef::Object(id)))
            if heap.get_object_class_name(id)? == "java/lang/String" =>
        {
            heap.get_string(id)
        }
        _ => Ok(String::new()),
    }
}

/// The text an `append` overload adds. A char array takes an optional offset and
/// count, and a CharSequence takes a start and an end, both in UTF-16 units.
fn append_text(heap: &Heap, descriptor: &str, args: &[Value]) -> VMResult<String> {
    if descriptor.starts_with("([C") {
        let units = char_array_units(heap, &args[0])?;
        let (offset, count) = match args.get(1..) {
            Some([offset, count]) => (int_arg(offset)?, int_arg(count)?),
            _ => (0, units.len()),
        };
        let range = checked_range(offset, count, units.len())?;
        return Ok(String::from_utf16_lossy(&units[range]));
    }
    if descriptor.starts_with("(Ljava/lang/CharSequence;II)") {
        let units: Vec<u16> = value_to_string_lossy(heap, &args[0])?
            .encode_utf16()
            .collect();
        let (start, end) = (int_arg(&args[1])?, int_arg(&args[2])?);
        let count = end.checked_sub(start).ok_or_else(|| {
            VMError::IllegalArgumentException(format!("start {start} is after end {end}"))
        })?;
        let range = checked_range(start, count, units.len())?;
        return Ok(String::from_utf16_lossy(&units[range]));
    }
    Ok(match &args[0] {
        Value::Int(value) if descriptor.starts_with("(C)") => char::from_u32(*value as u32)
            .map(|character| character.to_string())
            .unwrap_or_default(),
        Value::Int(value) if descriptor.starts_with("(Z)") => {
            if *value != 0 {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        other => value_to_string_lossy(heap, other)?,
    })
}

fn int_arg(value: &Value) -> VMResult<usize> {
    let value = value.as_int()?;
    usize::try_from(value)
        .map_err(|_| VMError::IllegalArgumentException(format!("negative index {value}")))
}

/// The `offset..offset + count` range, if it lies within `length`.
fn checked_range(offset: usize, count: usize, length: usize) -> VMResult<std::ops::Range<usize>> {
    match offset.checked_add(count) {
        Some(end) if end <= length => Ok(offset..end),
        _ => Err(VMError::IllegalArgumentException(format!(
            "offset {offset}, count {count}, length {length}"
        ))),
    }
}

/// The UTF-16 units of a char array, read element by element.
fn char_array_units(heap: &Heap, array: &Value) -> VMResult<Vec<u16>> {
    let Value::Reference(ObjectRef::Array(id)) = array else {
        return Err(VMError::NullPointerException);
    };
    (0..heap.array_length(*id)?)
        .map(|index| {
            let unit = heap.array_load(*id, index)?.as_int()?;
            Ok(u16::try_from(unit).unwrap_or(0))
        })
        .collect()
}

fn set_content(heap: &mut Heap, heap_id: HeapId, content: &str) -> VMResult<()> {
    let interned = heap.new_string(content)?;
    heap.set_field(heap_id, 0, Value::Reference(interned))
}
