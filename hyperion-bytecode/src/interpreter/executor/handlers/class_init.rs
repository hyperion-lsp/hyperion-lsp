// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM class and interface initialization (JVMS 5.5, 12.4.2).

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::{access_flags, Executor, NestedExecutionState};
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::Heap;
use crate::interpreter::method_area::{ClassInitState, LoadedClass, MethodArea};
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::ObjectRef;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

impl Executor {
    /// Complete initialization synchronously before an active use proceeds.
    pub(crate) fn ensure_initialized(
        &mut self,
        class_name: &str,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        let class_name = class_name.replace('/', ".");
        let method_area = self
            .method_area
            .clone()
            .ok_or_else(|| VMError::internal("MethodArea not initialized"))?;
        let class = method_area
            .read()
            .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
            .get_class(&class_name)
            .map_err(|error| match error {
                VMError::ClassNotFound(name) => VMError::NoClassDefFoundError(name),
                other => other,
            })?;

        match &class.init_state {
            ClassInitState::Initialized => return Ok(()),
            ClassInitState::Initializing(owner) if *owner == thread.id => return Ok(()),
            ClassInitState::Initializing(owner) => {
                return Err(VMError::internal(format!(
                    "class {class_name} is being initialized by {owner:?}; concurrent initialization is unsupported"
                )));
            }
            ClassInitState::InitError(message) => {
                let message_ref = match heap.intern_string(&format!(
                    "Could not initialize class {class_name}: {message}"
                )) {
                    Ok(reference) => reference,
                    Err(VMError::MemoryLimitExceeded) => {
                        return Err(VMError::ThrownException(heap.emergency_out_of_memory()));
                    }
                    Err(error) => return Err(error),
                };
                let exception = self.allocate_exception(
                    "java/lang/NoClassDefFoundError",
                    message_ref,
                    heap.class_init_failure(&class_name),
                    heap,
                )?;
                return Err(VMError::ThrownException(exception));
            }
            ClassInitState::NotInitialized => {}
        }

        // ConstantValue assignment is preparation, not initialization. In
        // particular, an allocation failure while interning a String constant
        // must not make the class erroneous.
        method_area
            .read()
            .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
            .prepare_constant_fields(&class_name, heap)?;

        method_area
            .read()
            .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
            .set_init_state(&class_name, ClassInitState::Initializing(thread.id))?;

        self.initialize_marked_class(&class, thread, heap, monitors, &method_area)
    }

    fn initialize_marked_class(
        &mut self,
        class: &LoadedClass,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        method_area: &Arc<RwLock<MethodArea>>,
    ) -> VMResult<()> {
        let class_name = class.name.replace('/', ".");

        // An interface does not initialize Object or its parents. A class
        // initializes its superclass (including Object) and then qualifying
        // superinterfaces in left-to-right DFS postorder.
        if !class.is_interface {
            if let Some(superclass) = &class.superclass {
                if let Err(error) = self.ensure_initialized(superclass, thread, heap, monitors) {
                    return self.fail_initialization(
                        &class_name,
                        thread,
                        heap,
                        method_area,
                        error,
                        false,
                    );
                }
            }

            let interfaces = match initialization_superinterfaces(method_area, class) {
                Ok(interfaces) => interfaces,
                Err(error) => {
                    return self.fail_initialization(
                        &class_name,
                        thread,
                        heap,
                        method_area,
                        error,
                        false,
                    );
                }
            };
            for interface in interfaces {
                if let Err(error) = self.ensure_initialized(&interface, thread, heap, monitors) {
                    return self.fail_initialization(
                        &class_name,
                        thread,
                        heap,
                        method_area,
                        error,
                        false,
                    );
                }
            }
        }

        if let Err(error) = self.run_class_initializer(&class_name, thread, heap, monitors) {
            return self.fail_initialization(&class_name, thread, heap, method_area, error, true);
        }

        method_area
            .read()
            .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
            .set_init_state(&class_name, ClassInitState::Initialized)
    }

    fn run_class_initializer(
        &mut self,
        class_name: &str,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        let Some((registry_owner, bytecode)) =
            self.lookup_method_either_form(class_name, "<clinit>", "()V")
        else {
            return Ok(());
        };
        let frame = Frame::with_bytecode(
            registry_owner,
            "<clinit>".to_string(),
            bytecode.max_locals,
            bytecode.max_stack,
            Vec::new(),
            bytecode.code,
            "()V".to_string(),
            bytecode.exception_table,
        )?;
        let state = NestedExecutionState::capture(self, thread, "class initializer");
        thread.push_frame(frame)?;
        self.finish_nested_execution(thread, heap, monitors, state)
    }

    fn fail_initialization(
        &self,
        class_name: &str,
        thread: &Thread,
        heap: &mut Heap,
        method_area: &Arc<RwLock<MethodArea>>,
        error: VMError,
        wrap_non_error: bool,
    ) -> VMResult<()> {
        let original = if error.is_catchable() || matches!(error, VMError::ThrownException(_)) {
            self.materialize_exception(&error, heap)?
        } else {
            method_area
                .read()
                .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
                .fail_initialization_if_owned(class_name, thread.id, error.to_string())?;
            return Err(error);
        };

        let original_is_error = original
            .heap_id()
            .and_then(|id| heap.get_object_class_name(id).ok())
            .and_then(|name| {
                method_area
                    .read()
                    .ok()
                    .and_then(|area| area.is_subtype(&name, "java/lang/Error").ok())
            })
            .unwrap_or(false);
        let propagated = if wrap_non_error && !original_is_error {
            self.allocate_exception(
                "java/lang/ExceptionInInitializerError",
                ObjectRef::Null,
                Some(original),
                heap,
            )?
        } else {
            original
        };

        heap.record_class_init_failure(class_name, propagated);
        let propagated_class = heap.get_class(
            propagated
                .heap_id()
                .ok_or_else(VMError::invalid_object_ref)?,
        )?;
        method_area
            .read()
            .map_err(|_| VMError::internal("MethodArea lock poisoned"))?
            .fail_initialization_if_owned(class_name, thread.id, propagated_class.name)?;
        Err(VMError::ThrownException(propagated))
    }
}

/// Recursively enumerate interfaces declaring a non-static, non-abstract
/// method. Private interface methods qualify, so inspect both declaration
/// tables rather than equating this set with public defaults.
fn initialization_superinterfaces(
    method_area: &Arc<RwLock<MethodArea>>,
    class: &LoadedClass,
) -> VMResult<Vec<String>> {
    let area = method_area
        .read()
        .map_err(|_| VMError::internal("MethodArea lock poisoned"))?;
    let mut visited = HashSet::new();
    let mut result = Vec::new();
    for interface in &class.interfaces {
        collect_initialization_interface(&area, interface, &mut visited, &mut result)?;
    }
    Ok(result)
}

fn collect_initialization_interface(
    method_area: &MethodArea,
    interface_name: &str,
    visited: &mut HashSet<String>,
    result: &mut Vec<String>,
) -> VMResult<()> {
    let interface = method_area
        .get_class(interface_name)
        .map_err(|error| match error {
            VMError::ClassNotFound(name) => VMError::NoClassDefFoundError(name),
            other => other,
        })?;
    let name = interface.name.replace('/', ".");
    if !visited.insert(name.clone()) {
        return Ok(());
    }
    for parent in &interface.interfaces {
        collect_initialization_interface(method_area, parent, visited, result)?;
    }
    let declares_concrete_instance_method = interface
        .vtable
        .iter()
        .any(|entry| entry.declaring_class().replace('/', ".") == name && !entry.is_abstract())
        || interface.non_virtual_methods.iter().any(|method| {
            method.access_flags & (access_flags::ACC_STATIC | access_flags::ACC_ABSTRACT) == 0
        });
    if declares_concrete_instance_method {
        result.push(name);
    }
    Ok(())
}
