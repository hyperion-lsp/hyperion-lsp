// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Native implementations for `java.lang.Integer` and `java.lang.Long`.

use super::{Heap, NativeRegistry, ObjectRef, VMError, VMResult, Value};

pub(super) fn register(registry: &mut NativeRegistry) {
    // java.lang.Integer / Long static helpers
    registry.register(
        "java/lang/Integer",
        "parseInt",
        "(Ljava/lang/String;)I",
        integer_parse_int,
    );
    registry.register("java/lang/Integer", "compare", "(II)I", integer_compare);
    registry.register("java/lang/Long", "compare", "(JJ)I", long_compare);
    registry.register(
        "java/lang/Long",
        "parseLong",
        "(Ljava/lang/String;)J",
        long_parse_long,
    );

    // java.lang.Integer / Long radix + bit helpers.
    registry.register(
        "java/lang/Integer",
        "toBinaryString",
        "(I)Ljava/lang/String;",
        |h, a| int_to_radix_string(h, a, 2),
    );
    registry.register(
        "java/lang/Integer",
        "toHexString",
        "(I)Ljava/lang/String;",
        |h, a| int_to_radix_string(h, a, 16),
    );
    registry.register(
        "java/lang/Integer",
        "toOctalString",
        "(I)Ljava/lang/String;",
        |h, a| int_to_radix_string(h, a, 8),
    );
    registry.register(
        "java/lang/Integer",
        "toString",
        "(I)Ljava/lang/String;",
        |h, a| {
            let v = a.first().and_then(|v| v.as_int().ok()).unwrap_or(0);
            Ok(Some(Value::Reference(h.intern_string(&v.to_string())?)))
        },
    );
    registry.register("java/lang/Integer", "bitCount", "(I)I", |_, a| {
        Ok(Some(Value::Int(arg_i32(a).count_ones() as i32)))
    });
    registry.register(
        "java/lang/Integer",
        "numberOfLeadingZeros",
        "(I)I",
        |_, a| Ok(Some(Value::Int(arg_i32(a).leading_zeros() as i32))),
    );
    registry.register(
        "java/lang/Integer",
        "numberOfTrailingZeros",
        "(I)I",
        |_, a| Ok(Some(Value::Int(arg_i32(a).trailing_zeros() as i32))),
    );
    registry.register("java/lang/Integer", "highestOneBit", "(I)I", |_, a| {
        let x = arg_i32(a) as u32;
        let r = if x == 0 {
            0
        } else {
            1u32 << (31 - x.leading_zeros())
        };
        Ok(Some(Value::Int(r as i32)))
    });
    registry.register("java/lang/Integer", "reverse", "(I)I", |_, a| {
        Ok(Some(Value::Int((arg_i32(a) as u32).reverse_bits() as i32)))
    });
    registry.register("java/lang/Integer", "signum", "(I)I", |_, a| {
        Ok(Some(Value::Int(arg_i32(a).signum())))
    });
    registry.register(
        "java/lang/Long",
        "toHexString",
        "(J)Ljava/lang/String;",
        |h, a| {
            let v = arg_i64(a) as u64;
            Ok(Some(Value::Reference(
                h.intern_string(&format!("{:x}", v))?,
            )))
        },
    );
    registry.register(
        "java/lang/Long",
        "toBinaryString",
        "(J)Ljava/lang/String;",
        |h, a| {
            let v = arg_i64(a) as u64;
            Ok(Some(Value::Reference(
                h.intern_string(&format!("{:b}", v))?,
            )))
        },
    );
    registry.register(
        "java/lang/Long",
        "toString",
        "(J)Ljava/lang/String;",
        |h, a| {
            Ok(Some(Value::Reference(
                h.intern_string(&arg_i64(a).to_string())?,
            )))
        },
    );
    registry.register("java/lang/Long", "bitCount", "(J)I", |_, a| {
        Ok(Some(Value::Int((arg_i64(a) as u64).count_ones() as i32)))
    });
    registry.register("java/lang/Long", "signum", "(J)I", |_, a| {
        Ok(Some(Value::Int(arg_i64(a).signum() as i32)))
    });
}

fn arg_i32(args: &[Value]) -> i32 {
    args.first()
        .and_then(|value| value.as_int().ok())
        .unwrap_or(0)
}

fn arg_i64(args: &[Value]) -> i64 {
    match args.first() {
        Some(Value::Long(l)) => *l,
        Some(Value::Int(i)) => *i as i64,
        _ => 0,
    }
}

/// Integer.to{Binary,Hex,Octal}String — Java treats the int as unsigned.
fn int_to_radix_string(heap: &mut Heap, args: &[Value], radix: u32) -> VMResult<Option<Value>> {
    let x = arg_i32(args) as u32;
    let s = match radix {
        2 => format!("{:b}", x),
        8 => format!("{:o}", x),
        16 => format!("{:x}", x),
        _ => x.to_string(),
    };
    Ok(Some(Value::Reference(heap.intern_string(&s)?)))
}

fn integer_compare(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match (args.first(), args.get(1)) {
        (Some(Value::Int(a)), Some(Value::Int(b))) => Ok(Some(Value::Int(a.cmp(b) as i32))),
        _ => Err(VMError::TypeMismatch {
            expected: "int, int".to_string(),
            got: "other".to_string(),
        }),
    }
}

fn long_compare(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match (args.first(), args.get(1)) {
        (Some(Value::Long(a)), Some(Value::Long(b))) => Ok(Some(Value::Int(a.cmp(b) as i32))),
        _ => Err(VMError::TypeMismatch {
            expected: "long, long".to_string(),
            got: "other".to_string(),
        }),
    }
}

fn integer_parse_int(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let s = parse_string_arg(heap, args)?;
    let n = s
        .parse::<i32>()
        .map_err(|_| VMError::NumberFormatException(format!("For input string: \"{s}\"")))?;
    Ok(Some(Value::Int(n)))
}

fn long_parse_long(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let s = parse_string_arg(heap, args)?;
    let n = s
        .parse::<i64>()
        .map_err(|_| VMError::NumberFormatException(format!("For input string: \"{s}\"")))?;
    Ok(Some(Value::Long(n)))
}

/// Read the first argument as a heap String.
fn parse_string_arg(heap: &mut Heap, args: &[Value]) -> VMResult<String> {
    match args.first() {
        Some(Value::Reference(ObjectRef::Object(id))) => heap.get_string(*id),
        Some(Value::Reference(ObjectRef::Null)) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "other".to_string(),
        }),
    }
}
