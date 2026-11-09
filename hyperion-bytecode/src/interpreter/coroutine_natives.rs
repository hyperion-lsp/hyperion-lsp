// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Coroutine Native Stubs
//!
//! This module provides minimal stubs for Kotlin coroutine intrinsics.
//! We're NOT implementing full coroutine execution - just enough for static
//! analysis and basic invocation to not crash.

use crate::interpreter::{Heap, ObjectRef, VMError, VMResult, Value};

/// Register all Kotlin coroutine stubs on a NativeRegistry
pub fn register_coroutine_natives(registry: &mut super::natives::NativeRegistry) {
    // kotlin.coroutines.intrinsics.IntrinsicsKt
    registry.register(
        "kotlin/coroutines/intrinsics/IntrinsicsKt",
        "getCOROUTINE_SUSPENDED",
        "()Ljava/lang/Object;",
        coroutine_get_suspended,
    );

    // kotlin.coroutines.Continuation
    registry.register(
        "kotlin/coroutines/Continuation",
        "resumeWith",
        "(Ljava/lang/Object;)V",
        continuation_resume_with,
    );
    registry.register(
        "kotlin/coroutines/Continuation",
        "getContext",
        "()Lkotlin/coroutines/CoroutineContext;",
        continuation_get_context,
    );

    // kotlin.coroutines.jvm.internal.ContinuationImpl
    registry.register(
        "kotlin/coroutines/jvm/internal/ContinuationImpl",
        "intercepted",
        "()Lkotlin/coroutines/Continuation;",
        continuation_intercepted,
    );

    // kotlin.coroutines.EmptyCoroutineContext
    registry.register(
        "kotlin/coroutines/EmptyCoroutineContext",
        "INSTANCE",
        "Lkotlin/coroutines/CoroutineContext;",
        empty_coroutine_context_instance,
    );

    // kotlin.coroutines.CoroutineContext
    registry.register(
        "kotlin/coroutines/CoroutineContext",
        "plus",
        "(Lkotlin/coroutines/CoroutineContext;)Lkotlin/coroutines/CoroutineContext;",
        coroutine_context_plus,
    );

    // kotlin.Result
    registry.register(
        "kotlin/Result",
        "isSuccess-impl",
        "(Ljava/lang/Object;)Z",
        result_is_success,
    );
    registry.register(
        "kotlin/Result",
        "isFailure-impl",
        "(Ljava/lang/Object;)Z",
        result_is_failure,
    );
    registry.register(
        "kotlin/Result",
        "getOrNull-impl",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        result_get_or_null,
    );
    registry.register(
        "kotlin/Result",
        "getOrThrow-impl",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        result_get_or_throw,
    );
}

/// COROUTINE_SUSPENDED marker object
/// Returns a special marker value indicating suspension
pub fn coroutine_get_suspended(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // A stable, non-null identity. Kotlin compares results with `===`, so a null marker
    // would make every function that returns null look suspended. The interned Class
    // object is unique per name, which is all the comparison needs.
    let marker = heap.intern_class(COROUTINE_SUSPENDED_MARKER)?;
    Ok(Some(Value::Reference(marker)))
}

/// Internal name whose interned Class object stands for COROUTINE_SUSPENDED.
const COROUTINE_SUSPENDED_MARKER: &str = "kotlin/coroutines/intrinsics/CoroutineSingletons";

/// Continuation.resumeWith - stub implementation
pub fn continuation_resume_with(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // Resuming needs a suspended coroutine, which this runtime never produces.
    Err(VMError::internal(
        "Continuation.resumeWith is not modelled: suspended coroutines cannot be resumed",
    ))
}

/// Continuation.getContext - returns CoroutineContext
pub fn continuation_get_context(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // A continuation keeps no context, so there is nothing real to return.
    Err(VMError::internal(
        "Continuation.getContext is not modelled: continuations keep no context",
    ))
}

/// Continuation.intercepted - returns intercepted continuation
pub fn continuation_intercepted(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    // Continuations keep no context, so no ContinuationInterceptor is ever present, and
    // intercepted() is the continuation itself. That is the only model this runtime has.
    if args.is_empty() {
        return Ok(Some(Value::Reference(ObjectRef::Null)));
    }
    Ok(Some(args[0].clone()))
}

/// EmptyCoroutineContext.INSTANCE
pub fn empty_coroutine_context_instance(
    _heap: &mut Heap,
    _args: &[Value],
) -> VMResult<Option<Value>> {
    // Return a null reference as placeholder for empty context
    Ok(Some(Value::Reference(ObjectRef::Null)))
}

/// CoroutineContext.plus - combines contexts
pub fn coroutine_context_plus(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    // The empty context is null, and combining it with a context returns that context.
    // Combining two real contexts would need their elements merged, which is not modelled.
    let is_empty = |value: &Value| matches!(value, Value::Reference(ObjectRef::Null));
    match args {
        [this, other] if is_empty(this) => Ok(Some(other.clone())),
        [this, other] if is_empty(other) => Ok(Some(this.clone())),
        [_, _] => Err(VMError::internal(
            "CoroutineContext.plus of two non-empty contexts is not modelled",
        )),
        _ => Err(VMError::internal(
            "CoroutineContext.plus needs two operands",
        )),
    }
}

/// Result.isSuccess
pub fn result_is_success(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Ok(Some(Value::Int(1))) // true
}

/// Result.isFailure
pub fn result_is_failure(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Ok(Some(Value::Int(0))) // false
}

/// Result.getOrNull
pub fn result_get_or_null(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Ok(Some(Value::Reference(ObjectRef::Null)))
}

/// Result.getOrThrow
pub fn result_get_or_throw(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Ok(Some(Value::Reference(ObjectRef::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::Heap;

    fn test_heap() -> Heap {
        Heap::new(1024 * 1024)
    }

    #[test]
    fn test_coroutine_get_suspended() {
        let mut heap = test_heap();
        let result = coroutine_get_suspended(&mut heap, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_result_is_success() {
        let mut heap = test_heap();
        let result = result_is_success(&mut heap, &[]);
        assert!(result.is_ok());
        match result.unwrap() {
            Some(Value::Int(1)) => {}
            _ => panic!("Expected true (1)"),
        }
    }

    #[test]
    fn plus_with_the_empty_context_returns_the_other_context() {
        let mut heap = Heap::new(1024 * 1024);
        let context = Value::Reference(heap.intern_string("context").unwrap());
        let empty = Value::Reference(ObjectRef::Null);

        let left = coroutine_context_plus(&mut heap, &[empty.clone(), context.clone()]);
        let right = coroutine_context_plus(&mut heap, &[context.clone(), empty]);

        assert_eq!(left.unwrap(), Some(context.clone()));
        assert_eq!(right.unwrap(), Some(context));
    }

    #[test]
    fn plus_of_two_real_contexts_is_rejected_not_guessed() {
        let mut heap = Heap::new(1024 * 1024);
        let first = Value::Reference(heap.intern_string("first").unwrap());
        let second = Value::Reference(heap.intern_string("second").unwrap());

        assert!(coroutine_context_plus(&mut heap, &[first, second]).is_err());
    }

    #[test]
    fn suspended_marker_is_stable_and_never_null() {
        let mut heap = Heap::new(1024 * 1024);
        let first = coroutine_get_suspended(&mut heap, &[]).unwrap();
        let second = coroutine_get_suspended(&mut heap, &[]).unwrap();

        assert_eq!(first, second);
        assert!(!matches!(
            first,
            Some(Value::Reference(ObjectRef::Null)) | None
        ));
    }
}
