// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM value types
//!
//! Represents all possible values in the JVM, including primitives and references.

use super::errors::{VMError, VMResult};
use super::method_handles::MethodHandleId;
use std::fmt;

/// A JVM value - either a primitive or a reference
#[derive(Clone, Debug)]
pub enum Value {
    // Category 1 (32-bit)
    Int(i32),
    Float(f32),
    Reference(ObjectRef),
    ReturnAddress(u32),

    // Category 2 (64-bit, take 2 stack slots)
    Long(i64),
    Double(f64),

    // Placeholder for the second slot of category-2 values in local variable array
    // Per JVM spec §2.11.1, long and double occupy two consecutive local variable slots.
    // The actual value is stored in slot N, and slot N+1 contains this placeholder.
    CategoryTwoPlaceholder,
}

impl Value {
    /// Returns true if this value occupies 2 stack slots
    pub fn is_category2(&self) -> bool {
        matches!(self, Value::Long(_) | Value::Double(_))
    }

    /// Get the type name for debugging
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Long(_) => "long",
            Value::Double(_) => "double",
            Value::Reference(_) => "reference",
            Value::ReturnAddress(_) => "return_address",
            Value::CategoryTwoPlaceholder => "category_two_placeholder",
        }
    }

    /// Convert to int
    pub fn as_int(&self) -> VMResult<i32> {
        match self {
            Value::Int(v) => Ok(*v),
            _ => Err(VMError::type_mismatch("int", self.type_name())),
        }
    }

    /// Convert to long
    pub fn as_long(&self) -> VMResult<i64> {
        match self {
            Value::Long(v) => Ok(*v),
            _ => Err(VMError::type_mismatch("long", self.type_name())),
        }
    }

    /// Convert to float
    pub fn as_float(&self) -> VMResult<f32> {
        match self {
            Value::Float(v) => Ok(*v),
            _ => Err(VMError::type_mismatch("float", self.type_name())),
        }
    }

    /// Convert to double
    pub fn as_double(&self) -> VMResult<f64> {
        match self {
            Value::Double(v) => Ok(*v),
            _ => Err(VMError::type_mismatch("double", self.type_name())),
        }
    }

    /// Convert to reference
    pub fn as_reference(&self) -> VMResult<ObjectRef> {
        match self {
            Value::Reference(r) => Ok(*r),
            _ => Err(VMError::type_mismatch("reference", self.type_name())),
        }
    }

    /// Convert to return address
    pub fn as_return_address(&self) -> VMResult<u32> {
        match self {
            Value::ReturnAddress(addr) => Ok(*addr),
            _ => Err(VMError::type_mismatch("return_address", self.type_name())),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{}", v),
            Value::Float(v) => write!(f, "{}", v),
            Value::Long(v) => write!(f, "{}L", v),
            Value::Double(v) => write!(f, "{}d", v),
            Value::Reference(r) => write!(f, "{:?}", r),
            Value::ReturnAddress(addr) => write!(f, "@{}", addr),
            Value::CategoryTwoPlaceholder => write!(f, "<cat2-placeholder>"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Double(a), Value::Double(b)) => a == b,
            (Value::Reference(a), Value::Reference(b)) => a == b,
            (Value::ReturnAddress(a), Value::ReturnAddress(b)) => a == b,
            (Value::CategoryTwoPlaceholder, Value::CategoryTwoPlaceholder) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Int(v) => v.hash(state),
            Value::Float(v) => v.to_bits().hash(state),
            Value::Long(v) => v.hash(state),
            Value::Double(v) => v.to_bits().hash(state),
            Value::Reference(r) => r.hash(state),
            Value::ReturnAddress(addr) => addr.hash(state),
            Value::CategoryTwoPlaceholder => {}
        }
    }
}

/// Reference to a heap object or array
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectRef {
    /// Null reference
    Null,
    /// Reference to an object or array
    Object(HeapId),
    /// Reference to an array (same as Object, just for clarity)
    Array(HeapId),
    /// Reference to a method handle
    MethodHandle(MethodHandleId),
}

impl ObjectRef {
    /// Check if this is a null reference
    pub fn is_null(&self) -> bool {
        matches!(self, ObjectRef::Null)
    }

    /// Get the heap ID if this is a heap object
    pub fn heap_id(&self) -> Option<HeapId> {
        match self {
            ObjectRef::Null => None,
            ObjectRef::Object(id) | ObjectRef::Array(id) => Some(*id),
            ObjectRef::MethodHandle(_) => None,
        }
    }

    /// Get the method handle ID if this is a method handle
    pub fn method_handle_id(&self) -> Option<MethodHandleId> {
        match self {
            ObjectRef::MethodHandle(id) => Some(*id),
            _ => None,
        }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectRef::Null => write!(f, "null"),
            ObjectRef::Object(id) | ObjectRef::Array(id) => write!(f, "ref({})", id.0),
            ObjectRef::MethodHandle(id) => write!(f, "handle({})", id.0),
        }
    }
}

/// Unique identifier for heap objects
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HeapId(pub u32);

impl fmt::Display for HeapId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_category() {
        assert!(!Value::Int(42).is_category2());
        assert!(!Value::Float(3.14).is_category2());
        assert!(Value::Long(42).is_category2());
        assert!(Value::Double(3.14).is_category2());
    }

    #[test]
    fn test_value_conversion() {
        match Value::Int(42).as_int() {
            Ok(v) => assert_eq!(v, 42),
            Err(_) => panic!("Expected Int conversion to succeed"),
        }
        assert!(Value::Int(42).as_long().is_err());

        match Value::Long(42).as_long() {
            Ok(v) => assert_eq!(v, 42),
            Err(_) => panic!("Expected Long conversion to succeed"),
        }
        assert!(Value::Long(42).as_int().is_err());
    }

    #[test]
    fn test_object_ref() {
        assert!(ObjectRef::Null.is_null());
        assert!(!ObjectRef::Object(HeapId(1)).is_null());

        assert_eq!(ObjectRef::Null.heap_id(), None);
        assert_eq!(ObjectRef::Object(HeapId(1)).heap_id(), Some(HeapId(1)));
    }
}
