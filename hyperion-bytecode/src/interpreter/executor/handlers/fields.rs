// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Field access handlers for the Executor
//!
//! Handles Getfield, Putfield, Getstatic, and Putstatic opcodes.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::access_flags;
use crate::interpreter::executor::handlers::cp_resolution::ResolvedField;
use crate::interpreter::executor::Executor;
use crate::interpreter::frame::Frame;
use crate::interpreter::heap::Heap;
use crate::interpreter::monitor::MonitorTable;
use crate::interpreter::thread::Thread;
use crate::interpreter::value::{ObjectRef, Value};

/// The ordinal of a JDK enum constant the runtime makes as an object, or `None` for an
/// ordinary static field.
fn jdk_enum_ordinal(owner: &str, name: &str) -> Option<i32> {
    const TIME_UNITS: [&str; 7] = [
        "NANOSECONDS",
        "MICROSECONDS",
        "MILLISECONDS",
        "SECONDS",
        "MINUTES",
        "HOURS",
        "DAYS",
    ];
    if owner != "java/util/concurrent/TimeUnit" {
        return None;
    }
    TIME_UNITS
        .iter()
        .position(|unit| *unit == name)
        .and_then(|index| i32::try_from(index).ok())
}

impl Executor {
    /// Execute Getfield: Pop objectref, read field by index, push value.
    /// Stack: ..., objectref -> ..., value
    pub(crate) fn execute_getfield(&mut self, frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        let resolved = self.resolve_field(cp_index, &frame.class_name)?;
        if resolved.declaration.access_flags & access_flags::ACC_STATIC != 0 {
            return Err(field_kind_error("getfield", &resolved));
        }

        // Resolution and access checks precede operand consumption.
        let obj_ref = frame.pop()?.as_reference()?;
        if obj_ref.is_null() {
            return Err(VMError::NullPointerException);
        }

        // Get the heap ID
        let heap_id = obj_ref.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        self.check_instance_field_receiver(&resolved, &frame.class_name, heap, heap_id)?;
        let field_offset = resolved
            .instance_offset
            .ok_or_else(|| VMError::internal("resolved instance field has no object offset"))?;

        // Read field from heap
        let field_value = heap.get_field(heap_id, field_offset)?;

        match field_value {
            Some(value) => frame.push(value)?,
            None => frame.push(
                crate::interpreter::MethodArea::default_value_for_descriptor(
                    &resolved.declaration.descriptor,
                ),
            )?,
        }
        Ok(())
    }

    /// Execute Putfield: Pop value and objectref, write field.
    /// Stack: ..., objectref, value -> ...
    pub(crate) fn execute_putfield(&mut self, frame: &mut Frame, heap: &mut Heap) -> VMResult<()> {
        // Read the constant pool index (2 bytes)
        if self.pc + 1 >= self.bytecode.len() {
            return Err(VMError::InvalidBytecode);
        }
        let cp_index =
            u16::from_be_bytes([self.bytecode[self.pc], self.bytecode[self.pc + 1]]) as usize;
        self.pc += 2;

        let resolved = self.resolve_field(cp_index, &frame.class_name)?;
        if resolved.declaration.access_flags & access_flags::ACC_STATIC != 0 {
            return Err(field_kind_error("putfield", &resolved));
        }
        check_final_write(&resolved, frame, false)?;

        // Resolution, kind, access, and final checks precede operand consumption.
        let value = frame.pop()?;
        let obj_ref = frame.pop()?.as_reference()?;
        if obj_ref.is_null() {
            return Err(VMError::NullPointerException);
        }

        // Get the heap ID
        let heap_id = obj_ref.heap_id().ok_or_else(VMError::invalid_object_ref)?;
        self.check_instance_field_receiver(&resolved, &frame.class_name, heap, heap_id)?;
        let field_offset = resolved
            .instance_offset
            .ok_or_else(|| VMError::internal("resolved instance field has no object offset"))?;

        // Write field to heap
        heap.put_field(heap_id, field_offset, value)?;
        Ok(())
    }

    /// Execute Getstatic: Read static field, push value.
    /// Stack: ... -> ..., value
    ///
    /// Takes `thread` instead of `frame` because it may trigger class initialization
    /// which pushes new frames, requiring the current frame borrow to be dropped.
    pub(crate) fn execute_getstatic(
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

        // Constants of JDK enums are objects, which the stubs cannot declare as fields.
        // They are made here as singletons.
        let (owner, name, _) = self.resolve_field_ref(cp_index)?;
        if let Some(ordinal) = jdk_enum_ordinal(&owner, &name) {
            let constant = heap.jdk_enum_constant(&owner, &name, ordinal)?;
            thread
                .current_frame_mut()
                .ok_or(VMError::NoActiveFrame)?
                .push(Value::Reference(ObjectRef::Object(constant)))?;
            return Ok(());
        }

        let caller_class = thread
            .current_frame()
            .ok_or(VMError::NoActiveFrame)?
            .class_name
            .clone();
        let resolved = self.resolve_field(cp_index, &caller_class)?;
        if resolved.declaration.access_flags & access_flags::ACC_STATIC == 0 {
            return Err(field_kind_error("getstatic", &resolved));
        }

        // Every getstatic instruction is an active use (JVMS 5.5), even when
        // binary evolution makes it resolve to a field with ConstantValue.
        // Java source references to constant variables normally compile to an
        // ldc instruction instead, which is why they do not initialize the
        // declaring class. Initialize only the resolved declaring class, not
        // the symbolic owner used by old client bytecode.
        self.ensure_initialized(&resolved.declaring_class, thread, heap, monitors)?;

        let field_value = self.read_static_field(&resolved, heap)?;
        thread
            .current_frame_mut()
            .ok_or(VMError::NoActiveFrame)?
            .push(field_value)?;
        Ok(())
    }

    /// Execute Putstatic: Pop value, write static field.
    /// Stack: ..., value -> ...
    ///
    /// Takes `thread` instead of `frame` because it needs to drop the frame
    /// reference before accessing the method area.
    pub(crate) fn execute_putstatic(
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

        let frame = thread.current_frame().ok_or(VMError::NoActiveFrame)?;
        let resolved = self.resolve_field(cp_index, &frame.class_name)?;
        if resolved.declaration.access_flags & access_flags::ACC_STATIC == 0 {
            return Err(field_kind_error("putstatic", &resolved));
        }
        check_final_write(&resolved, frame, true)?;

        // JVMS §5.5: putstatic is an active use that triggers initialization of
        // the field's class. `ensure_initialized` runs <clinit> synchronously and
        // is re-entrant for the initializing thread (so a putstatic inside the
        // class's own <clinit> is a no-op), then restores executor state. The
        // operand remains on the caller frame until initialization completes,
        // so initializer writes happen before this active store.
        self.ensure_initialized(&resolved.declaring_class, thread, heap, monitors)?;

        // Initialization runs before the active store; the value remains on the
        // caller's operand stack while <clinit> executes.
        let value = thread
            .current_frame_mut()
            .ok_or(VMError::NoActiveFrame)?
            .pop()?;

        // Store the value in the method area
        if let Some(method_area) = &self.method_area {
            let method_area_locked = method_area.write().map_err(|_| {
                VMError::InternalError("Failed to acquire write lock on method_area".to_string())
            })?;
            method_area_locked.put_static_resolved(&resolved.static_key(), value)?;
            drop(method_area_locked);
        } else {
            return Err(VMError::InternalError(
                "method_area not initialized".to_string(),
            ));
        }
        Ok(())
    }

    fn check_instance_field_receiver(
        &self,
        resolved: &ResolvedField,
        caller_class: &str,
        heap: &Heap,
        heap_id: crate::interpreter::HeapId,
    ) -> VMResult<()> {
        let receiver_class = heap.get_object_class_name(heap_id)?;
        if !self.is_subclass_of(&receiver_class, &resolved.symbolic_owner) {
            return Err(VMError::IncompatibleClassChangeError(format!(
                "receiver {receiver_class} is not compatible with field owner {}",
                resolved.symbolic_owner
            )));
        }
        if resolved.declaration.access_flags & access_flags::ACC_PROTECTED != 0
            && !self.check_protected_access(
                caller_class,
                &receiver_class,
                &resolved.declaring_class,
            )
        {
            return Err(VMError::IllegalAccessError(format!(
                "Cannot access protected field {}.{}:{} through receiver {} from {}",
                resolved.declaring_class,
                resolved.declaration.name,
                resolved.declaration.descriptor,
                receiver_class,
                caller_class
            )));
        }
        Ok(())
    }

    fn read_static_field(&self, resolved: &ResolvedField, heap: &mut Heap) -> VMResult<Value> {
        let static_key = resolved.static_key();
        let method_area = self
            .method_area
            .as_ref()
            .ok_or_else(|| VMError::internal("MethodArea not available"))?;
        let method_area_guard = method_area
            .read()
            .map_err(|_| VMError::internal("Failed to acquire MethodArea lock"))?;
        let current_value = method_area_guard.get_static_resolved(&static_key)?;
        drop(method_area_guard);
        if !matches!(current_value, Value::Reference(ObjectRef::Null)) {
            return Ok(current_value);
        }
        if !resolved.is_native_stub {
            return Ok(current_value);
        }

        let builtin_owner = resolved.declaring_class.replace('.', "/");
        let synthesized = if builtin_owner == "java/lang/System"
            && matches!(resolved.declaration.name.as_ref(), "out" | "err")
        {
            let object = heap.allocate_object(crate::interpreter::heap::ClassRef {
                name: "java/io/PrintStream".to_string(),
                instance_field_count: 1,
                static_field_count: 0,
            })?;
            heap.set_field(
                object,
                0,
                Value::Int(i32::from(resolved.declaration.name.as_ref() == "err")),
            )?;
            Some(Value::Reference(ObjectRef::Object(object)))
        } else if builtin_owner == "java/awt/Color" {
            awt_color_constant(&resolved.declaration.name)
                .map(|packed| {
                    let object = heap.allocate_object(crate::interpreter::heap::ClassRef {
                        name: "java/awt/Color".to_string(),
                        instance_field_count: 1,
                        static_field_count: 0,
                    })?;
                    heap.set_field(object, 0, Value::Int(packed as i32))?;
                    Ok::<_, VMError>(Value::Reference(ObjectRef::Object(object)))
                })
                .transpose()?
        } else {
            None
        };
        let Some(synthesized) = synthesized else {
            return Ok(current_value);
        };
        let method_area = method_area
            .read()
            .map_err(|_| VMError::internal("Failed to acquire MethodArea lock"))?;
        if builtin_owner == "java/awt/Color" {
            let alias = awt_color_alias(&resolved.declaration.name)
                .ok_or_else(|| VMError::internal("Color constant has no alias"))?;
            let alias_key = (
                static_key.0.clone(),
                std::sync::Arc::from(alias),
                static_key.2.clone(),
            );
            method_area.publish_static_alias_group_if_null(&[static_key, alias_key], synthesized)
        } else {
            method_area.publish_static_reference_if_null(&static_key, synthesized)
        }
    }
}

fn field_kind_error(instruction: &str, resolved: &ResolvedField) -> VMError {
    VMError::IncompatibleClassChangeError(format!(
        "{instruction} has the wrong static kind for {}.{}:{}",
        resolved.declaring_class, resolved.declaration.name, resolved.declaration.descriptor
    ))
}

fn check_final_write(resolved: &ResolvedField, frame: &Frame, is_static: bool) -> VMResult<()> {
    if resolved.declaration.access_flags & 0x0010 == 0 {
        return Ok(());
    }
    let required_method = if is_static { "<clinit>" } else { "<init>" };
    if frame.class_name.replace('/', ".") == resolved.declaring_class.replace('/', ".")
        && frame.method_name == required_method
    {
        return Ok(());
    }
    Err(VMError::IllegalAccessError(format!(
        "final field {}.{}:{} may only be assigned by {}",
        resolved.declaring_class,
        resolved.declaration.name,
        resolved.declaration.descriptor,
        required_method
    )))
}

/// Packed `0xAARRGGBB` value of a `java.awt.Color` named constant, if known.
fn awt_color_constant(name: &str) -> Option<u32> {
    Some(match name {
        "BLACK" | "black" => 0xFF00_0000,
        "WHITE" | "white" => 0xFFFF_FFFF,
        "RED" | "red" => 0xFFFF_0000,
        "GREEN" | "green" => 0xFF00_FF00,
        "BLUE" | "blue" => 0xFF00_00FF,
        "YELLOW" | "yellow" => 0xFFFF_FF00,
        "CYAN" | "cyan" => 0xFF00_FFFF,
        "MAGENTA" | "magenta" => 0xFFFF_00FF,
        "GRAY" | "gray" => 0xFF80_8080,
        "LIGHT_GRAY" | "lightGray" => 0xFFC0_C0C0,
        "DARK_GRAY" | "darkGray" => 0xFF40_4040,
        "ORANGE" | "orange" => 0xFFFF_C800,
        "PINK" | "pink" => 0xFFFF_AFAF,
        _ => return None,
    })
}

fn awt_color_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "BLACK" => "black",
        "black" => "BLACK",
        "WHITE" => "white",
        "white" => "WHITE",
        "RED" => "red",
        "red" => "RED",
        "GREEN" => "green",
        "green" => "GREEN",
        "BLUE" => "blue",
        "blue" => "BLUE",
        "YELLOW" => "yellow",
        "yellow" => "YELLOW",
        "CYAN" => "cyan",
        "cyan" => "CYAN",
        "MAGENTA" => "magenta",
        "magenta" => "MAGENTA",
        "GRAY" => "gray",
        "gray" => "GRAY",
        "LIGHT_GRAY" => "lightGray",
        "lightGray" => "LIGHT_GRAY",
        "DARK_GRAY" => "darkGray",
        "darkGray" => "DARK_GRAY",
        "ORANGE" => "orange",
        "orange" => "ORANGE",
        "PINK" => "pink",
        "pink" => "PINK",
        _ => return None,
    })
}
