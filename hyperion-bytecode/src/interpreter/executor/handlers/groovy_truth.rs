// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical Groovy-truth evaluation for compiler-emitted control flow and DGMs.

use bigdecimal::Zero;

use crate::groovy_runtime::{TRUTH_CLASS, TRUTH_DESCRIPTOR, TRUTH_METHOD};
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::Heap;
use crate::interpreter::value::{ObjectRef, Value};

/// Execute the private truth-coercion call emitted by the Groovy compiler.
pub(crate) fn try_invoke(
    heap: &mut Heap,
    class_name: &str,
    method_name: &str,
    descriptor: &str,
    arguments: &[Value],
) -> VMResult<Option<Value>> {
    if class_name != TRUTH_CLASS || method_name != TRUTH_METHOD || descriptor != TRUTH_DESCRIPTOR {
        return Ok(None);
    }

    let [value] = arguments else {
        return Err(VMError::internal(
            "Groovy truth coercion requires one operand",
        ));
    };
    Ok(Some(Value::Int(i32::from(is_truthy(heap, value)?))))
}

/// Apply Groovy's boolean-coercion rules to an interpreter value.
pub(crate) fn is_truthy(heap: &Heap, value: &Value) -> VMResult<bool> {
    let id = match value {
        Value::Int(value) => return Ok(*value != 0),
        Value::Long(value) => return Ok(*value != 0),
        Value::Float(value) => return Ok(*value != 0.0),
        Value::Double(value) => return Ok(*value != 0.0),
        Value::Reference(ObjectRef::Null) => return Ok(false),
        Value::Reference(ObjectRef::MethodHandle(_)) => return Ok(true),
        Value::Reference(ObjectRef::Object(id)) | Value::Reference(ObjectRef::Array(id)) => *id,
        Value::ReturnAddress(_) | Value::CategoryTwoPlaceholder => return Ok(true),
    };

    if let Ok(length) = heap.array_length(id) {
        return Ok(length != 0);
    }

    let class_name = heap.get_object_class_name(id)?;
    match class_name.as_str() {
        "java/lang/Boolean"
        | "java/lang/Integer"
        | "java/lang/Short"
        | "java/lang/Byte"
        | "java/lang/Character" => match heap.get_field(id, 0)? {
            Some(Value::Int(value)) => Ok(value != 0),
            _ => Err(VMError::internal(format!(
                "{class_name} is missing its primitive value"
            ))),
        },
        "java/lang/Long" => Ok(heap.get_boxed_long(id)? != 0),
        "java/lang/Float" => match heap.get_field(id, 0)? {
            Some(Value::Float(value)) => Ok(value != 0.0),
            _ => Err(VMError::internal("Float is missing its primitive value")),
        },
        "java/lang/Double" => match heap.get_field(id, 0)? {
            Some(Value::Double(value)) => Ok(value != 0.0),
            _ => Err(VMError::internal("Double is missing its primitive value")),
        },
        "java/math/BigDecimal" => Ok(!heap.get_big_decimal(id)?.is_zero()),
        "java/lang/String" => Ok(!heap.get_string(id)?.is_empty()),
        "java/util/HashMap" | "java/util/LinkedHashMap" => Ok(heap.hashmap_size(id)? != 0),
        _ => match heap.collection_snapshot(id) {
            Ok(items) => Ok(!items.is_empty()),
            Err(_) => Ok(true),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_and_collections_follow_groovy_truth() {
        let mut heap = Heap::new(1024 * 1024);
        let empty_string = heap.intern_string("").unwrap();
        let text = heap.intern_string("value").unwrap();
        let empty_list = heap.allocate_arraylist().unwrap();
        let list = heap.allocate_arraylist().unwrap();
        heap.arraylist_add(list, Value::Int(1)).unwrap();

        assert!(!is_truthy(&heap, &Value::Reference(empty_string)).unwrap());
        assert!(is_truthy(&heap, &Value::Reference(text)).unwrap());
        assert!(!is_truthy(&heap, &Value::Reference(ObjectRef::Object(empty_list))).unwrap());
        assert!(is_truthy(&heap, &Value::Reference(ObjectRef::Object(list))).unwrap());
    }
}
