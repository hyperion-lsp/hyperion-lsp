// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Native `String` behavior and formatting policy.

use super::{Heap, NativeRegistry, ObjectRef, VMError, VMResult, Value};

pub(crate) fn java_string_utf16_len(value: &str) -> i32 {
    value.encode_utf16().count() as i32
}

pub(crate) fn java_string_char_at(value: &str, index: i32) -> VMResult<u16> {
    let length = java_string_utf16_len(value);
    if index < 0 || index >= length {
        return Err(VMError::StringIndexOutOfBounds { index, length });
    }
    value
        .encode_utf16()
        .nth(index as usize)
        .ok_or(VMError::StringIndexOutOfBounds { index, length })
}

pub(crate) fn java_string_substring(value: &str, begin: i32, end: i32) -> VMResult<String> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let length = units.len() as i32;
    if begin < 0 || begin > length {
        return Err(VMError::StringIndexOutOfBounds {
            index: begin,
            length,
        });
    }
    if end < begin || end > length {
        return Err(VMError::StringIndexOutOfBounds { index: end, length });
    }
    String::from_utf16(&units[begin as usize..end as usize]).map_err(|_| {
        VMError::Unimplemented(
            "String substring producing an unpaired UTF-16 surrogate".to_string(),
        )
    })
}

pub(crate) fn java_string_replace_range(
    value: &str,
    begin: i32,
    end: i32,
    replacement: &[u16],
) -> VMResult<String> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let length = units.len() as i32;
    if begin < 0 || begin > length {
        return Err(VMError::StringIndexOutOfBounds {
            index: begin,
            length,
        });
    }
    if end < begin || end > length {
        return Err(VMError::StringIndexOutOfBounds { index: end, length });
    }

    let mut result = Vec::with_capacity(units.len() - (end - begin) as usize + replacement.len());
    result.extend_from_slice(&units[..begin as usize]);
    result.extend_from_slice(replacement);
    result.extend_from_slice(&units[end as usize..]);
    String::from_utf16(&result).map_err(|_| {
        VMError::Unimplemented("String mutation producing an unpaired UTF-16 surrogate".to_string())
    })
}

pub(crate) fn java_string_index_of(value: &str, needle: &[u16], from_index: i32) -> i32 {
    let units: Vec<u16> = value.encode_utf16().collect();
    let from = from_index.max(0) as usize;
    if needle.is_empty() {
        return from.min(units.len()) as i32;
    }
    if from > units.len() || needle.len() > units.len() {
        return -1;
    }
    units[from..]
        .windows(needle.len())
        .position(|candidate| candidate == needle)
        .map_or(-1, |offset| (from + offset) as i32)
}

pub(crate) fn java_string_last_index_of(value: &str, needle: &[u16], from_index: i32) -> i32 {
    if from_index < 0 {
        return -1;
    }
    let units: Vec<u16> = value.encode_utf16().collect();
    if needle.is_empty() {
        return (from_index as usize).min(units.len()) as i32;
    }
    if needle.len() > units.len() {
        return -1;
    }
    let last_start = units.len() - needle.len();
    let from = (from_index as usize).min(last_start);
    (0..=from)
        .rev()
        .find(|&start| units[start..].starts_with(needle))
        .map_or(-1, |index| index as i32)
}

pub(crate) fn java_string_compare(left: &str, right: &str) -> i32 {
    let mut left = left.encode_utf16();
    let mut right = right.encode_utf16();
    loop {
        match (left.next(), right.next()) {
            (Some(left), Some(right)) if left != right => {
                return i32::from(left) - i32::from(right)
            }
            (Some(_), Some(_)) => {}
            (Some(_), None) => return left.count() as i32 + 1,
            (None, Some(_)) => return -(right.count() as i32 + 1),
            (None, None) => return 0,
        }
    }
}

pub(crate) fn java_string_hash(value: &str) -> i32 {
    value.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    })
}

pub(crate) fn java_string_trim(value: &str) -> String {
    value
        .trim_matches(|character: char| (character as u32) <= 0x20)
        .to_string()
}

pub(super) fn register(registry: &mut NativeRegistry) {
    // java.lang.String
    registry.register(
        "java/lang/String",
        "intern",
        "()Ljava/lang/String;",
        string_intern,
    );

    // java.lang.String methods
    registry.register("java/lang/String", "length", "()I", string_length);
    registry.register("java/lang/String", "charAt", "(I)C", string_char_at);
    registry.register(
        "java/lang/String",
        "substring",
        "(I)Ljava/lang/String;",
        string_substring_from,
    );
    registry.register(
        "java/lang/String",
        "substring",
        "(II)Ljava/lang/String;",
        string_substring,
    );
    registry.register(
        "java/lang/String",
        "toUpperCase",
        "()Ljava/lang/String;",
        string_to_upper_case,
    );
    registry.register(
        "java/lang/String",
        "toLowerCase",
        "()Ljava/lang/String;",
        string_to_lower_case,
    );
    registry.register(
        "java/lang/String",
        "trim",
        "()Ljava/lang/String;",
        string_trim,
    );
    registry.register(
        "java/lang/String",
        "startsWith",
        "(Ljava/lang/String;)Z",
        string_starts_with,
    );
    registry.register(
        "java/lang/String",
        "endsWith",
        "(Ljava/lang/String;)Z",
        string_ends_with,
    );
    registry.register(
        "java/lang/String",
        "replace",
        "(CC)Ljava/lang/String;",
        string_replace_char,
    );
    registry.register(
        "java/lang/String",
        "valueOf",
        "(I)Ljava/lang/String;",
        string_value_of_int,
    );
    registry.register(
        "java/lang/String",
        "valueOf",
        "(J)Ljava/lang/String;",
        string_value_of_long,
    );
    registry.register(
        "java/lang/String",
        "valueOf",
        "(Z)Ljava/lang/String;",
        string_value_of_boolean,
    );
    registry.register(
        "java/lang/String",
        "equals",
        "(Ljava/lang/Object;)Z",
        string_equals,
    );

    // java.lang.Object
    registry.register(
        "java/lang/Object",
        "toString",
        "()Ljava/lang/String;",
        object_to_string,
    );

    // java.lang.String.format(String, Object...) and the String.valueOf
    // numeric overloads that need formatting.
    registry.register(
        "java/lang/String",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        string_format,
    );
    registry.register(
        "java/lang/String",
        "formatted",
        "([Ljava/lang/Object;)Ljava/lang/String;",
        string_format,
    );
    registry.register(
        "java/lang/String",
        "join",
        "(Ljava/lang/CharSequence;[Ljava/lang/CharSequence;)Ljava/lang/String;",
        string_join,
    );
}

/// `String.join(delimiter, elements...)` — elements arrive as a `CharSequence[]`.
fn string_join(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let delim = match args.first() {
        Some(v) => heap.render_value(v),
        None => String::new(),
    };
    let parts: Vec<String> = match args.get(1) {
        Some(Value::Reference(ObjectRef::Array(aid))) => {
            let len = heap.array_length(*aid)?;
            (0..len)
                .map(|i| heap.array_load(*aid, i).map(|v| heap.render_value(&v)))
                .collect::<VMResult<Vec<_>>>()?
        }
        _ => Vec::new(),
    };
    let joined = parts.join(&delim);
    Ok(Some(Value::Reference(heap.intern_string(&joined)?)))
}

/// `String.format(fmt, args)` — `args` is `[fmtString, Object[] varargs]`.
/// Supports the common conversions used by real programs: `%s %d %x %f %c %n %%`
/// with optional flags/width/precision (width/precision honored for s/d/f).
fn string_format(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let fmt = match args.first() {
        Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(*id)?,
        _ => String::new(),
    };
    // Collect the varargs Object[] into a Vec<Value>.
    let varargs: Vec<Value> = match args.get(1) {
        Some(Value::Reference(ObjectRef::Array(id))) => {
            let len = heap.array_length(*id)?;
            let mut v = Vec::with_capacity(len as usize);
            for i in 0..len {
                v.push(heap.array_load(*id, i)?);
            }
            v
        }
        _ => Vec::new(),
    };
    let out = java_format(heap, &fmt, &varargs);
    Ok(Some(Value::Reference(heap.intern_string(&out)?)))
}

/// Unwrap a boxed primitive wrapper (Integer/Long/Double/Float/...) to its
/// stored field-0 `Value`, or return the value unchanged if it isn't a boxed
/// reference. The wrappers store the primitive directly in field 0.
fn unbox(heap: &Heap, v: &Value) -> Value {
    if let Value::Reference(ObjectRef::Object(id)) = v {
        if let Ok(Some(inner)) = heap.get_field(*id, 0) {
            if matches!(
                inner,
                Value::Int(_) | Value::Long(_) | Value::Float(_) | Value::Double(_)
            ) {
                return inner;
            }
        }
    }
    v.clone()
}

/// Coerce a (possibly boxed) value to i64 for integer conversions.
fn value_as_i64(heap: &Heap, v: &Value) -> i64 {
    match unbox(heap, v) {
        Value::Int(i) => i as i64,
        Value::Long(l) => l,
        Value::Float(f) => f as i64,
        Value::Double(d) => d as i64,
        _ => 0,
    }
}

/// Coerce a (possibly boxed) value to f64 for float conversions.
fn value_as_f64(heap: &Heap, v: &Value) -> f64 {
    match unbox(heap, v) {
        Value::Double(d) => d,
        Value::Float(f) => f as f64,
        Value::Int(i) => i as f64,
        Value::Long(l) => l as f64,
        _ => 0.0,
    }
}

/// A pragmatic `java.util.Formatter` covering the conversions real corpus code
/// uses. Unknown conversions are emitted verbatim.
pub(crate) fn java_format(heap: &Heap, fmt: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0usize;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // Parse: %[flags][width][.precision]conversion
        let mut spec = String::new();
        while let Some(&n) = chars.peek() {
            if "-+ 0,#(".contains(n) {
                spec.push(n);
                chars.next();
            } else {
                break;
            }
        }
        let mut width = String::new();
        while let Some(&n) = chars.peek() {
            if n.is_ascii_digit() {
                width.push(n);
                chars.next();
            } else {
                break;
            }
        }
        let mut precision = String::new();
        if chars.peek() == Some(&'.') {
            chars.next();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() {
                    precision.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
        }
        let conv = match chars.next() {
            Some(c) => c,
            None => break,
        };
        let width: usize = width.parse().unwrap_or(0);
        let left = spec.contains('-');
        let zero = spec.contains('0');
        let mut piece = match conv {
            '%' => "%".to_string(),
            'n' => "\n".to_string(),
            's' | 'S' => {
                let s = args
                    .get(arg_idx)
                    .map(|v| heap.render_value(v))
                    .unwrap_or_default();
                arg_idx += 1;
                let s = if let Ok(p) = precision.parse::<usize>() {
                    s.chars().take(p).collect()
                } else {
                    s
                };
                if conv == 'S' {
                    s.to_uppercase()
                } else {
                    s
                }
            }
            'd' => {
                let n = args
                    .get(arg_idx)
                    .map(|v| value_as_i64(heap, v))
                    .unwrap_or(0);
                arg_idx += 1;
                n.to_string()
            }
            'x' | 'X' => {
                let n = args
                    .get(arg_idx)
                    .map(|v| value_as_i64(heap, v))
                    .unwrap_or(0);
                arg_idx += 1;
                if conv == 'X' {
                    format!("{:X}", n)
                } else {
                    format!("{:x}", n)
                }
            }
            'f' => {
                let n = args
                    .get(arg_idx)
                    .map(|v| value_as_f64(heap, v))
                    .unwrap_or(0.0);
                arg_idx += 1;
                let p: usize = precision.parse().unwrap_or(6);
                format!("{:.*}", p, n)
            }
            'c' => {
                let n = args
                    .get(arg_idx)
                    .map(|v| value_as_i64(heap, v))
                    .unwrap_or(0);
                arg_idx += 1;
                char::from_u32(n as u32)
                    .map(String::from)
                    .unwrap_or_default()
            }
            'b' => {
                let v = args.get(arg_idx).cloned();
                arg_idx += 1;
                match v {
                    Some(Value::Int(0)) | Some(Value::Reference(ObjectRef::Null)) | None => {
                        "false".to_string()
                    }
                    _ => "true".to_string(),
                }
            }
            other => {
                let mut s = String::from('%');
                s.push(other);
                s
            }
        };
        // Apply width padding (space- or zero-padded) for the simple cases.
        if width > piece.chars().count() && conv != 'n' && conv != '%' {
            let pad = width - piece.chars().count();
            let padch = if zero && !left { '0' } else { ' ' };
            if left {
                piece.push_str(&" ".repeat(pad));
            } else {
                let mut p = std::iter::repeat_n(padch, pad).collect::<String>();
                p.push_str(&piece);
                piece = p;
            }
        }
        out.push_str(&piece);
    }
    out
}

impl Default for NativeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// java.lang.String.intern()Ljava/lang/String;
fn string_intern(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("intern requires 'this'".to_string()));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            // Intern returns existing or creates new - either way it's already in the pool
            let new_ref = heap.intern_string(&string)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.length()I
fn string_length(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("length requires 'this'".to_string()));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            Ok(Some(Value::Int(java_string_utf16_len(&string))))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.charAt(I)C
fn string_char_at(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "charAt requires 'this' and index".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let index = args[1].as_int()?;
            Ok(Some(
                Value::Int(java_string_char_at(&string, index)? as i32),
            ))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.substring(I)Ljava/lang/String;
fn string_substring_from(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "substring requires 'this' and beginIndex".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let begin_index = args[1].as_int()?;
            let substring =
                java_string_substring(&string, begin_index, java_string_utf16_len(&string))?;
            let new_ref = heap.intern_string(&substring)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.substring(II)Ljava/lang/String;
fn string_substring(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 3 {
        return Err(VMError::NoSuchMethod(
            "substring requires 'this', beginIndex, and endIndex".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let begin_index = args[1].as_int()?;
            let end_index = args[2].as_int()?;
            let substring = java_string_substring(&string, begin_index, end_index)?;
            let new_ref = heap.intern_string(&substring)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.toUpperCase()Ljava/lang/String;
fn string_to_upper_case(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "toUpperCase requires 'this'".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let upper = string.to_uppercase();
            let new_ref = heap.intern_string(&upper)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.toLowerCase()Ljava/lang/String;
fn string_to_lower_case(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "toLowerCase requires 'this'".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let lower = string.to_lowercase();
            let new_ref = heap.intern_string(&lower)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.trim()Ljava/lang/String;
fn string_trim(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("trim requires 'this'".to_string()));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let trimmed = java_string_trim(&string);
            let new_ref = heap.intern_string(&trimmed)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.startsWith(Ljava/lang/String;)Z
fn string_starts_with(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "startsWith requires 'this' and prefix".to_string(),
        ));
    }

    match (&args[0], &args[1]) {
        (
            Value::Reference(ObjectRef::Object(this_id)),
            Value::Reference(ObjectRef::Object(prefix_id)),
        ) => {
            let this_string = heap.get_string(*this_id)?;
            let prefix_string = heap.get_string(*prefix_id)?;
            let result = if this_string.starts_with(&prefix_string) {
                1
            } else {
                0
            };
            Ok(Some(Value::Int(result)))
        }
        (Value::Reference(ObjectRef::Null), _) | (_, Value::Reference(ObjectRef::Null)) => {
            Err(VMError::NullPointerException)
        }
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.endsWith(Ljava/lang/String;)Z
fn string_ends_with(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "endsWith requires 'this' and suffix".to_string(),
        ));
    }

    match (&args[0], &args[1]) {
        (
            Value::Reference(ObjectRef::Object(this_id)),
            Value::Reference(ObjectRef::Object(suffix_id)),
        ) => {
            let this_string = heap.get_string(*this_id)?;
            let suffix_string = heap.get_string(*suffix_id)?;
            let result = if this_string.ends_with(&suffix_string) {
                1
            } else {
                0
            };
            Ok(Some(Value::Int(result)))
        }
        (Value::Reference(ObjectRef::Null), _) | (_, Value::Reference(ObjectRef::Null)) => {
            Err(VMError::NullPointerException)
        }
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.replace(CC)Ljava/lang/String;
fn string_replace_char(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 3 {
        return Err(VMError::NoSuchMethod(
            "replace requires 'this', oldChar, and newChar".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            let string = heap.get_string(*id)?;
            let old_char_code = args[1].as_int()?;
            let new_char_code = args[2].as_int()?;

            let old_char = char::from_u32(old_char_code as u32).ok_or_else(|| {
                VMError::InternalError(format!("Invalid char code: {}", old_char_code))
            })?;
            let new_char = char::from_u32(new_char_code as u32).ok_or_else(|| {
                VMError::InternalError(format!("Invalid char code: {}", new_char_code))
            })?;

            let replaced = string.replace(old_char, &new_char.to_string());
            let new_ref = heap.intern_string(&replaced)?;
            Ok(Some(Value::Reference(new_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.String.valueOf(I)Ljava/lang/String;
fn string_value_of_int(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "valueOf requires int argument".to_string(),
        ));
    }

    let value = args[0].as_int()?;
    let string_repr = value.to_string();
    let new_ref = heap.intern_string(&string_repr)?;
    Ok(Some(Value::Reference(new_ref)))
}

/// java.lang.String.valueOf(J)Ljava/lang/String;
fn string_value_of_long(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "valueOf requires long argument".to_string(),
        ));
    }

    let value = args[0].as_long()?;
    let string_repr = value.to_string();
    let new_ref = heap.intern_string(&string_repr)?;
    Ok(Some(Value::Reference(new_ref)))
}

/// java.lang.String.valueOf(Z)Ljava/lang/String;
fn string_value_of_boolean(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "valueOf requires boolean argument".to_string(),
        ));
    }

    let value = args[0].as_int()?; // boolean is stored as int
    let string_repr = if value != 0 { "true" } else { "false" };
    let new_ref = heap.intern_string(string_repr)?;
    Ok(Some(Value::Reference(new_ref)))
}

/// java.lang.String.equals(Ljava/lang/Object;)Z
fn string_equals(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "equals requires 'this' and other object".to_string(),
        ));
    }

    match (&args[0], &args[1]) {
        (
            Value::Reference(ObjectRef::Object(this_id)),
            Value::Reference(ObjectRef::Object(other_id)),
        ) => {
            let this_string = heap.get_string(*this_id)?;
            let other_string = heap.get_string(*other_id)?;
            let result = if this_string == other_string { 1 } else { 0 };
            Ok(Some(Value::Int(result)))
        }
        (Value::Reference(ObjectRef::Null), _) => Err(VMError::NullPointerException),
        (_, Value::Reference(ObjectRef::Null)) => {
            // In Java, "string".equals(null) returns false, not NPE
            Ok(Some(Value::Int(0)))
        }
        _ => {
            // If other is not a String object, return false
            Ok(Some(Value::Int(0)))
        }
    }
}

/// java.lang.Object.toString()Ljava/lang/String; - `getClass().getName() + "@" + hex(hashCode())`
fn object_to_string(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let class_name = match args.first() {
        Some(Value::Reference(ObjectRef::Object(id) | ObjectRef::Array(id))) => {
            heap.get_object_class_name(*id)?
        }
        Some(Value::Reference(ObjectRef::Null)) => return Err(VMError::NullPointerException),
        _ => {
            return Err(VMError::NoSuchMethod(
                "toString requires 'this'".to_string(),
            ))
        }
    };
    let Some(Value::Int(hash)) = super::object_hash_code(heap, args)? else {
        return Err(VMError::internal("identity hash code is unavailable"));
    };
    let text = format!("{}@{:x}", class_name.replace('/', "."), hash as u32);
    Ok(Some(Value::Reference(heap.intern_string(&text)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_heap() -> Heap {
        Heap::new(1024 * 1024)
    }

    #[test]
    fn test_string_length() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello").unwrap();
        let result = string_length(&mut heap, &[Value::Reference(test_str)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn test_string_char_at() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello").unwrap();
        let result = string_char_at(&mut heap, &[Value::Reference(test_str), Value::Int(1)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int('e' as i32)));
    }

    #[test]
    fn test_string_char_at_bounds() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello").unwrap();
        let result = string_char_at(&mut heap, &[Value::Reference(test_str), Value::Int(10)]);
        assert!(result.is_err());
    }

    #[test]
    fn utf16_policy_uses_code_units_for_supplementary_characters() {
        let value = "A😀B";
        assert_eq!(java_string_utf16_len(value), 4);
        assert_eq!(java_string_char_at(value, 1).unwrap(), 55_357);
        assert_eq!(java_string_char_at(value, 2).unwrap(), 56_832);
        assert_eq!(java_string_substring(value, 1, 3).unwrap(), "😀");
        assert_eq!(java_string_hash(value), 56_896_350);
        assert_eq!(java_string_compare("😀", "\u{e000}"), -1_987);
        let emoji: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(java_string_index_of(value, &emoji, 0), 1);
        assert_eq!(java_string_index_of(value, &emoji, 2), -1);
        assert_eq!(java_string_index_of(value, &[56_832], 0), 2);
        assert_eq!(java_string_last_index_of(value, &emoji, 1), 1);
        assert_eq!(java_string_last_index_of(value, &emoji, 0), -1);
        assert_eq!(
            java_string_replace_range(value, 3, 4, &['C' as u16]).unwrap(),
            "A😀C"
        );
        assert_eq!(java_string_replace_range(value, 3, 4, &[]).unwrap(), "A😀");
        assert_eq!(
            java_string_replace_range(value, 3, 3, &['X' as u16]).unwrap(),
            "A😀XB"
        );
        assert!(matches!(
            java_string_replace_range(value, 1, 2, &[]),
            Err(VMError::Unimplemented(_))
        ));
    }

    #[test]
    fn test_string_substring_from() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello world").unwrap();
        let result = string_substring_from(&mut heap, &[Value::Reference(test_str), Value::Int(6)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let substr = heap.get_string(id).unwrap();
            assert_eq!(substr, "world");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_substring() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello world").unwrap();
        let result = string_substring(
            &mut heap,
            &[Value::Reference(test_str), Value::Int(0), Value::Int(5)],
        );
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let substr = heap.get_string(id).unwrap();
            assert_eq!(substr, "hello");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_to_upper_case() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello").unwrap();
        let result = string_to_upper_case(&mut heap, &[Value::Reference(test_str)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let upper = heap.get_string(id).unwrap();
            assert_eq!(upper, "HELLO");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_to_lower_case() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("HELLO").unwrap();
        let result = string_to_lower_case(&mut heap, &[Value::Reference(test_str)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let lower = heap.get_string(id).unwrap();
            assert_eq!(lower, "hello");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_trim() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("  hello  ").unwrap();
        let result = string_trim(&mut heap, &[Value::Reference(test_str)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let trimmed = heap.get_string(id).unwrap();
            assert_eq!(trimmed, "hello");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_starts_with() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello world").unwrap();
        let prefix = heap.intern_string("hello").unwrap();
        let result = string_starts_with(
            &mut heap,
            &[Value::Reference(test_str), Value::Reference(prefix)],
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(1))); // true
    }

    #[test]
    fn test_string_ends_with() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello world").unwrap();
        let suffix = heap.intern_string("world").unwrap();
        let result = string_ends_with(
            &mut heap,
            &[Value::Reference(test_str), Value::Reference(suffix)],
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(1))); // true
    }

    #[test]
    fn test_string_replace_char() {
        let mut heap = test_heap();
        let test_str = heap.intern_string("hello").unwrap();
        let result = string_replace_char(
            &mut heap,
            &[
                Value::Reference(test_str),
                Value::Int('l' as i32),
                Value::Int('r' as i32),
            ],
        );
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let replaced = heap.get_string(id).unwrap();
            assert_eq!(replaced, "herro");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_value_of_int() {
        let mut heap = test_heap();
        let result = string_value_of_int(&mut heap, &[Value::Int(42)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let str_repr = heap.get_string(id).unwrap();
            assert_eq!(str_repr, "42");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_value_of_boolean() {
        let mut heap = test_heap();
        let result = string_value_of_boolean(&mut heap, &[Value::Int(1)]);
        assert!(result.is_ok());
        if let Some(Value::Reference(ObjectRef::Object(id))) = result.unwrap() {
            let str_repr = heap.get_string(id).unwrap();
            assert_eq!(str_repr, "true");
        } else {
            panic!("Expected string reference");
        }
    }

    #[test]
    fn test_string_equals_same() {
        let mut heap = test_heap();
        let str1 = heap.intern_string("hello").unwrap();
        let str2 = heap.intern_string("hello").unwrap();
        let result = string_equals(&mut heap, &[Value::Reference(str1), Value::Reference(str2)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(1))); // true
    }

    #[test]
    fn test_string_equals_different() {
        let mut heap = test_heap();
        let str1 = heap.intern_string("hello").unwrap();
        let str2 = heap.intern_string("world").unwrap();
        let result = string_equals(&mut heap, &[Value::Reference(str1), Value::Reference(str2)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(0))); // false
    }

    #[test]
    fn test_string_equals_null() {
        let mut heap = test_heap();
        let str1 = heap.intern_string("hello").unwrap();
        let result = string_equals(
            &mut heap,
            &[Value::Reference(str1), Value::Reference(ObjectRef::Null)],
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(Value::Int(0))); // false (not NPE)
    }
}
