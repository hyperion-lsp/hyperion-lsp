// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method invocation handlers for the Executor
//!
//! Handles invokestatic, native virtual dispatch, and native method invocation.

use crate::descriptor::MethodDescriptor;
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::{Executor, NestedExecutionState};
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::Heap;
use crate::interpreter::monitor::enter_static_synchronized;
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};
use tracing::debug;

/// Acquire the monitor on the receiver object if ACC_SYNCHRONIZED is set (JVMS 2.11.10).
///
/// Must be called after creating the new frame but before pushing it onto the thread.
/// If the method is synchronized, the monitor is entered on the receiver (local 0)
/// and tracked in the frame's `acquired_monitors` for automatic release on return.
fn apply_synchronized_if_needed(
    access_flags: u16,
    frame: &mut Frame,
    thread: &Thread,
    monitors: &mut MonitorTable,
) -> VMResult<()> {
    if access_flags & crate::interpreter::executor::access_flags::ACC_SYNCHRONIZED == 0 {
        return Ok(());
    }
    if let Some(Some(Value::Reference(obj_ref))) = frame.locals.first() {
        match obj_ref {
            ObjectRef::Object(heap_id) | ObjectRef::Array(heap_id) => {
                monitors.enter(*heap_id, thread.id).map_err(|_| {
                    VMError::internal("Monitor deadlock in single-threaded execution")
                })?;
                frame.acquired_monitors.push(*heap_id);
            }
            ObjectRef::Null => {
                return Err(VMError::NullPointerException);
            }
            ObjectRef::MethodHandle(_) => {
                // MethodHandles don't support synchronized - skip
            }
        }
    }
    Ok(())
}

impl Executor {
    /// Try to invoke a native method (returns true if invoked, false if not native or registry not available)
    pub(crate) fn try_invoke_native(
        &self,
        heap: &mut Heap,
        thread: &mut Thread,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: Vec<Value>,
    ) -> VMResult<Option<(bool, Option<Value>)>> {
        if let Some(result) =
            super::groovy_truth::try_invoke(heap, class_name, method_name, descriptor, &args)?
        {
            return Ok(Some((true, Some(result))));
        }

        if let Some(result) =
            super::groovy_numbers::try_invoke(heap, class_name, method_name, descriptor, &args)?
        {
            return Ok(Some((true, Some(result))));
        }

        // First check if this is a Gradle stub method (gradle/* classes)
        if let Some(bridge) = &self.gradle_native_bridge {
            if class_name.starts_with("gradle/") {
                let result = bridge.invoke_gradle_method(
                    heap,
                    &self.method_registry,
                    thread,
                    class_name,
                    method_name,
                    descriptor,
                    &args,
                )?;
                return Ok(Some((true, result)));
            }

            if class_name.starts_with("org/gradle/") {
                let mapped_class = match class_name {
                    "org/gradle/api/Project" => Some("gradle/Project"),
                    "org/gradle/api/artifacts/dsl/DependencyHandler" => {
                        Some("gradle/DependencyHandler")
                    }
                    "org/gradle/api/plugins/PluginContainer" => Some("gradle/PluginManager"),
                    _ => None,
                };

                if let Some(mapped_class) = mapped_class {
                    let result = bridge.invoke_gradle_method(
                        heap,
                        &self.method_registry,
                        thread,
                        mapped_class,
                        method_name,
                        descriptor,
                        &args,
                    )?;
                    return Ok(Some((true, result)));
                }
            }

            if let Some(result) = bridge.try_invoke_sniffer_intrinsic(
                heap,
                class_name,
                method_name,
                descriptor,
                &args,
            )? {
                return Ok(Some((true, Some(result))));
            }

            if matches!(method_name, "dependencies" | "repositories" | "plugins") {
                let result = bridge.invoke_gradle_method(
                    heap,
                    &self.method_registry,
                    thread,
                    "gradle/Project",
                    method_name,
                    descriptor,
                    &args,
                )?;
                return Ok(Some((true, result)));
            }
        }

        // java.lang.Thread.currentThread(): return the synchronously-running
        // thread object (set by start()), or a synthetic "main" Thread when none
        // is active, so currentThread().getName() works from any context.
        if class_name == "java/lang/Thread" && method_name == "currentThread" {
            let obj = match self.current_thread {
                Some(id) => ObjectRef::Object(id),
                None => {
                    let id = heap.allocate_object(crate::interpreter::heap::ClassRef {
                        name: "java/lang/Thread".to_string(),
                        instance_field_count: 0,
                        static_field_count: 0,
                    })?;
                    let name = heap.intern_string("main")?;
                    heap.set_dynamic_property(id, "$thread_name", Value::Reference(name));
                    ObjectRef::Object(id)
                }
            };
            return Ok(Some((true, Some(Value::Reference(obj)))));
        }

        // java.lang.Enum.valueOf(Class, name): find the constant by scanning the
        // enum's synthetic `$VALUES` static array (matching on $enum_name).
        if class_name == "java/lang/Enum" && method_name == "valueOf" {
            // A null argument is a NullPointerException, as on the JVM. A
            // non-string or non-class argument is a type error, not an empty name
            // that would silently fail to match any constant.
            let cls = match args.first() {
                Some(Value::Reference(ObjectRef::Object(id))) => heap.get_class_name(*id)?,
                Some(Value::Reference(ObjectRef::Null)) | None => {
                    return Err(VMError::NullPointerException)
                }
                Some(other) => return Err(VMError::type_mismatch("Class", &format!("{other:?}"))),
            };
            let name = match args.get(1) {
                Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(*id)?,
                Some(Value::Reference(ObjectRef::Null)) | None => {
                    return Err(VMError::NullPointerException)
                }
                Some(other) => return Err(VMError::type_mismatch("String", &format!("{other:?}"))),
            };
            let values = self.method_area.as_ref().and_then(|ma| {
                let ma = ma.read().ok()?;
                ma.get_static(&cls, "$VALUES")
                    .or_else(|_| ma.get_static(&cls.replace('/', "."), "$VALUES"))
                    .ok()
            });
            if let Some(Value::Reference(ObjectRef::Array(aid))) = values {
                for i in 0..heap.array_length(aid)? {
                    if let Value::Reference(ObjectRef::Object(eid)) = heap.array_load(aid, i)? {
                        if let Some(Value::Reference(ObjectRef::Object(nid))) =
                            heap.get_dynamic_property(eid, "$enum_name")
                        {
                            if heap.get_string(nid)? == name {
                                return Ok(Some((
                                    true,
                                    Some(Value::Reference(ObjectRef::Object(eid))),
                                )));
                            }
                        }
                    }
                }
            }
            return Err(VMError::internal(format!(
                "No enum constant {}.{}",
                cls, name
            )));
        }

        // javax.imageio.ImageIO.write(RenderedImage, String, File): encode the
        // image's pixel buffer to PNG and write it to the File's path. Returns
        // boolean (true on success). Matched before the descriptor-keyed table
        // since the static descriptor varies by import resolution.
        if class_name == "javax/imageio/ImageIO" && method_name == "write" {
            let ok = write_image_to_file(heap, &args).unwrap_or(false);
            return Ok(Some((true, Some(Value::Int(ok as i32)))));
        }

        // Handle JDK boxing methods that need heap access
        match (class_name, method_name, descriptor) {
            ("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;") => {
                // Integer.valueOf(int) - create boxed Integer or return existing boxed value.
                if let Some(Value::Reference(ObjectRef::Object(id))) = args.first() {
                    match heap.get_object_class_name(*id)?.as_str() {
                        "java/lang/Integer" => {
                            return Ok(Some((
                                true,
                                Some(Value::Reference(ObjectRef::Object(*id))),
                            )));
                        }
                        "java/lang/Long" => {
                            let int_val = heap.get_boxed_long(*id)? as i32;
                            let heap_id = heap.box_integer_cached(int_val)?;
                            return Ok(Some((
                                true,
                                Some(Value::Reference(ObjectRef::Object(heap_id))),
                            )));
                        }
                        _ => {}
                    }
                }

                let int_val = match args.first() {
                    Some(Value::Int(i)) => *i,
                    Some(Value::Long(l)) => *l as i32,
                    Some(Value::Reference(ObjectRef::Object(id))) => {
                        match heap.get_object_class_name(*id)?.as_str() {
                            "java/lang/Integer" => heap.get_boxed_int(*id)?,
                            "java/lang/Long" => heap.get_boxed_long(*id)? as i32,
                            _ => 0,
                        }
                    }
                    _ => 0,
                };
                let heap_id = heap.box_integer_cached(int_val)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;") => {
                let long_val = args
                    .first()
                    .and_then(|v| {
                        if let Value::Long(l) = v {
                            Some(*l)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| VMError::TypeMismatch {
                        expected: "long".to_string(),
                        got: "other".to_string(),
                    })?;
                let heap_id = heap.allocate_long(long_val)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/Double", "valueOf", "(D)Ljava/lang/Double;") => {
                let double_val = args
                    .first()
                    .and_then(|v| match v {
                        Value::Double(d) => Some(*d),
                        _ => None,
                    })
                    .ok_or_else(|| VMError::TypeMismatch {
                        expected: "double".to_string(),
                        got: "other".to_string(),
                    })?;
                let heap_id = heap.allocate_double(double_val)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/Float", "valueOf", "(F)Ljava/lang/Float;") => {
                let float_val = args
                    .first()
                    .and_then(|v| match v {
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .ok_or_else(|| VMError::TypeMismatch {
                        expected: "float".to_string(),
                        got: "other".to_string(),
                    })?;
                let heap_id = heap.allocate_float(float_val)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;") => {
                let bool_val = args
                    .first()
                    .and_then(|v| {
                        if let Value::Int(i) = v {
                            Some(*i != 0)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| VMError::TypeMismatch {
                        expected: "boolean".to_string(),
                        got: "other".to_string(),
                    })?;
                let heap_id = heap.allocate_boolean(bool_val)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/Character", "valueOf", "(C)Ljava/lang/Character;") => {
                let code_unit = match args.first() {
                    Some(Value::Int(code_unit)) => *code_unit,
                    _ => {
                        return Err(VMError::TypeMismatch {
                            expected: "char".to_string(),
                            got: "other".to_string(),
                        })
                    }
                };
                let heap_id = heap.allocate_character(code_unit)?;
                return Ok(Some((
                    true,
                    Some(Value::Reference(ObjectRef::Object(heap_id))),
                )));
            }
            ("java/lang/String", "valueOf", "(Ljava/lang/Object;)Ljava/lang/String;")
            | ("java/util/Objects", "toString", "(Ljava/lang/Object;)Ljava/lang/String;") => {
                // Render the argument the way Object.toString / String.valueOf
                // would. Known types (String, wrappers, collections, arrays, null)
                // render exactly; user objects with an overridden toString are
                // handled before this native (see execute_invokestatic) so they
                // dispatch to their real toString. The fallback here is the
                // default `Class@hash`-style handle.
                let rendered = match args.first() {
                    Some(v) => heap.render_value(v),
                    None => "null".to_string(),
                };
                let string_ref = heap.intern_string(&rendered)?;
                return Ok(Some((true, Some(Value::Reference(string_ref)))));
            }
            _ => {}
        }

        // Fall back to regular native method registry
        let registry = match &self.native_registry {
            Some(reg) => reg,
            None => return Ok(None),
        };

        // Check if this is a native method
        if !registry.is_native(class_name, method_name, descriptor) {
            return Ok(None);
        }

        // Invoke the native method and return success indicator
        let result = registry.invoke(heap, class_name, method_name, descriptor, &args)?;
        Ok(Some((true, result)))
    }

    fn resolve_invokestatic_symbolic(
        &self,
        owner: &str,
        method_name: &str,
        descriptor: &str,
    ) -> VMResult<(String, u16, bool)> {
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;
        let symbolic = match method_area.get_class(owner) {
            Ok(class) => class,
            Err(error) => {
                if let Some((registry_owner, method)) =
                    self.lookup_method_either_form(owner, method_name, descriptor)
                {
                    return Ok((registry_owner, method.access_flags, false));
                }
                if self
                    .native_registry
                    .as_ref()
                    .is_some_and(|registry| registry.is_native(owner, method_name, descriptor))
                {
                    return Ok((owner.to_string(), 0x0001 | 0x0008, false));
                }
                return Err(invocation_resolution_error(error, owner));
            }
        };
        let symbolic_is_interface = symbolic.is_interface;
        let mut current = Some(symbolic);
        while let Some(class) = current {
            if let Some(method) = class.declared_non_virtual_method(method_name, descriptor) {
                return Ok((
                    class.name.clone(),
                    method.access_flags,
                    symbolic_is_interface,
                ));
            }
            if let Ok(method) = class.resolve_virtual_declaration(method_name, descriptor) {
                if method.declaring_class.replace('/', ".") == class.name.replace('/', ".") {
                    return Ok((class.name, method.access_flags, symbolic_is_interface));
                }
            }
            current = class
                .superclass
                .as_deref()
                .and_then(|parent| method_area.get_class(parent).ok());
        }
        Err(VMError::NoSuchMethod(format!(
            "{owner}.{method_name}{descriptor}"
        )))
    }

    /// Execute invokestatic opcode - call static method
    pub(crate) fn execute_invokestatic(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Resolve method reference from constant pool
        let (class_name, method_name, descriptor, _reference_kind) =
            self.resolve_method_ref_with_kind(cp_index)?;
        reject_illegal_initializer(&class_name, &method_name, &descriptor, false)?;
        let caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        let (resolved_class, resolved_flags, symbolic_is_interface) =
            self.resolve_invokestatic_symbolic(&class_name, &method_name, &descriptor)?;
        let reference_is_interface = matches!(
            _reference_kind,
            crate::interpreter::executor::handlers::cp_resolution::MethodReferenceKind::Interface
        );
        validate_invokestatic_reference_kind(
            reference_is_interface,
            symbolic_is_interface,
            &class_name,
        )?;
        let has_symbolic_metadata = self
            .method_area
            .as_ref()
            .and_then(|area| area.read().ok())
            .is_some_and(|area| area.get_class(&class_name).is_ok());
        if has_symbolic_metadata {
            self.check_symbolic_class_access(&caller_class, &class_name)
                .map_err(|error| invocation_resolution_error(error, &class_name))?;
        }
        if resolved_flags & crate::interpreter::executor::access_flags::ACC_STATIC == 0 {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "invokestatic resolved non-static method {resolved_class}.{method_name}{descriptor}"
            )));
        }
        if resolved_flags & 0x0400 != 0 {
            return Err(VMError::AbstractMethodError(format!(
                "{resolved_class}.{method_name}{descriptor}"
            )));
        }
        self.check_method_symbolic_access(
            &caller_class,
            &resolved_class,
            &method_name,
            resolved_flags,
        )?;
        if has_symbolic_metadata {
            self.ensure_initialized(&resolved_class, thread, heap, monitors)?;
        }

        // Parse the method descriptor to get the argument count. Use the number
        // of *parameters* (not slots): the operand stack stores one entry per
        // value, so a long/double argument is a single entry to pop — matching
        // how invokevirtual/invokespecial pop their arguments.
        let method_desc =
            MethodDescriptor::parse(&descriptor).map_err(|_| VMError::InvalidBytecode)?;
        let arg_count = method_desc.parameters.len();

        // Pop arguments from the caller's stack
        let mut args = Vec::new();
        {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            for _ in 0..arg_count {
                args.push(frame.pop()?);
            }
        }
        args.reverse(); // Stack is LIFO, but we want args in order

        // String.valueOf(Object)/Objects.toString(Object) with a user object
        // runs the object's real toString (a record's generated one, or a user
        // override); the native fallback below only renders known JDK types.
        if matches!(
            (
                class_name.as_str(),
                method_name.as_str(),
                descriptor.as_str()
            ),
            (
                "java/lang/String",
                "valueOf",
                "(Ljava/lang/Object;)Ljava/lang/String;"
            ) | (
                "java/util/Objects",
                "toString",
                "(Ljava/lang/Object;)Ljava/lang/String;"
            )
        ) {
            if let Some(Value::Reference(ObjectRef::Object(arg_id))) = args.first().cloned() {
                if let Some(s) = self.user_tostring_sync(thread, heap, monitors, arg_id)? {
                    let rendered = heap.intern_string(&s)?;
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push(Value::Reference(rendered))?;
                    return Ok(());
                }
            }
        }

        // Collectors factories (joining/toList/toSet): allocate a marker object
        // recording the kind and arguments; `Stream.collect` reads it back.
        if class_name == "java/util/stream/Collectors" {
            let id = heap.allocate_collector()?;
            let kind = heap.intern_string(&method_name)?;
            heap.set_dynamic_property(id, "$collector", Value::Reference(kind));
            // joining() / joining(delim) / joining(delim, prefix, suffix)
            for (i, key) in ["$delimiter", "$prefix", "$suffix"].iter().enumerate() {
                if let Some(v) = args.get(i) {
                    heap.set_dynamic_property(id, key, v.clone());
                }
            }
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            frame.push_raw(Value::Reference(ObjectRef::Object(id)));
            return Ok(());
        }

        // Collections.sort(list) / sort(list, comparator): sort in place. Handled
        // here (not as a plain native) because a comparator is a SAM that must be
        // invoked through the executor per comparison.
        if class_name == "java/util/Collections" && method_name == "sort" {
            if let Some(Value::Reference(ObjectRef::Object(list_id))) = args.first() {
                let comparator = match args.get(1) {
                    Some(Value::Reference(ObjectRef::Object(id))) => Some(*id),
                    _ => None,
                };
                self.sort_list(thread, heap, monitors, *list_id, comparator)?;
            }
            return Ok(());
        }

        // Try to invoke as native method with actual arguments
        if let Some((true, result)) = self.try_invoke_native(
            heap,
            thread,
            &class_name,
            &method_name,
            &descriptor,
            args.clone(),
        )? {
            // Native method was invoked
            if let Some(value) = result {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push(value)?;
            }
            return Ok(());
        }

        // Try to invoke as user-defined method. The method registry is keyed by
        // different class-name forms across entry points (declared dot form on
        // the main VM path; internal slash form on the Gradle plugin path) and
        // constant-pool refs are slash — try both forms.
        if let Some((registry_class, method_bytecode)) =
            self.lookup_method_either_form(&resolved_class, &method_name, &descriptor)
        {
            // JVMS §5.5: invokestatic is an active use — initialize the class
            // (runs <clinit>) before the call. No-op if already initialized or if
            // there is no <clinit>.
            if has_symbolic_metadata {
                self.ensure_initialized(&registry_class, thread, heap, monitors)?;
            }
            // Check access control (JVMS 5.4.4)
            self.check_method_access(
                &caller_class,
                &registry_class,
                &method_name,
                method_bytecode.access_flags,
            )?;

            // Save caller's PC before pushing new frame
            // This ensures the caller resumes at the correct position after return
            if let Some(caller_frame) = thread.current_frame_mut() {
                caller_frame.pc = self.pc;
            }

            // Create a new frame for the method (dot-form class name so its
            // constant pool is found when the frame executes).
            let access_flags = method_bytecode.access_flags;
            let mut method_frame = Frame::with_bytecode(
                registry_class.clone(),
                method_name,
                method_bytecode.max_locals,
                method_bytecode.max_stack,
                args,
                method_bytecode.code,
                descriptor,
                method_bytecode.exception_table,
            )?;
            enter_static_synchronized(
                access_flags,
                &registry_class,
                &mut method_frame,
                thread,
                heap,
                monitors,
            )?;

            // Push the new frame onto the thread
            thread.push_frame(method_frame)?;

            return Ok(());
        }

        // Method not found in either registry
        Err(VMError::NoSuchMethod(format!(
            "{}.{}:{}",
            class_name, method_name, descriptor
        )))
    }

    /// Walk the superclass chain to decide whether `class_name` is (or extends)
    /// `java.lang.Thread`. Used to intercept the Thread API on user subclasses.
    pub(crate) fn is_thread_subclass(&self, class_name: &str) -> bool {
        let mut cur = class_name.replace('.', "/");
        if cur == "java/lang/Thread" {
            return true;
        }
        let Some(ma) = self.method_area.as_ref() else {
            return false;
        };
        let Ok(ma) = ma.read() else {
            return false;
        };
        for _ in 0..64 {
            let Ok(cls) = ma.get_class(&cur) else {
                return false;
            };
            match &cls.superclass {
                Some(s) => {
                    cur = s.replace('.', "/");
                    if cur == "java/lang/Thread" {
                        return true;
                    }
                    if cur == "java/lang/Object" {
                        return false;
                    }
                }
                None => return false,
            }
        }
        false
    }

    /// Handle native virtual method dispatch for standard Java classes.
    /// Returns Ok(true) if the method was handled, Ok(false) if not a native method.
    #[allow(clippy::too_many_arguments)]
    /// Whether a class in the receiver's superclass chain, below `java.lang.Object`,
    /// declares the method with bytecode. JDK stub declarations do not count.
    fn has_bytecode_declaration(
        &self,
        receiver_class: &str,
        method: &str,
        descriptor: &str,
    ) -> bool {
        let Some(method_area) = self.method_area.as_ref() else {
            return false;
        };
        let mut current = receiver_class.to_string();
        for _ in 0..64 {
            if current.replace('/', ".") == "java.lang.Object" {
                return false;
            }
            if self
                .lookup_method_either_form(&current, method, descriptor)
                .is_some_and(|(_, bytecode)| !bytecode.code.is_empty())
            {
                return true;
            }
            let superclass = {
                let Ok(method_area) = method_area.read() else {
                    return false;
                };
                let Ok(loaded) = method_area.get_class(&current) else {
                    return false;
                };
                loaded.superclass
            };
            match superclass {
                Some(superclass) => current = superclass,
                None => return false,
            }
        }
        false
    }

    /// Whether the receiver's class hierarchy supplies a bytecode override of an
    /// `Object` method. JDK stub declarations do not count: their natives still serve them.
    fn overrides_object_method(
        &self,
        receiver_class: &str,
        method_name: &str,
        descriptor: &str,
        route: Option<&std::sync::Arc<crate::interpreter::method_area::VirtualMethodDeclaration>>,
    ) -> bool {
        let (Some(route), Some(method_area)) = (route, self.method_area.as_ref()) else {
            return false;
        };
        let declaring = {
            let Ok(method_area) = method_area.read() else {
                return false;
            };
            let Ok(loaded) = method_area.get_class(receiver_class) else {
                return false;
            };
            let Ok(entry) = loaded.dispatch_target(method_name, descriptor, route) else {
                return false;
            };
            entry.declaring_class().to_string()
        };
        if declaring.replace('/', ".") == "java.lang.Object" {
            return false;
        }
        self.lookup_method_either_form(&declaring, method_name, descriptor)
            .is_some_and(|(_, bytecode)| !bytecode.code.is_empty())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_native_virtual_dispatch(
        &self,
        receiver_class_name: &str,
        method_name: &str,
        descriptor: &str,
        receiver: ObjectRef,
        heap_id: crate::interpreter::HeapId,
        args: &[Value],
        thread: &mut Thread,
        heap: &mut Heap,
    ) -> VMResult<bool> {
        // A bytecode toString() is the receiver's own method. The native arms would
        // answer with a default (an enum's name, a throwable's text), so they step aside.
        if method_name == "toString"
            && descriptor == "()Ljava/lang/String;"
            && self.has_bytecode_declaration(receiver_class_name, method_name, descriptor)
        {
            return Ok(false);
        }
        use super::native_virtual::{dispatch_native_virtual, NativeDispatchResult};
        match dispatch_native_virtual(
            receiver_class_name,
            method_name,
            descriptor,
            receiver,
            heap_id,
            args,
            heap,
        )? {
            NativeDispatchResult::Handled(value) => {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(value);
                Ok(true)
            }
            NativeDispatchResult::HandledVoid => Ok(true),
            NativeDispatchResult::NotHandled => Ok(false),
        }
    }

    /// Execute invokevirtual: Call instance method with virtual dispatch.
    /// Stack: ..., objectref, [args] -> ..., [result]
    /// Dispatch a functional-interface SAM call on a Java lambda to its closure
    /// `doCall`. Primitive arguments are boxed (doCall params are
    /// `Object`); the boxed result is pushed unless the SAM returns `void`.
    fn dispatch_lambda_sam(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        closure_id: crate::interpreter::HeapId,
        args: Vec<Value>,
        desc: &MethodDescriptor,
    ) -> VMResult<()> {
        // javac lambda (LambdaMetafactory): invoke the recorded implementation
        // method with captured vars ++ SAM args, by pushing a frame.
        if heap
            .get_dynamic_property(closure_id, "$lambda_method")
            .is_some()
        {
            return self.invoke_java_lambda(thread, heap, closure_id, args);
        }
        // Groovy/Hyperion closure: run its doCall to completion.
        let mut boxed = Vec::with_capacity(args.len());
        for a in args {
            boxed.push(box_primitive_for_lambda(heap, a)?);
        }
        let result = self.run_closure_to_completion(thread, heap, monitors, closure_id, boxed)?;
        if !matches!(desc.return_type, crate::descriptor::DescriptorType::Void) {
            let result = unbox_for_primitive_return(heap, &desc.return_type, result);
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            frame.push_raw(result);
        }
        Ok(())
    }

    /// Sort a heap-backed list in place, by an optional comparator (a SAM
    /// functional object invoked per comparison) or natural ordering. Uses an
    /// insertion sort so the comparator callback runs through `call_sam_sync`
    /// (which needs `&mut self`, precluding a `sort_by` closure).
    pub(crate) fn sort_list(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        list_id: crate::interpreter::HeapId,
        comparator: Option<crate::interpreter::HeapId>,
    ) -> VMResult<()> {
        let mut items = heap.collection_snapshot(list_id)?;
        let n = items.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 {
                let ord = match comparator {
                    Some(c) => self
                        .call_sam_sync(
                            thread,
                            heap,
                            monitors,
                            c,
                            vec![items[j - 1].clone(), items[j].clone()],
                        )?
                        .as_int()
                        .unwrap_or(0),
                    None => natural_compare(heap, &items[j - 1], &items[j]),
                };
                if ord > 0 {
                    items.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        // Written back per index, as an Arrays.asList view allows set but not add.
        for (index, item) in items.into_iter().enumerate() {
            let index = i32::try_from(index).map_err(|_| VMError::internal("list too large"))?;
            heap.arraylist_set(list_id, index, item)?;
        }
        Ok(())
    }

    /// `Collection.removeIf(predicate)` on a heap list or set: removes the elements the
    /// predicate (a SAM functional object) accepts, and returns whether it removed any.
    /// On a map's keySet, values or entrySet, the entries are removed from the map.
    pub(crate) fn remove_if(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        collection_id: crate::interpreter::HeapId,
        predicate: crate::interpreter::HeapId,
    ) -> VMResult<bool> {
        let items = heap.collection_snapshot(collection_id)?;
        let mut accepted = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let accepts = self
                .call_sam_sync(thread, heap, monitors, predicate, vec![item.clone()])?
                .as_int()
                .unwrap_or(0)
                != 0;
            if accepts {
                accepted.push((index, item));
            }
        }
        let removed_any = !accepted.is_empty();
        if heap.is_list_container(collection_id) {
            // From the last, so the indices of the earlier ones stay valid.
            for (index, _) in accepted.into_iter().rev() {
                let index =
                    i32::try_from(index).map_err(|_| VMError::internal("list too large"))?;
                heap.arraylist_remove(collection_id, index)?;
            }
        } else {
            for (_, item) in accepted {
                heap.hashset_remove(collection_id, &item)?;
            }
        }
        Ok(removed_any)
    }

    /// Invoke a SAM functional object synchronously and return its result.
    /// Supports javac lambdas (run their impl to completion) and Groovy closures
    /// (run doCall). Used by callback-driven natives such as `List.sort`.
    pub(crate) fn call_sam_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        sam_id: crate::interpreter::HeapId,
        args: Vec<Value>,
    ) -> VMResult<Value> {
        if heap
            .get_dynamic_property(sam_id, "$lambda_method")
            .is_none()
        {
            // Groovy closure path.
            let boxed = args
                .into_iter()
                .map(|a| box_primitive_for_lambda(heap, a))
                .collect::<VMResult<Vec<_>>>()?;
            return self.run_closure_to_completion(thread, heap, monitors, sam_id, boxed);
        }
        // javac lambda: push the impl frame, run to completion, take the result.
        let state = NestedExecutionState::capture(self, thread, "lambda call");
        if let Err(error) = self.invoke_java_lambda(thread, heap, sam_id, args) {
            thread.unwind_after_failure(state.target_depth(), monitors);
            state.restore(self);
            return Err(error);
        }
        self.finish_nested_execution(thread, heap, monitors, state)?;
        let result = thread
            .current_frame_mut()
            .and_then(|f| f.pop_raw())
            .unwrap_or(Value::Reference(ObjectRef::Null));
        Ok(result)
    }

    /// Run a zero-argument instance method to completion without exposing its
    /// frame to the caller. Used by the synchronous Thread model so both a
    /// Thread subclass and a concrete Runnable target execute through normal
    /// virtual method lookup.
    fn run_void_method_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        receiver_id: crate::interpreter::HeapId,
        method: &str,
        descriptor: &str,
    ) -> VMResult<()> {
        let class_name = heap.get_class(receiver_id)?.name;
        let (declaring_class, method_bytecode) = self
            .resolve_method_in_hierarchy(&class_name, method, descriptor)
            .map_err(|_| VMError::NoSuchMethod(format!("{class_name}.{method}{descriptor}")))?;
        let state = NestedExecutionState::capture(self, thread, "method call");
        if let Some(caller) = thread.current_frame_mut() {
            caller.pc = self.pc;
        }
        let frame = Frame::with_bytecode(
            declaring_class,
            method.to_string(),
            method_bytecode.max_locals,
            method_bytecode.max_stack,
            vec![Value::Reference(ObjectRef::Object(receiver_id))],
            method_bytecode.code,
            descriptor.to_string(),
            method_bytecode.exception_table,
        )?;
        thread.push_frame(frame)?;

        self.finish_nested_execution(thread, heap, monitors, state)
    }

    /// Run a user-defined `toString()` on `obj_id` to completion and return its
    /// string, or `None` when the object's class doesn't define one (callers
    /// fall back to the shared renderer). This is what lets `println(obj)` and
    /// `String.valueOf(obj)` print a record's generated `toString` (and any
    /// user override) instead of the renderer's default.
    pub(crate) fn user_tostring_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        obj_id: crate::interpreter::HeapId,
    ) -> VMResult<Option<String>> {
        match self.user_string_method_sync(thread, heap, monitors, obj_id, "toString")? {
            Some(ObjectRef::Object(id)) => Ok(heap.get_string(id).ok()),
            _ => Ok(None),
        }
    }

    /// `AbstractCollection.toString()` and `AbstractMap.toString()`. Each element is
    /// rendered with its own `toString()`, which for a user class runs the interpreter.
    pub(crate) fn collection_text_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        container: crate::interpreter::HeapId,
    ) -> VMResult<String> {
        if heap.is_map(container) {
            let mut entries = Vec::new();
            for (key, value) in heap.map_snapshot(container)? {
                let key = self.element_text_sync(thread, heap, monitors, container, &key)?;
                let value = self.element_text_sync(thread, heap, monitors, container, &value)?;
                entries.push(format!("{key}={value}"));
            }
            return Ok(format!("{{{}}}", entries.join(", ")));
        }
        let mut items = Vec::new();
        for item in heap.collection_snapshot(container)? {
            items.push(self.element_text_sync(thread, heap, monitors, container, &item)?);
        }
        Ok(format!("[{}]", items.join(", ")))
    }

    /// One element of a container, as `AbstractCollection.toString()` shows it.
    fn element_text_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        container: crate::interpreter::HeapId,
        value: &Value,
    ) -> VMResult<String> {
        let Value::Reference(ObjectRef::Object(id)) = value else {
            return Ok(heap.render_value(value));
        };
        if *id == container {
            return Ok(if heap.is_map(container) {
                "(this Map)".to_string()
            } else {
                "(this Collection)".to_string()
            });
        }
        if heap.is_container(*id) {
            return self.collection_text_sync(thread, heap, monitors, *id);
        }
        match self.user_tostring_sync(thread, heap, monitors, *id)? {
            Some(text) => Ok(text),
            None => Ok(heap.render_value(value)),
        }
    }

    /// Execute a user-bytecode no-arg String-returning virtual method without
    /// allowing modeled JDK native bodies to stand in for an override.
    fn user_string_method_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        obj_id: crate::interpreter::HeapId,
        method_name: &str,
    ) -> VMResult<Option<ObjectRef>> {
        let class_name = match heap.get_class(obj_id) {
            Ok(c) => c.name,
            Err(_) => return Ok(None),
        };
        let Some((decl, mb)) = self
            .lookup_method_either_form(&class_name, method_name, "()Ljava/lang/String;")
            .or_else(|| {
                // The implementation may be inherited from a user superclass.
                self.resolve_method_in_hierarchy(&class_name, method_name, "()Ljava/lang/String;")
                    .ok()
            })
        else {
            return Ok(None);
        };
        let state = NestedExecutionState::capture(self, thread, "String-returning virtual call");
        if let Some(caller) = thread.current_frame_mut() {
            caller.pc = self.pc;
        }
        let frame = Frame::with_bytecode(
            decl,
            method_name.to_string(),
            mb.max_locals,
            mb.max_stack,
            vec![Value::Reference(ObjectRef::Object(obj_id))],
            mb.code.clone(),
            "()Ljava/lang/String;".to_string(),
            mb.exception_table.clone(),
        )?;
        thread.push_frame(frame)?;
        self.finish_nested_execution(thread, heap, monitors, state)?;
        let result = thread.current_frame_mut().and_then(|f| f.pop_raw());
        match result {
            Some(Value::Reference(reference)) => Ok(Some(reference)),
            _ => Ok(None),
        }
    }

    /// Runs a bytecode method of `obj_id`'s class to completion and returns its result,
    /// or `None` for a void method. The caller checks that a bytecode declaration exists.
    #[allow(clippy::too_many_arguments)]
    fn invoke_user_method_sync(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        obj_id: crate::interpreter::HeapId,
        method_name: &str,
        descriptor: &str,
        args: Vec<Value>,
    ) -> VMResult<Option<Value>> {
        let class_name = heap.get_class(obj_id)?.name;
        let Some((decl, mb)) = self
            .lookup_method_either_form(&class_name, method_name, descriptor)
            .or_else(|| {
                self.resolve_method_in_hierarchy(&class_name, method_name, descriptor)
                    .ok()
            })
        else {
            return Err(VMError::NoSuchMethod(format!(
                "{class_name}.{method_name}{descriptor}"
            )));
        };
        let state = NestedExecutionState::capture(self, thread, "user method call");
        if let Some(caller) = thread.current_frame_mut() {
            caller.pc = self.pc;
        }
        let mut locals = vec![Value::Reference(ObjectRef::Object(obj_id))];
        locals.extend(args);
        let frame = Frame::with_bytecode(
            decl,
            method_name.to_string(),
            mb.max_locals,
            mb.max_stack,
            locals,
            mb.code.clone(),
            descriptor.to_string(),
            mb.exception_table.clone(),
        )?;
        thread.push_frame(frame)?;
        self.finish_nested_execution(thread, heap, monitors, state)?;
        if descriptor.ends_with(")V") {
            return Ok(None);
        }
        Ok(thread.current_frame_mut().and_then(|f| f.pop_raw()))
    }

    /// The key the hash-based map or set `container` stores for `key`. A user object whose
    /// class overrides both `hashCode()` and `equals()` is keyed by the key of `container`
    /// that is equal to it, so equal objects are one key. When there is none, `key` is
    /// its own key, and `inserting` records it as a key of `container`. Every other
    /// object keeps identity, which is what the map would compare without the overrides.
    pub(super) fn canonical_container_key(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        container: crate::interpreter::HeapId,
        key: Value,
        inserting: bool,
    ) -> VMResult<Value> {
        let Value::Reference(ObjectRef::Object(id)) = key else {
            return Ok(key);
        };
        let class_name = heap.get_class(id)?.name;
        // JDK classes (strings, boxes, collections) have their own canonical forms.
        if class_name.starts_with("java/") {
            return Ok(key);
        }
        if !(self.has_bytecode_declaration(&class_name, "hashCode", "()I")
            && self.has_bytecode_declaration(&class_name, "equals", "(Ljava/lang/Object;)Z"))
        {
            return Ok(key);
        }
        let hash = match self.invoke_user_method_sync(
            thread,
            heap,
            monitors,
            id,
            "hashCode",
            "()I",
            Vec::new(),
        )? {
            Some(Value::Int(hash)) => hash,
            _ => return Ok(key),
        };
        for candidate in heap.user_key_candidates(container, hash) {
            if candidate == id {
                return Ok(key);
            }
            let equal = self.invoke_user_method_sync(
                thread,
                heap,
                monitors,
                id,
                "equals",
                "(Ljava/lang/Object;)Z",
                vec![Value::Reference(ObjectRef::Object(candidate))],
            )?;
            if matches!(equal, Some(Value::Int(flag)) if flag != 0) {
                return Ok(Value::Reference(ObjectRef::Object(candidate)));
            }
        }
        if inserting {
            heap.add_user_key(container, hash, id);
        }
        Ok(key)
    }

    /// The element of `list` that `key.equals(element)` accepts, so the list's identity
    /// search finds it. A list compares with `equals()` alone, so a class that overrides
    /// only `equals()` is enough. Any other value keeps identity.
    fn list_element_equal_to(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        list: crate::interpreter::HeapId,
        key: Value,
    ) -> VMResult<Value> {
        let Value::Reference(ObjectRef::Object(key_id)) = key else {
            return Ok(key);
        };
        let class_name = heap.get_class(key_id)?.name;
        if class_name.starts_with("java/")
            || !self.has_bytecode_declaration(&class_name, "equals", "(Ljava/lang/Object;)Z")
        {
            return Ok(key);
        }
        for element in heap.collection_snapshot(list)? {
            let equal = self.invoke_user_method_sync(
                thread,
                heap,
                monitors,
                key_id,
                "equals",
                "(Ljava/lang/Object;)Z",
                vec![element.clone()],
            )?;
            if matches!(equal, Some(Value::Int(flag)) if flag != 0) {
                return Ok(element);
            }
        }
        Ok(key)
    }

    /// Canonicalizes the key argument of a put, get, containsKey, add, contains or remove on
    /// a hash-based map or set, so the map's identity comparison matches `equals()`.
    fn canonicalize_container_args(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        container: crate::interpreter::HeapId,
        method_name: &str,
        mut args: Vec<Value>,
    ) -> VMResult<Vec<Value>> {
        if heap.is_list_container(container) {
            if matches!(
                (method_name, args.len()),
                ("contains" | "indexOf" | "remove", 1)
            ) {
                let key = args[0].clone();
                args[0] = self.list_element_equal_to(thread, heap, monitors, container, key)?;
            }
            return Ok(args);
        }
        if !heap.is_hashed_container(container) {
            return Ok(args);
        }
        let keyed = matches!(
            (method_name, args.len()),
            ("put", 2)
                | (
                    "get" | "getAt" | "containsKey" | "add" | "contains" | "remove",
                    1
                )
        );
        if keyed {
            let key = args[0].clone();
            let inserting = matches!(method_name, "put" | "add");
            args[0] =
                self.canonical_container_key(thread, heap, monitors, container, key, inserting)?;
        }
        Ok(args)
    }

    /// `StringBuilder.append(Object)` appends the object's `toString()`, which a user
    /// class runs in bytecode. The object is rendered here, before the builder sees it.
    #[allow(clippy::too_many_arguments)]
    fn render_user_append_argument(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
        builder: crate::interpreter::HeapId,
        method_name: &str,
        descriptor: &str,
        mut args: Vec<Value>,
    ) -> VMResult<Vec<Value>> {
        if method_name != "append" || !descriptor.starts_with("(Ljava/lang/Object;)") {
            return Ok(args);
        }
        if heap.get_object_class_name(builder)? != "java/lang/StringBuilder" {
            return Ok(args);
        }
        let Some(Value::Reference(ObjectRef::Object(object))) = args.first().cloned() else {
            return Ok(args);
        };
        // JDK objects render through the shared renderer; only user classes need bytecode.
        if heap.get_object_class_name(object)?.starts_with("java/") {
            return Ok(args);
        }
        let text = match self.user_tostring_sync(thread, heap, monitors, object)? {
            Some(text) => text,
            None => heap.render_value(&Value::Reference(ObjectRef::Object(object))),
        };
        args[0] = Value::Reference(heap.new_string(&text)?);
        Ok(args)
    }

    /// The SAM (functional-interface abstract method) name a javac lambda
    /// implements, recorded as the `$lambda_sam` dynamic property at creation.
    /// `None` for non-javac (Groovy) closures.
    fn lambda_sam_name(
        &self,
        heap: &Heap,
        lambda_id: crate::interpreter::HeapId,
    ) -> Option<String> {
        match heap.get_dynamic_property(lambda_id, "$lambda_sam") {
            Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(id).ok(),
            _ => None,
        }
    }

    /// Run a javac lambda: gather captured variables (object fields 0..ncap) +
    /// the SAM call args, then dispatch the recorded implementation method.
    /// Static impls push a frame directly; instance-method-reference impls treat
    /// the first value as the receiver (handling JDK natives like `String::length`).
    fn invoke_java_lambda(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        lambda_id: crate::interpreter::HeapId,
        sam_args: Vec<Value>,
    ) -> VMResult<()> {
        let read_str = |heap: &Heap, key: &str| -> VMResult<String> {
            match heap.get_dynamic_property(lambda_id, key) {
                Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(id),
                _ => Ok(String::new()),
            }
        };
        let impl_class = read_str(heap, "$lambda_class")?;
        let impl_method = read_str(heap, "$lambda_method")?;
        let impl_desc = read_str(heap, "$lambda_desc")?;
        let kind = match heap.get_dynamic_property(lambda_id, "$lambda_kind") {
            Some(Value::Int(k)) => k,
            _ => 0,
        };
        let ncap = match heap.get_dynamic_property(lambda_id, "$lambda_ncap") {
            Some(Value::Int(n)) => n.max(0) as usize,
            _ => 0,
        };
        let mut full = Vec::with_capacity(ncap + sam_args.len());
        for i in 0..ncap {
            full.push(
                heap.get_field(lambda_id, i)?
                    .unwrap_or(Value::Reference(ObjectRef::Null)),
            );
        }
        full.extend(sam_args);

        if kind == 0 {
            // Static implementation: locals = captures ++ args.
            let (cls, mb) = self
                .lookup_method_either_form(&impl_class, &impl_method, &impl_desc)
                .ok_or_else(|| {
                    VMError::NoSuchMethod(format!("{impl_class}.{impl_method}{impl_desc}"))
                })?;
            let new_frame = Frame::with_bytecode(
                cls,
                impl_method,
                mb.max_locals,
                mb.max_stack,
                full,
                mb.code.clone(),
                impl_desc,
                mb.exception_table.clone(),
            )?;
            if let Some(caller) = thread.current_frame_mut() {
                caller.pc = self.pc;
            }
            thread.push_frame(new_frame)?;
            return Ok(());
        }

        // Instance method reference: the first value is the receiver.
        let receiver = full.remove(0);
        let recv_ref = receiver.as_reference()?;
        if recv_ref.is_null() {
            return Err(VMError::NullPointerException);
        }
        let heap_id = recv_ref.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        let recv_class = heap.get_class(heap_id)?.name;
        // Try a JDK native (e.g. String::length) first.
        if self.try_native_virtual_dispatch(
            &recv_class,
            &impl_method,
            &impl_desc,
            recv_ref,
            heap_id,
            &full,
            thread,
            heap,
        )? {
            return Ok(());
        }
        // Otherwise a user instance method: resolve and push a frame.
        let (decl, mb) = self.resolve_method_in_hierarchy(&recv_class, &impl_method, &impl_desc)?;
        let mut locals = vec![receiver];
        locals.extend(full);
        let new_frame = Frame::with_bytecode(
            decl,
            impl_method,
            mb.max_locals,
            mb.max_stack,
            locals,
            mb.code.clone(),
            impl_desc,
            mb.exception_table.clone(),
        )?;
        if let Some(caller) = thread.current_frame_mut() {
            caller.pc = self.pc;
        }
        thread.push_frame(new_frame)?;
        Ok(())
    }

    fn resolve_invokevirtual_symbolic(
        &self,
        declared_class: &str,
        method_name: &str,
        descriptor: &str,
    ) -> VMResult<(
        String,
        u16,
        Option<std::sync::Arc<crate::interpreter::method_area::VirtualMethodDeclaration>>,
    )> {
        if declared_class.starts_with('[')
            && method_name == "clone"
            && descriptor == "()Ljava/lang/Object;"
        {
            return Ok((
                declared_class.to_string(),
                crate::interpreter::executor::access_flags::ACC_PUBLIC,
                None,
            ));
        }
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;
        let symbolic_class = method_area
            .get_class(declared_class)
            .map_err(|error| invocation_resolution_error(error, declared_class))?;
        let symbolic_is_native_stub = symbolic_class.is_native_stub;
        if symbolic_class.is_interface && !symbolic_is_native_stub {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "invokevirtual symbolic owner {declared_class} is an interface"
            )));
        }

        let mut current = Some(symbolic_class.clone());
        while let Some(class) = current {
            if let Some(method) = class.declared_non_virtual_method(method_name, descriptor) {
                if method.access_flags & crate::interpreter::executor::access_flags::ACC_STATIC != 0
                {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "invokevirtual resolved static method {}.{}{}",
                        class.name, method_name, descriptor
                    )));
                }
                return Ok((class.name.clone(), method.access_flags, None));
            }
            if let Ok(route) = class.resolve_virtual_declaration(method_name, descriptor) {
                if !route.declared_in_interface
                    && route.declaring_class.replace('/', ".") == class.name.replace('/', ".")
                {
                    return Ok((
                        route.declaring_class.to_string(),
                        route.access_flags,
                        Some(route),
                    ));
                }
            }
            current = class
                .superclass
                .as_deref()
                .and_then(|parent| method_area.get_class(parent).ok());
        }

        if symbolic_class.is_interface && symbolic_is_native_stub {
            return Ok((
                symbolic_class.name,
                crate::interpreter::executor::access_flags::ACC_PUBLIC,
                None,
            ));
        }
        match symbolic_class.resolve_virtual_declaration(method_name, descriptor) {
            Ok(route) => Ok((
                route.declaring_class.to_string(),
                route.access_flags,
                Some(route),
            )),
            Err(_)
                if symbolic_is_native_stub
                    && native_stub_has_compatible_method(
                        &method_area,
                        declared_class,
                        method_name,
                        descriptor,
                    ) =>
            {
                Ok((
                    symbolic_class.name,
                    crate::interpreter::executor::access_flags::ACC_PUBLIC,
                    None,
                ))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn execute_invokevirtual(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Resolve method reference from constant pool
        let (mut declared_class, method_name, descriptor) =
            self.resolve_class_method_ref(cp_index)?;
        reject_illegal_initializer(&declared_class, &method_name, &descriptor, false)?;
        if declared_class == "Runnable" {
            declared_class = "java/lang/Runnable".to_string();
        }

        // Parse descriptor to get argument count
        let desc = MethodDescriptor::parse(&descriptor)
            .map_err(|_| VMError::internal("Invalid method descriptor"))?;
        let arg_count = desc.parameters.len();
        let symbolic_caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        self.check_symbolic_class_access(&symbolic_caller_class, &declared_class)
            .map_err(|error| invocation_resolution_error(error, &declared_class))?;
        let (resolved_class, resolved_access_flags, resolved_route) =
            self.resolve_invokevirtual_symbolic(&declared_class, &method_name, &descriptor)?;
        self.check_method_symbolic_access(
            &symbolic_caller_class,
            &resolved_class,
            &method_name,
            resolved_access_flags,
        )?;

        // Pop arguments first (to avoid double-borrow issue)
        let args = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            debug!(
                "INVOKEVIRTUAL {}: popping {} args, frame={}, stack_size={}",
                method_name,
                arg_count,
                frame.class_name,
                frame.operand_stack.len()
            );
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            args.reverse();
            args
        };

        // Auto-unbox tolerance: a wrapper `xValue()` call on an
        // already-raw primitive receiver — e.g. a collection element stored
        // unboxed, then coerced via `Integer.intValue()` — is the identity. The
        // boxed case falls through to the native below.
        if desc.parameters.is_empty() {
            let top = thread
                .current_frame()
                .and_then(|f| f.operand_stack.last().cloned());
            if let Some(v @ (Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_))) =
                top
            {
                let trusted_wrapper_method = self
                    .method_area
                    .as_ref()
                    .and_then(|area| area.read().ok())
                    .and_then(|area| area.get_class(&declared_class).ok())
                    .is_some_and(|class| {
                        is_trusted_raw_unbox_declaration(
                            &class,
                            &declared_class,
                            &method_name,
                            &descriptor,
                        )
                    });
                if trusted_wrapper_method {
                    if let Some(converted) = unbox_identity(&method_name, &v) {
                        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                        frame.pop_raw();
                        frame.push_raw(converted);
                        return Ok(());
                    }
                }
            }
        }

        // Pop receiver
        let receiver = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            debug!(
                "INVOKEVIRTUAL {}: popping receiver, frame={}, stack_size={}",
                method_name,
                frame.class_name,
                frame.operand_stack.len()
            );
            frame.pop()?.as_reference()?
        };
        if receiver.is_null() {
            return Err(VMError::NullPointerException);
        }

        // Array methods: `clone()` shallow-copies; there's no loadable class for
        // an array type, so handle it before the class-based dispatch below.
        if let ObjectRef::Array(arr_id) = receiver {
            if method_name == "clone" {
                let cloned = heap.clone_array(arr_id)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(Value::Reference(ObjectRef::Array(cloned)));
                return Ok(());
            }
        }

        // Get receiver class for native method dispatch
        let heap_id = receiver.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        let args =
            self.canonicalize_container_args(thread, heap, monitors, heap_id, &method_name, args)?;
        let args = self.render_user_append_argument(
            thread,
            heap,
            monitors,
            heap_id,
            &method_name,
            &descriptor,
            args,
        )?;
        let receiver_class = heap.get_class(heap_id)?;
        let receiver_linkage = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?
            .get_class(&receiver_class.name)?;
        if resolved_route.is_some()
            && receiver_linkage.has_default_method_conflict(&method_name, &descriptor)
        {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "{} inherits conflicting defaults for {}{}",
                receiver_class.name, method_name, descriptor
            )));
        }

        let receiver_binary_name = receiver_class.name.replace('/', ".");
        let declared_binary_name = declared_class.replace('/', ".");
        let native_stub_proxy = self
            .method_area
            .as_ref()
            .and_then(|area| area.read().ok())
            .is_some_and(|area| {
                area.get_class(&receiver_class.name)
                    .is_ok_and(|class| class.is_native_stub)
                    && area
                        .get_class(&declared_class)
                        .is_ok_and(|class| class.is_native_stub)
            });
        if !native_stub_proxy
            && !receiver_class.name.contains("$$Lambda")
            && receiver_binary_name != declared_binary_name
            && !self.is_subclass_of(&receiver_binary_name, &declared_binary_name)
        {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "{} is not an instance of {}",
                receiver_class.name, declared_class
            )));
        }
        let caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        self.check_method_access_on_receiver(
            &caller_class,
            &resolved_class,
            &receiver_class.name,
            &method_name,
            resolved_access_flags,
        )?;

        // The source backend currently emits Methodref/invokevirtual for calls
        // whose static receiver type is a trusted JDK List stub. Symbolic
        // resolution above still requires the exact public non-static
        // declaration; after that, run the native comparator bridge here just
        // as invokeinterface does below.
        if native_stub_proxy
            && matches!(
                declared_class.as_str(),
                "java/util/List" | "java/util/ArrayList"
            )
            && method_name == "sort"
            && descriptor == "(Ljava/util/Comparator;)V"
        {
            let comparator = match args.first() {
                Some(Value::Reference(ObjectRef::Object(id))) => Some(*id),
                _ => None,
            };
            self.sort_list(thread, heap, monitors, heap_id, comparator)?;
            return Ok(());
        }
        if native_stub_proxy {
            if let Some(predicate) =
                removes_if_from_heap_collection(heap, heap_id, &method_name, &descriptor, &args)
            {
                let removed = self.remove_if(thread, heap, monitors, heap_id, predicate)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(Value::Int(i32::from(removed)));
                return Ok(());
            }
        }

        // A functional-interface SAM call (`f.apply(x)`, `s.get()`, …)
        // on a Java lambda routes to the synthetic closure's `doCall`. Default
        // interface methods reaching here fall through to normal dispatch.
        if receiver_class.name.contains("$$Lambda") {
            let sam = self.lambda_sam_name(heap, heap_id);
            if sam.is_none() || sam.as_deref() == Some(method_name.as_str()) {
                return self.dispatch_lambda_sam(thread, heap, monitors, heap_id, args, &desc);
            }
        }

        // ── java.lang.Thread API on a user subclass (synchronous threads) ──
        // We don't model OS threads: `start()` runs the body `run()` inline, and
        // the lifecycle/query methods collapse to their post-completion answers.
        if self.is_thread_subclass(&receiver_class.name) {
            match (method_name.as_str(), descriptor.as_str()) {
                ("start", "()V") => {
                    // A thread starts once; Java rejects a second start.
                    if heap
                        .get_dynamic_property(heap_id, "$thread_started")
                        .is_some()
                    {
                        return Err(VMError::InternalError(
                            "IllegalThreadStateException: Thread already started".to_string(),
                        ));
                    }
                    heap.set_dynamic_property(heap_id, "$thread_started", Value::Int(1));
                    let previous_thread = self.current_thread.replace(heap_id);
                    let runnable = heap.get_dynamic_property(heap_id, "$thread_runnable");
                    let result = (|| match runnable {
                        Some(Value::Reference(ObjectRef::Object(target_id))) => {
                            let target_class = heap.get_class(target_id)?.name;
                            if target_class.contains("$$Lambda")
                                || heap
                                    .get_dynamic_property(target_id, "$lambda_method")
                                    .is_some()
                            {
                                self.call_sam_sync(thread, heap, monitors, target_id, Vec::new())
                                    .map(|_| ())
                            } else {
                                self.run_void_method_sync(
                                    thread, heap, monitors, target_id, "run", "()V",
                                )
                            }
                        }
                        _ if receiver_class.name == "java/lang/Thread" => Ok(()),
                        _ => {
                            self.run_void_method_sync(thread, heap, monitors, heap_id, "run", "()V")
                        }
                    })();
                    self.current_thread = previous_thread;
                    result?;
                    return Ok(());
                }
                ("setName", "(Ljava/lang/String;)V") => {
                    if let Some(v) = args.first() {
                        heap.set_dynamic_property(heap_id, "$thread_name", v.clone());
                    }
                    return Ok(());
                }
                ("getName", "()Ljava/lang/String;") => {
                    let name = thread_name_of(heap, heap_id);
                    let r = heap.intern_string(&name)?;
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(Value::Reference(r));
                    return Ok(());
                }
                // The body already ran, so a finished thread is never alive. The interrupt
                // and daemon flags are kept on the thread object, as Thread's fields are.
                ("isAlive", "()Z") => {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(Value::Int(0));
                    return Ok(());
                }
                ("isInterrupted", "()Z") => {
                    let flag = heap.get_dynamic_property(heap_id, "$thread_interrupted");
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(Value::Int(i32::from(flag.is_some())));
                    return Ok(());
                }
                ("isDaemon", "()Z") => {
                    let flag = heap.get_dynamic_property(heap_id, "$thread_daemon");
                    let daemon = matches!(flag, Some(Value::Int(v)) if v != 0);
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(Value::Int(i32::from(daemon)));
                    return Ok(());
                }
                ("interrupt", "()V") => {
                    heap.set_dynamic_property(heap_id, "$thread_interrupted", Value::Int(1));
                    return Ok(());
                }
                ("setDaemon", "(Z)V") => {
                    if let Some(flag) = args.first() {
                        heap.set_dynamic_property(heap_id, "$thread_daemon", flag.clone());
                    }
                    return Ok(());
                }
                // join/setPriority/etc.: the body already ran synchronously, so these are
                // no-ops with a void return.
                ("join" | "setPriority" | "setUncaughtExceptionHandler", _) => return Ok(()),
                _ => {}
            }
        }

        // `println(obj)`/`print(obj)` with a user object must run the object's
        // real `toString` (a record's generated one, or a user override) — the
        // native PrintStream renderer below only knows JDK types. Pre-render
        // the argument and let the native print the resulting string.
        let mut args = args;
        if receiver_class.name == "java/io/PrintStream"
            && matches!(method_name.as_str(), "println" | "print")
            && descriptor.starts_with("(Ljava/lang/Object;)")
        {
            if let Some(Value::Reference(ObjectRef::Object(arg_id))) = args.first().cloned() {
                if let Some(s) = self.user_tostring_sync(thread, heap, monitors, arg_id)? {
                    let rendered = heap.intern_string(&s)?;
                    args[0] = Value::Reference(rendered);
                }
            }
        }

        // ── java.util.stream ───────────────────────────────────────────────
        // `stream()` on a collection snapshots its elements; operations on a
        // stream evaluate eagerly (lambdas run through the executor per
        // element, like comparator sort).
        if method_name == "stream" && descriptor == "()Ljava/util/stream/Stream;" {
            // Only when the receiver is a heap collection — a user-defined
            // `stream()` method still dispatches normally below.
            if let Ok(items) = heap.collection_snapshot(heap_id) {
                let sid = heap.allocate_stream(items)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(Value::Reference(ObjectRef::Object(sid)));
                return Ok(());
            }
        }
        if receiver_class.name.starts_with("java/util/stream/") {
            return self.dispatch_stream_method(
                thread,
                heap,
                monitors,
                heap_id,
                &method_name,
                &descriptor,
                args,
            );
        }

        // A Throwable sidecar describes state, not dispatch ownership. Do not
        // let the native Throwable implementation preempt a real user override.
        let has_throwable_sidecar = heap.throwable_data(heap_id).ok().flatten().is_some();
        let user_throwable_override = if has_throwable_sidecar {
            resolved_route.as_ref().is_some_and(|symbolic_route| {
                self.method_area
                    .as_ref()
                    .and_then(|area| area.read().ok())
                    .and_then(|area| area.get_class(&receiver_class.name).ok())
                    .and_then(|class| {
                        class
                            .dispatch_target(&method_name, &descriptor, symbolic_route)
                            .ok()
                            .map(|entry| entry.declaring_class().to_string())
                    })
                    .is_some_and(|owner| {
                        !crate::interpreter::executor::handlers::objects::is_jdk_throwable(&owner)
                    })
            })
        } else {
            false
        };
        if has_throwable_sidecar && !user_throwable_override && descriptor == "()Ljava/lang/String;"
        {
            if method_name == "getLocalizedMessage" {
                if let Some(message) =
                    self.user_string_method_sync(thread, heap, monitors, heap_id, "getMessage")?
                {
                    thread
                        .current_frame_mut()
                        .ok_or(VMError::NoActiveFrame)?
                        .push_raw(Value::Reference(message));
                    return Ok(());
                }
            } else if method_name == "toString" {
                let mut localized = self.user_string_method_sync(
                    thread,
                    heap,
                    monitors,
                    heap_id,
                    "getLocalizedMessage",
                )?;
                if localized.is_none() {
                    localized = self.user_string_method_sync(
                        thread,
                        heap,
                        monitors,
                        heap_id,
                        "getMessage",
                    )?;
                }
                if let Some(localized) = localized {
                    let class_name = receiver_class.name.replace('/', ".");
                    let rendered = match localized {
                        ObjectRef::Object(id)
                            if heap.get_object_class_name(id)? == "java/lang/String" =>
                        {
                            format!("{class_name}: {}", heap.get_string(id)?)
                        }
                        _ => class_name,
                    };
                    let rendered = heap.intern_string(&rendered)?;
                    thread
                        .current_frame_mut()
                        .ok_or(VMError::NoActiveFrame)?
                        .push_raw(Value::Reference(rendered));
                    return Ok(());
                }
            }
        }
        // `toString()` on a built-in list, set or map lists the elements, rendering each
        // with its own `toString()`. The native registry has no such method for them.
        if method_name == "toString"
            && descriptor == "()Ljava/lang/String;"
            && heap.is_container(heap_id)
        {
            let text = self.collection_text_sync(thread, heap, monitors, heap_id)?;
            let rendered = heap.intern_string(&text)?;
            thread
                .current_frame_mut()
                .ok_or(VMError::NoActiveFrame)?
                .push_raw(Value::Reference(rendered));
            return Ok(());
        }
        let handled_native = if user_throwable_override {
            false
        } else {
            self.try_native_virtual_dispatch(
                &receiver_class.name,
                &method_name,
                &descriptor,
                receiver,
                heap_id,
                &args,
                thread,
                heap,
            )?
        };

        if handled_native {
            return Ok(());
        }

        // Fall back to the NativeRegistry for JDK instance methods that aren't
        // user-defined (e.g. Object.getClass/hashCode, Class.getSimpleName,
        // wrapper helpers). These are keyed by the *declaring* class, so try the
        // constant-pool ref class, the receiver's runtime class, and Object.
        {
            let mut native_args = Vec::with_capacity(args.len() + 1);
            native_args.push(Value::Reference(receiver));
            native_args.extend(args.iter().cloned());
            // An Object method that the receiver's class overrides must run the override.
            // The Object natives only describe an instance that inherits the method.
            let overridden = self.overrides_object_method(
                &receiver_class.name,
                &method_name,
                &descriptor,
                resolved_route.as_ref(),
            );
            for owner in [
                declared_class.as_str(),
                receiver_class.name.as_str(),
                "java/lang/Object",
            ] {
                if overridden && owner == "java/lang/Object" {
                    continue;
                }
                let is_native = self
                    .native_registry
                    .as_ref()
                    .is_some_and(|r| r.is_native(owner, &method_name, &descriptor));
                if is_native {
                    if let Some((true, result)) = self.try_invoke_native(
                        heap,
                        thread,
                        owner,
                        &method_name,
                        &descriptor,
                        native_args.clone(),
                    )? {
                        if !descriptor.ends_with(")V") {
                            if let Some(v) = result {
                                let frame =
                                    thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                                frame.push_raw(v);
                            }
                        }
                        return Ok(());
                    }
                }
            }
        }

        // A private instance method is never a virtual slot. Recent javac
        // versions may nevertheless encode a same-class private call with
        // invokevirtual, so resolve an exact private declaration directly.
        // Access checking below still rejects a forged call from another
        // class. All other methods go through symbolic resolution and runtime
        // slot dispatch.
        let (declaring_class, method_bytecode) = if let Some(symbolic_route) = resolved_route {
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;

            let loaded_class = method_area.get_class(&receiver_class.name)?;
            let vtable_entry =
                loaded_class.dispatch_target(&method_name, &descriptor, &symbolic_route)?;
            vtable_entry.require_concrete(&receiver_class.name)?;
            let vtable_declaring = vtable_entry.declaring_class().to_string();

            // Drop the metadata lock before resolving bytecode/building frames.
            drop(method_area);
            let pair = self
                .lookup_method_either_form(&vtable_declaring, &method_name, &descriptor)
                .ok_or_else(|| {
                    VMError::NoSuchMethod(format!(
                        "{}.{}{}",
                        vtable_declaring, method_name, descriptor
                    ))
                })?;
            pair
        } else {
            self.lookup_method_either_form(&resolved_class, &method_name, &descriptor)
                .ok_or_else(|| {
                    VMError::NoSuchMethod(format!(
                        "{}.{}{}",
                        resolved_class, method_name, descriptor
                    ))
                })?
        };

        // Create frame for the called method
        // Local 0 = receiver (this), locals 1..n = arguments
        let mut locals = vec![Value::Reference(receiver)];
        for arg in args {
            locals.push(arg);
        }

        let mut new_frame = Frame::with_bytecode(
            declaring_class,
            method_name,
            method_bytecode.max_locals,
            method_bytecode.max_stack,
            locals,
            method_bytecode.code.clone(),
            descriptor,
            method_bytecode.exception_table.clone(),
        )?;

        apply_synchronized_if_needed(
            method_bytecode.access_flags,
            &mut new_frame,
            thread,
            monitors,
        )?;

        // Save caller's PC before pushing new frame
        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        thread.push_frame(new_frame)?;
        Ok(())
    }

    /// Execute invokespecial: Call constructor, superclass, or private method.
    /// Stack: ..., objectref, [args] -> ..., [result]
    pub(crate) fn execute_invokespecial(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Resolve method reference from constant pool
        let (class_name, method_name, descriptor, reference_kind) =
            self.resolve_method_ref_with_kind(cp_index)?;
        reject_illegal_initializer(&class_name, &method_name, &descriptor, true)?;
        let caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        self.check_symbolic_class_access(&caller_class, &class_name)
            .map_err(|error| invocation_resolution_error(error, &class_name))?;
        let (mut resolved_class, mut resolved_flags) = {
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;
            let owner = method_area
                .get_class(&class_name)
                .map_err(|error| invocation_resolution_error(error, &class_name))?;
            let expects_interface = matches!(
                reference_kind,
                crate::interpreter::executor::handlers::cp_resolution::MethodReferenceKind::Interface
            );
            if owner.is_interface != expects_interface {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "invokespecial reference kind does not match {class_name}"
                )));
            }
            if method_name == "<init>" {
                if owner.is_interface {
                    return Err(VMError::NoSuchMethod(format!(
                        "interface constructor {class_name}.{method_name}{descriptor}"
                    )));
                }
                let declaration = owner
                    .declared_non_virtual_method(&method_name, &descriptor)
                    .ok_or_else(|| {
                        VMError::NoSuchMethod(format!("{class_name}.{method_name}{descriptor}"))
                    })?;
                (owner.name.clone(), declaration.access_flags)
            } else if let Some(declaration) =
                owner.declared_non_virtual_method(&method_name, &descriptor)
            {
                (owner.name.clone(), declaration.access_flags)
            } else {
                let declaration = owner.resolve_virtual_declaration(&method_name, &descriptor)?;
                (
                    declaration.declaring_class.to_string(),
                    declaration.access_flags,
                )
            }
        };
        if matches!(
            reference_kind,
            crate::interpreter::executor::handlers::cp_resolution::MethodReferenceKind::Interface
        ) {
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;
            let caller = method_area.get_class(&caller_class)?;
            let owner_is_caller = caller.name.replace('/', ".") == class_name.replace('/', ".");
            let direct_superinterface = caller
                .interfaces
                .iter()
                .any(|interface| interface.replace('/', ".") == class_name.replace('/', "."));
            if !owner_is_caller && !direct_superinterface {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "{class_name} is not a direct superinterface of {caller_class}"
                )));
            }
        } else if method_name != "<init>"
            && caller_class.replace('/', ".") != class_name.replace('/', ".")
            && self.is_subclass_of(&caller_class, &class_name)
        {
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;
            let caller = method_area.get_class(&caller_class)?;
            if caller.modifiers & 0x0020 != 0 {
                let mut current = caller
                    .superclass
                    .as_deref()
                    .and_then(|parent| method_area.get_class(parent).ok());
                while let Some(class) = current {
                    if let Ok(declaration) =
                        class.resolve_virtual_declaration(&method_name, &descriptor)
                    {
                        if declaration.declaring_class.replace('/', ".")
                            == class.name.replace('/', ".")
                        {
                            resolved_class = class.name;
                            resolved_flags = declaration.access_flags;
                            break;
                        }
                    }
                    current = class
                        .superclass
                        .as_deref()
                        .and_then(|parent| method_area.get_class(parent).ok());
                }
            }
        }
        if resolved_flags & crate::interpreter::executor::access_flags::ACC_STATIC != 0 {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "invokespecial resolved static method {resolved_class}.{method_name}{descriptor}"
            )));
        }
        if resolved_flags & 0x0400 != 0 {
            return Err(VMError::AbstractMethodError(format!(
                "{resolved_class}.{method_name}{descriptor}"
            )));
        }
        self.check_method_symbolic_access(
            &caller_class,
            &resolved_class,
            &method_name,
            resolved_flags,
        )?;

        // Parse descriptor to get argument count
        let desc = MethodDescriptor::parse(&descriptor)
            .map_err(|_| VMError::internal("Invalid method descriptor"))?;
        let arg_count = desc.parameters.len();

        // Pop arguments first (to avoid double-borrow issue)
        let args = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            args.reverse();
            args
        };

        // Pop receiver
        let receiver = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            frame.pop()?.as_reference()?
        };
        if receiver.is_null() {
            return Err(VMError::NullPointerException);
        }
        let receiver_class = receiver
            .heap_id()
            .and_then(|id| heap.get_class(id).ok())
            .ok_or_else(VMError::invalid_object_ref)?;
        let compatible = if matches!(
            reference_kind,
            crate::interpreter::executor::handlers::cp_resolution::MethodReferenceKind::Interface
        ) {
            self.class_implements_interface(&receiver_class.name, &class_name)
                || receiver_class.name.replace('/', ".") == class_name.replace('/', ".")
        } else {
            receiver_class.name.replace('/', ".") == class_name.replace('/', ".")
                || self.is_subclass_of(&receiver_class.name, &class_name)
        };
        if !compatible {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "{} is not compatible with invokespecial owner {}",
                receiver_class.name, class_name
            )));
        }

        // Handle native constructors for standard Java/Groovy classes
        // These classes don't exist in our method registry - we simulate their initialization
        let handled_native_constructor = if method_name == "<init>" {
            match (class_name.as_str(), descriptor.as_str()) {
                // `new StringBuilder(String)` / `(CharSequence)` seeds the buffer
                // (stored in field 0); `()`/`(int capacity)` start empty.
                ("java/lang/StringBuilder", "(Ljava/lang/String;)V")
                | ("java/lang/StringBuilder", "(Ljava/lang/CharSequence;)V") => {
                    if let (ObjectRef::Object(id), Some(seed)) = (receiver, args.first()) {
                        heap.set_field(id, 0, seed.clone())?;
                    }
                    true
                }
                // A new String object takes its characters here; a `String(String)` copy
                // is its own object, so it is not `==` to its source.
                ("java/lang/String", "()V") => {
                    if let ObjectRef::Object(id) = receiver {
                        heap.set_string_value(id, "")?;
                    }
                    true
                }
                ("java/lang/String", "(Ljava/lang/String;)V") => {
                    let source = match args.first() {
                        Some(Value::Reference(ObjectRef::Object(source))) => *source,
                        _ => return Err(VMError::NullPointerException),
                    };
                    let text = heap.get_string(source)?;
                    if let ObjectRef::Object(id) = receiver {
                        heap.set_string_value(id, &text)?;
                    }
                    true
                }
                ("java/lang/StringBuilder", "()V" | "(I)V")
                | ("java/lang/Object", "()V")
                // A record's canonical constructor calls super java.lang.Record().
                | ("java/lang/Record", "()V")
                // Heap-backed containers: `new` already allocated the backing
                // structure, so a no-arg or capacity/load-factor constructor needs
                // no further initialization. Collection-copy and Comparator-taking
                // constructors are intentionally left to fall through (they carry
                // data/ordering we cannot yet honor).
                | ("java/util/ArrayList", "()V" | "(I)V")
                | ("java/util/LinkedList", "()V")
                | ("java/util/HashMap", "()V" | "(I)V" | "(IF)V")
                | ("java/util/LinkedHashMap", "()V" | "(I)V" | "(IF)V")
                | ("java/util/HashSet", "()V" | "(I)V" | "(IF)V")
                | ("java/util/LinkedHashSet", "()V" | "(I)V" | "(IF)V")
                | ("java/util/TreeMap" | "java/util/TreeSet", "()V") => {
                    // No further initialization needed - object is ready to use.
                    true
                }
                // Copy constructors: new ArrayList<>(otherCollection) etc. The
                // `new` already allocated an empty backing structure; seed it
                // with the source collection's elements.
                (
                    "java/util/ArrayList" | "java/util/LinkedList" | "java/util/HashSet"
                    | "java/util/LinkedHashSet" | "java/util/TreeSet",
                    "(Ljava/util/Collection;)V",
                ) => {
                    if let (ObjectRef::Object(dest), Some(Value::Reference(ObjectRef::Object(src)))) =
                        (receiver, args.first())
                    {
                        if let Ok(items) = heap.collection_snapshot(*src) {
                            for v in items {
                                match class_name.as_str() {
                                    // Hash-based sets key their elements by equals() and
                                    // hashCode(), as the map operations do.
                                    "java/util/HashSet" | "java/util/LinkedHashSet" => {
                                        let key = self.canonical_container_key(
                                            thread, heap, monitors, dest, v, true,
                                        )?;
                                        heap.hashset_add(dest, key)?;
                                    }
                                    "java/util/TreeSet" => {
                                        heap.hashset_add(dest, v)?;
                                    }
                                    "java/util/LinkedList" => {
                                        heap.linkedlist_add(dest, v)?;
                                    }
                                    _ => {
                                        heap.arraylist_add(dest, v)?;
                                    }
                                }
                            }
                        }
                    }
                    true
                }
                // ── AWT / ImageIO ──────────────────────────────────────────
                // BufferedImage(width, height, type): allocate the backing pixel
                // buffer (an int[] of width*height) and record dimensions.
                ("java/awt/image/BufferedImage", _) => {
                    if let ObjectRef::Object(id) = receiver {
                        let w = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
                        let h = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
                        let pixels = heap.allocate_array(
                            crate::interpreter::heap::ArrayElementType::Int,
                            w.saturating_mul(h).max(0),
                        )?;
                        heap.set_field(id, 0, Value::Int(w))?;
                        heap.set_field(id, 1, Value::Int(h))?;
                        heap.set_field(id, 2, Value::Reference(ObjectRef::Array(pixels)))?;
                    }
                    true
                }
                // Color(r, g, b) / Color(r, g, b, a) / Color(rgb): store as packed
                // 0xAARRGGBB (alpha forced opaque for the 3-arg form).
                ("java/awt/Color", _) => {
                    if let ObjectRef::Object(id) = receiver {
                        let packed = match args.len() {
                            1 => 0xFF00_0000u32 | (args[0].as_int().unwrap_or(0) as u32),
                            _ => {
                                let r = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
                                let g = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
                                let b = args.get(2).and_then(|v| v.as_int().ok()).unwrap_or(0);
                                0xFF00_0000
                                    | ((r as u32 & 0xff) << 16)
                                    | ((g as u32 & 0xff) << 8)
                                    | (b as u32 & 0xff)
                            }
                        };
                        heap.set_field(id, 0, Value::Int(packed as i32))?;
                    }
                    true
                }
                // java.lang.Thread(...) — the receiver is a user subclass of
                // Thread (the implicit/explicit super() call). We don't model OS
                // threads; record the optional thread name as a dynamic property
                // so getName()/currentThread().getName() can return it, and stash
                // a Runnable target if one was supplied.
                ("java/lang/Thread", _) => {
                    if let ObjectRef::Object(id) = receiver {
                        let params = MethodDescriptor::parse(&descriptor)
                            .map_err(|error| VMError::internal(format!("malformed method descriptor {descriptor}: {error}")))?
                            .parameters;
                        for (i, p) in params.iter().enumerate() {
                            if let crate::descriptor::DescriptorType::Class { name } = p {
                                match name.as_str() {
                                    "java.lang.String" => {
                                        if let Some(v) = args.get(i) {
                                            heap.set_dynamic_property(id, "$thread_name", v.clone());
                                        }
                                    }
                                    "java.lang.Runnable" => {
                                        if let Some(v) = args.get(i) {
                                            heap.set_dynamic_property(
                                                id,
                                                "$thread_runnable",
                                                v.clone(),
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    true
                }
                // java.lang.Enum(name, ordinal) — the synthetic super() call from
                // an enum's constructor. Store the constant's name/ordinal as
                // dynamic properties (the subclass has no inherited Enum slots).
                ("java/lang/Enum", "(Ljava/lang/String;I)V") => {
                    if let ObjectRef::Object(id) = receiver {
                        if let Some(name) = args.first() {
                            heap.set_dynamic_property(id, "$enum_name", name.clone());
                        }
                        if let Some(ord) = args.get(1) {
                            heap.set_dynamic_property(id, "$enum_ordinal", ord.clone());
                        }
                    }
                    true
                }
                // java.util.concurrent.CountDownLatch(count): store remaining count.
                ("java/util/concurrent/CountDownLatch", "(I)V") => {
                    if let ObjectRef::Object(id) = receiver {
                        let count = args.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
                        heap.set_field(id, 0, Value::Int(count.max(0)))?;
                    }
                    true
                }
                // Properties()/Hashtable(): heap-backed map already allocated by `new`.
                ("java/util/Properties", "()V") | ("java/util/Hashtable", "()V" | "(I)V") => true,
                // File(pathname): retain the path string for later writes.
                ("java/io/File", "(Ljava/lang/String;)V") => {
                    if let (ObjectRef::Object(id), Some(path)) = (receiver, args.first()) {
                        heap.set_field(id, 0, path.clone())?;
                    }
                    true
                }
                // BasicStroke / other File ctors: nothing to initialize (stroke
                // width is visual-only and unused by our rasterizer).
                ("java/awt/BasicStroke", _) | ("java/io/File", _) => true,
                ("groovy/lang/IntRange", "(ZII)V") => {
                    // IntRange(inclusive, from, to) - store normalized fields.
                    if let ObjectRef::Object(heap_id) = receiver {
                        if args.len() >= 3 {
                            heap.set_field(heap_id, 0, args[1].clone())?; // from
                            heap.set_field(heap_id, 1, args[2].clone())?; // to
                            heap.set_field(heap_id, 2, args[0].clone())?; // inclusive
                        }
                    }
                    true
                }
                ("groovy/lang/Closure", "(Ljava/lang/Object;)V")
                | ("groovy/lang/Closure", "(Ljava/lang/Object;Ljava/lang/Object;)V") => {
                    if let ObjectRef::Object(heap_id) = receiver {
                        use crate::interpreter::closure_runtime::{field_offset, ResolveStrategy};
                        let owner = args
                            .first()
                            .cloned()
                            .unwrap_or(Value::Reference(ObjectRef::Null));
                        heap.set_field(
                            heap_id,
                            field_offset::DELEGATE,
                            Value::Reference(ObjectRef::Null),
                        )?;
                        heap.set_field(heap_id, field_offset::OWNER, owner)?;
                        heap.set_field(
                            heap_id,
                            field_offset::RESOLVE_STRATEGY,
                            Value::Int(ResolveStrategy::OwnerFirst as i32),
                        )?;
                    }
                    true
                }
                // JDK Throwable(...) — initialize the VM-owned Throwable
                // payload. It is shared by stubs and user subclasses, so these
                // constructors cannot alias ordinary subclass field offsets.
                (cn, _) if crate::interpreter::executor::handlers::objects::is_jdk_throwable(cn) => {
                    if let ObjectRef::Object(id) = receiver {
                        let params = MethodDescriptor::parse(&descriptor)
                            .map_err(|error| VMError::internal(format!("malformed method descriptor {descriptor}: {error}")))?
                            .parameters;
                        let mut message = ObjectRef::Null;
                        let mut cause = None;
                        for (i, p) in params.iter().enumerate() {
                            if let crate::descriptor::DescriptorType::Class { name } = p {
                                match name.as_str() {
                                    "java.lang.String" => {
                                        if let Some(Value::Reference(reference)) = args.get(i) {
                                            message = *reference;
                                        }
                                    }
                                    "java.lang.Throwable" => {
                                        if let Some(Value::Reference(reference)) = args.get(i) {
                                            cause = Some(*reference);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if message.is_null() && descriptor == "(Ljava/lang/Throwable;)V" {
                            if let Some(ObjectRef::Object(cause_id)) = cause {
                                let rendered = match self
                                    .user_tostring_sync(thread, heap, monitors, cause_id)?
                                {
                                    Some(rendered) => rendered,
                                    None => {
                                        let class = heap.get_object_class_name(cause_id)?;
                                        let suffix = heap
                                            .throwable_message(cause_id)
                                            .ok()
                                            .and_then(|reference| reference.heap_id())
                                            .and_then(|message_id| heap.get_string(message_id).ok())
                                            .map(|message| format!(": {message}"))
                                            .unwrap_or_default();
                                        format!("{}{suffix}", class.replace('/', "."))
                                    }
                                };
                                message = heap.intern_string(&rendered)?;
                            }
                        }
                        heap.initialize_throwable(id, message, cause)?;
                    }
                    true
                }
                // Note: Don't short-circuit closure subclass constructors ($_closure_)
                // They need to run to store captured variables in fields
                _ => false,
            }
        } else {
            false
        };

        if handled_native_constructor {
            return Ok(());
        }

        // A non-constructor invokespecial may deliberately target an inherited
        // native Throwable method, for example `super.getMessage()` in a user
        // exception. The symbolic resolution above has already selected and
        // access-checked that non-virtual declaration; route only that JDK
        // Throwable family through the sidecar implementation. Use the runtime
        // class for methods such as Throwable.toString(), whose result names the
        // actual receiver class.
        if crate::interpreter::executor::handlers::objects::is_jdk_throwable(&resolved_class)
            && self.try_native_virtual_dispatch(
                &receiver_class.name,
                &method_name,
                &descriptor,
                receiver,
                receiver.heap_id().ok_or_else(VMError::invalid_object_ref)?,
                &args,
                thread,
                heap,
            )?
        {
            return Ok(());
        }

        // Look up method in method registry, searching superclass chain
        // Per JVMS 6.5.invokespecial: if method not in referenced class,
        // search superclass chain (e.g., super.method() where method is in grandparent)
        let (actual_class, method_bytecode) =
            self.resolve_method_in_hierarchy(&resolved_class, &method_name, &descriptor)?;

        // Check access control (JVMS 5.4.4)
        // Note: invokespecial can call private methods of the same class (e.g., private helpers)
        let caller_class = {
            let frame = thread.current_frame().ok_or(VMError::NoActiveFrame)?;
            frame.class_name.clone()
        };
        self.check_method_symbolic_access(
            &caller_class,
            &actual_class,
            &method_name,
            method_bytecode.access_flags,
        )?;

        // Create frame for the called method
        // Local 0 = receiver (this), locals 1..n = arguments
        let mut locals = vec![Value::Reference(receiver)];
        for arg in args {
            locals.push(arg);
        }

        let mut new_frame = Frame::with_bytecode(
            actual_class, // Use actual class where method was found
            method_name,
            method_bytecode.max_locals,
            method_bytecode.max_stack,
            locals,
            method_bytecode.code.clone(),
            descriptor,
            method_bytecode.exception_table.clone(),
        )?;

        apply_synchronized_if_needed(
            method_bytecode.access_flags,
            &mut new_frame,
            thread,
            monitors,
        )?;

        // Save caller's pc before pushing new frame
        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        thread.push_frame(new_frame)?;
        Ok(())
    }

    /// Execute invokeinterface: Call interface method with itable dispatch.
    /// Stack: ..., objectref, [args] -> ..., [result]
    fn resolve_invokeinterface_symbolic(
        &self,
        interface_name: &str,
        method_name: &str,
        descriptor: &str,
    ) -> VMResult<(String, u16, bool)> {
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;
        let interface = method_area
            .get_class(interface_name)
            .map_err(|error| invocation_resolution_error(error, interface_name))?;
        let interface_is_native_stub = interface.is_native_stub;
        if !interface.is_interface {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "invokeinterface symbolic owner {interface_name} is not an interface"
            )));
        }
        if let Some(method) = interface.declared_non_virtual_method(method_name, descriptor) {
            if method.access_flags & crate::interpreter::executor::access_flags::ACC_STATIC != 0 {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "invokeinterface resolved static method {interface_name}.{method_name}{descriptor}"
                )));
            }
            return Ok((
                interface.name.clone(),
                method.access_flags,
                method.access_flags & crate::interpreter::executor::access_flags::ACC_PRIVATE != 0,
            ));
        }
        match interface.resolve_virtual_declaration(method_name, descriptor) {
            Ok(declaration) => Ok((
                declaration.declaring_class.to_string(),
                declaration.access_flags,
                false,
            )),
            Err(interface_error) => {
                if let Ok(object) = method_area.get_class("java/lang/Object") {
                    if let Ok(declaration) =
                        object.resolve_virtual_declaration(method_name, descriptor)
                    {
                        if declaration.access_flags
                            & crate::interpreter::executor::access_flags::ACC_PUBLIC
                            != 0
                        {
                            return Ok((
                                declaration.declaring_class.to_string(),
                                declaration.access_flags,
                                false,
                            ));
                        }
                    }
                }
                if interface_is_native_stub
                    && native_stub_has_compatible_method(
                        &method_area,
                        interface_name,
                        method_name,
                        descriptor,
                    )
                {
                    Ok((
                        interface.name,
                        crate::interpreter::executor::access_flags::ACC_PUBLIC,
                        false,
                    ))
                } else {
                    Err(interface_error)
                }
            }
        }
    }

    pub(crate) fn execute_invokeinterface(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut MonitorTable,
    ) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 3 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Read count byte (number of arguments + 1 for receiver)
        let count = self.bytecode[self.pc];
        self.pc += 1;

        // Read reserved byte (must be 0 per JVM spec)
        let reserved = self.bytecode[self.pc];
        self.pc += 1;
        if reserved != 0 {
            return Err(VMError::ClassFormatError(format!(
                "invokeinterface reserved byte must be 0, got {}",
                reserved
            )));
        }

        // Resolve method reference from constant pool
        let (mut interface_name, method_name, descriptor) =
            self.resolve_interface_method_ref(cp_index)?;
        reject_illegal_initializer(&interface_name, &method_name, &descriptor, false)?;
        if interface_name == "Runnable" {
            interface_name = "java/lang/Runnable".to_string();
        }
        let desc = MethodDescriptor::parse(&descriptor)
            .map_err(|_| VMError::internal("Invalid method descriptor"))?;
        let symbolic_caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        self.check_symbolic_class_access(&symbolic_caller_class, &interface_name)
            .map_err(|error| invocation_resolution_error(error, &interface_name))?;
        // `List.toString()` is not declared in `List`, so the resolution below would fail.
        // A built-in container answers it here. The receiver is the only operand.
        if method_name == "toString" && descriptor == "()Ljava/lang/String;" {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            let container = match frame.peek()? {
                Value::Reference(ObjectRef::Object(id)) if heap.is_container(*id) => Some(*id),
                _ => None,
            };
            if let Some(container) = container {
                frame.pop()?;
                let text = self.collection_text_sync(thread, heap, monitors, container)?;
                let rendered = heap.intern_string(&text)?;
                thread
                    .current_frame_mut()
                    .ok_or(VMError::NoActiveFrame)?
                    .push_raw(Value::Reference(rendered));
                return Ok(());
            }
        }
        let (resolved_interface_class, resolved_interface_access, exact_private) =
            self.resolve_invokeinterface_symbolic(&interface_name, &method_name, &descriptor)?;
        self.check_method_access(
            &symbolic_caller_class,
            &resolved_interface_class,
            &method_name,
            resolved_interface_access,
        )?;

        // Validate count byte matches descriptor (JVM spec 4.10.1.9.invokeinterface)
        let expected_count = desc.parameters.iter().try_fold(1_u8, |slots, parameter| {
            let width = if matches!(
                parameter,
                crate::descriptor::DescriptorType::Long | crate::descriptor::DescriptorType::Double
            ) {
                2
            } else {
                1
            };
            slots.checked_add(width).ok_or_else(|| {
                VMError::ClassFormatError(
                    "invokeinterface argument slot count overflow".to_string(),
                )
            })
        })?;
        if count != expected_count {
            return Err(VMError::ClassFormatError(format!(
                "invokeinterface count byte mismatch: got {}, expected {}",
                count, expected_count
            )));
        }

        // Pop arguments first (to avoid double-borrow issue)
        let args = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            let mut args = Vec::with_capacity(desc.parameters.len());
            for _ in 0..desc.parameters.len() {
                args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            args.reverse();
            args
        };

        // Pop receiver
        let receiver = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            frame.pop()?.as_reference()?
        };

        // Handle null reference
        if receiver.is_null() {
            return Err(VMError::NullPointerException);
        }

        // Get receiver's runtime class for itable dispatch
        let heap_id = receiver.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        let args =
            self.canonicalize_container_args(thread, heap, monitors, heap_id, &method_name, args)?;
        let receiver_class = heap.get_class(heap_id)?;
        let lambda_receiver = receiver_class.name.contains("$$Lambda");
        if !exact_private && !lambda_receiver {
            let receiver_linkage = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?
                .get_class(&receiver_class.name)?;
            if receiver_linkage.has_default_method_conflict(&method_name, &descriptor) {
                return Err(VMError::IncompatibleClassChangeError(format!(
                    "{} inherits conflicting defaults for {}{}",
                    receiver_class.name, method_name, descriptor
                )));
            }
        }
        let native_stub_receiver = self
            .method_area
            .as_ref()
            .and_then(|area| area.read().ok())
            .and_then(|area| area.get_class(&receiver_class.name).ok())
            .is_some_and(|class| class.is_native_stub);
        let receiver_is_symbolic_interface =
            receiver_class.name.replace('/', ".") == interface_name.replace('/', ".");
        if !lambda_receiver
            && !native_stub_receiver
            && !receiver_is_symbolic_interface
            && !self.class_implements_interface(&receiver_class.name, &interface_name)
        {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "{} does not implement {}",
                receiver_class.name, interface_name
            )));
        }

        // Select the runtime target before native dispatch. Strict linking
        // guarantees a complete itable, so missing routes are linkage errors,
        // not an invitation to execute an arbitrary same-signature class method.
        let selected_method = if exact_private || lambda_receiver || native_stub_receiver {
            None
        } else {
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;
            let loaded_class = method_area.get_class(&receiver_class.name)?;
            let entry = if receiver_is_symbolic_interface {
                // Native-backed interface objects use the interface name as
                // their runtime heap class. Resolve their own declaration;
                // ordinary Java objects must use the strict itable path.
                let origin = loaded_class.resolve_virtual_declaration(&method_name, &descriptor)?;
                loaded_class.dispatch_target(&method_name, &descriptor, &origin)?
            } else {
                let canonical_interface_name = interface_name.replace('/', ".");
                let linked_entry = loaded_class
                    .itable
                    .iter()
                    .find(|entry| {
                        entry.interface_name.replace('/', ".") == canonical_interface_name
                            && entry.method_name.as_ref() == method_name
                            && entry.descriptor.as_ref() == descriptor
                    })
                    .and_then(|entry| loaded_class.vtable.get(entry.vtable_index));
                if let Some(entry) = linked_entry {
                    entry
                } else if loaded_class.is_native_stub {
                    // Built-in native collection objects predate strict batch
                    // linking and have no materialized itable. Their registry
                    // provenance is explicit, so resolve only an exact method
                    // declared by that trusted stub; never apply this to user
                    // classes or an arbitrary same-signature method.
                    let origin =
                        loaded_class.resolve_virtual_declaration(&method_name, &descriptor)?;
                    loaded_class.dispatch_target(&method_name, &descriptor, &origin)?
                } else {
                    return Err(VMError::IncompatibleClassChangeError(format!(
                        "missing linked interface route {}.{}{} on {}",
                        interface_name, method_name, descriptor, receiver_class.name
                    )));
                }
            };
            let selected_access = entry.access_flags();
            if selected_access
                & (crate::interpreter::executor::access_flags::ACC_PUBLIC
                    | crate::interpreter::executor::access_flags::ACC_PRIVATE)
                == 0
            {
                return Err(VMError::IllegalAccessError(format!(
                    "invokeinterface selected non-public method {}.{}{}",
                    entry.declaring_class(),
                    method_name,
                    descriptor
                )));
            }
            Some((entry.declaring_class().to_string(), entry.is_abstract()))
        };
        let symbolic_native_stub = self
            .method_area
            .as_ref()
            .and_then(|area| area.read().ok())
            .and_then(|area| area.get_class(&interface_name).ok())
            .is_some_and(|class| class.is_native_stub);
        // The plugin executor materializes its Project facade from trusted
        // Gradle API bytecode while the receiver itself is VM-native. Keep
        // that one explicit bridge; arbitrary user interfaces do not bypass
        // abstract selection merely because a native hook shares a name.
        let trusted_gradle_project_facade = interface_name
            .replace('.', "/")
            .starts_with("org/gradle/api/");
        let selected_native_stub = selected_method.as_ref().is_some_and(|(owner, _)| {
            self.method_area
                .as_ref()
                .and_then(|area| area.read().ok())
                .and_then(|area| area.get_class(owner).ok())
                .is_some_and(|class| class.is_native_stub)
        });
        let native_dispatch_allowed =
            native_stub_receiver || selected_native_stub || trusted_gradle_project_facade;
        if !native_dispatch_allowed
            && !symbolic_native_stub
            && matches!(selected_method, Some((_, true)))
        {
            return Err(VMError::AbstractMethodError(format!(
                "{}.{}{}",
                receiver_class.name, method_name, descriptor
            )));
        }

        // List.sort(comparator): sort the receiver list in place via the
        // comparator SAM (or natural order when null).
        if native_dispatch_allowed && interface_name == "java/util/List" && method_name == "sort" {
            let comparator = match args.first() {
                Some(Value::Reference(ObjectRef::Object(id))) => Some(*id),
                _ => None,
            };
            self.sort_list(thread, heap, monitors, heap_id, comparator)?;
            return Ok(());
        }

        if native_dispatch_allowed {
            if let Some(predicate) =
                removes_if_from_heap_collection(heap, heap_id, &method_name, &descriptor, &args)
            {
                let removed = self.remove_if(thread, heap, monitors, heap_id, predicate)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(Value::Int(i32::from(removed)));
                return Ok(());
            }
        }

        // ── java.util.stream ───────────────────────────────────────────────
        // `Collection.stream()` and the Stream/IntStream operations both reach
        // here through interface dispatch (the static types are interfaces).
        if native_dispatch_allowed
            && method_name == "stream"
            && descriptor == "()Ljava/util/stream/Stream;"
        {
            // Only when the receiver is a heap collection — a user-defined
            // `stream()` method still dispatches normally below.
            if let Ok(items) = heap.collection_snapshot(heap_id) {
                let sid = heap.allocate_stream(items)?;
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(Value::Reference(ObjectRef::Object(sid)));
                return Ok(());
            }
        }
        if native_dispatch_allowed
            && (interface_name.starts_with("java/util/stream/")
                || receiver_class.name.starts_with("java/util/stream/"))
        {
            return self.dispatch_stream_method(
                thread,
                heap,
                monitors,
                heap_id,
                &method_name,
                &descriptor,
                args,
            );
        }

        // Calls on a Java lambda: the SAM routes to the lambda body; any other
        // interface method is a default method and runs its own bytecode (with
        // the lambda as `this`).
        if lambda_receiver {
            // As invokevirtual does: a lambda with no recorded SAM name takes any call that is not
            // a default method, and the SAM name, when recorded, selects the SAM.
            let sam = self.lambda_sam_name(heap, heap_id);
            if sam.is_none() || sam.as_deref() == Some(method_name.as_str()) {
                return self.dispatch_lambda_sam(thread, heap, monitors, heap_id, args, &desc);
            }
            let (decl, mb) =
                self.resolve_method_in_hierarchy(&interface_name, &method_name, &descriptor)?;
            let mut locals = vec![Value::Reference(receiver)];
            locals.extend(args);
            let new_frame = Frame::with_bytecode(
                decl,
                method_name.clone(),
                mb.max_locals,
                mb.max_stack,
                locals,
                mb.code.clone(),
                descriptor.clone(),
                mb.exception_table.clone(),
            )?;
            if let Some(caller) = thread.current_frame_mut() {
                caller.pc = self.pc;
            }
            thread.push_frame(new_frame)?;
            return Ok(());
        }

        // Give native hooks the first chance to handle interface calls (e.g. Gradle API stubs).
        let caller_index = thread.frames.len().saturating_sub(1);
        let mut native_args = Vec::with_capacity(args.len() + 1);
        native_args.push(Value::Reference(receiver));
        native_args.extend(args.iter().cloned());
        if native_dispatch_allowed {
            if let Some((true, result)) = self.try_invoke_native(
                heap,
                thread,
                &interface_name,
                &method_name,
                &descriptor,
                native_args,
            )? {
                if let Some(caller_frame) = thread.frames.get_mut(caller_index) {
                    caller_frame.pc = self.pc;
                    // Only push a return value for non-void methods.
                    if !descriptor.ends_with(")V") {
                        if let Some(value) = result {
                            caller_frame.push(value)?;
                        }
                    } else {
                        // void return: ignore any placeholder native value
                    }
                }
                return Ok(());
            }
        }

        // Handle native interface methods for standard Java classes
        let handled_native = native_dispatch_allowed
            && match (
                receiver_class.name.as_str(),
                interface_name.as_str(),
                method_name.as_str(),
            ) {
                (_, "java/util/List", "add") | (_, "java/util/Collection", "add") => {
                    let element = args
                        .first()
                        .cloned()
                        .unwrap_or(Value::Reference(ObjectRef::Null));

                    let added = match receiver_class.name.as_str() {
                        "java/util/ArrayList" => heap.arraylist_add(heap_id, element)?,
                        "java/util/LinkedList" => heap.linkedlist_add(heap_id, element)?,
                        "java/util/HashSet" => heap.hashset_add(heap_id, element)?,
                        _ => true,
                    };

                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame
                        .operand_stack
                        .push(Value::Int(if added { 1 } else { 0 }));
                    true
                }
                (_, "java/util/Map", "put") => {
                    let key = args
                        .first()
                        .cloned()
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let value = args
                        .get(1)
                        .cloned()
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let old = heap
                        .hashmap_put(heap_id, key, value)
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(old);
                    true
                }
                (_, "java/util/Map", "get") => {
                    let key = args
                        .first()
                        .cloned()
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let value = heap
                        .hashmap_get(heap_id, &key)
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(value);
                    true
                }
                _ => false,
            };

        if handled_native {
            return Ok(());
        }

        // Route interface calls through the shared native-virtual dispatch keyed
        // on the receiver's *runtime* class (e.g. `List.iterator()` on an
        // ArrayList, `Map.get` on a HashMap, `Iterator.hasNext/next`). This
        // reuses every collection/iterator native available to invokevirtual.
        if native_dispatch_allowed
            && self.try_native_virtual_dispatch(
                &receiver_class.name,
                &method_name,
                &descriptor,
                receiver,
                heap_id,
                &args,
                thread,
                heap,
            )?
        {
            return Ok(());
        }

        let bytecode_owner = match selected_method {
            Some((_owner, true)) => {
                return Err(VMError::AbstractMethodError(format!(
                    "{}.{}{}",
                    receiver_class.name, method_name, descriptor
                )));
            }
            Some((owner, false)) => owner,
            None => resolved_interface_class,
        };
        let (declaring_class, method_bytecode) = self
            .lookup_method_either_form(&bytecode_owner, &method_name, &descriptor)
            .ok_or_else(|| {
                VMError::NoSuchMethod(format!("{}.{}{}", bytecode_owner, method_name, descriptor))
            })?;

        // Create frame: receiver at local 0, args follow
        let mut locals = vec![Value::Reference(receiver)];
        for arg in args {
            locals.push(arg);
        }

        // Create and push new frame
        let mut new_frame = Frame::with_bytecode(
            declaring_class,
            method_name.clone(),
            method_bytecode.max_locals,
            method_bytecode.max_stack,
            locals,
            method_bytecode.code.clone(),
            descriptor,
            method_bytecode.exception_table.clone(),
        )?;

        apply_synchronized_if_needed(
            method_bytecode.access_flags,
            &mut new_frame,
            thread,
            monitors,
        )?;

        // Save caller's PC before pushing new frame
        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        thread.push_frame(new_frame)?;
        Ok(())
    }
}

/// Encode a BufferedImage to PNG and write it to a File's path, implementing
/// `javax.imageio.ImageIO.write(image, format, file)`. `args` is
/// `[image, formatString, file]`. Returns whether the file was written.
fn write_image_to_file(heap: &Heap, args: &[Value]) -> Option<bool> {
    use crate::interpreter::png_encode::encode_rgb;

    let img_id = match args.first()? {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return None,
    };
    // Only PNG is supported; mirror ImageIO.write returning false ("no writer")
    // for any other format rather than writing PNG bytes under a wrong extension.
    if let Some(Value::Reference(ObjectRef::Object(fmt_id))) = args.get(1) {
        if let Ok(fmt) = heap.get_string(*fmt_id) {
            if !fmt.eq_ignore_ascii_case("png") {
                return Some(false);
            }
        }
    }
    let width = heap.get_field(img_id, 0).ok()??.as_int().ok()?;
    let height = heap.get_field(img_id, 1).ok()??.as_int().ok()?;
    let arr = match heap.get_field(img_id, 2).ok()?? {
        Value::Reference(ObjectRef::Array(id)) => id,
        _ => return None,
    };

    let count = (width as i64 * height as i64).max(0) as usize;
    let mut pixels = Vec::with_capacity(count);
    for i in 0..count as i32 {
        pixels.push(heap.array_load(arr, i).ok()?.as_int().unwrap_or(0));
    }

    // The destination path lives in the File's field 0 (the path String).
    let file_id = match args.get(2)? {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return None,
    };
    let path_id = match heap.get_field(file_id, 0).ok()?? {
        Value::Reference(ObjectRef::Object(id)) => id,
        _ => return None,
    };
    let path = heap.get_string(path_id).ok()?;

    let png = encode_rgb(width.max(0) as u32, height.max(0) as u32, &pixels);
    std::fs::write(&path, png).ok()?;
    Some(true)
}

/// Natural ordering (`Comparable.compareTo`) for the common element types:
/// strings lexicographically, numeric wrappers by value. Returns -1/0/1.
pub(crate) fn natural_compare(heap: &Heap, a: &Value, b: &Value) -> i32 {
    crate::interpreter::heap::natural_cmp(heap, a, b) as i32
}

/// Read a thread object's name (stored as the `$thread_name` dynamic property by
/// the Thread constructor / setName), defaulting to "Thread-0" when unset.
fn thread_name_of(heap: &Heap, thread_obj: crate::interpreter::HeapId) -> String {
    match heap.get_dynamic_property(thread_obj, "$thread_name") {
        Some(Value::Reference(ObjectRef::Object(id))) => heap
            .get_string(id)
            .unwrap_or_else(|_| "Thread-0".to_string()),
        _ => "Thread-0".to_string(),
    }
}

/// Box a primitive `Value` for passing to a lambda's `doCall` (whose params are
/// `Object`, loaded via `aload`). References pass through unchanged.
/// A closure returns its result boxed. A call whose descriptor declares a primitive return takes the
/// primitive, as the Java call would have returned it. Other results are returned unchanged.
fn unbox_for_primitive_return(
    heap: &Heap,
    return_type: &crate::descriptor::DescriptorType,
    value: Value,
) -> Value {
    match value {
        Value::Reference(ObjectRef::Object(id)) => match return_type {
            crate::descriptor::DescriptorType::Int => heap
                .get_boxed_int(id)
                .map_or(Value::Reference(ObjectRef::Object(id)), Value::Int),
            crate::descriptor::DescriptorType::Long => heap
                .get_boxed_long(id)
                .map_or(Value::Reference(ObjectRef::Object(id)), Value::Long),
            _ => Value::Reference(ObjectRef::Object(id)),
        },
        other => other,
    }
}

fn box_primitive_for_lambda(heap: &mut Heap, value: Value) -> VMResult<Value> {
    let id = match value {
        Value::Int(i) => heap.allocate_integer(i)?,
        Value::Long(l) => heap.allocate_long(l)?,
        Value::Float(f) => heap.allocate_float(f)?,
        Value::Double(d) => heap.allocate_double(d)?,
        other => return Ok(other),
    };
    Ok(Value::Reference(ObjectRef::Object(id)))
}

/// Map a wrapper unboxing method (`intValue`, `longValue`, …) applied to an
/// already-raw primitive to the converted primitive — the identity that the
/// boxed path would produce. Returns `None` for non-unboxing methods.
fn unbox_identity(method: &str, v: &Value) -> Option<Value> {
    let as_i32 = || match v {
        Value::Int(value) => Some(*value),
        Value::Long(value) => Some(*value as i32),
        Value::Float(value) => Some(*value as i32),
        Value::Double(value) => Some(*value as i32),
        _ => None,
    };
    let as_i64 = || match v {
        Value::Int(value) => Some(i64::from(*value)),
        Value::Long(value) => Some(*value),
        Value::Float(value) => Some(*value as i64),
        Value::Double(value) => Some(*value as i64),
        _ => None,
    };
    let as_f32 = || match v {
        Value::Int(value) => Some(*value as f32),
        Value::Long(value) => Some(*value as f32),
        Value::Float(value) => Some(*value),
        Value::Double(value) => Some(*value as f32),
        _ => None,
    };
    let as_f64 = || match v {
        Value::Int(value) => Some(f64::from(*value)),
        Value::Long(value) => Some(*value as f64),
        Value::Float(value) => Some(f64::from(*value)),
        Value::Double(value) => Some(*value),
        _ => None,
    };
    match method {
        "intValue" => Some(Value::Int(as_i32()?)),
        "shortValue" => Some(Value::Int(as_i32()? as i16 as i32)),
        "byteValue" => Some(Value::Int(as_i32()? as i8 as i32)),
        "charValue" => Some(Value::Int(as_i32()? as u16 as i32)),
        "longValue" => Some(Value::Long(as_i64()?)),
        "floatValue" => Some(Value::Float(as_f32()?)),
        "doubleValue" => Some(Value::Double(as_f64()?)),
        "booleanValue" => Some(v.clone()),
        _ => None,
    }
}

fn validate_invokestatic_reference_kind(
    reference_is_interface: bool,
    symbolic_is_interface: bool,
    owner: &str,
) -> VMResult<()> {
    if reference_is_interface == symbolic_is_interface {
        Ok(())
    } else {
        Err(VMError::IncompatibleClassChangeError(format!(
            "invokestatic reference kind does not match {owner}"
        )))
    }
}

/// Convert the MethodArea's host-side lookup sentinel into the Java linkage
/// error required while resolving an instruction's symbolic owner.
fn invocation_resolution_error(error: VMError, owner: &str) -> VMError {
    match error {
        VMError::ClassNotFound(missing) => {
            VMError::NoClassDefFoundError(format!("{owner} method resolution requires {missing}"))
        }
        other => other,
    }
}

fn reject_illegal_initializer(
    owner: &str,
    method_name: &str,
    descriptor: &str,
    allow_constructor: bool,
) -> VMResult<()> {
    if method_name == "<clinit>" || (method_name == "<init>" && !allow_constructor) {
        Err(VMError::NoSuchMethod(format!(
            "{owner}.{method_name}{descriptor}"
        )))
    } else {
        Ok(())
    }
}

fn is_trusted_raw_unbox_declaration(
    class: &crate::interpreter::method_area::LoadedClass,
    declared_class: &str,
    method_name: &str,
    descriptor: &str,
) -> bool {
    class.is_native_stub
        && matches!(
            declared_class.replace('.', "/").as_str(),
            "java/lang/Integer"
                | "java/lang/Long"
                | "java/lang/Short"
                | "java/lang/Byte"
                | "java/lang/Double"
                | "java/lang/Float"
                | "java/lang/Boolean"
                | "java/lang/Character"
        )
        && class
            .resolve_virtual_declaration(method_name, descriptor)
            .is_ok_and(|declaration| {
                declaration.access_flags & crate::interpreter::executor::access_flags::ACC_PUBLIC
                    != 0
            })
}

fn native_stub_has_compatible_method(
    method_area: &crate::interpreter::MethodArea,
    owner: &str,
    method_name: &str,
    descriptor: &str,
) -> bool {
    fn backend_specialization_compatible(
        requested: &str,
        declared: &str,
        allow_sam_return_erasure: bool,
    ) -> bool {
        let (Ok(requested), Ok(declared)) = (
            MethodDescriptor::parse(requested),
            MethodDescriptor::parse(declared),
        ) else {
            return false;
        };
        fn same_stack_type(
            left: &crate::descriptor::DescriptorType,
            right: &crate::descriptor::DescriptorType,
        ) -> bool {
            use crate::descriptor::DescriptorType;
            matches!(
                (left, right),
                (
                    DescriptorType::Class { .. } | DescriptorType::Array { .. },
                    DescriptorType::Class { .. } | DescriptorType::Array { .. }
                )
            ) || left == right
        }
        requested.parameters.len() == declared.parameters.len()
            && requested
                .parameters
                .iter()
                .zip(&declared.parameters)
                .all(|(left, right)| same_stack_type(left, right))
            && (allow_sam_return_erasure
                || same_stack_type(&requested.return_type, &declared.return_type))
    }
    let mut pending = vec![owner.to_string()];
    let mut visited = std::collections::HashSet::new();
    while let Some(name) = pending.pop() {
        if !visited.insert(name.replace('/', ".")) {
            continue;
        }
        let Ok(class) = method_area.get_class(&name) else {
            continue;
        };
        if !class.is_native_stub {
            continue;
        }
        if class.vtable.iter().any(|method| {
            method.name.as_ref() == method_name
                && (method.descriptor.as_ref() == descriptor
                    || backend_specialization_compatible(
                        descriptor,
                        method.descriptor.as_ref(),
                        owner.replace('.', "/").starts_with("java/util/function/"),
                    ))
        }) {
            return true;
        }
        pending.extend(class.interfaces);
        pending.extend(class.superclass);
    }
    false
}

/// The predicate of a `removeIf(predicate)` call on a heap list or set, which
/// `remove_if` handles.
fn removes_if_from_heap_collection(
    heap: &Heap,
    receiver: crate::interpreter::HeapId,
    method_name: &str,
    descriptor: &str,
    args: &[Value],
) -> Option<crate::interpreter::HeapId> {
    if method_name != "removeIf"
        || descriptor != "(Ljava/util/function/Predicate;)Z"
        || !(heap.is_list_container(receiver) || heap.is_hashed_container(receiver))
    {
        return None;
    }
    match args.first() {
        Some(Value::Reference(ObjectRef::Object(predicate))) => Some(*predicate),
        _ => None,
    }
}

#[cfg(test)]
mod raw_unbox_tests {
    use super::{
        is_trusted_raw_unbox_declaration, native_stub_has_compatible_method,
        reject_illegal_initializer, unbox_identity, validate_invokestatic_reference_kind,
    };
    use crate::interpreter::method_area::{LoadedClass, MethodArea};
    use crate::interpreter::Value;

    #[test]
    fn raw_unbox_requires_native_wrapper_provenance_and_exact_declaration() {
        let mut wrapper = LoadedClass::new("java/lang/Integer".to_string());
        wrapper.is_native_stub = true;
        wrapper.add_vtable_entry_with_flags(
            "intValue".to_string(),
            "()I".to_string(),
            "java/lang/Integer".to_string(),
            0x0001,
        );
        assert!(is_trusted_raw_unbox_declaration(
            &wrapper,
            "java/lang/Integer",
            "intValue",
            "()I"
        ));

        let mut forged = wrapper.clone();
        forged.is_native_stub = false;
        assert!(!is_trusted_raw_unbox_declaration(
            &forged,
            "java/lang/Integer",
            "intValue",
            "()I"
        ));
        assert!(!is_trusted_raw_unbox_declaration(
            &wrapper,
            "evil/lang/Integer",
            "intValue",
            "()I"
        ));
        assert!(!is_trusted_raw_unbox_declaration(
            &wrapper,
            "java/lang/Integer",
            "intValue",
            "()J"
        ));
    }

    #[test]
    fn native_stub_compatibility_requires_a_declared_method_family() {
        let method_area = MethodArea::new();
        assert!(native_stub_has_compatible_method(
            &method_area,
            "java/util/List",
            "stream",
            "()Ljava/util/stream/Stream;"
        ));
        assert!(!native_stub_has_compatible_method(
            &method_area,
            "java/util/List",
            "forgedNativeName",
            "()V"
        ));
        assert!(!native_stub_has_compatible_method(
            &method_area,
            "java/util/List",
            "get",
            "(I)J"
        ));
    }

    #[test]
    fn boolean_and_character_raw_unbox_are_exact_and_category_safe() {
        for (owner, method, descriptor) in [
            ("java/lang/Boolean", "booleanValue", "()Z"),
            ("java/lang/Character", "charValue", "()C"),
        ] {
            let mut wrapper = LoadedClass::new(owner.to_string());
            wrapper.is_native_stub = true;
            wrapper.add_vtable_entry_with_flags(
                method.to_string(),
                descriptor.to_string(),
                owner.to_string(),
                0x0001,
            );
            assert!(is_trusted_raw_unbox_declaration(
                &wrapper, owner, method, descriptor
            ));
            assert!(!is_trusted_raw_unbox_declaration(
                &wrapper, owner, method, "()J"
            ));
        }
        assert_eq!(
            unbox_identity("booleanValue", &Value::Int(1)),
            Some(Value::Int(1))
        );
        assert_eq!(
            unbox_identity("charValue", &Value::Int(65)),
            Some(Value::Int(65))
        );
        let beyond_f64_integer_precision = 9_007_199_254_740_993_i64;
        assert_eq!(
            unbox_identity("longValue", &Value::Long(beyond_f64_integer_precision)),
            Some(Value::Long(beyond_f64_integer_precision))
        );
    }

    #[test]
    fn invokestatic_reference_kind_must_match_symbolic_owner_kind() {
        assert!(validate_invokestatic_reference_kind(false, false, "app.Class").is_ok());
        assert!(validate_invokestatic_reference_kind(true, true, "api.Service").is_ok());
        assert!(matches!(
            validate_invokestatic_reference_kind(false, true, "api.Service"),
            Err(crate::interpreter::VMError::IncompatibleClassChangeError(_))
        ));
        assert!(matches!(
            validate_invokestatic_reference_kind(true, false, "app.Class"),
            Err(crate::interpreter::VMError::IncompatibleClassChangeError(_))
        ));
    }

    #[test]
    fn only_invokespecial_may_name_an_initializer_and_never_clinit() {
        assert!(reject_illegal_initializer("app.C", "run", "()V", false).is_ok());
        assert!(reject_illegal_initializer("app.C", "<init>", "()V", true).is_ok());
        for (name, allow_constructor) in
            [("<init>", false), ("<clinit>", false), ("<clinit>", true)]
        {
            assert!(matches!(
                reject_illegal_initializer("app.C", name, "()V", allow_constructor),
                Err(crate::interpreter::VMError::NoSuchMethod(_))
            ));
        }
    }
}
