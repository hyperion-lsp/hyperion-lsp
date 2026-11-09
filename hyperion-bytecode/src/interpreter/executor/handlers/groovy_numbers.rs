// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Exact numeric operations used by Groovy operator lowering.

use std::num::NonZeroU64;

use bigdecimal::{BigDecimal, RoundingMode, ToPrimitive, Zero};

use crate::groovy_runtime::{
    BINARY_NUMBER_DESCRIPTOR, COMPARE_NUMBER_DESCRIPTOR, NUMBERS_CLASS, UNARY_NUMBER_DESCRIPTOR,
};
use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::heap::Heap;
use crate::interpreter::value::{ObjectRef, Value};

/// Execute a compiler-emitted Groovy numeric operation.
///
/// Returns `None` for unrelated calls so the ordinary native/user method paths
/// remain authoritative. Groovy `/` uses floating division if either operand is
/// `Float`/`Double`, and exact decimal division for every integral/BigDecimal
/// combination.
pub(crate) fn try_invoke(
    heap: &mut Heap,
    class_name: &str,
    method_name: &str,
    descriptor: &str,
    args: &[Value],
) -> VMResult<Option<Value>> {
    if class_name != NUMBERS_CLASS {
        return Ok(None);
    }

    if method_name == "negate" && descriptor == UNARY_NUMBER_DESCRIPTOR {
        let [operand] = args else {
            return Err(VMError::internal(
                "Groovy numeric negation requires one operand",
            ));
        };
        if is_floating(heap, operand)? {
            let id = heap.allocate_double(-to_f64(heap, operand)?)?;
            return Ok(Some(Value::Reference(ObjectRef::Object(id))));
        }
        let id = heap.allocate_big_decimal(&-to_decimal(heap, operand)?)?;
        return Ok(Some(Value::Reference(ObjectRef::Object(id))));
    }

    if method_name == "compare" && descriptor == COMPARE_NUMBER_DESCRIPTOR {
        let [left, right] = args else {
            return Err(VMError::internal(
                "Groovy numeric comparison requires two operands",
            ));
        };
        let ordering = if is_floating(heap, left)? || is_floating(heap, right)? {
            to_f64(heap, left)?
                .partial_cmp(&to_f64(heap, right)?)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            to_decimal(heap, left)?.cmp(&to_decimal(heap, right)?)
        };
        let value = match ordering {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
        return Ok(Some(Value::Int(value)));
    }

    if descriptor != BINARY_NUMBER_DESCRIPTOR
        || !matches!(
            method_name,
            "plus" | "minus" | "multiply" | "divide" | "remainder"
        )
    {
        return Ok(None);
    }

    let [left, right] = args else {
        return Err(VMError::internal(
            "Groovy numeric operation requires two operands",
        ));
    };

    if is_floating(heap, left)? || is_floating(heap, right)? {
        let left = to_f64(heap, left)?;
        let right = to_f64(heap, right)?;
        let value = match method_name {
            "plus" => left + right,
            "minus" => left - right,
            "multiply" => left * right,
            "divide" => left / right,
            "remainder" => left % right,
            _ => unreachable!(),
        };
        let id = heap.allocate_double(value)?;
        return Ok(Some(Value::Reference(ObjectRef::Object(id))));
    }

    let left = to_decimal(heap, left)?;
    let right = to_decimal(heap, right)?;
    let value = match method_name {
        "plus" => left + right,
        "minus" => left - right,
        "multiply" => left * right,
        "divide" => groovy_decimal_divide(&left, &right)?,
        "remainder" => {
            if right.is_zero() {
                return Err(VMError::ArithmeticException("division by zero".to_string()));
            }
            left % right
        }
        _ => unreachable!(),
    };
    let id = heap.allocate_big_decimal(&value)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

fn is_floating(heap: &Heap, value: &Value) -> VMResult<bool> {
    match value {
        Value::Float(_) | Value::Double(_) => Ok(true),
        Value::Reference(ObjectRef::Object(id)) => Ok(matches!(
            heap.get_object_class_name(*id)?.as_str(),
            "java/lang/Float" | "java/lang/Double"
        )),
        _ => Ok(false),
    }
}

fn to_decimal(heap: &Heap, value: &Value) -> VMResult<BigDecimal> {
    match value {
        Value::Int(value) => Ok(BigDecimal::from(*value)),
        Value::Long(value) => Ok(BigDecimal::from(*value)),
        Value::Reference(ObjectRef::Object(id)) => {
            match heap.get_object_class_name(*id)?.as_str() {
                "java/math/BigDecimal" => heap.get_big_decimal(*id),
                "java/lang/Integer"
                | "java/lang/Short"
                | "java/lang/Byte"
                | "java/lang/Character" => match heap.get_field(*id, 0)? {
                    Some(Value::Int(value)) => Ok(BigDecimal::from(value)),
                    _ => Err(numeric_type_error()),
                },
                "java/lang/Long" => match heap.get_field(*id, 0)? {
                    Some(Value::Long(value)) => Ok(BigDecimal::from(value)),
                    _ => Err(numeric_type_error()),
                },
                _ => Err(numeric_type_error()),
            }
        }
        _ => Err(numeric_type_error()),
    }
}

fn to_f64(heap: &Heap, value: &Value) -> VMResult<f64> {
    match value {
        Value::Int(value) => Ok(f64::from(*value)),
        Value::Long(value) => Ok(*value as f64),
        Value::Float(value) => Ok(f64::from(*value)),
        Value::Double(value) => Ok(*value),
        Value::Reference(ObjectRef::Object(id)) => {
            match heap.get_object_class_name(*id)?.as_str() {
                "java/lang/Integer"
                | "java/lang/Short"
                | "java/lang/Byte"
                | "java/lang/Character" => match heap.get_field(*id, 0)? {
                    Some(Value::Int(value)) => Ok(f64::from(value)),
                    _ => Err(numeric_type_error()),
                },
                "java/lang/Long" => match heap.get_field(*id, 0)? {
                    Some(Value::Long(value)) => Ok(value as f64),
                    _ => Err(numeric_type_error()),
                },
                "java/lang/Float" => match heap.get_field(*id, 0)? {
                    Some(Value::Float(value)) => Ok(f64::from(value)),
                    _ => Err(numeric_type_error()),
                },
                "java/lang/Double" => match heap.get_field(*id, 0)? {
                    Some(Value::Double(value)) => Ok(value),
                    _ => Err(numeric_type_error()),
                },
                "java/math/BigDecimal" => heap
                    .get_big_decimal(*id)?
                    .to_f64()
                    .ok_or_else(numeric_type_error),
                _ => Err(numeric_type_error()),
            }
        }
        _ => Err(numeric_type_error()),
    }
}

fn numeric_type_error() -> VMError {
    VMError::type_mismatch("numeric value", "non-numeric value")
}

/// Groovy first attempts an exact `BigDecimal.divide`. For a recurring result,
/// it uses `max(lhs precision, rhs precision) + 10` significant digits and a
/// minimum scale of 10, rounded half-up.
fn groovy_decimal_divide(left: &BigDecimal, right: &BigDecimal) -> VMResult<BigDecimal> {
    if right.is_zero() {
        return Err(VMError::ArithmeticException("division by zero".to_string()));
    }

    let quotient = left / right;
    if &quotient * right == *left {
        return Ok(quotient.normalized());
    }

    let precision = left
        .decimal_digit_count()
        .max(right.decimal_digit_count())
        .saturating_add(10);
    let precision = NonZeroU64::new(precision).expect("operand precision plus ten is non-zero");
    let scale = left
        .fractional_digit_count()
        .max(right.fractional_digit_count())
        .max(10);
    Ok(quotient
        .with_precision_round(precision, RoundingMode::HalfUp)
        .with_scale_round(scale, RoundingMode::HalfUp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groovy_division_keeps_exact_results_compact() {
        assert_eq!(
            groovy_decimal_divide(&BigDecimal::from(10), &BigDecimal::from(2)).unwrap(),
            BigDecimal::from(5)
        );
    }

    #[test]
    fn groovy_division_rounds_recurring_results_to_its_scale_rule() {
        assert_eq!(
            groovy_decimal_divide(&BigDecimal::from(10), &BigDecimal::from(3))
                .unwrap()
                .to_plain_string(),
            "3.3333333333"
        );
        assert_eq!(
            groovy_decimal_divide(&BigDecimal::from(-10), &BigDecimal::from(3))
                .unwrap()
                .to_plain_string(),
            "-3.3333333333"
        );
    }
}
