// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `String` virtual-method policy.

use super::{
    coerce_to_int, maybe_box_boolean, maybe_box_int, value_to_string_lossy, NativeDispatchResult,
};
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::{ArrayElementType, ClassRef, Heap};
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
        "valueOf" => Ok(NativeDispatchResult::Handled(Value::Reference(
            heap.intern_string("")?,
        ))),
        "append" => {
            let builder = heap.allocate_object(ClassRef {
                name: "java/lang/StringBuilder".to_string(),
                instance_field_count: 1,
                static_field_count: 0,
            })?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Object(builder),
            )))
        }
        "toString" => Ok(NativeDispatchResult::Handled(Value::Reference(receiver))),
        "length" => {
            let length =
                crate::interpreter::natives::java_string_utf16_len(&heap.get_string(heap_id)?);
            Ok(NativeDispatchResult::Handled(maybe_box_int(
                length, descriptor, heap,
            )?))
        }
        "isEmpty" => {
            let empty = heap.get_string(heap_id)?.is_empty();
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                empty, descriptor, heap,
            )?))
        }
        "charAt" => {
            let value = heap.get_string(heap_id)?;
            let index = args.first().map(Value::as_int).transpose()?.unwrap_or(0);
            let unit = crate::interpreter::natives::java_string_char_at(&value, index)?;
            Ok(NativeDispatchResult::Handled(Value::Int(i32::from(unit))))
        }
        "equals" => {
            let value = heap.get_string(heap_id)?;
            let equal = match args.first() {
                Some(Value::Reference(ObjectRef::Object(id)))
                    if heap
                        .get_object_class_name(*id)
                        .is_ok_and(|name| name == "java/lang/String") =>
                {
                    heap.get_string(*id)? == value
                }
                _ => false,
            };
            Ok(NativeDispatchResult::Handled(maybe_box_boolean(
                equal, descriptor, heap,
            )?))
        }
        "compareTo" => {
            let left = heap.get_string(heap_id)?;
            let right = value_to_string_lossy(heap, args.first().unwrap_or(&Value::Int(0)))?;
            Ok(NativeDispatchResult::Handled(Value::Int(
                crate::interpreter::natives::java_string_compare(&left, &right),
            )))
        }
        "substring" => {
            let value = heap.get_string(heap_id)?;
            let begin = args.first().map(Value::as_int).transpose()?.unwrap_or(0);
            let end = match args.get(1) {
                Some(value) => value.as_int()?,
                None => crate::interpreter::natives::java_string_utf16_len(&value),
            };
            let substring = crate::interpreter::natives::java_string_substring(&value, begin, end)?;
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&substring)?,
            )))
        }
        "indexOf" => {
            let value = heap.get_string(heap_id)?;
            let needle = search_units(heap, args.first())?;
            let from = args.get(1).map(Value::as_int).transpose()?.unwrap_or(0);
            let index = needle.as_deref().map_or(-1, |needle| {
                crate::interpreter::natives::java_string_index_of(&value, needle, from)
            });
            Ok(NativeDispatchResult::Handled(Value::Int(index)))
        }
        "lastIndexOf" => {
            let value = heap.get_string(heap_id)?;
            let needle = search_units(heap, args.first())?;
            let from = args
                .get(1)
                .map(Value::as_int)
                .transpose()?
                .unwrap_or_else(|| crate::interpreter::natives::java_string_utf16_len(&value));
            let index = needle.as_deref().map_or(-1, |needle| {
                crate::interpreter::natives::java_string_last_index_of(&value, needle, from)
            });
            Ok(NativeDispatchResult::Handled(Value::Int(index)))
        }
        "startsWith" => starts_with(heap, heap_id, args, descriptor),
        "endsWith" => predicate_argument(heap, heap_id, args, descriptor, |value, argument| {
            value.ends_with(argument)
        }),
        "contains" => predicate_argument(heap, heap_id, args, descriptor, |value, argument| {
            value.contains(argument)
        }),
        "toLowerCase" => intern_transformed(heap, heap_id, str::to_lowercase),
        "toUpperCase" => intern_transformed(heap, heap_id, str::to_uppercase),
        "replace" => replace(heap, heap_id, args),
        "split" => split(heap, heap_id, args),
        "repeat" => {
            let value = heap.get_string(heap_id)?;
            let count = coerce_to_int(args.first().unwrap_or(&Value::Int(0)), heap)?;
            if count < 0 {
                return Err(VMError::IllegalArgumentException(format!(
                    "count is negative: {count}"
                )));
            }
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&value.repeat(count as usize))?,
            )))
        }
        "trim" => {
            let value = crate::interpreter::natives::java_string_trim(&heap.get_string(heap_id)?);
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&value)?,
            )))
        }
        "strip" => {
            let value = heap.get_string(heap_id)?.trim().to_string();
            Ok(NativeDispatchResult::Handled(Value::Reference(
                heap.new_string(&value)?,
            )))
        }
        "hashCode" => Ok(NativeDispatchResult::Handled(Value::Int(
            crate::interpreter::natives::java_string_hash(&heap.get_string(heap_id)?),
        ))),
        "toCharArray" => {
            let units: Vec<u16> = heap.get_string(heap_id)?.encode_utf16().collect();
            let array = heap.allocate_array(ArrayElementType::Char, units.len() as i32)?;
            for (index, unit) in units.iter().enumerate() {
                heap.array_store(array, index as i32, Value::Int(i32::from(*unit)))?;
            }
            Ok(NativeDispatchResult::Handled(Value::Reference(
                ObjectRef::Array(array),
            )))
        }
        _ => Ok(NativeDispatchResult::NotHandled),
    }
}

fn search_units(heap: &Heap, argument: Option<&Value>) -> VMResult<Option<Vec<u16>>> {
    Ok(match argument {
        Some(Value::Int(value @ 0..=0xffff)) => Some(vec![*value as u16]),
        Some(Value::Int(value)) => char::from_u32(*value as u32)
            .map(|character| character.encode_utf16(&mut [0; 2]).to_vec()),
        Some(value) => Some(value_to_string_lossy(heap, value)?.encode_utf16().collect()),
        None => Some(Vec::new()),
    })
}

/// `startsWith(prefix)` and `startsWith(prefix, toffset)`. The offset counts UTF-16
/// code units, as Java's does, and an offset outside the string matches nothing.
fn starts_with(
    heap: &mut Heap,
    heap_id: HeapId,
    args: &[Value],
    descriptor: &str,
) -> VMResult<NativeDispatchResult> {
    let value = heap.get_string(heap_id)?;
    let prefix = value_to_string_lossy(heap, args.first().unwrap_or(&Value::Int(0)))?;
    let offset = match args.get(1) {
        Some(offset) => offset.as_int()?,
        None => 0,
    };
    let units: Vec<u16> = value.encode_utf16().collect();
    let prefix_units: Vec<u16> = prefix.encode_utf16().collect();
    let matched = usize::try_from(offset)
        .ok()
        .and_then(|offset| units.get(offset..))
        .is_some_and(|rest| rest.starts_with(&prefix_units));
    Ok(NativeDispatchResult::Handled(maybe_box_boolean(
        matched, descriptor, heap,
    )?))
}

fn predicate_argument(
    heap: &mut Heap,
    heap_id: HeapId,
    args: &[Value],
    descriptor: &str,
    predicate: fn(&str, &str) -> bool,
) -> VMResult<NativeDispatchResult> {
    let value = heap.get_string(heap_id)?;
    let argument = value_to_string_lossy(heap, args.first().unwrap_or(&Value::Int(0)))?;
    Ok(NativeDispatchResult::Handled(maybe_box_boolean(
        predicate(&value, &argument),
        descriptor,
        heap,
    )?))
}

fn intern_transformed(
    heap: &mut Heap,
    heap_id: HeapId,
    transform: fn(&str) -> String,
) -> VMResult<NativeDispatchResult> {
    let value = transform(&heap.get_string(heap_id)?);
    Ok(NativeDispatchResult::Handled(Value::Reference(
        heap.new_string(&value)?,
    )))
}

fn replace(heap: &mut Heap, heap_id: HeapId, args: &[Value]) -> VMResult<NativeDispatchResult> {
    let value = heap.get_string(heap_id)?;
    let (from, to) = match (args.first(), args.get(1)) {
        (Some(Value::Int(from)), Some(Value::Int(to))) => (
            char::from_u32(*from as u32)
                .map(String::from)
                .unwrap_or_default(),
            char::from_u32(*to as u32)
                .map(String::from)
                .unwrap_or_default(),
        ),
        (Some(from), Some(to)) => (
            value_to_string_lossy(heap, from)?,
            value_to_string_lossy(heap, to)?,
        ),
        _ => (String::new(), String::new()),
    };
    let replaced = if from.is_empty() {
        value
    } else {
        value.replace(&from, &to)
    };
    Ok(NativeDispatchResult::Handled(Value::Reference(
        heap.new_string(&replaced)?,
    )))
}

/// `String.split(regex, limit)` with `Pattern.split`'s rules, which the regex crate's
/// `split` does not follow. A zero-width match at the start yields no leading empty
/// piece, so `"abc".split("")` is `[a, b, c]`. A positive limit caps the number of
/// pieces. A zero limit drops every trailing empty piece. Input with no match is
/// returned whole.
fn java_split<'a>(regex: &regex::Regex, value: &'a str, limit: i32) -> Vec<&'a str> {
    let limited = limit > 0;
    let mut parts = Vec::new();
    let mut index = 0;
    for found in regex.find_iter(value) {
        if limited && parts.len() as i64 >= i64::from(limit) - 1 {
            break;
        }
        if index == 0 && found.start() == 0 && found.end() == 0 {
            continue;
        }
        parts.push(&value[index..found.start()]);
        index = found.end();
    }
    if index == 0 {
        return vec![value];
    }
    if !limited || (parts.len() as i64) < i64::from(limit) {
        parts.push(&value[index..]);
    }
    if limit == 0 {
        while parts.last().is_some_and(|part| part.is_empty()) {
            parts.pop();
        }
    }
    parts
}

fn split(heap: &mut Heap, heap_id: HeapId, args: &[Value]) -> VMResult<NativeDispatchResult> {
    let value = heap.get_string(heap_id)?;
    let pattern = value_to_string_lossy(heap, args.first().unwrap_or(&Value::Int(0)))?;
    let regex = regex::Regex::new(&pattern)
        .map_err(|error| VMError::PatternSyntaxException(error.to_string()))?;
    let limit = args.get(1).map(Value::as_int).transpose()?.unwrap_or(0);
    let parts = java_split(&regex, &value, limit);

    let array = heap.allocate_array(
        ArrayElementType::Reference("java/lang/String".to_string()),
        parts.len() as i32,
    )?;
    for (index, part) in parts.iter().enumerate() {
        let part = heap.new_string(part)?;
        heap.array_store(array, index as i32, Value::Reference(part))?;
    }
    Ok(NativeDispatchResult::Handled(Value::Reference(
        ObjectRef::Array(array),
    )))
}
