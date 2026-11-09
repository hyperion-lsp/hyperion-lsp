// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `java.lang.Math` native registration and implementations.

use super::{Heap, NativeRegistry, VMError, VMResult, Value};

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register("java/lang/Math", "max", "(II)I", math_max_int);
    registry.register("java/lang/Math", "max", "(JJ)J", math_max_long);
    registry.register("java/lang/Math", "max", "(FF)F", math_max_float);
    registry.register("java/lang/Math", "max", "(DD)D", math_max_double);
    registry.register("java/lang/Math", "min", "(II)I", math_min_int);
    registry.register("java/lang/Math", "min", "(JJ)J", math_min_long);
    registry.register("java/lang/Math", "min", "(FF)F", math_min_float);
    registry.register("java/lang/Math", "min", "(DD)D", math_min_double);
    registry.register("java/lang/Math", "abs", "(I)I", math_abs_int);
    registry.register("java/lang/Math", "abs", "(J)J", math_abs_long);
    registry.register("java/lang/Math", "abs", "(F)F", math_abs_float);
    registry.register("java/lang/Math", "abs", "(D)D", math_abs_double);
    registry.register("java/lang/Math", "sqrt", "(D)D", math_sqrt);
    registry.register("java/lang/Math", "pow", "(DD)D", math_pow);
    registry.register("java/lang/Math", "floor", "(D)D", math_floor);
    registry.register("java/lang/Math", "ceil", "(D)D", math_ceil);
    registry.register("java/lang/Math", "round", "(D)J", math_round_double);
    registry.register("java/lang/Math", "round", "(F)I", math_round_float);
    registry.register("java/lang/Math", "random", "()D", math_random);
    registry.register("java/lang/Math", "sin", "(D)D", math_sin);
    registry.register("java/lang/Math", "cos", "(D)D", math_cos);
    registry.register("java/lang/Math", "tan", "(D)D", math_tan);
    registry.register("java/lang/Math", "log", "(D)D", math_log);
    registry.register("java/lang/Math", "exp", "(D)D", math_exp);
    registry.register("java/lang/Math", "log10", "(D)D", math_log10);
    registry.register("java/lang/Math", "cbrt", "(D)D", math_cbrt);
    registry.register("java/lang/Math", "atan", "(D)D", math_atan);
    registry.register("java/lang/Math", "asin", "(D)D", math_asin);
    registry.register("java/lang/Math", "acos", "(D)D", math_acos);
    registry.register("java/lang/Math", "hypot", "(DD)D", math_hypot);
    registry.register("java/lang/Math", "toRadians", "(D)D", math_to_radians);
    registry.register("java/lang/Math", "toDegrees", "(D)D", math_to_degrees);

    registry.register("java/lang/Math", "floorDiv", "(II)I", math_floor_div_int);
    registry.register("java/lang/Math", "floorMod", "(II)I", math_floor_mod_int);
    registry.register("java/lang/Math", "floorDiv", "(JJ)J", math_floor_div_long);
    registry.register("java/lang/Math", "floorMod", "(JJ)J", math_floor_mod_long);
    registry.register("java/lang/Math", "addExact", "(II)I", math_add_exact_int);
}

// ==== java.lang.Math Native Implementations ====

fn int_pair(args: &[Value]) -> VMResult<(i32, i32)> {
    match (args.first(), args.get(1)) {
        (Some(Value::Int(a)), Some(Value::Int(b))) => Ok((*a, *b)),
        _ => Err(VMError::TypeMismatch {
            expected: "int, int".to_string(),
            got: "other".to_string(),
        }),
    }
}

fn long_pair(args: &[Value]) -> VMResult<(i64, i64)> {
    match (args.first(), args.get(1)) {
        (Some(Value::Long(a)), Some(Value::Long(b))) => Ok((*a, *b)),
        _ => Err(VMError::TypeMismatch {
            expected: "long, long".to_string(),
            got: "other".to_string(),
        }),
    }
}

fn floor_div_i32(dividend: i32, divisor: i32) -> VMResult<i32> {
    if divisor == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    if dividend == i32::MIN && divisor == -1 {
        return Ok(i32::MIN);
    }
    let quotient = dividend / divisor;
    let remainder = dividend % divisor;
    Ok(if remainder != 0 && (dividend < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    })
}

fn floor_div_i64(dividend: i64, divisor: i64) -> VMResult<i64> {
    if divisor == 0 {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }
    if dividend == i64::MIN && divisor == -1 {
        return Ok(i64::MIN);
    }
    let quotient = dividend / divisor;
    let remainder = dividend % divisor;
    Ok(if remainder != 0 && (dividend < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    })
}

fn math_floor_div_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (dividend, divisor) = int_pair(args)?;
    Ok(Some(Value::Int(floor_div_i32(dividend, divisor)?)))
}

fn math_floor_mod_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (dividend, divisor) = int_pair(args)?;
    let quotient = floor_div_i32(dividend, divisor)?;
    Ok(Some(Value::Int(
        dividend.wrapping_sub(quotient.wrapping_mul(divisor)),
    )))
}

fn math_floor_div_long(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (dividend, divisor) = long_pair(args)?;
    Ok(Some(Value::Long(floor_div_i64(dividend, divisor)?)))
}

fn math_floor_mod_long(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (dividend, divisor) = long_pair(args)?;
    let quotient = floor_div_i64(dividend, divisor)?;
    Ok(Some(Value::Long(
        dividend.wrapping_sub(quotient.wrapping_mul(divisor)),
    )))
}

fn math_add_exact_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (a, b) = int_pair(args)?;
    a.checked_add(b)
        .map(|sum| Some(Value::Int(sum)))
        .ok_or_else(|| VMError::ArithmeticException("integer overflow".to_string()))
}

/// java.lang.Math.max(II)I - Return the larger of two int values
fn math_max_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Int(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "int".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Int(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "int".to_string(),
                got: "other".to_string(),
            })
        }
    };
    Ok(Some(Value::Int(a.max(b))))
}

/// java.lang.Math.max(JJ)J - Return the larger of two long values
fn math_max_long(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Long(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "long".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Long(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "long".to_string(),
                got: "other".to_string(),
            })
        }
    };
    Ok(Some(Value::Long(a.max(b))))
}

/// java.lang.Math.max(FF)F - Return the larger of two float values
fn math_max_float(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Float(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "float".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Float(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "float".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let result = if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() || b.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else {
        a.max(b)
    };
    Ok(Some(Value::Float(result)))
}

/// java.lang.Math.max(DD)D - Return the larger of two double values
fn math_max_double(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let result = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_positive() || b.is_sign_positive() {
            0.0
        } else {
            -0.0
        }
    } else {
        a.max(b)
    };
    Ok(Some(Value::Double(result)))
}

/// java.lang.Math.min(II)I - Return the smaller of two int values
fn math_min_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Int(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "int".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Int(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "int".to_string(),
                got: "other".to_string(),
            })
        }
    };
    Ok(Some(Value::Int(a.min(b))))
}

/// java.lang.Math.min(JJ)J - Return the smaller of two long values
fn math_min_long(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Long(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "long".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Long(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "long".to_string(),
                got: "other".to_string(),
            })
        }
    };
    Ok(Some(Value::Long(a.min(b))))
}

/// java.lang.Math.min(FF)F - Return the smaller of two float values
fn math_min_float(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Float(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "float".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Float(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "float".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let result = if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() || b.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else {
        a.min(b)
    };
    Ok(Some(Value::Float(result)))
}

/// java.lang.Math.min(DD)D - Return the smaller of two double values
fn math_min_double(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let a = match args.first() {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let b = match args.get(1) {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let result = if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() || b.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else {
        a.min(b)
    };
    Ok(Some(Value::Double(result)))
}

/// java.lang.Math.abs(I)I - Return the absolute value of an int
fn math_abs_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Int(v)) => Ok(Some(Value::Int(v.wrapping_abs()))),
        _ => Err(VMError::TypeMismatch {
            expected: "int".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.abs(J)J - Return the absolute value of a long
fn math_abs_long(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Long(v)) => Ok(Some(Value::Long(v.wrapping_abs()))),
        _ => Err(VMError::TypeMismatch {
            expected: "long".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.abs(F)F - Return the absolute value of a float
fn math_abs_float(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Float(v)) => Ok(Some(Value::Float(v.abs()))),
        _ => Err(VMError::TypeMismatch {
            expected: "float".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.abs(D)D - Return the absolute value of a double
fn math_abs_double(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.abs()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.sqrt(D)D - Return the square root of a double
fn math_sqrt(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.sqrt()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.pow(DD)D - Return the first argument raised to the power of the second
fn math_pow(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let base = match args.first() {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    let exp = match args.get(1) {
        Some(Value::Double(v)) => *v,
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "double".to_string(),
                got: "other".to_string(),
            })
        }
    };
    Ok(Some(Value::Double(base.powf(exp))))
}

/// java.lang.Math.floor(D)D - Return the largest integer less than or equal to argument
fn math_floor(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.floor()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.ceil(D)D - Return the smallest integer greater than or equal to argument
fn math_ceil(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.ceil()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.round(D)J - Round double to long
fn math_round_double(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Long((v + 0.5).floor() as i64))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.round(F)I - Round float to int
fn math_round_float(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Float(v)) => Ok(Some(Value::Int((v + 0.5).floor() as i32))),
        _ => Err(VMError::TypeMismatch {
            expected: "float".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.random()D - Return a pseudorandom double between 0.0 and 1.0
fn math_random(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    // Simple linear congruential generator - not cryptographically secure
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0) as u64;
    // Simple transformation to get a value in [0, 1)
    let random = (seed % 1_000_000_000) as f64 / 1_000_000_000.0;
    Ok(Some(Value::Double(random)))
}

/// java.lang.Math.sin(D)D - Return the sine of an angle in radians
fn math_sin(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.sin()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.cos(D)D - Return the cosine of an angle in radians
fn math_cos(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.cos()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.tan(D)D - Return the tangent of an angle in radians
fn math_tan(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.tan()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.log(D)D - Return the natural logarithm of a double
fn math_log(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.ln()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// java.lang.Math.exp(D)D - Return e raised to the power of a double
fn math_exp(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(v.exp()))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

/// Apply a unary `f64 -> f64` Math function to a single double argument.
fn math_unary(args: &[Value], f: impl Fn(f64) -> f64) -> VMResult<Option<Value>> {
    match args.first() {
        Some(Value::Double(v)) => Ok(Some(Value::Double(f(*v)))),
        _ => Err(VMError::TypeMismatch {
            expected: "double".to_string(),
            got: "other".to_string(),
        }),
    }
}

fn math_log10(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::log10)
}
fn math_cbrt(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::cbrt)
}
fn math_atan(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::atan)
}
fn math_asin(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::asin)
}
fn math_acos(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::acos)
}
fn math_to_radians(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::to_radians)
}
fn math_to_degrees(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    math_unary(args, f64::to_degrees)
}
fn math_hypot(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    match (args.first(), args.get(1)) {
        (Some(Value::Double(a)), Some(Value::Double(b))) => Ok(Some(Value::Double(a.hypot(*b)))),
        _ => Err(VMError::TypeMismatch {
            expected: "double, double".to_string(),
            got: "other".to_string(),
        }),
    }
}
