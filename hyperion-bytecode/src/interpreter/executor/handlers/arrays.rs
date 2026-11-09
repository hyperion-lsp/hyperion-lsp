// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Array opcode handlers
//!
//! Handles JVM bytecode opcodes for array operations:
//! - Array loads: iaload, laload, faload, daload, aaload, baload, caload, saload
//! - Array stores: iastore, lastore, fastore, dastore, aastore, bastore, castore, sastore
//! - Array length: arraylength
//! - Array creation: newarray, anewarray

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::{ArrayElementType, Heap};
use crate::interpreter::value::{ObjectRef, Value};

// =============================================================================
// Array Loads
// =============================================================================

/// Handle iaload (0x2e) - Load int from array
#[inline]
pub fn handle_iaload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    frame.push(value)
}

/// Handle laload (0x2f) - Load long from array
#[inline]
pub fn handle_laload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    frame.push(value)
}

/// Handle faload (0x30) - Load float from array
#[inline]
pub fn handle_faload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    frame.push(value)
}

/// Handle daload (0x31) - Load double from array
#[inline]
pub fn handle_daload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    frame.push(value)
}

/// Handle aaload (0x32) - Load reference from array
#[inline]
pub fn handle_aaload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    frame.push(value)
}

/// Handle baload (0x33) - Load byte/boolean from array
#[inline]
pub fn handle_baload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    // Heap already returns Int for byte/boolean arrays
    frame.push(value)
}

/// Handle caload (0x34) - Load char from array
#[inline]
pub fn handle_caload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    // Heap already returns Int for char arrays
    frame.push(value)
}

/// Handle saload (0x35) - Load short from array
#[inline]
pub fn handle_saload(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let value = heap.array_load(heap_id, index)?;
    // Heap already returns Int for short arrays
    frame.push(value)
}

// =============================================================================
// Array Stores
// =============================================================================

/// Handle iastore (0x4f) - Store int into array
#[inline]
pub fn handle_iastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle lastore (0x50) - Store long into array
#[inline]
pub fn handle_lastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle fastore (0x51) - Store float into array
#[inline]
pub fn handle_fastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle dastore (0x52) - Store double into array
#[inline]
pub fn handle_dastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle bastore (0x54) - Store byte/boolean into array
#[inline]
pub fn handle_bastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle castore (0x55) - Store char into array
#[inline]
pub fn handle_castore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Handle sastore (0x56) - Store short into array
#[inline]
pub fn handle_sastore(frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    heap.array_store(heap_id, index, value)
}

/// Type checking function for aastore (needed for ArrayStoreException validation)
pub type IsSubclassOfFn<'a> = &'a dyn Fn(&str, &str) -> bool;

/// Handle aastore (0x53) - Store reference into array
///
/// Per JVMS §6.5.aastore: If value is not null and not assignment compatible
/// with the array component type, throws ArrayStoreException.
#[inline]
pub fn handle_aastore(
    frame: &mut Frame,
    heap: &mut Heap,
    is_subclass_of: IsSubclassOfFn<'_>,
) -> VMResult<()> {
    let value = frame.pop()?;
    let index = frame.pop()?.as_int()?;
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;

    // JVMS 6.5.aastore checks null, then bounds, then assignment
    // compatibility. Keep this check before inspecting the stored value's
    // runtime class so an incompatible out-of-bounds store reports AIOOBE.
    heap.validate_array_index(heap_id, index)?;

    // Type validation for reference arrays (JVMS §6.5.aastore)
    if let Value::Reference(obj_ref) = &value {
        if !matches!(obj_ref, ObjectRef::Null) {
            // Get array's element type
            if let Some(element_class_name) = heap.array_element_type(heap_id)? {
                // This is a reference array, validate the assignment
                let value_class_name = match obj_ref {
                    ObjectRef::Object(heap_id) | ObjectRef::Array(heap_id) => {
                        // The tag is an execution hint, not heap provenance:
                        // ObjectRef::Object can still designate an array.
                        heap.get_class(*heap_id)?.name
                    }
                    ObjectRef::Null => unreachable!(), // Already checked above
                    ObjectRef::MethodHandle(_) => {
                        // MethodHandles have their own type
                        "java/lang/invoke/MethodHandle".to_string()
                    }
                };

                // Check if value_class_name is assignment compatible with element_class_name
                if !is_subclass_of(&value_class_name, &element_class_name) {
                    return Err(VMError::ArrayStoreException {
                        array_type: element_class_name,
                        value_type: value_class_name,
                    });
                }
            }
        }
    }

    heap.array_store(heap_id, index, value)
}

// =============================================================================
// Array Utility Operations
// =============================================================================

/// Handle arraylength (0xbe) - Get length of array
#[inline]
pub fn handle_arraylength(frame: &mut Frame, heap: &Heap) -> VMResult<()> {
    let array_ref = frame.pop()?.as_reference()?;
    let heap_id = array_ref.heap_id().ok_or_else(VMError::null_pointer)?;
    let length = heap.array_length(heap_id)?;
    frame.push(Value::Int(length))
}

/// Handle newarray (0xbc) - Create new primitive array
///
/// Type codes:
/// - 4: boolean
/// - 5: char
/// - 6: float
/// - 7: double
/// - 8: byte
/// - 9: short
/// - 10: int
/// - 11: long
#[inline]
pub fn handle_newarray(frame: &mut Frame, heap: &mut Heap, type_code: u8) -> VMResult<()> {
    let count = frame.pop()?.as_int()?;

    // Validate array size (JVMS §6.5.newarray)
    if count < 0 {
        return Err(VMError::NegativeArraySize(count));
    }

    let element_type = match type_code {
        4 => ArrayElementType::Boolean,
        5 => ArrayElementType::Char,
        6 => ArrayElementType::Float,
        7 => ArrayElementType::Double,
        8 => ArrayElementType::Byte,
        9 => ArrayElementType::Short,
        10 => ArrayElementType::Int,
        11 => ArrayElementType::Long,
        _ => return Err(VMError::InvalidBytecode),
    };

    let heap_id = heap.allocate_array(element_type, count)?;
    frame.push(Value::Reference(ObjectRef::Array(heap_id)))
}

/// Handle anewarray (0xbd) - Create new object array
#[inline]
pub fn handle_anewarray(
    frame: &mut Frame,
    heap: &mut Heap,
    element_class_name: String,
) -> VMResult<()> {
    let count = frame.pop()?.as_int()?;

    // Validate array size (JVMS §6.5.anewarray)
    if count < 0 {
        return Err(VMError::NegativeArraySize(count));
    }

    let element_type = ArrayElementType::Reference(element_class_name);
    let heap_id = heap.allocate_array(element_type, count)?;
    frame.push(Value::Reference(ObjectRef::Array(heap_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::heap::ArrayElementType;

    fn create_test_frame() -> Frame {
        Frame::new(
            "TestClass".to_string(),
            "testMethod".to_string(),
            10,
            10,
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn test_newarray_int() {
        let mut frame = create_test_frame();
        let mut heap = Heap::new(1024);

        frame.push(Value::Int(5)).unwrap();
        handle_newarray(&mut frame, &mut heap, 10).unwrap(); // 10 = int

        let array_ref = frame.pop().unwrap().as_reference().unwrap();
        assert!(!array_ref.is_null());
    }

    #[test]
    fn test_newarray_negative_size() {
        let mut frame = create_test_frame();
        let mut heap = Heap::new(1024);

        frame.push(Value::Int(-1)).unwrap();
        let result = handle_newarray(&mut frame, &mut heap, 10);
        assert!(matches!(result, Err(VMError::NegativeArraySize(-1))));
    }

    #[test]
    fn test_arraylength() {
        let mut frame = create_test_frame();
        let mut heap = Heap::new(1024);

        // Create an array of size 10
        let heap_id = heap.allocate_array(ArrayElementType::Int, 10).unwrap();
        frame
            .push(Value::Reference(ObjectRef::Array(heap_id)))
            .unwrap();

        handle_arraylength(&mut frame, &heap).unwrap();
        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 10);
    }

    #[test]
    fn test_iaload_iastore() {
        let mut frame = create_test_frame();
        let mut heap = Heap::new(1024);

        // Create an int array
        let heap_id = heap.allocate_array(ArrayElementType::Int, 5).unwrap();

        // Store value 42 at index 2
        frame
            .push(Value::Reference(ObjectRef::Array(heap_id)))
            .unwrap();
        frame.push(Value::Int(2)).unwrap(); // index
        frame.push(Value::Int(42)).unwrap(); // value
        handle_iastore(&mut frame, &mut heap).unwrap();

        // Load value from index 2
        frame
            .push(Value::Reference(ObjectRef::Array(heap_id)))
            .unwrap();
        frame.push(Value::Int(2)).unwrap(); // index
        handle_iaload(&mut frame, &heap).unwrap();

        assert_eq!(frame.pop().unwrap().as_int().unwrap(), 42);
    }
}
