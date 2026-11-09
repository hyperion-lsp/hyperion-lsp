// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Object creation opcode handlers
//!
//! Handles JVM bytecode opcodes for object and array creation:
//! - new (0xbb) - Create new object instance
//! - multianewarray (0xc5) - Create multi-dimensional array
//!
//! Note: newarray (0xbc) and anewarray (0xbd) are in arrays.rs

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::{ArrayElementType, ClassRef, Heap};
use crate::interpreter::method_area::MethodArea;
use crate::interpreter::value::{HeapId, ObjectRef, Value};
use std::sync::{Arc, RwLock};

/// Stub field counts for well-known JDK/Groovy classes
/// Returns Some(count) if class is a stub class, None otherwise
pub fn get_stub_field_count(class_name: &str) -> Option<usize> {
    match class_name {
        // StringBuilder: internal buffer (we store as single string field)
        "java/lang/StringBuilder" => Some(1),
        // ArrayList: internal array (we store elements in heap fields)
        "java/util/ArrayList" => Some(2), // size + elements array
        // LinkedHashMap: internal table (we store as key/value arrays)
        "java/util/LinkedHashMap" | "java/util/HashMap" => Some(3), // size + keys + values
        // IntRange: from, to, inclusive
        "groovy/lang/IntRange" => Some(3),
        // Integer boxing
        "java/lang/Integer" => Some(1),
        // java.util.concurrent.CountDownLatch: remaining count (int) in field 0
        "java/util/concurrent/CountDownLatch" => Some(1),
        // java.util.Optional: contained value (or Null for empty) in field 0
        "java/util/Optional" => Some(1),
        // AWT / ImageIO image support (software-rendered).
        "java/awt/image/BufferedImage" => Some(3), // width, height, pixel int[] ref
        "java/awt/Color" => Some(1),               // packed 0xAARRGGBB
        "java/awt/Graphics2D" | "java/awt/Graphics" => Some(2), // image ref, current color
        "java/io/File" => Some(1),                 // path string ref
        "java/awt/BasicStroke" => Some(1),         // line width (unused; visual only)
        // Object base class
        "java/lang/Object" => Some(0),
        _ => None,
    }
}

/// Whether `class_name` names a JDK `Throwable` subtype we model generically
/// (so `new SomeException(msg)` works without a real loaded class). Heuristic:
/// a `java.*` class whose simple name ends in `Exception`, `Error`, or is
/// `Throwable` itself. User exception types are loaded classes and resolve
/// normally before this fallback is consulted.
pub fn is_jdk_throwable(class_name: &str) -> bool {
    let in_jdk = class_name.starts_with("java/")
        || class_name.starts_with("java.")
        || class_name.starts_with("javax/")
        || class_name.starts_with("javax.")
        || class_name.starts_with("jdk/")
        || class_name.starts_with("jdk.");
    in_jdk
        && (class_name.ends_with("Exception")
            || class_name.ends_with("Error")
            || matches!(class_name, "java/lang/Throwable" | "java.lang.Throwable"))
}

/// Handle new (0xbb) - Create new object instance
///
/// # Arguments
/// * `frame` - Current execution frame
/// * `heap` - Heap for object allocation
/// * `method_area` - Optional method area for class info lookup
/// * `class_name` - Resolved class name from constant pool
#[inline]
pub fn handle_new(
    frame: &mut Frame,
    heap: &mut Heap,
    method_area: Option<&Arc<RwLock<MethodArea>>>,
    class_name: String,
) -> VMResult<()> {
    // Check for stub classes (standard JDK/Groovy classes we simulate)
    let stub_field_count = get_stub_field_count(&class_name);

    // Look up the class in the method area to get actual field count
    let instance_field_count = if let Some(field_count) = stub_field_count {
        // Use stub field count for known classes
        field_count
    } else if let Some(ma) = method_area {
        let method_area_locked = ma
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;

        match method_area_locked.get_class(&class_name) {
            Ok(loaded_class) => loaded_class.field_layout.field_count,
            // Throwable state lives in the heap sidecar, not in invented Java
            // field slots that would collide with user-subclass layouts.
            Err(_) if is_jdk_throwable(&class_name) => 0,
            Err(e) => return Err(e),
        }
    } else if is_jdk_throwable(&class_name) {
        0
    } else {
        // Fallback if method area not available
        0
    };

    // Handle collections specially - they need Collection heap entries
    let is_throwable = is_jdk_throwable(&class_name)
        || method_area
            .and_then(|area| area.read().ok())
            .and_then(|area| area.is_subtype(&class_name, "java/lang/Throwable").ok())
            .unwrap_or(false);
    let heap_id = match class_name.as_str() {
        "java/util/ArrayList" => heap.allocate_arraylist()?,
        "java/util/HashMap" => heap.allocate_hashmap()?,
        "java/util/LinkedHashMap" => heap.allocate_linked_hashmap()?,
        "java/util/TreeMap" => heap.allocate_treemap()?,
        "java/util/TreeSet" => heap.allocate_treeset()?,
        // Properties and Hashtable are hash maps. The object keeps its own class, so
        // the constructor invoked on it (Properties.<init>) accepts it.
        "java/util/Properties" | "java/util/Hashtable" => heap.allocate_map(&class_name)?,
        "java/util/HashSet" | "java/util/LinkedHashSet" => heap.allocate_hashset()?,
        "java/util/LinkedList" => heap.allocate_linkedlist()?,
        _ => {
            // Create regular object
            let class_ref = ClassRef {
                name: class_name,
                instance_field_count,
                static_field_count: 0,
            };
            if is_throwable {
                heap.allocate_throwable(class_ref, ObjectRef::Null, None)?
                    .heap_id()
                    .ok_or_else(VMError::invalid_object_ref)?
            } else {
                heap.allocate_object(class_ref)?
            }
        }
    };
    frame.push(Value::Reference(ObjectRef::Object(heap_id)))
}

/// Handle multianewarray (0xc5) - Create multi-dimensional array
///
/// # Arguments
/// * `frame` - Current execution frame (dimension counts are popped from stack)
/// * `heap` - Heap for array allocation
/// * `dimensions` - Number of dimensions to allocate
#[inline]
pub fn handle_multianewarray(
    frame: &mut Frame,
    heap: &mut Heap,
    dimensions: usize,
    array_descriptor: &str,
) -> VMResult<()> {
    // JVM Spec §6.5.multianewarray: dimensions must be >= 1
    if dimensions == 0 {
        return Err(VMError::ClassFormatError(
            "multianewarray dimensions must be >= 1".to_string(),
        ));
    }
    let available_dimensions = array_descriptor
        .bytes()
        .take_while(|byte| *byte == b'[')
        .count();
    if dimensions > available_dimensions {
        return Err(VMError::ClassFormatError(format!(
            "multianewarray dimensions {dimensions} exceed descriptor rank {available_dimensions}"
        )));
    }

    // Pop dimension counts from stack (first pop is innermost, last pop is outermost)
    let mut counts = Vec::with_capacity(dimensions);
    for _ in 0..dimensions {
        let count = frame.pop()?.as_int()?;
        counts.push(count);
    }
    // counts is now [innermost, ..., outermost] - reverse to get [outermost, ..., innermost]
    counts.reverse();

    // Validate all counts are non-negative
    for &count in &counts {
        if count < 0 {
            return Err(VMError::negative_array_size());
        }
    }

    // Recursively allocate exactly the dimensions requested by the bytecode.
    // A partial allocation leaves the remaining inner-array slots null.
    let heap_id = allocate_multidimensional_array(heap, &counts, array_descriptor)?;
    frame.push(Value::Reference(ObjectRef::Array(heap_id)))
}

/// Recursively allocate a multi-dimensional array
///
/// # Arguments
/// * `heap` - Heap for array allocation
/// * `counts` - Array of dimension sizes [outermost, ..., innermost]
pub fn allocate_multidimensional_array(
    heap: &mut Heap,
    counts: &[i32],
    array_descriptor: &str,
) -> VMResult<HeapId> {
    if counts.is_empty() {
        return Err(VMError::InvalidBytecode);
    }

    let component_descriptor = array_descriptor.strip_prefix('[').ok_or_else(|| {
        VMError::ClassFormatError(format!(
            "multianewarray requires an array descriptor, got {array_descriptor}"
        ))
    })?;
    let element_type = array_component_type(component_descriptor)?;
    let outer_count = counts[0];
    let outer_id = heap.allocate_array(element_type, outer_count)?;

    if counts.len() > 1 {
        if !component_descriptor.starts_with('[') {
            return Err(VMError::ClassFormatError(
                "multianewarray dimensions exceed descriptor rank".to_string(),
            ));
        }
        for i in 0..outer_count {
            let inner_id =
                allocate_multidimensional_array(heap, &counts[1..], component_descriptor)?;
            heap.array_store(outer_id, i, Value::Reference(ObjectRef::Array(inner_id)))?;
        }
    }

    Ok(outer_id)
}

fn array_component_type(component_descriptor: &str) -> VMResult<ArrayElementType> {
    match component_descriptor.as_bytes() {
        [b'I'] => Ok(ArrayElementType::Int),
        [b'J'] => Ok(ArrayElementType::Long),
        [b'F'] => Ok(ArrayElementType::Float),
        [b'D'] => Ok(ArrayElementType::Double),
        [b'Z'] => Ok(ArrayElementType::Boolean),
        [b'B'] => Ok(ArrayElementType::Byte),
        [b'C'] => Ok(ArrayElementType::Char),
        [b'S'] => Ok(ArrayElementType::Short),
        [b'[', ..] => Ok(ArrayElementType::Reference(
            component_descriptor.to_string(),
        )),
        [b'L', .., b';'] if component_descriptor.len() > 2 => Ok(ArrayElementType::Reference(
            component_descriptor[1..component_descriptor.len() - 1].to_string(),
        )),
        _ => Err(VMError::ClassFormatError(format!(
            "invalid array component descriptor {component_descriptor}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_field_counts() {
        assert_eq!(get_stub_field_count("java/lang/StringBuilder"), Some(1));
        assert_eq!(get_stub_field_count("java/util/ArrayList"), Some(2));
        assert_eq!(get_stub_field_count("java/util/HashMap"), Some(3));
        assert_eq!(get_stub_field_count("com/example/Custom"), None);
    }

    #[test]
    fn jdk_throwable_classification_accepts_both_binary_name_forms() {
        for class_name in [
            "java/lang/Throwable",
            "java.lang.Throwable",
            "java/lang/RuntimeException",
            "java.lang.RuntimeException",
            "java/lang/NoSuchMethodError",
            "java.lang.NoSuchMethodError",
        ] {
            assert!(is_jdk_throwable(class_name), "{class_name}");
        }
        assert!(!is_jdk_throwable("example/RuntimeException"));
        assert!(!is_jdk_throwable("java/lang/String"));
    }
}
