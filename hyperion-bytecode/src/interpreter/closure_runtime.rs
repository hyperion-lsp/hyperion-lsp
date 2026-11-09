// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Closure Runtime Support
//!
//! Provides runtime infrastructure for Groovy closures including:
//! - Closure object instantiation on the heap
//! - Delegate/owner field management
//! - Resolution strategy handling (OWNER_FIRST, DELEGATE_FIRST, etc.)
//! - doCall() invocation

use super::errors::{VMError, VMResult};
use super::heap::{ClassRef, Heap};
use super::value::{HeapId, ObjectRef, Value};

/// Groovy closure resolution strategies (from groovy.lang.Closure)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ResolveStrategy {
    /// Try owner, then delegate, then closure (Groovy default)
    OwnerFirst = 0,
    /// Try delegate, then owner, then closure (Gradle default)
    DelegateFirst = 1,
    /// Only try owner
    OwnerOnly = 2,
    /// Only try delegate
    DelegateOnly = 3,
    /// Only try the closure itself
    ToSelf = 4,
}

impl ResolveStrategy {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(ResolveStrategy::OwnerFirst),
            1 => Some(ResolveStrategy::DelegateFirst),
            2 => Some(ResolveStrategy::OwnerOnly),
            3 => Some(ResolveStrategy::DelegateOnly),
            4 => Some(ResolveStrategy::ToSelf),
            _ => None,
        }
    }
}

/// Field offsets for Groovy closure objects
///
/// Closure object layout (extends groovy.lang.Closure):
/// - Field 0: delegate (Object)
/// - Field 1: owner (Object)
/// - Field 2: resolveStrategy (int)
/// - Field 3+: Captured variables (variable-length)
pub mod field_offset {
    pub const DELEGATE: usize = 0;
    pub const OWNER: usize = 1;
    pub const RESOLVE_STRATEGY: usize = 2;
    pub const CAPTURED_START: usize = 3;
}

/// Runtime support for Groovy closures
pub struct ClosureRuntime;

impl ClosureRuntime {
    /// Create a new closure instance on the heap
    ///
    /// Creates a closure object with fields:
    /// - delegate: initially null
    /// - owner: the defining class/closure instance
    /// - resolveStrategy: OWNER_FIRST by default
    /// - captured variables: passed as values
    ///
    /// Returns the HeapId of the allocated closure object
    pub fn instantiate_closure(
        heap: &mut Heap,
        class_name: &str,
        owner: ObjectRef,
        captured_values: Vec<Value>,
    ) -> VMResult<HeapId> {
        // Calculate field count: delegate + owner + resolveStrategy + captured vars
        let field_count = field_offset::CAPTURED_START + captured_values.len();

        // Create class reference for the closure
        let class_ref = ClassRef {
            name: class_name.to_string(),
            instance_field_count: field_count,
            static_field_count: 0,
        };

        // Allocate the object
        let closure_id = heap.allocate_object(class_ref)?;

        // Initialize standard closure fields
        heap.set_field(
            closure_id,
            field_offset::DELEGATE,
            Value::Reference(ObjectRef::Null),
        )?;
        heap.set_field(closure_id, field_offset::OWNER, Value::Reference(owner))?;
        heap.set_field(
            closure_id,
            field_offset::RESOLVE_STRATEGY,
            Value::Int(ResolveStrategy::OwnerFirst as i32),
        )?;

        // Set captured variables
        for (i, value) in captured_values.into_iter().enumerate() {
            heap.set_field(closure_id, field_offset::CAPTURED_START + i, value)?;
        }

        Ok(closure_id)
    }

    /// Set the delegate for a closure
    pub fn set_delegate(heap: &mut Heap, closure_id: HeapId, delegate: ObjectRef) -> VMResult<()> {
        heap.set_field(
            closure_id,
            field_offset::DELEGATE,
            Value::Reference(delegate),
        )
    }

    /// Set the resolution strategy for a closure
    pub fn set_resolve_strategy(
        heap: &mut Heap,
        closure_id: HeapId,
        strategy: ResolveStrategy,
    ) -> VMResult<()> {
        heap.set_field(
            closure_id,
            field_offset::RESOLVE_STRATEGY,
            Value::Int(strategy as i32),
        )
    }

    /// Get the delegate from a closure
    pub fn get_delegate(heap: &Heap, closure_id: HeapId) -> VMResult<ObjectRef> {
        match heap.get_field(closure_id, field_offset::DELEGATE)? {
            Some(Value::Reference(obj_ref)) => Ok(obj_ref),
            Some(_) => Err(VMError::internal("Closure delegate is not a reference")),
            None => Err(VMError::internal("Closure delegate field not initialized")),
        }
    }

    /// Get the owner from a closure
    pub fn get_owner(heap: &Heap, closure_id: HeapId) -> VMResult<ObjectRef> {
        match heap.get_field(closure_id, field_offset::OWNER)? {
            Some(Value::Reference(obj_ref)) => Ok(obj_ref),
            Some(_) => Err(VMError::internal("Closure owner is not a reference")),
            None => Err(VMError::internal("Closure owner field not initialized")),
        }
    }

    /// Get the resolution strategy from a closure
    pub fn get_resolve_strategy(heap: &Heap, closure_id: HeapId) -> VMResult<ResolveStrategy> {
        match heap.get_field(closure_id, field_offset::RESOLVE_STRATEGY)? {
            Some(Value::Int(strategy_int)) => {
                ResolveStrategy::from_i32(strategy_int).ok_or_else(|| {
                    VMError::internal(format!("Invalid resolve strategy: {}", strategy_int))
                })
            }
            Some(_) => Err(VMError::internal(
                "Closure resolveStrategy is not an integer",
            )),
            None => Err(VMError::internal(
                "Closure resolveStrategy field not initialized",
            )),
        }
    }

    /// Get a captured variable from a closure
    pub fn get_captured_var(heap: &Heap, closure_id: HeapId, var_index: usize) -> VMResult<Value> {
        match heap.get_field(closure_id, field_offset::CAPTURED_START + var_index)? {
            Some(value) => Ok(value),
            None => Err(VMError::internal(format!(
                "Captured variable {} not initialized",
                var_index
            ))),
        }
    }

    /// Invoke a closure's doCall method
    ///
    /// Pushes a new frame onto the thread for executing the closure's doCall() method.
    /// The caller should then continue calling executor.step() to execute the closure body.
    ///
    /// # Implementation
    ///
    /// 1. Creates doCall execution frame via ClosureInvoker
    /// 2. Pushes frame onto thread stack
    /// 3. Executor's step() loop will execute the closure bytecode
    /// 4. Delegate-aware method resolution happens during execution
    ///
    /// # Arguments
    ///
    /// - `heap`: Heap for reading closure metadata and allocating args array if needed
    /// - `method_registry`: Contains doCall bytecode for closure classes
    /// - `thread`: Thread to push the execution frame onto
    /// - `closure_id`: HeapId of the closure object to invoke
    /// - `args`: Arguments to pass to doCall
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if frame was successfully pushed.
    /// The closure's return value will be on the operand stack when doCall returns.
    pub fn invoke_do_call(
        heap: &mut Heap,
        method_registry: &std::collections::HashMap<
            (String, String, String),
            super::executor::MethodBytecode,
        >,
        thread: &mut super::thread::Thread,
        closure_id: HeapId,
        args: Vec<Value>,
    ) -> VMResult<()> {
        use super::closure_invoker::ClosureInvoker;

        // Create execution frame for doCall
        let frame = ClosureInvoker::create_do_call_frame(heap, method_registry, closure_id, args)?;

        // Push frame onto thread - executor will execute it
        thread.push_frame(frame)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::MemoryConfig;

    #[test]
    fn test_closure_instantiation() {
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        // Create a simple closure with no captured variables
        let closure_id = ClosureRuntime::instantiate_closure(
            &mut heap,
            "Script$_closure1",
            ObjectRef::Null, // No owner for now
            vec![],          // No captured variables
        )
        .unwrap();

        // Verify delegate is null initially
        let delegate = ClosureRuntime::get_delegate(&heap, closure_id).unwrap();
        assert_eq!(delegate, ObjectRef::Null);

        // Verify resolve strategy is OWNER_FIRST
        let strategy = ClosureRuntime::get_resolve_strategy(&heap, closure_id).unwrap();
        assert_eq!(strategy, ResolveStrategy::OwnerFirst);
    }

    #[test]
    fn test_set_delegate() {
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        let closure_id = ClosureRuntime::instantiate_closure(
            &mut heap,
            "Script$_closure1",
            ObjectRef::Null,
            vec![],
        )
        .unwrap();

        // Create a mock delegate object
        let delegate_class = ClassRef {
            name: "DependencyHandler".to_string(),
            instance_field_count: 0,
            static_field_count: 0,
        };
        let delegate_id = heap.allocate_object(delegate_class).unwrap();

        // Set delegate
        ClosureRuntime::set_delegate(&mut heap, closure_id, ObjectRef::Object(delegate_id))
            .unwrap();

        // Verify delegate was set
        let delegate = ClosureRuntime::get_delegate(&heap, closure_id).unwrap();
        assert_eq!(delegate, ObjectRef::Object(delegate_id));
    }

    #[test]
    fn test_set_resolve_strategy() {
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        let closure_id = ClosureRuntime::instantiate_closure(
            &mut heap,
            "Script$_closure1",
            ObjectRef::Null,
            vec![],
        )
        .unwrap();

        // Set to DELEGATE_FIRST (Gradle default)
        ClosureRuntime::set_resolve_strategy(&mut heap, closure_id, ResolveStrategy::DelegateFirst)
            .unwrap();

        // Verify strategy was set
        let strategy = ClosureRuntime::get_resolve_strategy(&heap, closure_id).unwrap();
        assert_eq!(strategy, ResolveStrategy::DelegateFirst);
    }

    #[test]
    fn test_captured_variables() {
        let mut heap = Heap::new(MemoryConfig::default().max_heap_size);

        // Create closure with captured variables
        let captured = vec![Value::Int(42), Value::Int(100)];
        let closure_id = ClosureRuntime::instantiate_closure(
            &mut heap,
            "Script$_closure1",
            ObjectRef::Null,
            captured,
        )
        .unwrap();

        // Retrieve captured variables
        let var0 = ClosureRuntime::get_captured_var(&heap, closure_id, 0).unwrap();
        let var1 = ClosureRuntime::get_captured_var(&heap, closure_id, 1).unwrap();

        assert_eq!(var0, Value::Int(42));
        assert_eq!(var1, Value::Int(100));
    }
}
