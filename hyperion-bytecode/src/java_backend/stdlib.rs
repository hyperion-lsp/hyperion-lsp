// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK and functional-interface signature policy for the Java backend.

use super::JavaType;

/// Validate arity where the backend already models a JDK instance signature.
/// `None` means the method is not in this deliberately small signature table
/// and must remain available to external-class descriptor inference.
pub(super) fn stdlib_instance_arity_is_valid(
    owner: &str,
    method: &str,
    argc: usize,
) -> Option<bool> {
    let accepted = match (owner, method) {
        (
            "java/lang/String",
            "length" | "isEmpty" | "hashCode" | "trim" | "strip" | "toLowerCase" | "toUpperCase"
            | "intern" | "toCharArray",
        ) => argc == 0,
        (
            "java/lang/String",
            "charAt"
            | "compareTo"
            | "compareToIgnoreCase"
            | "equals"
            | "equalsIgnoreCase"
            | "contains"
            | "endsWith"
            | "matches"
            | "concat",
        ) => argc == 1,
        ("java/lang/String", "indexOf" | "lastIndexOf" | "substring" | "startsWith") => {
            matches!(argc, 1 | 2)
        }
        ("java/lang/String", "replace") => argc == 2,
        ("java/lang/String", "formatted") => true,
        ("java/lang/StringBuilder", "toString" | "length" | "isEmpty" | "reverse") => argc == 0,
        ("java/lang/StringBuilder", "charAt" | "append" | "deleteCharAt") => argc == 1,
        ("java/lang/StringBuilder", "substring" | "insert" | "delete") => matches!(argc, 1 | 2),
        ("java/lang/StringBuilder", "replace") => argc == 3,
        _ => return None,
    };
    Some(accepted)
}

/// Validate fixed-arity JDK static methods that the backend types explicitly.
pub(super) fn stdlib_static_arity_is_valid(owner: &str, method: &str, argc: usize) -> Option<bool> {
    let accepted = match (owner, method) {
        (
            "java/lang/Math",
            "sqrt" | "cbrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "log" | "log10"
            | "exp" | "floor" | "ceil" | "toRadians" | "toDegrees" | "abs" | "round",
        ) => argc == 1,
        ("java/lang/Math", "pow" | "atan2" | "hypot" | "max" | "min") => argc == 2,
        ("java/lang/Integer" | "java/lang/Long", "parseInt" | "parseUnsignedInt" | "parseLong") => {
            matches!(argc, 1 | 2)
        }
        ("java/lang/Double", "parseDouble")
        | ("java/lang/Float", "parseFloat")
        | ("java/lang/Boolean", "parseBoolean") => argc == 1,
        (
            "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Double"
            | "java/lang/Float"
            | "java/lang/Character"
            | "java/lang/Boolean",
            "compare",
        ) => argc == 2,
        _ => return None,
    };
    Some(accepted)
}

/// Map a simple class name used as a static-call target to its internal name.
pub(super) fn qualify_runtime_class(name: &str) -> String {
    if name.contains('/') {
        return name.to_string();
    }
    match name {
        "Math" => "java/lang/Math".to_string(),
        "System" => "java/lang/System".to_string(),
        "Integer" => "java/lang/Integer".to_string(),
        "Long" => "java/lang/Long".to_string(),
        "Double" => "java/lang/Double".to_string(),
        "Float" => "java/lang/Float".to_string(),
        "Boolean" => "java/lang/Boolean".to_string(),
        "Character" => "java/lang/Character".to_string(),
        "String" => "java/lang/String".to_string(),
        "Object" => "java/lang/Object".to_string(),
        // Fall back to import-aware resolution so a static field/method on an
        // imported type names the fully-qualified owner the VM expects — e.g.
        // `BufferedImage.TYPE_INT_RGB` and `ImageIO.write` (the unit's imports
        // map the simple name to its package). Set during conversion and still
        // valid here since backend compilation runs right after on the same thread.
        other => crate::java_converter::qualify_class_name(other),
    }
}

/// Descriptor for a value in *argument* position. A value can never be `void`,
/// so an unresolved `Void` inference (an unknown call's result used as an
/// argument) is treated as `Object` — emitting `V` for a parameter produces a
/// structurally-invalid descriptor the class loader rejects (ClassFormatError).
pub(super) fn value_descriptor(ty: &JavaType) -> String {
    match ty {
        JavaType::Void => "Ljava/lang/Object;".to_string(),
        _ => ty.descriptor(),
    }
}

/// Return type of a well-known stdlib *instance* method, used when the call is
/// unresolved (the receiver is a stdlib class not in the unit) so the emitted
/// descriptor and the backend's stack tracking are correct (`s.equals(x)`
/// returns boolean, not void; `s.substring(..)` a String, etc.).
pub(super) fn stdlib_instance_return_type(owner: &str, method: &str) -> Option<JavaType> {
    let string_ty = || JavaType::Object("java/lang/String".to_string());
    let throwable_ty = || JavaType::Object("java/lang/Throwable".to_string());
    // The backend has no JDK class hierarchy, so an exception receiver's Throwable methods
    // are answered from the Throwable entries below.
    let owner = if owner == "java/lang/Throwable"
        || owner.ends_with("Exception")
        || owner.ends_with("Error")
    {
        "java/lang/Throwable"
    } else {
        owner
    };
    let ty = match (owner, method) {
        ("java/lang/Throwable", "getMessage" | "getLocalizedMessage" | "toString") => string_ty(),
        ("java/lang/Throwable", "getCause" | "initCause") => throwable_ty(),
        ("java/lang/Throwable", "getSuppressed") => JavaType::Array {
            element_type: Box::new(throwable_ty()),
            dimensions: 1,
        },
        ("java/lang/Throwable", "addSuppressed") => JavaType::Void,
        (
            "java/lang/String",
            "length" | "indexOf" | "lastIndexOf" | "compareTo" | "compareToIgnoreCase" | "hashCode",
        ) => JavaType::Int,
        ("java/lang/String", "charAt") => JavaType::Char,
        (
            "java/lang/String",
            "isEmpty" | "equals" | "equalsIgnoreCase" | "contains" | "startsWith" | "endsWith"
            | "matches",
        ) => JavaType::Boolean,
        (
            "java/lang/String",
            "substring" | "trim" | "strip" | "toLowerCase" | "toUpperCase" | "replace" | "concat"
            | "intern" | "formatted",
        ) => string_ty(),
        ("java/lang/String", "toCharArray") => JavaType::Array {
            element_type: Box::new(JavaType::Char),
            dimensions: 1,
        },
        // Iterable/Iterator (drives enhanced-for lowering and chaining).
        (
            "java/util/ArrayList"
            | "java/util/LinkedList"
            | "java/util/HashSet"
            | "java/util/LinkedHashSet"
            | "java/util/HashMap"
            | "java/util/LinkedHashMap"
            | "java/util/Set"
            | "java/util/List"
            | "java/util/Collection",
            "iterator",
        ) => JavaType::Object("java/util/Iterator".to_string()),
        ("java/util/Iterator", "hasNext") => JavaType::Boolean,
        ("java/util/Iterator", "next") => JavaType::Object("java/lang/Object".to_string()),
        // AWT / image methods.
        ("java/awt/image/BufferedImage", "getRGB" | "getWidth" | "getHeight") => JavaType::Int,
        ("java/awt/image/BufferedImage", "createGraphics" | "getGraphics") => {
            JavaType::Object("java/awt/Graphics2D".to_string())
        }
        ("java/awt/Color", "getRGB" | "getRed" | "getGreen" | "getBlue" | "getAlpha") => {
            JavaType::Int
        }
        ("java/lang/StringBuilder", "toString" | "substring") => string_ty(),
        ("java/lang/StringBuilder", "length") => JavaType::Int,
        ("java/lang/StringBuilder", "charAt") => JavaType::Char,
        ("java/lang/StringBuilder", "isEmpty") => JavaType::Boolean,
        // The mutating, chainable builders return the builder itself.
        (
            "java/lang/StringBuilder",
            "append" | "insert" | "reverse" | "deleteCharAt" | "delete" | "replace",
        ) => JavaType::Object("java/lang/StringBuilder".to_string()),
        // Value-returning java.util collection methods. Methods not listed here
        // (clear/addFirst/sort/forEach/…) are left as None → a `()V` descriptor,
        // so void mutators keep their correct stack effect.
        (o, "size" | "indexOf" | "lastIndexOf" | "hashCode") if is_java_util_container(o) => {
            JavaType::Int
        }
        (
            o,
            "isEmpty" | "contains" | "containsKey" | "containsValue" | "add" | "offer" | "addAll"
            | "equals",
        ) if is_java_util_container(o) => JavaType::Boolean,
        (
            o,
            "get" | "getOrDefault" | "put" | "putIfAbsent" | "remove" | "poll" | "pop" | "peek"
            | "element" | "first" | "last" | "floor" | "ceiling" | "higher" | "lower" | "getFirst"
            | "getLast" | "peekFirst" | "peekLast" | "pollFirst" | "pollLast" | "set" | "keySet"
            | "values" | "entrySet" | "toArray",
        ) if is_java_util_container(o) => JavaType::Object("java/lang/Object".to_string()),
        _ => return None,
    };
    Some(ty)
}

/// True for the java.util container types whose instance methods Hyperion models
/// (used to give their methods sensible return types for descriptor/stack).
fn is_java_util_container(owner: &str) -> bool {
    matches!(
        owner,
        "java/util/ArrayList"
            | "java/util/LinkedList"
            | "java/util/HashSet"
            | "java/util/LinkedHashSet"
            | "java/util/TreeSet"
            | "java/util/HashMap"
            | "java/util/LinkedHashMap"
            | "java/util/TreeMap"
            | "java/util/ArrayDeque"
            | "java/util/PriorityQueue"
            | "java/util/Stack"
            | "java/util/Vector"
            | "java/util/List"
            | "java/util/Set"
            | "java/util/Map"
            | "java/util/Collection"
            | "java/util/Queue"
            | "java/util/Deque"
    )
}

/// Canonical parameter types of a well-known stdlib *static* method whose
/// arguments must be coerced (e.g. `Math.pow(2, 10.0)` needs the int widened to
/// double so the descriptor is `(DD)D`). `None` leaves arguments as inferred.
/// Only methods whose parameter types are fixed (not type-preserving overloads
/// like `Math.abs`/`max`/`min`/`round`) are listed.
pub(super) fn stdlib_static_param_types(
    owner: &str,
    method: &str,
    argc: usize,
) -> Option<Vec<JavaType>> {
    if owner != "java/lang/Math" {
        return None;
    }
    let n = match method {
        "sqrt" | "cbrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "log" | "log10"
        | "exp" | "floor" | "ceil" | "toRadians" | "toDegrees" => 1,
        "pow" | "atan2" | "hypot" => 2,
        _ => return None,
    };
    if argc == n {
        Some(vec![JavaType::Double; n])
    } else {
        None
    }
}

/// Return type of a well-known stdlib *static* method, where the
/// first-argument heuristic would be wrong (e.g. `Integer.parseInt(String)`
/// returns int, not String). `None` falls back to the heuristic.
pub(super) fn stdlib_static_return_type(
    owner: &str,
    method: &str,
    arg_types: &[JavaType],
) -> Option<JavaType> {
    let ty = match (owner, method) {
        ("javax/imageio/ImageIO", "write") => JavaType::Boolean,
        (_, "parseInt") | (_, "parseUnsignedInt") => JavaType::Int,
        (_, "parseLong") => JavaType::Long,
        (_, "parseDouble") => JavaType::Double,
        (_, "parseFloat") => JavaType::Float,
        (_, "parseBoolean") => JavaType::Boolean,
        // X.compare(a, b) and bit utilities return int.
        (
            "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Double"
            | "java/lang/Float"
            | "java/lang/Character"
            | "java/lang/Boolean",
            "compare"
            | "bitCount"
            | "signum"
            | "numberOfLeadingZeros"
            | "numberOfTrailingZeros"
            | "hashCode"
            | "compareTo",
        ) => JavaType::Int,
        // Math.round(double)→long, Math.round(float)→int.
        ("java/lang/Math", "round") => {
            if matches!(arg_types.first(), Some(JavaType::Float)) {
                JavaType::Int
            } else {
                JavaType::Long
            }
        }
        (
            "java/lang/Math",
            "floor" | "ceil" | "sqrt" | "cbrt" | "log" | "log10" | "exp" | "sin" | "cos" | "tan"
            | "asin" | "acos" | "atan" | "atan2" | "toRadians" | "toDegrees" | "hypot" | "pow",
        ) => JavaType::Double,
        // String-producing conversions (valueOf is omitted — its return type is
        // the boxed type / context-dependent and handled by a dedicated native).
        (_, "toString" | "toBinaryString" | "toHexString" | "toOctalString") => {
            JavaType::Object("java/lang/String".to_string())
        }
        _ => return None,
    };
    Some(ty)
}

/// The type of a well-known stdlib static field constant (the ones the corpus
/// actually reads). `None` for anything else.
pub(super) fn stdlib_static_field_type(owner: &str, field: &str) -> Option<JavaType> {
    let ty = match (owner, field) {
        ("java/lang/Integer", "MAX_VALUE" | "MIN_VALUE" | "SIZE" | "BYTES") => JavaType::Int,
        ("java/lang/Long", "MAX_VALUE" | "MIN_VALUE") => JavaType::Long,
        ("java/lang/Long", "SIZE" | "BYTES") => JavaType::Int,
        (
            "java/lang/Short" | "java/lang/Byte" | "java/lang/Character",
            "MAX_VALUE" | "MIN_VALUE",
        ) => JavaType::Int,
        ("java/lang/Float", "MAX_VALUE" | "MIN_VALUE") => JavaType::Float,
        (
            "java/lang/Double",
            "MAX_VALUE" | "MIN_VALUE" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" | "NaN",
        ) => JavaType::Double,
        ("java/lang/Math", "PI" | "E") => JavaType::Double,
        _ => return None,
    };
    Some(ty)
}

pub(super) fn is_functional_sam(owner: &str, method: &str) -> bool {
    owner.starts_with("java/util/function/")
        || matches!(
            method,
            "apply"
                | "test"
                | "get"
                | "call"
                | "compare"
                | "applyAsInt"
                | "applyAsLong"
                | "applyAsDouble"
        )
}

pub(super) fn known_static_method_reference(
    owner: &str,
    method: &str,
    params: &[JavaType],
) -> bool {
    if stdlib_static_return_type(owner, method, params).is_some() {
        return true;
    }
    matches!(
        (owner, method),
        (
            "java/lang/Math",
            "abs" | "max" | "min" | "signum" | "copySign"
        ) | (
            "java/lang/String",
            "valueOf" | "format" | "join" | "copyValueOf"
        )
    )
}

pub(super) fn static_reference_uses_primitive_params(owner: &str) -> bool {
    matches!(
        owner,
        "java/lang/Math"
            | "java/lang/Integer"
            | "java/lang/Long"
            | "java/lang/Short"
            | "java/lang/Byte"
            | "java/lang/Double"
            | "java/lang/Float"
            | "java/lang/Character"
            | "java/lang/Boolean"
    )
}
