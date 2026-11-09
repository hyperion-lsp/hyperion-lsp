// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use bytes::{BufMut, BytesMut};

use crate::replies::{tag, JdwpValue};

/// JDWP command with its command set and command ID, plus serialized payload.
#[derive(Debug, Clone)]
pub struct Command {
    pub command_set: u8,
    pub command: u8,
    pub data: BytesMut,
}

// ---- VirtualMachine command set (1) ----

impl Command {
    /// `VirtualMachine.Version` — returns VM version info.
    pub fn vm_version() -> Self {
        Self {
            command_set: 1,
            command: 1,
            data: BytesMut::new(),
        }
    }

    /// `VirtualMachine.ClassesBySignature` — find classes matching a JNI signature.
    pub fn vm_classes_by_signature(signature: &str) -> Self {
        let mut data = BytesMut::new();
        put_string(&mut data, signature);
        Self {
            command_set: 1,
            command: 2,
            data,
        }
    }

    /// `VirtualMachine.AllThreads` — list all live threads.
    pub fn vm_all_threads() -> Self {
        Self {
            command_set: 1,
            command: 4,
            data: BytesMut::new(),
        }
    }

    /// `VirtualMachine.IDSizes` — get sizes of object IDs, reference type IDs, etc.
    /// Must be called before interpreting any ID fields.
    pub fn vm_id_sizes() -> Self {
        Self {
            command_set: 1,
            command: 7,
            data: BytesMut::new(),
        }
    }

    /// `VirtualMachine.Resume` — resume all threads.
    pub fn vm_resume() -> Self {
        Self {
            command_set: 1,
            command: 9,
            data: BytesMut::new(),
        }
    }

    /// `VirtualMachine.Exit` — terminate the debuggee VM.
    pub fn vm_exit(exit_code: i32) -> Self {
        let mut data = BytesMut::new();
        data.put_i32(exit_code);
        Self {
            command_set: 1,
            command: 10,
            data,
        }
    }

    /// `VirtualMachine.CreateString` — create a `java.lang.String` in the debuggee.
    pub fn vm_create_string(value: &str) -> Self {
        let mut data = BytesMut::new();
        put_string(&mut data, value);
        Self {
            command_set: 1,
            command: 11,
            data,
        }
    }

    /// `VirtualMachine.AllClassesWithGeneric` — all loaded classes with generic signatures.
    pub fn vm_all_classes_with_generic() -> Self {
        Self {
            command_set: 1,
            command: 20,
            data: BytesMut::new(),
        }
    }
}

// ---- ReferenceType command set (2) ----

impl Command {
    /// `ReferenceType.Signature` — get the JNI signature for a reference type.
    pub fn reference_type_signature(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 1,
            data,
        }
    }

    /// `ReferenceType.Methods` — list methods of a class/interface.
    pub fn reference_type_methods(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 5,
            data,
        }
    }

    /// `ReferenceType.SourceFile` — get the source file name for a reference type.
    pub fn reference_type_source_file(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 7,
            data,
        }
    }

    /// `ReferenceType.GetValues` — read static field values.
    pub fn reference_type_get_values(
        ref_type_id: u64,
        field_ids: &[u64],
        id_sizes: &IdSizes,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_sizes.reference_type_id_size);
        data.put_i32(field_ids.len() as i32);
        for &field_id in field_ids {
            put_id(&mut data, field_id, id_sizes.field_id_size);
        }
        Self {
            command_set: 2,
            command: 6,
            data,
        }
    }

    /// `ReferenceType.Interfaces` — directly implemented interfaces.
    pub fn reference_type_interfaces(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 10,
            data,
        }
    }

    /// `ReferenceType.MethodsWithGeneric` — methods with generic signatures.
    pub fn reference_type_methods_with_generic(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 15,
            data,
        }
    }
}

// ---- ClassType command set (3) ----

impl Command {
    /// `ClassType.Superclass` — get the superclass of a class type. Used to
    /// walk up the inheritance chain (e.g. to find `Throwable.detailMessage`
    /// declared above a concrete exception's own type). Returns 0 if this type
    /// is `java.lang.Object`.
    ///
    /// Note this is command set 3 (`ClassType`), *not* command set 2
    /// (`ReferenceType`) — command 3 in the `ReferenceType` set is actually
    /// `Modifiers`, a different command with a different reply shape.
    pub fn class_type_superclass(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 3,
            command: 1,
            data,
        }
    }
}

/// `InvokeMethod` option: resume only the invoking thread.
pub const INVOKE_SINGLE_THREADED: i32 = 1;

impl Command {
    fn invoke_payload(
        data: &mut BytesMut,
        thread_id: u64,
        class_id: u64,
        method_id: u64,
        args: &[JdwpValue],
        id_sizes: IdSizes,
    ) {
        put_id(data, thread_id, id_sizes.object_id_size);
        put_id(data, class_id, id_sizes.reference_type_id_size);
        put_id(data, method_id, id_sizes.method_id_size);
        data.put_i32(args.len() as i32);
        for arg in args {
            put_tagged_value(data, arg, &id_sizes);
        }
        data.put_i32(INVOKE_SINGLE_THREADED);
    }

    /// `ClassType.InvokeMethod` — call a static method on a suspended thread.
    pub fn class_invoke_method(
        class_id: u64,
        thread_id: u64,
        method_id: u64,
        args: &[JdwpValue],
        id_sizes: &IdSizes,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, class_id, id_sizes.reference_type_id_size);
        put_id(&mut data, thread_id, id_sizes.object_id_size);
        put_id(&mut data, method_id, id_sizes.method_id_size);
        data.put_i32(args.len() as i32);
        for arg in args {
            put_tagged_value(&mut data, arg, id_sizes);
        }
        data.put_i32(INVOKE_SINGLE_THREADED);
        Self {
            command_set: 3,
            command: 3,
            data,
        }
    }

    /// `ObjectReference.InvokeMethod` — call an instance method on a suspended thread.
    pub fn object_invoke_method(
        object_id: u64,
        thread_id: u64,
        class_id: u64,
        method_id: u64,
        args: &[JdwpValue],
        id_sizes: &IdSizes,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, object_id, id_sizes.object_id_size);
        Self::invoke_payload(&mut data, thread_id, class_id, method_id, args, *id_sizes);
        Self {
            command_set: 9,
            command: 6,
            data,
        }
    }
}

// ---- Method command set (6) ----

impl Command {
    /// `Method.LineTable` — get the line number table for a method.
    pub fn method_line_table(
        ref_type_id: u64,
        method_id: u64,
        ref_id_size: u8,
        method_id_size: u8,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, ref_id_size);
        put_id(&mut data, method_id, method_id_size);
        Self {
            command_set: 6,
            command: 1,
            data,
        }
    }

    /// `Method.VariableTableWithGeneric` — get local variable info with generic signatures.
    pub fn method_variable_table_with_generic(
        ref_type_id: u64,
        method_id: u64,
        ref_id_size: u8,
        method_id_size: u8,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, ref_id_size);
        put_id(&mut data, method_id, method_id_size);
        Self {
            command_set: 6,
            command: 5,
            data,
        }
    }
}

// ---- ThreadReference command set (11) ----

impl Command {
    /// `ThreadReference.Name` — get thread name.
    pub fn thread_name(thread_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, id_size);
        Self {
            command_set: 11,
            command: 1,
            data,
        }
    }

    /// `ThreadReference.Resume` — resume a single thread.
    pub fn thread_resume(thread_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, id_size);
        Self {
            command_set: 11,
            command: 3,
            data,
        }
    }

    /// `ThreadReference.Frames` — get stack frames for a thread.
    pub fn thread_frames(thread_id: u64, start: i32, length: i32, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, id_size);
        data.put_i32(start);
        data.put_i32(length);
        Self {
            command_set: 11,
            command: 6,
            data,
        }
    }

    /// `ThreadReference.FrameCount` — get frame count for a thread.
    pub fn thread_frame_count(thread_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, id_size);
        Self {
            command_set: 11,
            command: 7,
            data,
        }
    }
}

// ---- StackFrame command set (16) ----

impl Command {
    /// `StackFrame.GetValues` — read local variable values from a frame.
    pub fn stack_frame_get_values(
        thread_id: u64,
        frame_id: u64,
        slots: &[(i32, u8)], // (slot index, tag/sigbyte)
        object_id_size: u8,
        frame_id_size: u8,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, object_id_size);
        put_id(&mut data, frame_id, frame_id_size);
        data.put_i32(slots.len() as i32);
        for &(slot, tag) in slots {
            data.put_i32(slot);
            data.put_u8(tag);
        }
        Self {
            command_set: 16,
            command: 1,
            data,
        }
    }
}

impl Command {
    /// `StackFrame.ThisObject` — the `this` object of a frame (null for static frames).
    pub fn stack_frame_this_object(
        thread_id: u64,
        frame_id: u64,
        object_id_size: u8,
        frame_id_size: u8,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, thread_id, object_id_size);
        put_id(&mut data, frame_id, frame_id_size);
        Self {
            command_set: 16,
            command: 3,
            data,
        }
    }
}

// ---- ReferenceType.Fields command (set 2, cmd 4) ----

impl Command {
    /// `ReferenceType.Fields` — get field IDs, names, signatures, and access flags for a class.
    pub fn reference_type_fields(ref_type_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, ref_type_id, id_size);
        Self {
            command_set: 2,
            command: 4,
            data,
        }
    }
}

// ---- ObjectReference command set (9) ----

impl Command {
    /// `ObjectReference.ReferenceType` — get the runtime type of an object.
    pub fn object_reference_type(object_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, object_id, id_size);
        Self {
            command_set: 9,
            command: 1,
            data,
        }
    }

    /// `ObjectReference.GetValues` — read field values from an object.
    pub fn object_get_values(
        object_id: u64,
        field_ids: &[u64],
        obj_id_size: u8,
        field_id_size: u8,
    ) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, object_id, obj_id_size);
        data.put_i32(field_ids.len() as i32);
        for &fid in field_ids {
            put_id(&mut data, fid, field_id_size);
        }
        Self {
            command_set: 9,
            command: 2,
            data,
        }
    }
}

// ---- ArrayReference command set (13) ----

impl Command {
    /// `ArrayReference.Length` — get the length of an array.
    pub fn array_length(array_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, array_id, id_size);
        Self {
            command_set: 13,
            command: 1,
            data,
        }
    }

    /// `ArrayReference.GetValues` — get a range of array element values.
    pub fn array_get_values(array_id: u64, first_index: i32, length: i32, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, array_id, id_size);
        data.put_i32(first_index);
        data.put_i32(length);
        Self {
            command_set: 13,
            command: 2,
            data,
        }
    }
}

// ---- StringReference command set (10) ----

impl Command {
    /// `StringReference.Value` — get the UTF-8 string value of a String object.
    pub fn string_value(string_id: u64, id_size: u8) -> Self {
        let mut data = BytesMut::new();
        put_id(&mut data, string_id, id_size);
        Self {
            command_set: 10,
            command: 1,
            data,
        }
    }
}

// ---- EventRequest command set (15) ----

/// Modifier for event request filtering.
#[derive(Debug, Clone)]
pub enum EventModifier {
    /// Filter by location (breakpoint).
    LocationOnly {
        type_tag: u8,
        class_id: u64,
        method_id: u64,
        index: u64,
    },
    /// Step filter: thread + size + depth.
    Step {
        thread_id: u64,
        size: i32,  // 0=min, 1=line
        depth: i32, // 0=into, 1=over, 2=out
    },
    /// Filter by class match pattern (e.g., "com.example.*").
    ClassMatch(String),
    /// Filter to a specific thread.
    ThreadOnly(u64),
    /// Filter class-prepare events by the class's `SourceFile` attribute
    /// (`Foo.kt`; `*` wildcards allowed at either end). JDWP 1.6+.
    SourceNameMatch(String),
    /// Exception filter.
    ExceptionOnly {
        ref_type_id_or_null: u64,
        caught: bool,
        uncaught: bool,
    },
    /// Count filter — trigger after N occurrences.
    Count(i32),
}

/// Step size constants.
pub const STEP_SIZE_MIN: i32 = 0;
pub const STEP_SIZE_LINE: i32 = 1;

/// Step depth constants.
pub const STEP_DEPTH_INTO: i32 = 0;
pub const STEP_DEPTH_OVER: i32 = 1;
pub const STEP_DEPTH_OUT: i32 = 2;

/// Event kind constants.
pub const EVENT_KIND_SINGLE_STEP: u8 = 1;
pub const EVENT_KIND_BREAKPOINT: u8 = 2;
pub const EVENT_KIND_EXCEPTION: u8 = 4;
pub const EVENT_KIND_THREAD_START: u8 = 6;
pub const EVENT_KIND_THREAD_DEATH: u8 = 7;
pub const EVENT_KIND_CLASS_PREPARE: u8 = 8;
pub const EVENT_KIND_VM_DEATH: u8 = 99;

/// Suspend policy constants.
pub const SUSPEND_NONE: u8 = 0;
pub const SUSPEND_EVENT_THREAD: u8 = 1;
pub const SUSPEND_ALL: u8 = 2;

impl Command {
    /// `EventRequest.Set` — request notification when an event occurs.
    pub fn event_request_set(
        event_kind: u8,
        suspend_policy: u8,
        modifiers: &[EventModifier],
        id_sizes: &IdSizes,
    ) -> Self {
        let mut data = BytesMut::new();
        data.put_u8(event_kind);
        data.put_u8(suspend_policy);
        data.put_i32(modifiers.len() as i32);

        for modifier in modifiers {
            match modifier {
                EventModifier::Count(count) => {
                    data.put_u8(1); // modKind = Count
                    data.put_i32(*count);
                }
                EventModifier::ThreadOnly(thread_id) => {
                    data.put_u8(3); // modKind = ThreadOnly
                    put_id(&mut data, *thread_id, id_sizes.object_id_size);
                }
                EventModifier::ClassMatch(pattern) => {
                    data.put_u8(5); // modKind = ClassMatch
                    put_string(&mut data, pattern);
                }
                EventModifier::SourceNameMatch(pattern) => {
                    data.put_u8(12); // modKind = SourceNameMatch
                    put_string(&mut data, pattern);
                }
                EventModifier::LocationOnly {
                    type_tag,
                    class_id,
                    method_id,
                    index,
                } => {
                    data.put_u8(7); // modKind = LocationOnly
                    data.put_u8(*type_tag);
                    put_id(&mut data, *class_id, id_sizes.reference_type_id_size);
                    put_id(&mut data, *method_id, id_sizes.method_id_size);
                    data.put_u64(*index);
                }
                EventModifier::ExceptionOnly {
                    ref_type_id_or_null,
                    caught,
                    uncaught,
                } => {
                    data.put_u8(8); // modKind = ExceptionOnly
                    put_id(
                        &mut data,
                        *ref_type_id_or_null,
                        id_sizes.reference_type_id_size,
                    );
                    data.put_u8(u8::from(*caught));
                    data.put_u8(u8::from(*uncaught));
                }
                EventModifier::Step {
                    thread_id,
                    size,
                    depth,
                } => {
                    data.put_u8(10); // modKind = Step
                    put_id(&mut data, *thread_id, id_sizes.object_id_size);
                    data.put_i32(*size);
                    data.put_i32(*depth);
                }
            }
        }

        Self {
            command_set: 15,
            command: 1,
            data,
        }
    }

    /// `EventRequest.Clear` — cancel an event request.
    pub fn event_request_clear(event_kind: u8, request_id: u32) -> Self {
        let mut data = BytesMut::new();
        data.put_u8(event_kind);
        data.put_u32(request_id);
        Self {
            command_set: 15,
            command: 2,
            data,
        }
    }
}

/// ID sizes returned by `VirtualMachine.IDSizes`.
/// All JDWP ID fields are variable-width; this tells us how many bytes each uses.
#[derive(Debug, Clone, Copy)]
pub struct IdSizes {
    pub field_id_size: u8,
    pub method_id_size: u8,
    pub object_id_size: u8,
    pub reference_type_id_size: u8,
    pub frame_id_size: u8,
}

impl Default for IdSizes {
    fn default() -> Self {
        // Most JVMs use 8-byte IDs.
        Self {
            field_id_size: 8,
            method_id_size: 8,
            object_id_size: 8,
            reference_type_id_size: 8,
            frame_id_size: 8,
        }
    }
}

// ---- Encoding helpers ----

/// Write a variable-width ID (big-endian, zero-padded to `size` bytes).
pub fn put_id(buf: &mut BytesMut, id: u64, size: u8) {
    let bytes = id.to_be_bytes();
    let start = 8 - size as usize;
    buf.put_slice(&bytes[start..]);
}

/// Write a JDWP string: 4-byte length prefix + UTF-8 bytes.
pub fn put_string(buf: &mut BytesMut, s: &str) {
    buf.put_u32(s.len() as u32);
    buf.put_slice(s.as_bytes());
}

/// Write a tagged JDWP value (tag byte + payload), as `InvokeMethod` arguments require.
pub fn put_tagged_value(buf: &mut BytesMut, value: &JdwpValue, id_sizes: &IdSizes) {
    match value {
        JdwpValue::Void => buf.put_u8(tag::VOID),
        JdwpValue::Byte(v) => {
            buf.put_u8(tag::BYTE);
            buf.put_i8(*v);
        }
        JdwpValue::Boolean(v) => {
            buf.put_u8(tag::BOOLEAN);
            buf.put_u8(u8::from(*v));
        }
        JdwpValue::Char(v) => {
            buf.put_u8(tag::CHAR);
            buf.put_u16(*v);
        }
        JdwpValue::Short(v) => {
            buf.put_u8(tag::SHORT);
            buf.put_i16(*v);
        }
        JdwpValue::Int(v) => {
            buf.put_u8(tag::INT);
            buf.put_i32(*v);
        }
        JdwpValue::Long(v) => {
            buf.put_u8(tag::LONG);
            buf.put_i64(*v);
        }
        JdwpValue::Float(v) => {
            buf.put_u8(tag::FLOAT);
            buf.put_f32(*v);
        }
        JdwpValue::Double(v) => {
            buf.put_u8(tag::DOUBLE);
            buf.put_f64(*v);
        }
        JdwpValue::Object { tag: t, object_id } => {
            buf.put_u8(*t);
            put_id(buf, *object_id, id_sizes.object_id_size);
        }
        JdwpValue::Null => {
            buf.put_u8(tag::OBJECT);
            put_id(buf, 0, id_sizes.object_id_size);
        }
    }
}
