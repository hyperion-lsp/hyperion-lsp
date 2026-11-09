// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Coroutine Stubs for Static Analysis
//!
//! This module provides minimal stubs for Kotlin coroutine types and functions.
//! We're NOT implementing full coroutine execution - just enough for static
//! analysis and basic invocation to not crash.
//!
//! Kotlin coroutines compile to state machines with:
//! - Continuation interface: suspend functions take Continuation as last param
//! - CoroutineContext: runtime context for coroutines
//! - suspend modifier: marks suspendable functions

use super::{GenericFunctionSignature, StdlibRegistry, TypeExpr};

impl StdlibRegistry {
    /// Register coroutine stubs for static analysis
    pub fn register_coroutine_stubs(&mut self) {
        self.register_coroutine_builders();
        self.register_deferred_and_job();
        self.register_flow_stubs();
        self.register_continuation_interface();
        self.register_coroutine_intrinsics();
    }

    /// Core coroutine builder functions
    fn register_coroutine_builders(&mut self) {
        // fun CoroutineScope.launch(block: suspend CoroutineScope.() -> Unit): Job
        self.register(
            GenericFunctionSignature::new("launch")
                .extension(TypeExpr::concrete("CoroutineScope"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::concrete("Job")),
        );

        // fun <T> CoroutineScope.async(block: suspend CoroutineScope.() -> T): Deferred<T>
        self.register(
            GenericFunctionSignature::new("async")
                .type_params(&["T"])
                .extension(TypeExpr::concrete("CoroutineScope"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("T")),
                })
                .returns(TypeExpr::generic("Deferred", vec![TypeExpr::type_var("T")])),
        );

        // fun <T> runBlocking(block: suspend CoroutineScope.() -> T): T
        self.register(
            GenericFunctionSignature::new("runBlocking")
                .type_params(&["T"])
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("T")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // suspend fun <T> withContext(context: CoroutineContext, block: ...): T
        self.register(
            GenericFunctionSignature::new("withContext")
                .type_params(&["T"])
                .param(TypeExpr::concrete("CoroutineContext"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("T")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // suspend fun <R> coroutineScope(block: suspend CoroutineScope.() -> R): R
        self.register(
            GenericFunctionSignature::new("coroutineScope")
                .type_params(&["R"])
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // suspend fun <R> supervisorScope(block: suspend CoroutineScope.() -> R): R
        self.register(
            GenericFunctionSignature::new("supervisorScope")
                .type_params(&["R"])
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // suspend fun delay(timeMillis: Long): Unit
        self.register(
            GenericFunctionSignature::new("delay")
                .param(TypeExpr::concrete("Long"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // suspend fun yield(): Unit
        self.register(GenericFunctionSignature::new("yield").returns(TypeExpr::concrete("Unit")));
    }

    /// Deferred and Job operations
    fn register_deferred_and_job(&mut self) {
        // suspend fun Deferred<T>.await(): T
        self.register(
            GenericFunctionSignature::new("await")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Deferred", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T")),
        );

        // fun Job.cancel(): Unit
        self.register(
            GenericFunctionSignature::new("cancel")
                .extension(TypeExpr::concrete("Job"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // suspend fun Job.join(): Unit
        self.register(
            GenericFunctionSignature::new("join")
                .extension(TypeExpr::concrete("Job"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // val Job.isActive: Boolean
        self.register(
            GenericFunctionSignature::new("isActive")
                .extension(TypeExpr::concrete("Job"))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // val Job.isCancelled: Boolean
        self.register(
            GenericFunctionSignature::new("isCancelled")
                .extension(TypeExpr::concrete("Job"))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // val Job.isCompleted: Boolean
        self.register(
            GenericFunctionSignature::new("isCompleted")
                .extension(TypeExpr::concrete("Job"))
                .returns(TypeExpr::concrete("Boolean")),
        );
    }

    /// Flow builder and collector stubs
    fn register_flow_stubs(&mut self) {
        // fun <T> flow(block: suspend FlowCollector<T>.() -> Unit): Flow<T>
        self.register(
            GenericFunctionSignature::new("flow")
                .type_params(&["T"])
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::generic("Flow", vec![TypeExpr::type_var("T")])),
        );

        // fun <T> flowOf(vararg elements: T): Flow<T>
        self.register(
            GenericFunctionSignature::new("flowOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("Flow", vec![TypeExpr::type_var("T")])),
        );

        // suspend fun <T> Flow<T>.collect(collector: (T) -> Unit): Unit
        self.register(
            GenericFunctionSignature::new("collect")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Flow", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::concrete("Unit")),
        );

        // suspend fun <T> Flow<T>.first(): T
        self.register(
            GenericFunctionSignature::new("first")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Flow", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T")),
        );

        // suspend fun <T> Flow<T>.toList(): List<T>
        self.register(
            GenericFunctionSignature::new("toList")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Flow", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );
    }

    /// Continuation interface stubs
    ///
    /// interface Continuation<in T> {
    ///     val context: CoroutineContext
    ///     fun resumeWith(result: Result<T>)
    /// }
    fn register_continuation_interface(&mut self) {
        // Continuation<T>.context: CoroutineContext
        self.register(
            GenericFunctionSignature::new("context")
                .type_params(&["T"])
                .extension(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .returns(TypeExpr::concrete("CoroutineContext")),
        );

        // Continuation<T>.resumeWith(result: Result<T>): Unit
        self.register(
            GenericFunctionSignature::new("resumeWith")
                .type_params(&["T"])
                .extension(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .param(TypeExpr::generic("Result", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Unit")),
        );

        // Continuation<T>.resume(value: T): Unit (extension function)
        self.register(
            GenericFunctionSignature::new("resume")
                .type_params(&["T"])
                .extension(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .param(TypeExpr::type_var("T"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // Continuation<T>.resumeWithException(exception: Throwable): Unit
        self.register(
            GenericFunctionSignature::new("resumeWithException")
                .type_params(&["T"])
                .extension(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .param(TypeExpr::concrete("Throwable"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // CoroutineContext.plus(context: CoroutineContext): CoroutineContext
        self.register(
            GenericFunctionSignature::new("plus")
                .extension(TypeExpr::concrete("CoroutineContext"))
                .param(TypeExpr::concrete("CoroutineContext"))
                .returns(TypeExpr::concrete("CoroutineContext")),
        );
    }

    /// Coroutine intrinsics from kotlin.coroutines.intrinsics
    ///
    /// These are the fundamental intrinsics for implementing suspend functions.
    fn register_coroutine_intrinsics(&mut self) {
        // suspend fun <T> suspendCoroutineUninterceptedOrReturn(
        //     block: (Continuation<T>) -> Any?
        // ): T
        //
        // Returns COROUTINE_SUSPENDED if suspended, or the result directly.
        self.register(
            GenericFunctionSignature::new("suspendCoroutineUninterceptedOrReturn")
                .type_params(&["T"])
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::generic(
                        "Continuation",
                        vec![TypeExpr::type_var("T")],
                    )],
                    ret: Box::new(TypeExpr::concrete("Any").nullable()),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // suspend fun <T> suspendCoroutine(block: (Continuation<T>) -> Unit): T
        self.register(
            GenericFunctionSignature::new("suspendCoroutine")
                .type_params(&["T"])
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::generic(
                        "Continuation",
                        vec![TypeExpr::type_var("T")],
                    )],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // fun <T> Continuation<T>.intercepted(): Continuation<T>
        self.register(
            GenericFunctionSignature::new("intercepted")
                .type_params(&["T"])
                .extension(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .returns(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                )),
        );

        // fun <T> createCoroutineUnintercepted(completion: Continuation<T>): Continuation<Unit>
        self.register(
            GenericFunctionSignature::new("createCoroutineUnintercepted")
                .type_params(&["T"])
                .param(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .returns(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::concrete("Unit")],
                )),
        );

        // fun <T> startCoroutineUninterceptedOrReturn(completion: Continuation<T>): Any?
        self.register(
            GenericFunctionSignature::new("startCoroutineUninterceptedOrReturn")
                .type_params(&["T"])
                .param(TypeExpr::generic(
                    "Continuation",
                    vec![TypeExpr::type_var("T")],
                ))
                .returns(TypeExpr::concrete("Any").nullable()),
        );

        // Result<T>.getOrNull(): T?
        self.register(
            GenericFunctionSignature::new("getOrNull")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Result", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // Result<T>.getOrThrow(): T
        self.register(
            GenericFunctionSignature::new("getOrThrow")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Result", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T")),
        );

        // Result<T>.isSuccess: Boolean
        self.register(
            GenericFunctionSignature::new("isSuccess")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Result", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // Result<T>.isFailure: Boolean
        self.register(
            GenericFunctionSignature::new("isFailure")
                .type_params(&["T"])
                .extension(TypeExpr::generic("Result", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Boolean")),
        );
    }
}

/// Coroutine type FQN mappings
pub fn resolve_coroutine_type_name(name: &str) -> Option<&'static str> {
    match name {
        // Core coroutine types
        "Job" => Some("kotlinx.coroutines.Job"),
        "Deferred" => Some("kotlinx.coroutines.Deferred"),
        "CoroutineScope" => Some("kotlinx.coroutines.CoroutineScope"),
        "CoroutineContext" => Some("kotlin.coroutines.CoroutineContext"),
        "CoroutineStart" => Some("kotlinx.coroutines.CoroutineStart"),
        "Dispatchers" => Some("kotlinx.coroutines.Dispatchers"),
        "Continuation" => Some("kotlin.coroutines.Continuation"),
        "Result" => Some("kotlin.Result"),
        // Flow types
        "Flow" => Some("kotlinx.coroutines.flow.Flow"),
        "FlowCollector" => Some("kotlinx.coroutines.flow.FlowCollector"),
        "StateFlow" => Some("kotlinx.coroutines.flow.StateFlow"),
        "SharedFlow" => Some("kotlinx.coroutines.flow.SharedFlow"),
        "MutableStateFlow" => Some("kotlinx.coroutines.flow.MutableStateFlow"),
        "MutableSharedFlow" => Some("kotlinx.coroutines.flow.MutableSharedFlow"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::STDLIB;
    use std::collections::HashMap;

    #[test]
    fn test_coroutine_builders_registered() {
        assert!(STDLIB.lookup("launch").is_some());
        assert!(STDLIB.lookup("async").is_some());
        assert!(STDLIB.lookup("runBlocking").is_some());
        assert!(STDLIB.lookup("withContext").is_some());
        assert!(STDLIB.lookup("coroutineScope").is_some());
        assert!(STDLIB.lookup("supervisorScope").is_some());
        assert!(STDLIB.lookup("delay").is_some());
    }

    #[test]
    fn test_launch_returns_job() {
        let sigs = STDLIB.lookup("launch").unwrap();
        let sig = &sigs[0];
        assert!(sig.is_extension);
        let return_type = sig.return_type.to_kotlin_type(&HashMap::new());
        assert_eq!(return_type.to_string(), "Job");
    }

    #[test]
    fn test_async_returns_deferred() {
        use crate::types::KotlinType;
        let sigs = STDLIB.lookup("async").unwrap();
        let sig = &sigs[0];
        assert!(sig.is_extension);
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());
        let return_type = sig.return_type.to_kotlin_type(&subs);
        assert_eq!(return_type.to_string(), "Deferred<String>");
    }

    #[test]
    fn test_await_registered() {
        let sigs = STDLIB.lookup("await").unwrap();
        let sig = &sigs[0];
        assert!(sig.is_extension);
    }

    #[test]
    fn test_continuation_interface_registered() {
        assert!(STDLIB.lookup("resumeWith").is_some());
        assert!(STDLIB.lookup("resume").is_some());
        assert!(STDLIB.lookup("resumeWithException").is_some());
    }

    #[test]
    fn test_coroutine_intrinsics_registered() {
        assert!(STDLIB
            .lookup("suspendCoroutineUninterceptedOrReturn")
            .is_some());
        assert!(STDLIB.lookup("suspendCoroutine").is_some());
        assert!(STDLIB.lookup("intercepted").is_some());
    }

    #[test]
    fn test_flow_stubs_registered() {
        assert!(STDLIB.lookup("flowOf").is_some());
        assert!(STDLIB.lookup("collect").is_some());
    }

    #[test]
    fn test_coroutine_type_fqn() {
        assert_eq!(
            resolve_coroutine_type_name("Job"),
            Some("kotlinx.coroutines.Job")
        );
        assert_eq!(
            resolve_coroutine_type_name("Continuation"),
            Some("kotlin.coroutines.Continuation")
        );
        assert_eq!(
            resolve_coroutine_type_name("Flow"),
            Some("kotlinx.coroutines.flow.Flow")
        );
    }
}
