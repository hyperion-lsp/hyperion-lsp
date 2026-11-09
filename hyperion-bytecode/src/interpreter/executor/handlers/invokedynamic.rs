// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Invokedynamic handler for the Executor
//!
//! Handles the invokedynamic opcode including bootstrap method resolution,
//! call site caching, Groovy MOP dispatch, and closure delegation.

use crate::descriptor::{DescriptorType, MethodDescriptor};
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::{
    is_gradle_script_method, is_gradle_void_dsl_method, Executor, MethodBytecode,
};
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::Heap;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};
use crate::interpreter::HeapId;
use tracing::debug;

impl Executor {
    /// Create the lambda object for a `LambdaMetafactory` call site: capture the
    /// indy arguments (the lambda's captured variables) into object fields and
    /// remember the implementation method (class/name/descriptor/kind) as dynamic
    /// properties. dispatch_lambda_sam later runs it on the SAM call.
    pub(crate) fn create_lambda_object(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        call_site: &crate::interpreter::CallSite,
        indy_descriptor: &str,
    ) -> VMResult<()> {
        use crate::interpreter::MethodHandle;
        let desc = MethodDescriptor::parse(indy_descriptor)
            .map_err(|_| VMError::internal("bad lambda indy descriptor"))?;
        let capture_count = desc.parameters.len();
        let mut captures = Vec::with_capacity(capture_count);
        {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            for _ in 0..capture_count {
                captures.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            captures.reverse();
        }
        // (impl class, method, descriptor, kind) — kind: 0 static, 1 virtual/
        // special, 2 interface, 4 constructor.
        let (impl_class, impl_method, impl_desc, kind) = match &call_site.target {
            MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            } => (
                class_name.clone(),
                method_name.clone(),
                descriptor.clone(),
                0,
            ),
            MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            } => (
                class_name.clone(),
                method_name.clone(),
                descriptor.clone(),
                1,
            ),
            MethodHandle::InvokeInterface {
                class_name,
                method_name,
                descriptor,
            } => (
                class_name.clone(),
                method_name.clone(),
                descriptor.clone(),
                2,
            ),
            MethodHandle::NewInvokeSpecial {
                class_name,
                descriptor,
            } => (
                class_name.clone(),
                "<init>".to_string(),
                descriptor.clone(),
                4,
            ),
            _ => return Err(VMError::internal("unsupported lambda impl handle")),
        };
        let lambda_class = format!("{}$$Lambda", impl_class);
        let id = heap.allocate_object(crate::interpreter::heap::ClassRef {
            name: lambda_class,
            instance_field_count: capture_count,
            static_field_count: 0,
        })?;
        for (i, c) in captures.into_iter().enumerate() {
            heap.set_field(id, i, c)?;
        }
        let cref = heap.intern_string(&impl_class)?;
        let mref = heap.intern_string(&impl_method)?;
        let dref = heap.intern_string(&impl_desc)?;
        heap.set_dynamic_property(id, "$lambda_class", Value::Reference(cref));
        heap.set_dynamic_property(id, "$lambda_method", Value::Reference(mref));
        heap.set_dynamic_property(id, "$lambda_desc", Value::Reference(dref));
        heap.set_dynamic_property(id, "$lambda_kind", Value::Int(kind));
        heap.set_dynamic_property(id, "$lambda_ncap", Value::Int(capture_count as i32));
        // The SAM method name (the abstract interface method the lambda
        // implements); only this name routes to the lambda body — other
        // interface methods (defaults) run their own bytecode.
        let sam = heap.intern_string(&call_site.name)?;
        heap.set_dynamic_property(id, "$lambda_sam", Value::Reference(sam));
        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
        frame.push_raw(Value::Reference(ObjectRef::Object(id)));
        Ok(())
    }
}

impl Executor {
    /// Execute a record's generated `toString`/`equals`/`hashCode` (the
    /// `ObjectMethods` bootstrap). Record components are stored in the object's
    /// fields in declaration order, matching `components`.
    pub(crate) fn invoke_record_method(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        kind: &str,
        class_simple: &str,
        components: &[String],
        descriptor: &str,
    ) -> VMResult<()> {
        let desc = MethodDescriptor::parse(descriptor)
            .map_err(|_| VMError::internal("bad ObjectMethods descriptor"))?;
        let argc = desc.parameters.len();
        let mut args = Vec::with_capacity(argc);
        {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            for _ in 0..argc {
                args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            args.reverse();
        }
        let this_id = match args.first() {
            Some(Value::Reference(ObjectRef::Object(id))) => *id,
            _ => return Err(VMError::NullPointerException),
        };
        let result = match kind {
            "toString" => {
                let mut parts = Vec::with_capacity(components.len());
                for (i, name) in components.iter().enumerate() {
                    let v = heap
                        .get_field(this_id, i)?
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    parts.push(format!("{}={}", name, heap.render_value(&v)));
                }
                let s = format!("{}[{}]", class_simple, parts.join(", "));
                Value::Reference(heap.intern_string(&s)?)
            }
            "equals" => {
                let eq = match args.get(1) {
                    Some(Value::Reference(ObjectRef::Object(other)))
                        if heap.get_object_class_name(*other).ok()
                            == heap.get_object_class_name(this_id).ok() =>
                    {
                        (0..components.len()).all(|i| {
                            let a = heap.get_field(this_id, i).ok().flatten();
                            let b = heap.get_field(*other, i).ok().flatten();
                            match (a, b) {
                                (Some(x), Some(y)) => {
                                    heap.render_value(&x) == heap.render_value(&y)
                                }
                                (None, None) => true,
                                _ => false,
                            }
                        })
                    }
                    _ => false,
                };
                Value::Int(eq as i32)
            }
            _ => {
                // hashCode: combine component values (consistent with equals).
                let mut h = 0i32;
                for i in 0..components.len() {
                    let v = heap
                        .get_field(this_id, i)?
                        .unwrap_or(Value::Reference(ObjectRef::Null));
                    let part = match v {
                        Value::Int(n) => n,
                        Value::Long(n) => (n ^ (n >> 32)) as i32,
                        other => heap
                            .render_value(&other)
                            .chars()
                            .fold(0i32, |a, c| a.wrapping_mul(31).wrapping_add(c as i32)),
                    };
                    h = h.wrapping_mul(31).wrapping_add(part);
                }
                Value::Int(h)
            }
        };
        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
        frame.push_raw(result);
        Ok(())
    }
}

/// Build the result of a `StringConcatFactory` call. `recipe` is the Java 9+
/// concat recipe (`` = next dynamic arg, `` = next static constant,
/// everything else literal); an empty recipe means `makeConcat` (concatenate all
/// dynamic args in order). Returns the interned result String.
fn build_string_concat(
    heap: &mut Heap,
    recipe: &str,
    constants: &[String],
    args: &[Value],
    param_types: &[DescriptorType],
) -> VMResult<ObjectRef> {
    // Render an argument honoring its static type: a `boolean` argument is an
    // int 0/1 on the stack but must print true/false; a `char` prints the code
    // point. Everything else uses the value renderer.
    let render = |heap: &Heap, i: usize, v: &Value| -> String {
        match (param_types.get(i), v) {
            (Some(DescriptorType::Boolean), Value::Int(n)) => {
                if *n != 0 {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            (Some(DescriptorType::Char), Value::Int(n)) => char::from_u32(*n as u32)
                .map(String::from)
                .unwrap_or_default(),
            _ => heap.render_value(v),
        }
    };
    let mut out = String::new();
    if recipe.is_empty() {
        for (i, a) in args.iter().enumerate() {
            out.push_str(&render(heap, i, a));
        }
    } else {
        let mut arg_i = 0;
        let mut const_i = 0;
        for ch in recipe.chars() {
            match ch {
                '\u{0001}' => {
                    if let Some(a) = args.get(arg_i) {
                        out.push_str(&render(heap, arg_i, a));
                    }
                    arg_i += 1;
                }
                '\u{0002}' => {
                    if let Some(c) = constants.get(const_i) {
                        out.push_str(c);
                    }
                    const_i += 1;
                }
                other => out.push(other),
            }
        }
    }
    heap.new_string(&out)
}

/// Push a default return value for the given descriptor type onto the frame's operand stack.
/// Does nothing for void return types.
fn push_default_return_value(frame: &mut Frame, desc: &MethodDescriptor) {
    match desc.return_type {
        DescriptorType::Void => {}
        DescriptorType::Int
        | DescriptorType::Boolean
        | DescriptorType::Byte
        | DescriptorType::Char
        | DescriptorType::Short => {
            frame.push_raw(Value::Int(0));
        }
        DescriptorType::Long => {
            frame.push_raw(Value::Long(0));
        }
        DescriptorType::Float => {
            frame.push_raw(Value::Float(0.0));
        }
        DescriptorType::Double => {
            frame.push_raw(Value::Double(0.0));
        }
        DescriptorType::Class { .. } | DescriptorType::Array { .. } => {
            frame.push_raw(Value::Reference(ObjectRef::Null));
        }
    }
}

/// Extract (class_name, method_name, descriptor) from a resolved MOP MethodHandle.
fn extract_method_info(
    handle: &crate::interpreter::MethodHandle,
) -> VMResult<(String, String, String)> {
    match handle {
        crate::interpreter::MethodHandle::InvokeVirtual {
            class_name,
            method_name,
            descriptor,
        }
        | crate::interpreter::MethodHandle::InvokeStatic {
            class_name,
            method_name,
            descriptor,
        } => Ok((class_name.clone(), method_name.clone(), descriptor.clone())),
        _ => Err(VMError::internal("Unsupported MOP method handle type")),
    }
}

impl Executor {
    /// Execute invokedynamic: Resolve and invoke a dynamically-linked call site.
    ///
    /// This handles bootstrap method resolution, call site caching, Groovy MOP
    /// dispatch, closure delegation, and both InvokeVirtual and InvokeStatic
    /// method handle targets.
    pub(crate) fn execute_invokedynamic(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut crate::interpreter::MonitorTable,
    ) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 4 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        // Read reserved bytes (must be 0 per JVM spec)
        let reserved1 = self.bytecode[self.pc];
        let reserved2 = self.bytecode[self.pc + 1];
        if reserved1 != 0 || reserved2 != 0 {
            return Err(VMError::ClassFormatError(format!(
                "invokedynamic reserved bytes must be 0, got {} {}",
                reserved1, reserved2
            )));
        }
        self.pc += 2;

        // Phase 3.4: Resolve invokedynamic call site name and descriptor
        let (bootstrap_method_attr_index, call_site_name, method_type_descriptor) =
            self.resolve_invokedynamic(cp_index)?;

        debug!(
            "INVOKEDYNAMIC: cp_index={}, call_site_name={}, descriptor={}",
            cp_index, call_site_name, method_type_descriptor
        );

        // Get current class from frame for cache key (prevents cross-class pollution)
        let current_class = {
            let frame = thread.current_frame().ok_or(VMError::NoActiveFrame)?;
            frame.class_name.clone()
        };

        // Check if we have a cached call site (keyed by class + CP index)
        let cache_key = (current_class.clone(), cp_index as u16);
        debug!(
            "INVOKEDYNAMIC: cache_key={:?}, method_area.is_some()={}",
            cache_key,
            self.method_area.is_some()
        );
        let call_site = if let Some(cached) = self.call_site_cache.get(&cache_key) {
            cached.clone()
        } else if self.method_area.is_none() {
            tracing::debug!("INVOKEDYNAMIC: method_area is None, using fallback stub");
            // No method area available - simulate Groovy MOP behavior
            let desc = MethodDescriptor::parse(&method_type_descriptor)
                .map_err(|_| VMError::internal("Invalid method descriptor"))?;

            // Pop arguments from stack
            {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                for _ in 0..desc.parameters.len() {
                    frame.pop_raw();
                }
            }

            // Push a dummy result
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            push_default_return_value(frame, &desc);
            return Ok(());
        } else {
            // Phase 3.4: Execute bootstrap method to get CallSite
            let method_area = self
                .method_area
                .as_ref()
                .ok_or_else(|| VMError::internal("MethodArea not available"))?
                .read()
                .map_err(|_| VMError::internal("Failed to acquire lock"))?;

            let loaded_class = method_area.get_class(&current_class)?;
            let bootstrap_entry = loaded_class
                .bootstrap_methods
                .get(bootstrap_method_attr_index as usize)
                .ok_or_else(|| {
                    VMError::internal(format!(
                        "Bootstrap method index {} out of bounds",
                        bootstrap_method_attr_index
                    ))
                })?
                .clone();

            drop(method_area);

            // Mark LambdaMetafactory call sites so the indy creates a lambda
            // object (capturing its args) instead of invoking the impl method.
            if bootstrap_entry
                .method
                .class_name
                .contains("LambdaMetafactory")
            {
                self.lambda_sites.insert(cache_key.clone());
            }

            // Capture a StringConcatFactory recipe (Java 9+ `+` desugaring) so the
            // cached InvokeStatic call site can be executed as a concatenation.
            if bootstrap_entry
                .method
                .class_name
                .contains("StringConcatFactory")
            {
                use crate::interpreter::bootstrap::BootstrapArg;
                let mut args_iter = bootstrap_entry.arguments.iter();
                let recipe = match args_iter.next() {
                    Some(BootstrapArg::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let constants: Vec<String> = args_iter
                    .map(|a| match a {
                        BootstrapArg::String(s) => s.clone(),
                        BootstrapArg::Int(i) => i.to_string(),
                        BootstrapArg::Long(l) => l.to_string(),
                        BootstrapArg::Float(f) => f.to_string(),
                        BootstrapArg::Double(d) => d.to_string(),
                        _ => String::new(),
                    })
                    .collect();
                self.concat_recipes
                    .insert(cache_key.clone(), (recipe, constants));
            }

            // Records: ObjectMethods.bootstrap synthesizes toString/equals/
            // hashCode. Capture (kind, simple class name, component names) and
            // short-circuit — invoke_bootstrap doesn't model it.
            if bootstrap_entry.method.class_name.contains("ObjectMethods") {
                use crate::interpreter::bootstrap::BootstrapArg;
                let class_simple = match bootstrap_entry.arguments.first() {
                    Some(BootstrapArg::Class(c)) => {
                        c.rsplit(['/', '$']).next().unwrap_or(c).to_string()
                    }
                    _ => String::new(),
                };
                let components: Vec<String> = match bootstrap_entry.arguments.get(1) {
                    Some(BootstrapArg::String(s)) if !s.is_empty() => {
                        s.split(';').map(|x| x.to_string()).collect()
                    }
                    _ => Vec::new(),
                };
                self.record_methods.insert(
                    cache_key.clone(),
                    (call_site_name.clone(), class_simple, components),
                );
                // A placeholder call site; dispatch handles it via record_methods.
                crate::interpreter::CallSite {
                    target: crate::interpreter::MethodHandle::invoke_static(
                        "java/lang/runtime/ObjectMethods",
                        &call_site_name,
                        &method_type_descriptor,
                    ),
                    name: call_site_name.clone(),
                    method_type: method_type_descriptor.clone(),
                    kind: crate::interpreter::call_site::CallSiteKind::Constant,
                }
            } else {
                let call_site_method_type =
                    crate::interpreter::method_type::MethodTypeRef::from_descriptor(
                        &method_type_descriptor,
                    )
                    .unwrap_or_else(|_| {
                        crate::interpreter::method_type::MethodTypeRef {
                            return_type: crate::interpreter::method_type::TypeDescriptor::Reference(
                                "java/lang/Object".to_string(),
                            ),
                            parameter_types: vec![],
                        }
                    });

                let call_site = self.bootstrap_context.invoke_bootstrap(
                    &bootstrap_entry,
                    &call_site_name,
                    &call_site_method_type,
                )?;

                call_site.validate()?;

                if call_site.is_cacheable() {
                    self.call_site_cache
                        .insert(cache_key.clone(), call_site.clone());
                }
                call_site
            }
        };

        // LambdaMetafactory call site: create a lambda *object* that captures the
        // indy arguments and remembers its implementation method, instead of
        // invoking anything. It is later run by dispatch_lambda_sam on the SAM call.
        if self.lambda_sites.contains(&cache_key) {
            return self.create_lambda_object(thread, heap, &call_site, &method_type_descriptor);
        }

        // Record ObjectMethods call site (toString/equals/hashCode).
        if let Some((kind, class_simple, components)) = self.record_methods.get(&cache_key).cloned()
        {
            return self.invoke_record_method(
                thread,
                heap,
                &kind,
                &class_simple,
                &components,
                &method_type_descriptor,
            );
        }

        // Dispatch based on method handle type
        match &call_site.target {
            crate::interpreter::MethodHandle::InvokeVirtual {
                class_name: _,
                method_name,
                descriptor,
            } => {
                self.execute_invokedynamic_virtual(
                    thread,
                    heap,
                    monitors,
                    &call_site,
                    method_name,
                    descriptor,
                )?;
            }
            crate::interpreter::MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            } => {
                let desc = MethodDescriptor::parse(descriptor)
                    .map_err(|_| VMError::internal("Invalid method descriptor"))?;
                let arg_count = desc.parameters.len();

                let args = {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
                    }
                    args.reverse();
                    args
                };

                // Java 9+ string concatenation (`a + b`) desugars to an
                // invokedynamic bootstrapped by StringConcatFactory. Execute it
                // directly using the recipe captured at bootstrap resolution.
                if class_name.contains("StringConcatFactory") {
                    let (recipe, constants) = self
                        .concat_recipes
                        .get(&cache_key)
                        .cloned()
                        .unwrap_or_default();
                    let result =
                        build_string_concat(heap, &recipe, &constants, &args, &desc.parameters)?;
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(Value::Reference(result));
                    return Ok(());
                }

                let method_key = (class_name.clone(), method_name.clone(), descriptor.clone());
                let method_bytecode =
                    self.method_registry
                        .get(&method_key)
                        .cloned()
                        .ok_or_else(|| {
                            VMError::NoSuchMethod(format!(
                                "{}.{}:{}",
                                class_name, method_name, descriptor
                            ))
                        })?;

                let method_frame = Frame::with_bytecode(
                    class_name.clone(),
                    method_name.clone(),
                    method_bytecode.max_locals,
                    method_bytecode.max_stack,
                    args,
                    method_bytecode.code,
                    descriptor.clone(),
                    method_bytecode.exception_table,
                )?;

                if let Some(caller_frame) = thread.current_frame_mut() {
                    caller_frame.pc = self.pc;
                }
                thread.push_frame(method_frame)?;
            }
            _ => {
                return Err(VMError::UnimplementedOpcode(
                    "invokedynamic only supports InvokeVirtual and InvokeStatic in Phase 5.5 MVP"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Handle the InvokeVirtual branch of invokedynamic dispatch.
    ///
    /// Orchestrator that delegates to focused helpers:
    /// 1. Pop args and receiver from the operand stack
    /// 2. Try Gradle DSL dispatch
    /// 3. Try native virtual dispatch (collections, String, File, etc.)
    /// 4. Resolve target via vtable → MOP → registry
    /// 5. Push a new frame for the resolved method
    fn execute_invokedynamic_virtual(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        monitors: &mut crate::interpreter::MonitorTable,
        call_site: &crate::interpreter::CallSite,
        method_name: &str,
        descriptor: &str,
    ) -> VMResult<()> {
        // Step 1: Pop args and receiver
        let desc = MethodDescriptor::parse(descriptor)
            .map_err(|_| VMError::internal("Invalid method descriptor"))?;
        let arg_count = desc.parameters.len();
        let (mut args, mut receiver) = match self
            .pop_invokedynamic_args(thread, call_site, arg_count)
        {
            Ok(result) => result,
            Err(VMError::NullPointerException) if self.is_recording_proxy() => {
                // In recording proxy mode, substitute a proxy stub for null receivers
                // so method chains on null results don't crash the execution.
                let proxy_id = heap.allocate_object(crate::interpreter::heap::ClassRef {
                    name: "gradle/Proxy/NullReceiver".to_string(),
                    instance_field_count: 0,
                    static_field_count: 0,
                })?;
                debug!("Recording proxy: substituted NullReceiver proxy for null receiver on method {}", method_name);
                (Vec::new(), ObjectRef::Object(proxy_id))
            }
            Err(e) => return Err(e),
        };

        // Gradle closure fixup: if calling a known DSL method with no args on a
        // closure, inject the closure itself as the arg. This handles the Groovy
        // pattern where `dependencies { ... }` compiles to a method call with the
        // closure as receiver rather than argument.
        //
        // NOTE: Do NOT extend this to all methods in recording proxy mode — that
        // causes infinite recursion (closure injects itself as arg → bridge
        // executes it → closure calls the same method → loop).
        let is_script_method = is_gradle_script_method(method_name);
        if is_script_method && args.is_empty() {
            if let ObjectRef::Object(id) = receiver {
                if let Ok(class_name) = heap.get_object_class_name(id) {
                    if class_name.contains("$_closure_")
                        || class_name.contains("$_nested_")
                        || class_name == "groovy/lang/Closure"
                    {
                        args.push(Value::Reference(ObjectRef::Object(id)));
                        receiver = ObjectRef::Object(id);
                    }
                }
            }
        }

        let heap_id = receiver.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        let receiver_class = heap.get_class(heap_id)?;

        // Step 2: Try Gradle DSL dispatch
        if let Some(()) = self.try_invokedynamic_gradle_dispatch(
            thread,
            heap,
            method_name,
            descriptor,
            &receiver_class.name,
            receiver,
            heap_id,
            &args,
        )? {
            return Ok(());
        }

        // Step 3: Try native virtual dispatch (collections, String, File, etc.)
        {
            use super::native_virtual::{dispatch_native_virtual, NativeDispatchResult};

            // builtBy is special — routes through gradle bridge
            let native_result =
                if receiver_class.name == "java/lang/String" && method_name == "builtBy" {
                    let result = if let Some(gradle_bridge) = self.gradle_native_bridge.as_ref() {
                        gradle_bridge.invoke_gradle_method(
                            heap,
                            &self.method_registry,
                            thread,
                            "gradle/Project",
                            "files",
                            descriptor,
                            &[Value::Reference(receiver)],
                        )?
                    } else {
                        None
                    };
                    Some(result.unwrap_or(Value::Reference(ObjectRef::Null)))
                } else {
                    match dispatch_native_virtual(
                        &receiver_class.name,
                        method_name,
                        descriptor,
                        receiver,
                        heap_id,
                        &args,
                        heap,
                    )? {
                        NativeDispatchResult::Handled(value) => Some(value),
                        NativeDispatchResult::HandledVoid => {
                            debug!("INVOKEDYNAMIC: handled as native method");
                            return Ok(());
                        }
                        NativeDispatchResult::NotHandled => None,
                    }
                };

            if let Some(value) = native_result {
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(value);
                debug!("INVOKEDYNAMIC: handled as native method");
                return Ok(());
            }
        }

        // Step 3.5: Try Default Groovy Methods (each/collect/find/sum/…) on
        // collection receivers. These resolve through the MOP to bodiless
        // DefaultGroovyMethods statics, so we implement the common ones natively.
        if let Some(()) = self.try_dgm_dispatch(
            thread,
            heap,
            monitors,
            &receiver_class.name,
            method_name,
            receiver,
            heap_id,
            &args,
        )? {
            return Ok(());
        }

        // Step 4: Resolve target via vtable → MOP → registry
        let (declaring_class, resolved_method_name, mut resolved_descriptor, actual_receiver) =
            self.resolve_invokedynamic_target(
                thread,
                heap,
                method_name,
                descriptor,
                &receiver_class.name,
                receiver,
                heap_id,
            )?;

        // Post-resolution: if the resolved method lives in a gradle/ class, dispatch via bridge
        if declaring_class.starts_with("gradle/") && self.gradle_native_bridge.is_some() {
            return self.dispatch_gradle_resolved(
                thread,
                heap,
                &declaring_class,
                &resolved_method_name,
                &resolved_descriptor,
                &args,
            );
        }

        // Post-resolution: allow native dispatch for methods defined on a superclass/stub.
        //
        // Example: Groovy scripts resolve `getProperty`/`setProperty` to groovy/lang/Script,
        // but the receiver class is the generated script class (e.g. Test). Native dispatch
        // must key off the declaring class for these stubbed methods.
        if let Some(actual_heap_id) = actual_receiver.heap_id() {
            use super::native_virtual::{dispatch_native_virtual, NativeDispatchResult};
            match dispatch_native_virtual(
                &declaring_class,
                &resolved_method_name,
                &resolved_descriptor,
                actual_receiver,
                actual_heap_id,
                &args,
                heap,
            )? {
                NativeDispatchResult::Handled(value) => {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.push_raw(value);
                    debug!("INVOKEDYNAMIC: handled as native method (declaring_class)");
                    return Ok(());
                }
                NativeDispatchResult::HandledVoid => {
                    debug!("INVOKEDYNAMIC: handled as native method (declaring_class)");
                    return Ok(());
                }
                NativeDispatchResult::NotHandled => {}
            }
        }

        // Step 5: Look up bytecode and push frame
        self.push_invokedynamic_frame(
            thread,
            &declaring_class,
            &resolved_method_name,
            &mut resolved_descriptor,
            actual_receiver,
            args,
        )
    }

    /// Pop arguments and receiver from the operand stack for invokedynamic virtual dispatch.
    fn pop_invokedynamic_args(
        &self,
        thread: &mut Thread,
        call_site: &crate::interpreter::CallSite,
        arg_count: usize,
    ) -> VMResult<(Vec<Value>, ObjectRef)> {
        let (stack_len, mut args) = {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            let stack_len = frame.operand_stack.len();
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(frame.pop_raw().ok_or(VMError::StackUnderflow)?);
            }
            args.reverse();
            (stack_len, args)
        };

        let receiver = match &call_site.target {
            crate::interpreter::MethodHandle::InvokeVirtual { .. } => {
                let receiver = if stack_len > arg_count {
                    let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                    frame.pop()?.as_reference()?
                } else {
                    let value = args.first().cloned().ok_or(VMError::StackUnderflow)?;
                    args.remove(0);
                    value.as_reference()?
                };
                if receiver.is_null() {
                    return Err(VMError::NullPointerException);
                }
                receiver
            }
            _ => ObjectRef::Null,
        };

        Ok((args, receiver))
    }

    /// Try Gradle DSL dispatch for invokedynamic. Returns `Ok(Some(()))` if handled.
    #[allow(clippy::too_many_arguments)]
    fn try_invokedynamic_gradle_dispatch(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        method_name: &str,
        descriptor: &str,
        receiver_class_name: &str,
        _receiver: ObjectRef,
        heap_id: HeapId,
        args: &[Value],
    ) -> VMResult<Option<()>> {
        // getProperty on Map receivers: use Map.get(key) directly
        if method_name == "getProperty"
            && (receiver_class_name == "java/util/HashMap"
                || receiver_class_name == "java/util/LinkedHashMap")
        {
            if let Some(key) = args.first() {
                let value = heap
                    .hashmap_get(heap_id, key)
                    .unwrap_or(Value::Reference(ObjectRef::Null));
                let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
                frame.push_raw(value);
                return Ok(Some(()));
            }
        }

        // A dependencies { } closure delegates to the DependencyHandler, so the handler's
        // own methods (project, platform, the configurations) answer before the Project's.
        if receiver_class_name.contains("$_closure_") && self.gradle_native_bridge.is_some() {
            let delegate =
                crate::interpreter::closure_runtime::ClosureRuntime::get_delegate(heap, heap_id)?;
            if let ObjectRef::Object(delegate_id) = delegate {
                if heap.get_object_class_name(delegate_id)? == "gradle/DependencyHandler"
                    && is_dependency_handler_method(method_name)
                {
                    return self.invoke_gradle_and_push(
                        thread,
                        heap,
                        "gradle/DependencyHandler",
                        method_name,
                        descriptor,
                        args,
                    );
                }
            }
        }

        // Gradle DSL methods on script instances.
        // In recording proxy mode, route ALL methods through the bridge so unknown
        // extension blocks are handled instead of falling through to method resolution.
        if (is_gradle_script_method(method_name) || self.is_recording_proxy())
            && self.gradle_native_bridge.is_some()
        {
            return self.invoke_gradle_and_push(
                thread,
                heap,
                "gradle/Project",
                method_name,
                descriptor,
                args,
            );
        }

        // Methods on gradle/ receiver classes
        if receiver_class_name.starts_with("gradle/") && self.gradle_native_bridge.is_some() {
            return self.invoke_gradle_and_push(
                thread,
                heap,
                receiver_class_name,
                method_name,
                descriptor,
                args,
            );
        }

        // Fallback: delegate unknown methods on Groovy script classes to gradle/Project.
        // Gradle build scripts delegate to the Project object, so plugin-provided DSL
        // methods (e.g. compileThrift, springBoot) that aren't in the explicit list
        // should still route through the project stub.
        if self.gradle_native_bridge.is_some()
            && !receiver_class_name.starts_with("java/")
            && !receiver_class_name.starts_with("groovy/")
            && !receiver_class_name.contains("$_closure_")
            && !receiver_class_name.contains("$_nested_")
            && !self.method_registry.contains_key(&(
                receiver_class_name.to_string(),
                method_name.to_string(),
                descriptor.to_string(),
            ))
        {
            return self.invoke_gradle_and_push(
                thread,
                heap,
                "gradle/Project",
                method_name,
                descriptor,
                args,
            );
        }

        Ok(None)
    }

    /// Invoke a Gradle bridge method and push the result onto the operand stack.
    /// Returns `Ok(Some(()))` on success.
    fn invoke_gradle_and_push(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<Option<()>> {
        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        let result = self
            .gradle_native_bridge
            .as_ref()
            .unwrap()
            .invoke_gradle_method(
                heap,
                &self.method_registry,
                thread,
                class_name,
                method_name,
                descriptor,
                args,
            )?;
        let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
        if !is_gradle_void_dsl_method(method_name) {
            frame
                .operand_stack
                .push(result.unwrap_or(Value::Reference(ObjectRef::Null)));
        }
        Ok(Some(()))
    }

    /// Resolve the target method for invokedynamic via vtable -> MOP fallback.
    ///
    /// Returns `(declaring_class, method_name, descriptor, actual_receiver)`.
    /// The actual_receiver may differ from the original receiver when closure delegation
    /// redirects to a delegate or owner object.
    #[allow(clippy::too_many_arguments)]
    fn resolve_invokedynamic_target(
        &self,
        thread: &mut Thread,
        heap: &mut Heap,
        method_name: &str,
        descriptor: &str,
        receiver_class_name: &str,
        receiver: ObjectRef,
        heap_id: HeapId,
    ) -> VMResult<(String, String, String, ObjectRef)> {
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?
            .read()
            .map_err(|_| VMError::internal("Failed to acquire lock"))?;

        let loaded_class = method_area.get_class(receiver_class_name)?;
        debug!(
            "INVOKEDYNAMIC: looking up {}.{}:{} in vtable (vtable has {} entries)",
            receiver_class_name,
            method_name,
            descriptor,
            loaded_class.vtable.len()
        );

        let vtable_result = loaded_class.get_vtable_index(method_name, descriptor);
        debug!("INVOKEDYNAMIC: vtable_result={:?}", vtable_result);

        let mut actual_receiver = receiver;

        // Check closure context for delegate resolution
        let is_closure = receiver_class_name.contains("$_closure_")
            || receiver_class_name.contains("$_nested_")
            || receiver_class_name == "groovy/lang/Closure";
        let mut closure_context = None;
        if !is_closure {
            if let Some(frame) = thread.current_frame() {
                if frame.class_name.contains("$_closure_") || frame.class_name.contains("$_nested_")
                {
                    if let Some(Some(Value::Reference(ObjectRef::Object(id)))) =
                        frame.locals.first()
                    {
                        closure_context = Some(*id);
                    }
                }
            }
        }

        let result = match vtable_result {
            Ok(vtable_index) => {
                let vtable_entry = loaded_class
                    .vtable
                    .get(vtable_index)
                    .ok_or_else(|| VMError::internal("Invalid vtable index"))?;
                vtable_entry.require_concrete(receiver_class_name)?;
                (
                    vtable_entry.declaring_class().to_string(),
                    method_name.to_string(),
                    descriptor.to_string(),
                    actual_receiver,
                )
            }
            Err(_) => {
                let runtime_mop_ref = self.runtime_mop.as_ref().ok_or_else(|| {
                    VMError::NoSuchMethod(format!(
                        "{}.{}:{}",
                        receiver_class_name, method_name, descriptor
                    ))
                })?;

                let mut mop = runtime_mop_ref
                    .write()
                    .map_err(|_| VMError::internal("Failed to acquire MOP lock"))?;
                let loaded_classes = method_area.all_classes();

                if is_closure || closure_context.is_some() {
                    let closure_id = closure_context.unwrap_or(heap_id);
                    debug!(
                        "INVOKEDYNAMIC: using closure-aware resolution for {}",
                        receiver_class_name
                    );
                    match mop.resolve_closure_method(
                        heap,
                        closure_id,
                        method_name,
                        descriptor,
                        &loaded_classes,
                    ) {
                        Ok((resolved, delegate_receiver)) => {
                            actual_receiver = delegate_receiver;
                            debug!("INVOKEDYNAMIC: closure resolved to {:?}", actual_receiver);
                            let (c, m, d) = extract_method_info(&resolved.handle)?;
                            (c, m, d, actual_receiver)
                        }
                        Err(e) => {
                            if method_name == "doCall" {
                                self.find_docall_in_registry(receiver_class_name, e)?
                            } else {
                                return Err(e);
                            }
                        }
                    }
                } else {
                    {
                        let resolved = mop.resolve_method(
                            receiver_class_name,
                            method_name,
                            descriptor,
                            &loaded_classes,
                        )?;
                        let (c, m, d) = extract_method_info(&resolved.handle)?;
                        (c, m, d, actual_receiver)
                    }
                }
            }
        };

        drop(method_area);
        Ok(result)
    }

    /// Fallback: find a doCall method in the method registry when MOP resolution fails.
    fn find_docall_in_registry(
        &self,
        receiver_class_name: &str,
        original_error: VMError,
    ) -> VMResult<(String, String, String, ObjectRef)> {
        if let Some((class_name, method_name, descriptor)) =
            self.method_registry
                .keys()
                .find_map(|(class_name, method_name, descriptor)| {
                    if class_name == receiver_class_name && method_name == "doCall" {
                        Some((class_name.clone(), method_name.clone(), descriptor.clone()))
                    } else {
                        None
                    }
                })
        {
            Ok((class_name, method_name, descriptor, ObjectRef::Null))
        } else {
            Err(original_error)
        }
    }

    /// Dispatch a resolved gradle/ method through the gradle native bridge,
    /// pushing the appropriate return value based on the descriptor.
    fn dispatch_gradle_resolved(
        &mut self,
        thread: &mut Thread,
        heap: &mut Heap,
        declaring_class: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<()> {
        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        let result = self
            .gradle_native_bridge
            .as_ref()
            .unwrap()
            .invoke_gradle_method(
                heap,
                &self.method_registry,
                thread,
                declaring_class,
                method_name,
                descriptor,
                args,
            )?;

        let desc = MethodDescriptor::parse(descriptor)
            .map_err(|_| VMError::internal("Invalid method descriptor"))?;

        if !matches!(desc.return_type, DescriptorType::Void) {
            let frame = thread.current_frame_mut().ok_or(VMError::NoActiveFrame)?;
            if let Some(value) = result {
                frame.push_raw(value);
            } else {
                push_default_return_value(frame, &desc);
            }
        }
        Ok(())
    }

    /// Look up the method bytecode in the registry and push a new frame onto the thread.
    fn push_invokedynamic_frame(
        &self,
        thread: &mut Thread,
        declaring_class: &str,
        resolved_method_name: &str,
        resolved_descriptor: &mut String,
        actual_receiver: ObjectRef,
        args: Vec<Value>,
    ) -> VMResult<()> {
        let method_key = (
            declaring_class.to_string(),
            resolved_method_name.to_string(),
            resolved_descriptor.clone(),
        );
        debug!("INVOKEDYNAMIC: looking up method_key={:?}", method_key);

        let method_bytecode = self.lookup_method_bytecode(
            declaring_class,
            resolved_method_name,
            resolved_descriptor,
        )?;

        let mut frame_locals = vec![Value::Reference(actual_receiver)];
        frame_locals.extend(args);
        debug!(
            "INVOKEDYNAMIC: creating frame for {}.{} with {} locals, bytecode len={}",
            declaring_class,
            resolved_method_name,
            frame_locals.len(),
            method_bytecode.code.len()
        );

        let method_frame = Frame::with_bytecode(
            declaring_class.to_string(),
            resolved_method_name.to_string(),
            method_bytecode.max_locals,
            method_bytecode.max_stack,
            frame_locals,
            method_bytecode.code.clone(),
            resolved_descriptor.clone(),
            method_bytecode.exception_table,
        )?;

        if let Some(caller_frame) = thread.current_frame_mut() {
            caller_frame.pc = self.pc;
        }
        thread.push_frame(method_frame)?;
        debug!(
            "INVOKEDYNAMIC: pushed frame, stack depth={}",
            thread.depth()
        );
        Ok(())
    }

    /// Look up method bytecode, with fallback for doCall descriptor mismatch.
    fn lookup_method_bytecode(
        &self,
        declaring_class: &str,
        method_name: &str,
        resolved_descriptor: &mut String,
    ) -> VMResult<MethodBytecode> {
        let method_key = (
            declaring_class.to_string(),
            method_name.to_string(),
            resolved_descriptor.clone(),
        );

        if let Some(method) = self.method_registry.get(&method_key).cloned() {
            return Ok(method);
        }

        // doCall descriptor mismatch fallback: find any doCall on the declaring class
        if method_name == "doCall" {
            if let Some(((_, _, descriptor), method)) = self
                .method_registry
                .iter()
                .find(|((cn, mn, _), _)| cn == declaring_class && mn == "doCall")
            {
                *resolved_descriptor = descriptor.clone();
                return Ok(method.clone());
            }
        }

        debug!("INVOKEDYNAMIC: method not found in registry, available keys:");
        for key in self.method_registry.keys() {
            debug!("  - {:?}", key);
        }
        Err(VMError::NoSuchMethod(format!(
            "{}.{}:{}",
            declaring_class, method_name, resolved_descriptor
        )))
    }
}

/// The DependencyHandler methods a dependencies block calls without a receiver.
fn is_dependency_handler_method(name: &str) -> bool {
    matches!(
        name,
        "project"
            | "files"
            | "fileTree"
            | "platform"
            | "enforcedPlatform"
            | "add"
            | "implementation"
            | "testImplementation"
            | "api"
            | "runtimeOnly"
            | "compileOnly"
            | "annotationProcessor"
            | "testCompileOnly"
            | "testRuntimeOnly"
            | "developmentOnly"
            | "kapt"
            | "classpath"
    )
}
