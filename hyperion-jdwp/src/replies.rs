// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use bytes::{Buf, BytesMut};

use crate::commands::IdSizes;
use crate::error::{JdwpError, Result};

// ---- Reading helpers ----

const MAX_COLLECTION_ITEMS: usize = 1_000_000;

/// Read a variable-width ID from the buffer.
pub fn get_id(buf: &mut BytesMut, size: u8) -> Result<u64> {
    if !(1..=8).contains(&size) {
        return Err(JdwpError::BadReply(format!("invalid JDWP ID size: {size}")));
    }
    if buf.remaining() < size as usize {
        return Err(JdwpError::BadReply("truncated ID field".into()));
    }
    let mut val: u64 = 0;
    for _ in 0..size {
        val = (val << 8) | u64::from(buf.get_u8());
    }
    Ok(val)
}

pub(crate) fn get_u8(buf: &mut BytesMut, field: &str) -> Result<u8> {
    ensure_remaining_for(buf, 1, field)?;
    Ok(buf.get_u8())
}

pub(crate) fn get_u32(buf: &mut BytesMut, field: &str) -> Result<u32> {
    ensure_remaining_for(buf, 4, field)?;
    Ok(buf.get_u32())
}

pub(crate) fn get_i32(buf: &mut BytesMut, field: &str) -> Result<i32> {
    ensure_remaining_for(buf, 4, field)?;
    Ok(buf.get_i32())
}

fn get_u64(buf: &mut BytesMut, field: &str) -> Result<u64> {
    ensure_remaining_for(buf, 8, field)?;
    Ok(buf.get_u64())
}

/// Read a non-negative collection count and validate it against both a global allocation limit
/// and the minimum bytes required for each item in the remaining packet.
pub(crate) fn get_count(
    buf: &mut BytesMut,
    collection: &str,
    minimum_item_size: usize,
) -> Result<usize> {
    let raw = get_i32(buf, &format!("{collection} count"))?;
    let count = usize::try_from(raw)
        .map_err(|_| JdwpError::BadReply(format!("negative {collection} count: {raw}")))?;
    if count > MAX_COLLECTION_ITEMS {
        return Err(JdwpError::BadReply(format!(
            "{collection} count {count} exceeds limit {MAX_COLLECTION_ITEMS}"
        )));
    }
    if minimum_item_size != 0 && count > buf.remaining() / minimum_item_size {
        return Err(JdwpError::BadReply(format!(
            "{collection} count {count} exceeds remaining packet data"
        )));
    }
    Ok(count)
}

/// Read a JDWP string (4-byte length + UTF-8 bytes).
pub fn get_string(buf: &mut BytesMut) -> Result<String> {
    if buf.remaining() < 4 {
        return Err(JdwpError::BadReply("truncated string length".into()));
    }
    let len = buf.get_u32() as usize;
    if buf.remaining() < len {
        return Err(JdwpError::BadReply(format!(
            "string length {len} exceeds remaining {} bytes",
            buf.remaining()
        )));
    }
    let bytes = buf.split_to(len);
    String::from_utf8(bytes.to_vec())
        .map_err(|e| JdwpError::BadReply(format!("invalid UTF-8 in string: {e}")))
}

// ---- VirtualMachine.IDSizes reply ----

impl IdSizes {
    pub fn parse(buf: &mut BytesMut) -> Result<Self> {
        let raw = [
            get_i32(buf, "field ID size")?,
            get_i32(buf, "method ID size")?,
            get_i32(buf, "object ID size")?,
            get_i32(buf, "reference type ID size")?,
            get_i32(buf, "frame ID size")?,
        ];
        if raw.iter().any(|size| !(1..=8).contains(size)) {
            return Err(JdwpError::BadReply(format!(
                "invalid JDWP ID sizes: {raw:?}"
            )));
        }
        Ok(Self {
            field_id_size: raw[0] as u8,
            method_id_size: raw[1] as u8,
            object_id_size: raw[2] as u8,
            reference_type_id_size: raw[3] as u8,
            frame_id_size: raw[4] as u8,
        })
    }
}

// ---- VirtualMachine.Version reply ----

#[derive(Debug, Clone)]
pub struct VmVersionReply {
    pub description: String,
    pub jdwp_major: i32,
    pub jdwp_minor: i32,
    pub vm_version: String,
    pub vm_name: String,
}

impl VmVersionReply {
    pub fn parse(buf: &mut BytesMut) -> Result<Self> {
        let description = get_string(buf)?;
        let jdwp_major = get_i32(buf, "JDWP major version")?;
        let jdwp_minor = get_i32(buf, "JDWP minor version")?;
        let vm_version = get_string(buf)?;
        let vm_name = get_string(buf)?;
        Ok(Self {
            description,
            jdwp_major,
            jdwp_minor,
            vm_version,
            vm_name,
        })
    }
}

// ---- VirtualMachine.ClassesBySignature reply ----

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub ref_type_tag: u8, // 1=class, 2=interface, 3=array
    pub type_id: u64,
    pub status: i32, // bit flags: 1=verified, 2=prepared, 4=initialized, 8=error
}

pub fn parse_classes_by_signature(
    buf: &mut BytesMut,
    id_sizes: &IdSizes,
) -> Result<Vec<ClassInfo>> {
    let item_size = 1 + usize::from(id_sizes.reference_type_id_size) + 4;
    let count = get_count(buf, "class", item_size)?;
    let mut classes = Vec::with_capacity(count);
    for _ in 0..count {
        let ref_type_tag = get_u8(buf, "reference type tag")?;
        let type_id = get_id(buf, id_sizes.reference_type_id_size)?;
        let status = get_i32(buf, "class status")?;
        classes.push(ClassInfo {
            ref_type_tag,
            type_id,
            status,
        });
    }
    Ok(classes)
}

// ---- VirtualMachine.AllClassesWithGeneric reply ----

#[derive(Debug, Clone)]
pub struct LoadedClass {
    pub ref_type_tag: u8,
    pub type_id: u64,
    pub signature: String,
    pub status: i32,
}

pub fn parse_all_classes_with_generic(
    buf: &mut BytesMut,
    id_sizes: &IdSizes,
) -> Result<Vec<LoadedClass>> {
    let item_size = 1 + usize::from(id_sizes.reference_type_id_size) + 12;
    let count = get_count(buf, "loaded class", item_size)?;
    let mut classes = Vec::with_capacity(count);
    for _ in 0..count {
        let ref_type_tag = get_u8(buf, "reference type tag")?;
        let type_id = get_id(buf, id_sizes.reference_type_id_size)?;
        let signature = get_string(buf)?;
        let _generic_signature = get_string(buf)?;
        let status = get_i32(buf, "class status")?;
        classes.push(LoadedClass {
            ref_type_tag,
            type_id,
            signature,
            status,
        });
    }
    Ok(classes)
}

/// `VirtualMachine.CreateString` reply: the new string's object ID.
pub fn parse_create_string(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<u64> {
    get_id(buf, id_sizes.object_id_size)
}

/// `ReferenceType.Interfaces` reply.
pub fn parse_interfaces(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<u64>> {
    let count = get_count(
        buf,
        "interface",
        usize::from(id_sizes.reference_type_id_size),
    )?;
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(get_id(buf, id_sizes.reference_type_id_size)?);
    }
    Ok(ids)
}

/// `InvokeMethod` reply: the returned value and, when the call threw, the
/// exception object (tagged object ID; 0 when none).
#[derive(Debug, Clone)]
pub struct InvokeResult {
    pub value: JdwpValue,
    pub exception: Option<u64>,
}

pub fn parse_invoke_result(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<InvokeResult> {
    let value = get_tagged_value(buf, id_sizes)?;
    let _exception_tag = get_u8(buf, "invoke exception tag")?;
    let exception = get_id(buf, id_sizes.object_id_size)?;
    Ok(InvokeResult {
        value,
        exception: (exception != 0).then_some(exception),
    })
}

/// `StackFrame.ThisObject` reply: a tagged object (null when the frame is static).
pub fn parse_this_object(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<JdwpValue> {
    get_tagged_value(buf, id_sizes)
}

// ---- VirtualMachine.AllThreads reply ----

pub fn parse_all_threads(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<u64>> {
    let count = get_count(buf, "thread", usize::from(id_sizes.object_id_size))?;
    let mut threads = Vec::with_capacity(count);
    for _ in 0..count {
        threads.push(get_id(buf, id_sizes.object_id_size)?);
    }
    Ok(threads)
}

// ---- ThreadReference.Name reply ----

pub fn parse_thread_name(buf: &mut BytesMut) -> Result<String> {
    get_string(buf)
}

// ---- ThreadReference.Frames reply ----

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub frame_id: u64,
    pub location: Location,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub type_tag: u8, // 1=class, 2=interface
    pub class_id: u64,
    pub method_id: u64,
    pub index: u64, // bytecode index within the method
}

impl Location {
    pub fn parse(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Self> {
        let type_tag = get_u8(buf, "location type tag")?;
        let class_id = get_id(buf, id_sizes.reference_type_id_size)?;
        let method_id = get_id(buf, id_sizes.method_id_size)?;
        let index = get_u64(buf, "location index")?;
        Ok(Self {
            type_tag,
            class_id,
            method_id,
            index,
        })
    }
}

pub fn parse_thread_frames(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<FrameInfo>> {
    let item_size = usize::from(id_sizes.frame_id_size)
        + 1
        + usize::from(id_sizes.reference_type_id_size)
        + usize::from(id_sizes.method_id_size)
        + 8;
    let count = get_count(buf, "frame", item_size)?;
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        let frame_id = get_id(buf, id_sizes.frame_id_size)?;
        let location = Location::parse(buf, id_sizes)?;
        frames.push(FrameInfo { frame_id, location });
    }
    Ok(frames)
}

// ---- ThreadReference.FrameCount reply ----

pub fn parse_frame_count(buf: &mut BytesMut) -> Result<i32> {
    get_i32(buf, "frame count")
}

// ---- ReferenceType.Methods reply ----

#[derive(Debug, Clone)]
pub struct MethodInfo {
    pub method_id: u64,
    pub name: String,
    pub signature: String,
    pub mod_bits: i32,
}

pub fn parse_methods(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<MethodInfo>> {
    let minimum_item_size = usize::from(id_sizes.method_id_size) + 12;
    let count = get_count(buf, "method", minimum_item_size)?;
    let mut methods = Vec::with_capacity(count);
    for _ in 0..count {
        let method_id = get_id(buf, id_sizes.method_id_size)?;
        let name = get_string(buf)?;
        let signature = get_string(buf)?;
        let mod_bits = get_i32(buf, "method modifiers")?;
        methods.push(MethodInfo {
            method_id,
            name,
            signature,
            mod_bits,
        });
    }
    Ok(methods)
}

// ---- ReferenceType.MethodsWithGeneric reply ----

#[derive(Debug, Clone)]
pub struct MethodWithGenericInfo {
    pub method_id: u64,
    pub name: String,
    pub signature: String,
    pub generic_signature: String,
    pub mod_bits: i32,
}

pub fn parse_methods_with_generic(
    buf: &mut BytesMut,
    id_sizes: &IdSizes,
) -> Result<Vec<MethodWithGenericInfo>> {
    let minimum_item_size = usize::from(id_sizes.method_id_size) + 16;
    let count = get_count(buf, "generic method", minimum_item_size)?;
    let mut methods = Vec::with_capacity(count);
    for _ in 0..count {
        let method_id = get_id(buf, id_sizes.method_id_size)?;
        let name = get_string(buf)?;
        let signature = get_string(buf)?;
        let generic_signature = get_string(buf)?;
        let mod_bits = get_i32(buf, "method modifiers")?;
        methods.push(MethodWithGenericInfo {
            method_id,
            name,
            signature,
            generic_signature,
            mod_bits,
        });
    }
    Ok(methods)
}

// ---- ReferenceType.Fields reply ----

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub field_id: u64,
    pub name: String,
    pub signature: String,
    pub mod_bits: i32,
}

pub fn parse_fields(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<FieldInfo>> {
    let minimum_item_size = usize::from(id_sizes.field_id_size) + 12;
    let count = get_count(buf, "field", minimum_item_size)?;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let field_id = get_id(buf, id_sizes.field_id_size)?;
        let name = get_string(buf)?;
        let signature = get_string(buf)?;
        let mod_bits = get_i32(buf, "field modifiers")?;
        fields.push(FieldInfo {
            field_id,
            name,
            signature,
            mod_bits,
        });
    }
    Ok(fields)
}

// ---- ObjectReference.ReferenceType reply ----

pub fn parse_object_reference_type(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<(u8, u64)> {
    let ref_type_tag = get_u8(buf, "reference type tag")?;
    let type_id = get_id(buf, id_sizes.reference_type_id_size)?;
    Ok((ref_type_tag, type_id))
}

// ---- ObjectReference.GetValues reply ----

pub fn parse_object_get_values(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<JdwpValue>> {
    let count = get_count(buf, "object value", 1)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(get_tagged_value(buf, id_sizes)?);
    }
    Ok(values)
}

// ---- ArrayReference.Length reply ----

pub fn parse_array_length(buf: &mut BytesMut) -> Result<i32> {
    get_i32(buf, "array length")
}

// ---- ArrayReference.GetValues reply ----

/// Parse array values.
///
/// The reply contains a tag byte indicating element type, then the count, then
/// the values. For primitive arrays, values are untagged. For object arrays
/// (tag L, s, [, etc.), values are tagged.
pub fn parse_array_values(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<JdwpValue>> {
    let element_tag = get_u8(buf, "array element tag")?;
    // Primitive arrays have untagged elements; object arrays have tagged elements.
    let is_primitive = matches!(
        element_tag,
        tag::BYTE
            | tag::BOOLEAN
            | tag::CHAR
            | tag::SHORT
            | tag::INT
            | tag::LONG
            | tag::FLOAT
            | tag::DOUBLE
    );
    let minimum_item_size = if is_primitive {
        value_size(element_tag, *id_sizes).unwrap_or(1)
    } else if is_object_tag(element_tag) {
        1 + usize::from(id_sizes.object_id_size)
    } else {
        1
    };
    let count = get_count(buf, "array value", minimum_item_size)?;
    let mut values = Vec::with_capacity(count);

    for _ in 0..count {
        if is_primitive {
            values.push(get_untagged_value(buf, element_tag, id_sizes)?);
        } else {
            values.push(get_tagged_value(buf, id_sizes)?);
        }
    }
    Ok(values)
}

// ---- Method.LineTable reply ----

#[derive(Debug, Clone)]
pub struct LineTableReply {
    pub start: u64,
    pub end: u64,
    pub lines: Vec<LineTableEntry>,
}

#[derive(Debug, Clone)]
pub struct LineTableEntry {
    pub line_code_index: u64,
    pub line_number: i32,
}

pub fn parse_line_table(buf: &mut BytesMut) -> Result<LineTableReply> {
    let start = get_u64(buf, "line table start")?;
    let end = get_u64(buf, "line table end")?;
    let count = get_count(buf, "line table entry", 12)?;
    let mut lines = Vec::with_capacity(count);
    for _ in 0..count {
        let line_code_index = get_u64(buf, "line code index")?;
        let line_number = get_i32(buf, "line number")?;
        lines.push(LineTableEntry {
            line_code_index,
            line_number,
        });
    }
    Ok(LineTableReply { start, end, lines })
}

// ---- Method.VariableTableWithGeneric reply ----

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub code_index: u64,
    pub name: String,
    pub signature: String,
    pub generic_signature: String,
    pub length: u32,
    pub slot: i32,
}

pub fn parse_variable_table_with_generic(buf: &mut BytesMut) -> Result<(i32, Vec<VariableInfo>)> {
    let arg_count = get_i32(buf, "variable table argument count")?;
    let count = get_count(buf, "variable", 28)?;
    let mut vars = Vec::with_capacity(count);
    for _ in 0..count {
        let code_index = get_u64(buf, "variable code index")?;
        let name = get_string(buf)?;
        let signature = get_string(buf)?;
        let generic_signature = get_string(buf)?;
        let length = get_u32(buf, "variable scope length")?;
        let slot = get_i32(buf, "variable slot")?;
        vars.push(VariableInfo {
            code_index,
            name,
            signature,
            generic_signature,
            length,
            slot,
        });
    }
    Ok((arg_count, vars))
}

// ---- StackFrame.GetValues / ObjectReference.GetValues reply ----

/// A JVM value as read from the debuggee.
#[derive(Debug, Clone)]
pub enum JdwpValue {
    Void,
    Byte(i8),
    Boolean(bool),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// Object reference (tag + object_id). Tag indicates the runtime type hint.
    Object {
        tag: u8,
        object_id: u64,
    },
    /// Null reference.
    Null,
}

/// JVM type tag constants used in `JdwpValue`.
pub mod tag {
    pub const ARRAY: u8 = b'[';
    pub const BYTE: u8 = b'B';
    pub const CHAR: u8 = b'C';
    pub const OBJECT: u8 = b'L';
    pub const FLOAT: u8 = b'F';
    pub const DOUBLE: u8 = b'D';
    pub const INT: u8 = b'I';
    pub const LONG: u8 = b'J';
    pub const SHORT: u8 = b'S';
    pub const VOID: u8 = b'V';
    pub const BOOLEAN: u8 = b'Z';
    pub const STRING: u8 = b's';
    pub const THREAD: u8 = b't';
    pub const THREAD_GROUP: u8 = b'g';
    pub const CLASS_LOADER: u8 = b'l';
    pub const CLASS_OBJECT: u8 = b'c';
    pub const NULL: u8 = 0;
}

/// Read a tagged value from the buffer.
pub fn get_tagged_value(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<JdwpValue> {
    if buf.remaining() < 1 {
        return Err(JdwpError::BadReply("truncated tagged value".into()));
    }
    let t = buf.get_u8();
    get_untagged_value(buf, t, id_sizes)
}

/// Read an untagged value knowing the tag/type.
pub fn get_untagged_value(buf: &mut BytesMut, t: u8, id_sizes: &IdSizes) -> Result<JdwpValue> {
    match t {
        tag::VOID => Ok(JdwpValue::Void),
        tag::BYTE => {
            ensure_remaining(buf, 1)?;
            Ok(JdwpValue::Byte(buf.get_i8()))
        }
        tag::BOOLEAN => {
            ensure_remaining(buf, 1)?;
            Ok(JdwpValue::Boolean(buf.get_u8() != 0))
        }
        tag::CHAR => {
            ensure_remaining(buf, 2)?;
            Ok(JdwpValue::Char(buf.get_u16()))
        }
        tag::SHORT => {
            ensure_remaining(buf, 2)?;
            Ok(JdwpValue::Short(buf.get_i16()))
        }
        tag::INT => {
            ensure_remaining(buf, 4)?;
            Ok(JdwpValue::Int(buf.get_i32()))
        }
        tag::LONG => {
            ensure_remaining(buf, 8)?;
            Ok(JdwpValue::Long(buf.get_i64()))
        }
        tag::FLOAT => {
            ensure_remaining(buf, 4)?;
            Ok(JdwpValue::Float(buf.get_f32()))
        }
        tag::DOUBLE => {
            ensure_remaining(buf, 8)?;
            Ok(JdwpValue::Double(buf.get_f64()))
        }
        tag::NULL => {
            // Null still has an object-id-sized slot of zeros.
            let _zero = get_id(buf, id_sizes.object_id_size)?;
            Ok(JdwpValue::Null)
        }
        // All object-like types: Object, String, Thread, Array, ClassLoader, etc.
        t @ (tag::OBJECT
        | tag::STRING
        | tag::THREAD
        | tag::THREAD_GROUP
        | tag::CLASS_LOADER
        | tag::CLASS_OBJECT
        | tag::ARRAY) => {
            let object_id = get_id(buf, id_sizes.object_id_size)?;
            Ok(JdwpValue::Object { tag: t, object_id })
        }
        _ => Err(JdwpError::BadReply(format!("unknown value tag: {t}"))),
    }
}

/// Parse the reply from `StackFrame.GetValues` — returns a list of tagged values.
pub fn parse_stack_frame_values(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Vec<JdwpValue>> {
    let count = get_count(buf, "stack frame value", 1)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(get_tagged_value(buf, id_sizes)?);
    }
    Ok(values)
}

/// Parse the reply from `StringReference.Value`.
pub fn parse_string_value(buf: &mut BytesMut) -> Result<String> {
    get_string(buf)
}

/// Parse the reply from `ReferenceType.Signature`.
pub fn parse_reference_type_signature(buf: &mut BytesMut) -> Result<String> {
    get_string(buf)
}

/// Parse the reply from `ReferenceType.SourceFile`.
pub fn parse_source_file(buf: &mut BytesMut) -> Result<String> {
    get_string(buf)
}

/// Parse the reply from `ClassType.Superclass` — returns the superclass's
/// reference type ID, or 0 if there is none (`java.lang.Object`).
pub fn parse_class_type_superclass(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<u64> {
    get_id(buf, id_sizes.reference_type_id_size)
}

/// Parse the reply from `EventRequest.Set` — returns the request ID.
pub fn parse_event_request_set(buf: &mut BytesMut) -> Result<u32> {
    get_u32(buf, "event request ID")
}

fn ensure_remaining(buf: &BytesMut, n: usize) -> Result<()> {
    ensure_remaining_for(buf, n, "value")
}

fn ensure_remaining_for(buf: &BytesMut, n: usize, field: &str) -> Result<()> {
    if buf.remaining() < n {
        return Err(JdwpError::BadReply(format!(
            "truncated {field}: need {n} bytes, only {} remaining",
            buf.remaining()
        )));
    }
    Ok(())
}

fn value_size(tag: u8, id_sizes: IdSizes) -> Option<usize> {
    match tag {
        tag::VOID => Some(0),
        tag::BYTE | tag::BOOLEAN => Some(1),
        tag::CHAR | tag::SHORT => Some(2),
        tag::INT | tag::FLOAT => Some(4),
        tag::LONG | tag::DOUBLE => Some(8),
        tag::NULL
        | tag::OBJECT
        | tag::STRING
        | tag::THREAD
        | tag::THREAD_GROUP
        | tag::CLASS_LOADER
        | tag::CLASS_OBJECT
        | tag::ARRAY => Some(usize::from(id_sizes.object_id_size)),
        _ => None,
    }
}

const fn is_object_tag(tag: u8) -> bool {
    matches!(
        tag,
        tag::NULL
            | tag::OBJECT
            | tag::STRING
            | tag::THREAD
            | tag::THREAD_GROUP
            | tag::CLASS_LOADER
            | tag::CLASS_OBJECT
            | tag::ARRAY
    )
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    #[test]
    fn id_sizes_reject_values_outside_wire_range() {
        for invalid in [0, -1, 9] {
            let mut buf = BytesMut::new();
            for size in [invalid, 8, 8, 8, 8] {
                buf.put_i32(size);
            }
            assert!(matches!(
                IdSizes::parse(&mut buf),
                Err(JdwpError::BadReply(message)) if message.contains("invalid JDWP ID sizes")
            ));
        }
    }

    #[test]
    fn collection_parsers_reject_negative_and_impossible_counts() {
        let id_sizes = IdSizes::default();

        let mut negative = BytesMut::new();
        negative.put_i32(-1);
        assert!(parse_all_threads(&mut negative, &id_sizes).is_err());

        let mut truncated = BytesMut::new();
        truncated.put_i32(2);
        truncated.put_u64(1);
        assert!(parse_all_threads(&mut truncated, &id_sizes).is_err());
    }

    #[test]
    fn zero_length_array_reply_needs_only_its_five_byte_header() {
        let mut buf = BytesMut::new();
        buf.put_u8(tag::INT);
        buf.put_i32(0);

        assert!(parse_array_values(&mut buf, &IdSizes::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn line_table_rejects_missing_count() {
        let mut buf = BytesMut::new();
        buf.put_u64(0);
        buf.put_u64(10);

        assert!(matches!(
            parse_line_table(&mut buf),
            Err(JdwpError::BadReply(message)) if message.contains("line table entry count")
        ));
    }

    #[test]
    fn method_parser_checks_fields_after_variable_length_strings() {
        let mut buf = BytesMut::new();
        buf.put_i32(1);
        buf.put_u64(42);
        buf.put_u32(8); // name
        buf.put_slice(b"method01");
        buf.put_u32(0); // signature
                        // Missing four-byte modifier field, despite satisfying the conservative
                        // minimum packet size check above.

        assert!(matches!(
            parse_methods(&mut buf, &IdSizes::default()),
            Err(JdwpError::BadReply(message)) if message.contains("method")
        ));
    }
}
