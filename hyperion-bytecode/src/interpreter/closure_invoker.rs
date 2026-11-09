// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Closure Invocation
//!
//! Handles invocation of closure doCall() methods with proper frame setup.
//! Integrates with existing Executor method dispatch infrastructure.

use super::errors::{VMError, VMResult};
use super::executor::MethodBytecode;
use super::frame::Frame;
use super::heap::{ArrayElementType, Heap};
use super::value::{HeapId, ObjectRef, Value};
use std::collections::HashMap;

/// Handles closure invocation by creating proper execution frames
///
/// Architecture:
/// ```text
/// GradleNativeBridge.handle_project_dependencies(closure)
///     ↓
/// ClosureRuntime::invoke_do_call(closure_id, args)
///     ↓
/// ClosureInvoker::create_do_call_frame(closure_id, args)
///     ↓
/// Thread.push_frame(frame)
///     ↓
/// Executor.step() executes closure bytecode
/// ```
pub struct ClosureInvoker;

impl ClosureInvoker {
    /// Create a frame for invoking closure.doCall()
    ///
    /// This is the main entry point for closure invocation.
    ///
    /// # Frame Setup
    ///
    /// ```text
    /// Locals:
    ///   slot 0: this (closure object reference)
    ///   slot 1: args (Object[] array reference)
    ///   slot 2+: available for local variables
    /// ```
    ///
    /// # Method Signature
    ///
    /// doCall() signature matches the closure parameters:
    /// - no params: `()Ljava/lang/Object;`
    /// - N params: `(Ljava/lang/Object;...N times...)Ljava/lang/Object;`
    ///   Falls back to the array form `([Ljava/lang/Object;)Ljava/lang/Object;` when needed.
    ///
    /// # Arguments
    ///
    /// - `heap`: For reading closure class metadata and allocating args array
    /// - `method_registry`: Contains registered doCall bytecode for closure classes
    /// - `closure_id`: HeapId of the closure object to invoke
    /// - `args`: Arguments to pass to doCall (will be packed into Object[] array)
    pub fn create_do_call_frame(
        heap: &mut Heap,
        method_registry: &HashMap<(String, String, String), MethodBytecode>,
        closure_id: HeapId,
        args: Vec<Value>,
    ) -> VMResult<Frame> {
        // 1. Get closure class name from heap
        let class_name = heap.get_object_class_name(closure_id)?;

        // 2. Look up doCall method in registry
        let mut call_args = args;
        let direct_descriptor = Self::build_direct_descriptor(call_args.len());
        let array_descriptor = "([Ljava/lang/Object;)Ljava/lang/Object;".to_string();

        let mut method_key = (
            class_name.clone(),
            "doCall".to_string(),
            direct_descriptor.clone(),
        );
        let mut resolved_descriptor = direct_descriptor.clone();
        let mut uses_array_signature = false;

        let bytecode = match method_registry.get(&method_key) {
            Some(bytecode) => bytecode,
            None => {
                if call_args.is_empty() {
                    let implicit_descriptor = Self::build_direct_descriptor(1);
                    let implicit_key = (
                        class_name.clone(),
                        "doCall".to_string(),
                        implicit_descriptor.clone(),
                    );
                    if let Some(bytecode) = method_registry.get(&implicit_key) {
                        call_args = vec![Value::Reference(ObjectRef::Null)];
                        return Frame::with_bytecode(
                            class_name,
                            "doCall".to_string(),
                            bytecode.max_locals,
                            bytecode.max_stack,
                            {
                                let mut locals_vec =
                                    vec![Value::Reference(ObjectRef::Object(closure_id))];
                                locals_vec.extend(call_args);
                                locals_vec
                            },
                            bytecode.code.clone(),
                            implicit_descriptor,
                            bytecode.exception_table.clone(),
                        );
                    }
                }

                method_key = (
                    class_name.clone(),
                    "doCall".to_string(),
                    array_descriptor.clone(),
                );
                if let Some(bytecode) = method_registry.get(&method_key) {
                    uses_array_signature = true;
                    resolved_descriptor = array_descriptor.clone();
                    bytecode
                } else if let Some(((_, _, found_desc), found_bc)) = method_registry
                    .iter()
                    .find(|((cn, mn, _), _)| cn == &class_name && mn == "doCall")
                {
                    // General fallback: any `doCall` on this class, adapted to its
                    // declared arity. The exact-arity and array lookups above miss
                    // a closure whose `doCall` is registered under a different
                    // shape — e.g. a 2-parameter closure invoked through the DGM
                    // path (`list.inject(0) { a, b -> … }`). The dynamic-dispatch
                    // path already resolves `doCall` by name; mirror that here.
                    let mut locals_vec = vec![Value::Reference(ObjectRef::Object(closure_id))];
                    if found_desc.starts_with("([Ljava/lang/Object;)") {
                        locals_vec.push(Self::pack_args_array(heap, call_args)?);
                    } else {
                        let arity = found_desc[..found_desc.find(')').unwrap_or(0)]
                            .matches("Ljava/lang/Object;")
                            .count();
                        while call_args.len() < arity {
                            call_args.push(Value::Reference(ObjectRef::Null));
                        }
                        call_args.truncate(arity);
                        locals_vec.extend(call_args);
                    }
                    return Frame::with_bytecode(
                        class_name,
                        "doCall".to_string(),
                        found_bc.max_locals,
                        found_bc.max_stack,
                        locals_vec,
                        found_bc.code.clone(),
                        found_desc.clone(),
                        found_bc.exception_table.clone(),
                    );
                } else {
                    return Err(VMError::NoSuchMethod(format!("{}.doCall", class_name)));
                }
            }
        };

        // 3. Build locals: slot 0 = this (closure), then args
        let mut locals_vec = vec![Value::Reference(ObjectRef::Object(closure_id))];
        if uses_array_signature {
            locals_vec.push(Self::pack_args_array(heap, call_args)?);
        } else {
            locals_vec.extend(call_args);
        }

        // 4. Create execution frame
        Frame::with_bytecode(
            class_name,
            "doCall".to_string(),
            bytecode.max_locals,
            bytecode.max_stack,
            locals_vec,
            bytecode.code.clone(),
            if uses_array_signature {
                array_descriptor
            } else {
                resolved_descriptor
            },
            bytecode.exception_table.clone(),
        )
    }

    /// Pack arguments into Object[] array on heap
    ///
    /// Groovy closures can receive arguments as `Object[] args` in doCall().
    /// This method:
    /// 1. Allocates Object[] array on heap
    /// 2. Stores all argument values in the array
    /// 3. Returns array reference as Value
    fn pack_args_array(heap: &mut Heap, args: Vec<Value>) -> VMResult<Value> {
        // Special case: no arguments -> return null (Groovy convention for empty args)
        if args.is_empty() {
            return Ok(Value::Reference(ObjectRef::Null));
        }

        // Allocate Object[] array
        let array_id = heap.allocate_array(
            ArrayElementType::Reference("java/lang/Object".to_string()),
            args.len() as i32,
        )?;

        // Store each argument in the array
        for (i, arg) in args.into_iter().enumerate() {
            heap.array_store(array_id, i as i32, arg)?;
        }

        Ok(Value::Reference(ObjectRef::Object(array_id)))
    }

    fn build_direct_descriptor(arity: usize) -> String {
        let mut descriptor = String::from("(");
        for _ in 0..arity {
            descriptor.push_str("Ljava/lang/Object;");
        }
        descriptor.push_str(")Ljava/lang/Object;");
        descriptor
    }

    /// Extract closure class name from heap object
    ///
    /// This is a convenience method that wraps heap.get_object_class_name()
    /// with better error messages for closure-specific scenarios.
    pub fn get_closure_class_name(heap: &Heap, closure_id: HeapId) -> VMResult<String> {
        heap.get_object_class_name(closure_id)
            .map_err(|_| VMError::internal(format!("Invalid closure reference: {:?}", closure_id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::MemoryConfig;

    #[test]
    fn test_pack_args_array_empty() {
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        let result = ClosureInvoker::pack_args_array(&mut heap, vec![]);
        assert!(result.is_ok());

        // Empty args should return null
        match result.unwrap() {
            Value::Reference(ObjectRef::Null) => {} // Expected
            other => panic!("Expected null for empty args, got: {:?}", other),
        }
    }

    #[test]
    fn test_pack_args_array_with_values() {
        use super::super::heap::ClassRef;

        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        // Create some object references to use as arguments
        // (In real Groovy code, closure args are always object references)
        let class_ref = ClassRef {
            name: "TestObject".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };

        let obj1 = heap.allocate_object(class_ref.clone()).unwrap();
        let obj2 = heap.allocate_object(class_ref).unwrap();

        let args = vec![
            Value::Reference(ObjectRef::Object(obj1)),
            Value::Reference(ObjectRef::Object(obj2)),
        ];
        let result = ClosureInvoker::pack_args_array(&mut heap, args);

        if let Err(e) = &result {
            panic!("pack_args_array failed: {}", e);
        }

        assert!(result.is_ok());

        // Should return array reference
        match result.unwrap() {
            Value::Reference(ObjectRef::Object(array_id)) => {
                // Verify array contents
                let val0 = heap.array_load(array_id, 0).unwrap();
                let val1 = heap.array_load(array_id, 1).unwrap();

                assert_eq!(val0, Value::Reference(ObjectRef::Object(obj1)));
                assert_eq!(val1, Value::Reference(ObjectRef::Object(obj2)));
            }
            other => panic!("Expected array reference, got: {:?}", other),
        }
    }

    #[test]
    fn test_get_closure_class_name_invalid() {
        let heap = Heap::new(MemoryConfig::default().max_heap_size);

        // Invalid HeapId should return error
        let result = ClosureInvoker::get_closure_class_name(&heap, HeapId(999));
        assert!(result.is_err());
    }
}
