// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin null safety intrinsic method implementations
//!
//! This module provides implementations for Kotlin's null safety checks at runtime.
//! When Kotlin code uses non-null types or the !! operator, the compiler
//! inserts calls to these intrinsics to enforce null safety at runtime.

use crate::interpreter::{Heap, ObjectRef, VMError, VMResult, Value};

/// Register all Kotlin intrinsics on a NativeRegistry
pub fn register_kotlin_intrinsics(registry: &mut super::natives::NativeRegistry) {
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkNotNull",
        "(Ljava/lang/Object;)V",
        kotlin_check_not_null,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkNotNull",
        "(Ljava/lang/Object;Ljava/lang/String;)V",
        kotlin_check_not_null_with_message,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkParameterIsNotNull",
        "(Ljava/lang/Object;Ljava/lang/String;)V",
        kotlin_check_parameter_is_not_null,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkNotNullParameter",
        "(Ljava/lang/Object;Ljava/lang/String;)V",
        kotlin_check_not_null_parameter,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkExpressionValueIsNotNull",
        "(Ljava/lang/Object;Ljava/lang/String;)V",
        kotlin_check_expression_value_is_not_null,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkReturnedValueIsNotNull",
        "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;)V",
        kotlin_check_returned_value_is_not_null,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "checkFieldIsNotNull",
        "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;)V",
        kotlin_check_field_is_not_null,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "throwNpe",
        "()V",
        kotlin_throw_npe,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "throwUninitializedPropertyAccessException",
        "(Ljava/lang/String;)V",
        kotlin_throw_uninitialized_property,
    );
    registry.register(
        "kotlin/jvm/internal/Intrinsics",
        "areEqual",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        kotlin_are_equal,
    );
}

/// kotlin.jvm.internal.Intrinsics.checkNotNull(Object value)
///
/// Used for the !! (not-null assertion) operator.
/// Throws NullPointerException if value is null.
fn kotlin_check_not_null(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None), // Value is not null, continue execution
    }
}

/// kotlin.jvm.internal.Intrinsics.checkNotNull(Object value, String message)
///
/// Used for !! with custom error message.
fn kotlin_check_not_null_with_message(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.checkParameterIsNotNull(Object value, String paramName)
///
/// Legacy method for non-null parameter checking at function entry.
fn kotlin_check_parameter_is_not_null(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkParameterIsNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.checkNotNullParameter(Object value, String paramName)
///
/// Modern method for non-null parameter checking (Kotlin 1.4+).
fn kotlin_check_not_null_parameter(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkNotNullParameter requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.checkExpressionValueIsNotNull(Object value, String expression)
///
/// Used when a platform type is assigned to a non-null Kotlin type.
fn kotlin_check_expression_value_is_not_null(
    _heap: &mut Heap,
    args: &[Value],
) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkExpressionValueIsNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.checkReturnedValueIsNotNull(Object value, String className, String methodName)
fn kotlin_check_returned_value_is_not_null(
    _heap: &mut Heap,
    args: &[Value],
) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkReturnedValueIsNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.checkFieldIsNotNull(Object value, String className, String fieldName)
fn kotlin_check_field_is_not_null(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::InternalError(
            "checkFieldIsNotNull requires value argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Ok(None),
    }
}

/// kotlin.jvm.internal.Intrinsics.throwNpe()
///
/// Directly throws a NullPointerException.
fn kotlin_throw_npe(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Err(VMError::NullPointerException)
}

/// kotlin.jvm.internal.Intrinsics.throwUninitializedPropertyAccessException(String propertyName)
///
/// Thrown when accessing a lateinit property before initialization.
fn kotlin_throw_uninitialized_property(
    _heap: &mut Heap,
    _args: &[Value],
) -> VMResult<Option<Value>> {
    Err(VMError::InternalError(
        "lateinit property accessed before initialization".to_string(),
    ))
}

/// kotlin.jvm.internal.Intrinsics.areEqual(Object first, Object second)
///
/// Structural equality check used for == operator in Kotlin.
fn kotlin_are_equal(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::InternalError(
            "areEqual requires two arguments".to_string(),
        ));
    }

    let first = &args[0];
    let second = &args[1];

    let result = match (first, second) {
        // Both null -> equal
        (Value::Reference(ObjectRef::Null), Value::Reference(ObjectRef::Null)) => true,
        // One null, one not -> not equal
        (Value::Reference(ObjectRef::Null), _) | (_, Value::Reference(ObjectRef::Null)) => false,
        // Both are same object reference -> equal
        (Value::Reference(ObjectRef::Object(id1)), Value::Reference(ObjectRef::Object(id2))) => {
            id1.0 == id2.0
        }
        // Primitive comparisons
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Long(a), Value::Long(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Double(a), Value::Double(b)) => a == b,
        // Default: not equal for different types
        _ => false,
    };

    Ok(Some(Value::Int(if result { 1 } else { 0 })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::HeapId;

    fn test_heap() -> Heap {
        Heap::new(1024 * 1024)
    }

    #[test]
    fn test_kotlin_check_not_null_with_null() {
        let mut heap = test_heap();
        let result = kotlin_check_not_null(&mut heap, &[Value::Reference(ObjectRef::Null)]);
        assert!(matches!(result, Err(VMError::NullPointerException)));
    }

    #[test]
    fn test_kotlin_check_not_null_with_object() {
        let mut heap = test_heap();
        let result =
            kotlin_check_not_null(&mut heap, &[Value::Reference(ObjectRef::Object(HeapId(1)))]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_kotlin_throw_npe() {
        let mut heap = test_heap();
        let result = kotlin_throw_npe(&mut heap, &[]);
        assert!(matches!(result, Err(VMError::NullPointerException)));
    }

    #[test]
    fn test_kotlin_are_equal_both_null() {
        let mut heap = test_heap();
        let result = kotlin_are_equal(
            &mut heap,
            &[
                Value::Reference(ObjectRef::Null),
                Value::Reference(ObjectRef::Null),
            ],
        );
        assert!(matches!(result, Ok(Some(Value::Int(1)))));
    }

    #[test]
    fn test_kotlin_are_equal_one_null() {
        let mut heap = test_heap();
        let result = kotlin_are_equal(
            &mut heap,
            &[
                Value::Reference(ObjectRef::Null),
                Value::Reference(ObjectRef::Object(HeapId(1))),
            ],
        );
        assert!(matches!(result, Ok(Some(Value::Int(0)))));
    }

    #[test]
    fn test_kotlin_are_equal_same_object() {
        let mut heap = test_heap();
        let result = kotlin_are_equal(
            &mut heap,
            &[
                Value::Reference(ObjectRef::Object(HeapId(42))),
                Value::Reference(ObjectRef::Object(HeapId(42))),
            ],
        );
        assert!(matches!(result, Ok(Some(Value::Int(1)))));
    }

    #[test]
    fn test_kotlin_are_equal_different_objects() {
        let mut heap = test_heap();
        let result = kotlin_are_equal(
            &mut heap,
            &[
                Value::Reference(ObjectRef::Object(HeapId(1))),
                Value::Reference(ObjectRef::Object(HeapId(2))),
            ],
        );
        assert!(matches!(result, Ok(Some(Value::Int(0)))));
    }

    #[test]
    fn test_kotlin_are_equal_primitives() {
        let mut heap = test_heap();
        let result = kotlin_are_equal(&mut heap, &[Value::Int(42), Value::Int(42)]);
        assert!(matches!(result, Ok(Some(Value::Int(1)))));

        let result = kotlin_are_equal(&mut heap, &[Value::Int(1), Value::Int(2)]);
        assert!(matches!(result, Ok(Some(Value::Int(0)))));
    }
}
