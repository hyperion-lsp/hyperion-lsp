// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Three-valued Java conversion rules (JLS 5.1-5.3).
//!
//! A compile error is only reported when a conversion is *definitely*
//! impossible. Anything the analysis cannot prove -- unresolved classes,
//! generic type variables, types that may be inference fallbacks -- is
//! [`Convertibility::Maybe`], which callers must treat exactly like `Yes`.
//! Type arguments are deliberately ignored: generic inference is where a
//! syntax-directed engine is least reliable, and the erased relation already
//! catches the errors users actually make.

use super::facts::TypeFacts;
use crate::symbol::{PrimitiveType, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Convertibility {
    Yes,
    No,
    Maybe,
}

/// Which conversion context applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// Method/constructor argument to formal parameter (JLS 5.3, loose).
    Invocation,
    /// As [`Self::Invocation`], comparing type arguments too (see
    /// [`Self::AssignmentWithTypeArguments`]).
    InvocationWithTypeArguments,
    /// `T x = expr;` / `x = expr;` (JLS 5.2). Adds constant narrowing, which
    /// is undecidable without constant evaluation, so it is `Maybe`.
    Assignment,
    /// As [`Self::Assignment`], and type arguments are compared too. Only for
    /// sources whose type arguments are written or declared (a variable, a
    /// `new X<A>()`, a call to a non-generic method), never for the result of
    /// generic-method inference, where this engine falls back to `Object`.
    AssignmentWithTypeArguments,
}

/// Is a value of type `source` convertible to `target`?
pub fn convertible(
    facts: &TypeFacts<'_>,
    source: &Type,
    target: &Type,
    context: Context,
) -> Convertibility {
    use Convertibility::{Maybe, No, Yes};
    match (source, target) {
        (Type::Unknown, _) | (_, Type::Unknown) => Maybe,
        // Void is not a value type; other checks own that error.
        (Type::Void, _) | (_, Type::Void) => Maybe,
        // A type variable is a subtype of each of its bounds (JLS 4.4): it is
        // convertible when any bound is, and provably not when every bound
        // is provably not. An unbounded variable proves nothing.
        (Type::TypeVariable { bounds, .. }, target)
            if !bounds.is_empty()
                && !matches!(
                    target,
                    Type::TypeVariable { .. } | Type::Wildcard { .. } | Type::Intersection { .. }
                ) =>
        {
            let mut verdict = No;
            for bound in bounds {
                match convertible(facts, bound, target, context) {
                    Yes => return Yes,
                    Maybe => verdict = Maybe,
                    No => {}
                }
            }
            verdict
        }
        (Type::TypeVariable { .. } | Type::Wildcard { .. } | Type::Intersection { .. }, _)
        | (_, Type::TypeVariable { .. } | Type::Wildcard { .. } | Type::Intersection { .. }) => {
            Maybe
        }
        (Type::Union { .. }, _) | (_, Type::Union { .. }) => Maybe,

        (Type::Null, Type::Primitive(_)) => No,
        (Type::Null, _) => Yes,
        (_, Type::Null) => Maybe,

        (Type::Primitive(from), Type::Primitive(to)) => primitive_to_primitive(*from, *to, context),

        (Type::Primitive(from), Type::Class(target_class)) => {
            let wrapper = format!("java.lang.{}", from.wrapper_class());
            let target_fqn = target_class.raw_qualified_name();
            // Assignment allows `Byte b = 1;` (constant narrowing then boxing).
            if matches!(
                context,
                Context::Assignment | Context::AssignmentWithTypeArguments
            ) && matches!(
                target_fqn.as_str(),
                "java.lang.Byte" | "java.lang.Short" | "java.lang.Character"
            ) && matches!(
                from,
                PrimitiveType::Int
                    | PrimitiveType::Short
                    | PrimitiveType::Char
                    | PrimitiveType::Byte
            ) {
                return Maybe;
            }
            reference_subtype(facts, &wrapper, &target_fqn)
        }

        (Type::Class(source_class), Type::Primitive(to)) => {
            let source_fqn = source_class.raw_qualified_name();
            // `Object`/`Number`/`Comparable` reaching a primitive is what an
            // inference fallback looks like; never accuse on that evidence.
            if let Some(unboxed) = unboxed_primitive(&source_fqn) {
                return primitive_to_primitive(unboxed, *to, Context::Invocation);
            }
            if source_fqn == "java.lang.Object" || !facts.is_known_type(&source_fqn) {
                return Maybe;
            }
            match facts.supertypes(&source_fqn) {
                // A known non-wrapper class can never unbox.
                Some(_) => No,
                None => Maybe,
            }
        }

        (Type::Class(source_class), Type::Class(target_class)) => {
            let erased = reference_subtype(
                facts,
                &source_class.raw_qualified_name(),
                &target_class.raw_qualified_name(),
            );
            if erased == Yes
                && matches!(
                    context,
                    Context::AssignmentWithTypeArguments | Context::InvocationWithTypeArguments
                )
            {
                return type_arguments_verdict(facts, source_class, target_class);
            }
            erased
        }

        (Type::Array { .. }, Type::Class(target_class)) => {
            match target_class.raw_qualified_name().as_str() {
                "java.lang.Object" | "java.lang.Cloneable" | "java.io.Serializable" => Yes,
                other if facts.is_known_type(other) => No,
                _ => Maybe,
            }
        }

        (Type::Class(source_class), Type::Array { .. }) => {
            // Only a proven, non-Object class is certainly not an array.
            let source_fqn = source_class.raw_qualified_name();
            if source_fqn == "java.lang.Object" || !facts.is_known_type(&source_fqn) {
                Maybe
            } else {
                No
            }
        }

        (Type::Primitive(_), Type::Array { .. }) | (Type::Array { .. }, Type::Primitive(_)) => No,

        (
            Type::Array {
                element_type: source_element,
                dimensions: source_dimensions,
            },
            Type::Array {
                element_type: target_element,
                dimensions: target_dimensions,
            },
        ) => array_to_array(
            facts,
            source_element,
            *source_dimensions,
            target_element,
            *target_dimensions,
        ),
    }
}

fn primitive_to_primitive(
    from: PrimitiveType,
    to: PrimitiveType,
    context: Context,
) -> Convertibility {
    use Convertibility::{Maybe, No, Yes};
    if from == to || to.is_widening_from(from) {
        return Yes;
    }
    if from == PrimitiveType::Boolean || to == PrimitiveType::Boolean {
        return No;
    }
    // Narrowing. `byte b = 1; char c = 65;` are legal assignments of constants
    // of type int/short/char/byte; method invocation never narrows.
    if matches!(
        context,
        Context::Assignment | Context::AssignmentWithTypeArguments
    ) && matches!(
        from,
        PrimitiveType::Int | PrimitiveType::Short | PrimitiveType::Char | PrimitiveType::Byte
    ) && matches!(
        to,
        PrimitiveType::Byte | PrimitiveType::Short | PrimitiveType::Char
    ) {
        return Maybe;
    }
    No
}

fn array_to_array(
    facts: &TypeFacts<'_>,
    source_element: &Type,
    source_dimensions: u8,
    target_element: &Type,
    target_dimensions: u8,
) -> Convertibility {
    use Convertibility::{Maybe, No, Yes};
    if source_dimensions == target_dimensions {
        return match (source_element, target_element) {
            (Type::Primitive(a), Type::Primitive(b)) => {
                if a == b {
                    Yes
                } else {
                    No
                }
            }
            (Type::Primitive(_), Type::Class(_)) | (Type::Class(_), Type::Primitive(_)) => No,
            _ => convertible(facts, source_element, target_element, Context::Invocation),
        };
    }
    // `int[][]` -> `Object[]` and similar depth changes are legal only when
    // the shallower side is an Object/Cloneable/Serializable array.
    if source_dimensions > target_dimensions {
        if let Type::Class(target_class) = target_element {
            if matches!(
                target_class.raw_qualified_name().as_str(),
                "java.lang.Object" | "java.lang.Cloneable" | "java.io.Serializable"
            ) {
                return Yes;
            }
        }
        return Maybe;
    }
    Maybe
}

/// Erased class-to-class conversion.
fn reference_subtype(facts: &TypeFacts<'_>, source_fqn: &str, target_fqn: &str) -> Convertibility {
    use Convertibility::{Maybe, No, Yes};
    if source_fqn == target_fqn || target_fqn == "java.lang.Object" {
        return Yes;
    }
    // Only accuse on classes the index really contains; a bare `T` or an
    // unresolved spelling is a type variable or a missing dependency.
    if !facts.is_known_type(source_fqn) || !facts.is_known_type(target_fqn) {
        return Maybe;
    }
    // `java.lang.Object` sources are inference fallbacks far more often than
    // user intent, and other checks (missing symbols) own the real cases.
    if source_fqn == "java.lang.Object" {
        return Maybe;
    }
    match facts.is_subtype(source_fqn, target_fqn) {
        Some(true) => Yes,
        Some(false) => No,
        None => Maybe,
    }
}

fn unboxed_primitive(fqn: &str) -> Option<PrimitiveType> {
    Some(match fqn {
        "java.lang.Boolean" => PrimitiveType::Boolean,
        "java.lang.Byte" => PrimitiveType::Byte,
        "java.lang.Short" => PrimitiveType::Short,
        "java.lang.Integer" => PrimitiveType::Int,
        "java.lang.Long" => PrimitiveType::Long,
        "java.lang.Character" => PrimitiveType::Char,
        "java.lang.Float" => PrimitiveType::Float,
        "java.lang.Double" => PrimitiveType::Double,
        _ => return None,
    })
}

/// Invariant comparison of type arguments, once the erased types are related.
///
/// Deliberately narrow: only top-level arguments that are plain, known
/// classes are compared, and a source argument of `java.lang.Object` is never
/// evidence (it is what unresolved inference produces). Anything involving
/// wildcards, type variables, nested parameterizations or unknown classes is
/// `Maybe`.
fn type_arguments_verdict(
    facts: &TypeFacts<'_>,
    source: &crate::symbol::ClassType,
    target: &crate::symbol::ClassType,
) -> Convertibility {
    // A raw or diamond source is not an error source (unchecked conversion /
    // inference), but its *supertypes* may still fix an argument:
    // `class Fixed<T> implements Box<Other>` is never a `Box<Item>`.
    if target.type_arguments.is_empty() || target.is_diamond {
        return Convertibility::Maybe;
    }
    let source_fqn = source.raw_qualified_name();
    let target_fqn = target.raw_qualified_name();
    let projected: Vec<Option<Type>> = if source_fqn == target_fqn {
        source.type_arguments.iter().cloned().map(Some).collect()
    } else {
        let arguments: Vec<String> = source
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let Some(projected) =
            facts
                .index()
                .project_owner_type_arguments(&source_fqn, &arguments, &target_fqn)
        else {
            return Convertibility::Maybe;
        };
        projected
            .into_iter()
            .map(|(_, argument)| {
                argument
                    .as_deref()
                    .and_then(crate::type_parser::parse_type_string)
            })
            .collect()
    };
    if projected.len() != target.type_arguments.len() {
        return Convertibility::Maybe;
    }
    for (wanted, actual) in target.type_arguments.iter().zip(&projected) {
        let Some(actual) = actual else { continue };
        if let (Type::Class(wanted), Type::Class(actual)) = (wanted, actual) {
            if !wanted.type_arguments.is_empty() || !actual.type_arguments.is_empty() {
                continue;
            }
            let wanted_fqn = wanted.raw_qualified_name();
            let actual_fqn = actual.raw_qualified_name();
            if actual_fqn == "java.lang.Object"
                || wanted_fqn == actual_fqn
                || !facts.is_known_type(&wanted_fqn)
                || !facts.is_known_type(&actual_fqn)
            {
                continue;
            }
            return Convertibility::No;
        }
    }
    Convertibility::Maybe
}
