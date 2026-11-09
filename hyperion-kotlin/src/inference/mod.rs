// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Inference Engine for Kotlin Generics
//!
//! This module provides type unification and inference for generic function calls.
//! It analyzes argument types and resolves type variables to concrete types.

use crate::stdlib::{GenericFunctionSignature, ParameterSig, TypeExpr, STDLIB};
use crate::types::{KotlinType, Nullability};
use crate::user_registry::UserGenericRegistry;
use std::collections::HashMap;

/// Type unifier that collects constraints and solves for type variables
#[derive(Debug, Default)]
pub struct TypeUnifier {
    /// Mapping from type variable names to inferred concrete types
    substitutions: HashMap<String, KotlinType>,
}

impl TypeUnifier {
    /// Create a new type unifier
    pub fn new() -> Self {
        TypeUnifier {
            substitutions: HashMap::new(),
        }
    }

    /// Unify an expected type expression with an actual concrete type
    ///
    /// This binds type variables in the expected expression to the concrete type.
    pub fn unify(&mut self, expected: &TypeExpr, actual: &KotlinType) -> Result<(), UnifyError> {
        match expected {
            TypeExpr::TypeVar(name) => {
                if let Some(existing) = self.substitutions.get(name) {
                    // Type variable already bound - check compatibility
                    if !Self::types_compatible(existing, actual) {
                        return Err(UnifyError::TypeMismatch {
                            expected: existing.clone(),
                            actual: actual.clone(),
                        });
                    }
                    // Find common supertype (simplified: just use existing)
                } else {
                    // Bind type variable to concrete type
                    self.substitutions.insert(name.clone(), actual.clone());
                }
                Ok(())
            }

            TypeExpr::Generic(expected_name, expected_args) => {
                // For generic types, recursively unify type arguments
                match actual {
                    KotlinType::Class {
                        fqn,
                        type_arguments,
                        ..
                    } => {
                        // Check that the class names match (ignoring FQN for now)
                        let actual_simple = fqn.rsplit('.').next().unwrap_or(fqn);
                        if expected_name != actual_simple {
                            return Err(UnifyError::TypeMismatch {
                                expected: KotlinType::simple(expected_name),
                                actual: actual.clone(),
                            });
                        }

                        // Unify type arguments
                        for (exp_arg, act_arg) in expected_args.iter().zip(type_arguments.iter()) {
                            if let crate::types::TypeArgument::Type(act_type) = act_arg {
                                self.unify(exp_arg, act_type)?;
                            }
                            // Skip star projections and projected types for now
                        }
                        Ok(())
                    }
                    _ => Err(UnifyError::CannotUnify {
                        expected: expected.clone(),
                        actual: actual.clone(),
                    }),
                }
            }

            TypeExpr::Concrete(name) => {
                // A concrete type binds no variable, but it must name the actual class.
                // Subtypes are not modelled, so a mismatch means only "not this overload".
                let simple = |name: &str| name.rsplit('.').next().unwrap_or(name).to_string();
                match actual {
                    KotlinType::Class { fqn, .. }
                        if simple(name) != "Any" && simple(name) != simple(fqn) =>
                    {
                        Err(UnifyError::TypeMismatch {
                            expected: KotlinType::simple(name),
                            actual: actual.clone(),
                        })
                    }
                    _ => Ok(()),
                }
            }

            TypeExpr::Nullable(inner) => {
                // For nullable types, unify the inner type
                match actual {
                    KotlinType::Class { nullability, .. }
                        if *nullability == crate::types::Nullability::Nullable =>
                    {
                        // Actual is nullable - unify inner
                        self.unify(inner, actual)
                    }
                    _ => {
                        // Non-nullable actual can also match nullable expected
                        self.unify(inner, actual)
                    }
                }
            }

            TypeExpr::Function { params, ret } => {
                // Unify function types
                match actual {
                    KotlinType::Function {
                        parameters,
                        return_type,
                        ..
                    } => {
                        // Unify parameters
                        for (exp_param, act_param) in params.iter().zip(parameters.iter()) {
                            self.unify(exp_param, act_param)?;
                        }
                        // Unify return type
                        self.unify(ret, return_type)?;
                        Ok(())
                    }
                    _ => Err(UnifyError::CannotUnify {
                        expected: expected.clone(),
                        actual: actual.clone(),
                    }),
                }
            }
        }
    }

    /// Check if two types are compatible (simplified version)
    fn types_compatible(a: &KotlinType, b: &KotlinType) -> bool {
        // Simplified: check if they're structurally equal
        // A full implementation would check subtype relationships
        a == b
            || matches!(a, KotlinType::Unknown)
            || matches!(b, KotlinType::Unknown)
            || matches!(a, KotlinType::Any)
            || matches!(b, KotlinType::Any)
    }

    /// Get the current substitutions
    pub const fn substitutions(&self) -> &HashMap<String, KotlinType> {
        &self.substitutions
    }

    /// Apply substitutions to a type expression to get a concrete type
    pub fn apply(&self, expr: &TypeExpr) -> KotlinType {
        expr.to_kotlin_type(&self.substitutions)
    }
}

/// Errors that can occur during unification
#[derive(Debug, Clone)]
pub enum UnifyError {
    /// Type mismatch between expected and actual
    TypeMismatch {
        expected: KotlinType,
        actual: KotlinType,
    },
    /// Cannot unify the given types
    CannotUnify {
        expected: TypeExpr,
        actual: KotlinType,
    },
}

/// Infer the return type of a generic function call
///
/// Given a function call and its argument types, resolves type variables
/// and returns the concrete return type.
pub fn infer_generic_call(function_name: &str, arg_types: &[KotlinType]) -> Option<KotlinType> {
    infer_generic_call_with_type_arguments(function_name, &[], arg_types)
}

/// [`infer_generic_call`] for a call that writes type arguments.
///
/// Each of the function's own type parameters is bound to its argument in
/// `type_arguments`, in declaration order, before the value arguments are
/// read, and keeps it whatever they give, so `emptyList<String>()` is a
/// `List<String>`. Only an overload with one type parameter for each argument
/// is read; a call that writes none reads every overload, as
/// [`infer_generic_call`] does.
pub fn infer_generic_call_with_type_arguments(
    function_name: &str,
    type_arguments: &[KotlinType],
    arg_types: &[KotlinType],
) -> Option<KotlinType> {
    // Look up function signature in stdlib
    let sigs = STDLIB.lookup(function_name)?;

    // The first overload whose arguments all fit. Failing that, the first one that
    // takes this many arguments, with whatever types it can bind.
    let bind = |sig: &GenericFunctionSignature, strict: bool| {
        let mut unifier = explicitly_bound(sig, type_arguments)?;
        if !strict && !accepts_arity(sig, arg_types.len()) {
            return None;
        }
        unify_parameters(&mut unifier, sig, arg_types, strict).then_some(unifier)
    };
    let (sig, mut unifier) = sigs
        .iter()
        .find_map(|sig| bind(sig, true).map(|unifier| (sig, unifier)))
        .or_else(|| {
            sigs.iter()
                .find_map(|sig| bind(sig, false).map(|unifier| (sig, unifier)))
        })?;
    rebind_explicit(&mut unifier, sig, type_arguments);
    Some(unifier.apply(&sig.return_type))
}

/// A unifier with each of `sig`'s type parameters bound, in declaration order,
/// to the argument the call writes for it. None when the call writes type
/// arguments and not one for each of the type parameters. A call that writes
/// none binds nothing.
fn explicitly_bound(
    sig: &GenericFunctionSignature,
    type_arguments: &[KotlinType],
) -> Option<TypeUnifier> {
    if !type_arguments.is_empty() && type_arguments.len() != sig.type_parameters.len() {
        return None;
    }
    let mut unifier = TypeUnifier::new();
    rebind_explicit(&mut unifier, sig, type_arguments);
    Some(unifier)
}

/// Binds each of `sig`'s type parameters to the argument the call writes for
/// it, replacing whatever the value arguments bound it to: a written type
/// argument is what the parameter is.
fn rebind_explicit(
    unifier: &mut TypeUnifier,
    sig: &GenericFunctionSignature,
    type_arguments: &[KotlinType],
) {
    for (parameter, argument) in sig.type_parameters.iter().zip(type_arguments) {
        unifier
            .substitutions
            .insert(parameter.name.clone(), argument.clone());
    }
}

/// Whether a signature can take `arg_count` arguments. A vararg parameter takes
/// any number of arguments beyond the fixed ones.
fn accepts_arity(sig: &GenericFunctionSignature, arg_count: usize) -> bool {
    let fixed = sig
        .parameters
        .iter()
        .filter(|param| matches!(param, ParameterSig::Single(_)))
        .count();
    if sig
        .parameters
        .iter()
        .any(|param| matches!(param, ParameterSig::Vararg(_)))
    {
        arg_count >= fixed
    } else {
        arg_count == fixed
    }
}

/// Infer return type from a function signature and argument types
pub fn infer_from_signature(
    sig: &GenericFunctionSignature,
    arg_types: &[KotlinType],
) -> Option<KotlinType> {
    let mut unifier = TypeUnifier::new();
    unify_parameters(&mut unifier, sig, arg_types, false);

    // Apply substitutions to get concrete return type
    Some(unifier.apply(&sig.return_type))
}

/// Infer the return type of a method call on a receiver
///
/// Given a receiver type and method name, resolves the return type
/// with proper generic substitution.
pub fn infer_method_return_type(
    receiver_type: &KotlinType,
    method_name: &str,
    arg_types: &[KotlinType],
) -> Option<KotlinType> {
    infer_method_return_type_with_type_arguments(receiver_type, method_name, &[], arg_types)
}

/// [`infer_method_return_type`] for a call that writes type arguments.
///
/// The extension's own type parameters are bound to `type_arguments` as
/// [`infer_generic_call_with_type_arguments`] binds them: on a
/// `List<String>`, `filterIsInstance<Int>()`, declared `fun <R>
/// List<*>.filterIsInstance(): List<R>`, is a `List<Int>`.
pub fn infer_method_return_type_with_type_arguments(
    receiver_type: &KotlinType,
    method_name: &str,
    type_arguments: &[KotlinType],
    arg_types: &[KotlinType],
) -> Option<KotlinType> {
    // Look up method signature
    let sigs = STDLIB.lookup(method_name)?;
    let extensions: Vec<&GenericFunctionSignature> =
        sigs.iter().filter(|sig| sig.is_extension).collect();

    // The first extension overload whose receiver and every argument fit. Failing that,
    // the first whose receiver fits, binding what it can.
    let bind = |strict: bool| {
        extensions.iter().find_map(|sig| {
            bind_extension(sig, receiver_type, type_arguments, arg_types, strict)
                .map(|unifier| (*sig, unifier))
        })
    };
    let (sig, unifier) = bind(true).or_else(|| bind(false))?;
    Some(unifier.apply(&sig.return_type))
}

/// Binds an extension overload to a receiver and arguments, its type parameters
/// first to the type arguments the call writes. The receiver must unify. The
/// arguments must too when `strict`; otherwise mismatches are ignored.
fn bind_extension(
    sig: &GenericFunctionSignature,
    receiver_type: &KotlinType,
    type_arguments: &[KotlinType],
    arg_types: &[KotlinType],
    strict: bool,
) -> Option<TypeUnifier> {
    let receiver_expr = sig.receiver_type.as_ref()?;
    let mut unifier = explicitly_bound(sig, type_arguments)?;
    unifier.unify(receiver_expr, receiver_type).ok()?;
    if !unify_parameters(&mut unifier, sig, arg_types, strict) {
        return None;
    }
    rebind_explicit(&mut unifier, sig, type_arguments);
    Some(unifier)
}

/// The common supertype of two arguments that bind one type variable. The same class
/// with different nullability is that class, nullable if either side is. A `null`
/// (`Nothing?`) takes the other type in its nullable form, so `listOf(null, 1)` is
/// `List<Int?>`. Other mismatches widen to `Any`.
fn common_supertype(existing: &KotlinType, arg: &KotlinType) -> KotlinType {
    if is_null_type(existing) {
        return arg.clone().nullable();
    }
    if is_null_type(arg) {
        return existing.clone().nullable();
    }
    match (existing, arg) {
        (
            KotlinType::Class {
                fqn: existing_fqn,
                type_arguments: existing_args,
                nullability: existing_nullability,
            },
            KotlinType::Class {
                fqn: arg_fqn,
                type_arguments: arg_args,
                nullability: arg_nullability,
            },
        ) if existing_fqn == arg_fqn && existing_args == arg_args => {
            if *existing_nullability == Nullability::Nullable
                || *arg_nullability == Nullability::Nullable
            {
                existing.clone().nullable()
            } else {
                existing.clone()
            }
        }
        _ => KotlinType::simple("kotlin.Any"),
    }
}

/// Whether a type is the type of `null`: a nullable `Nothing`.
fn is_null_type(ty: &KotlinType) -> bool {
    matches!(
        ty,
        KotlinType::Class { fqn, nullability: Nullability::Nullable, .. }
            if fqn.as_str() == "Nothing" || fqn.as_str() == "kotlin.Nothing"
    )
}

/// Unifies the parameters of `sig` with `arg_types`. In strict mode an arity or type
/// mismatch returns false. Otherwise mismatches are ignored, as best-effort inference.
fn unify_parameters(
    unifier: &mut TypeUnifier,
    sig: &GenericFunctionSignature,
    arg_types: &[KotlinType],
    strict: bool,
) -> bool {
    if strict && !accepts_arity(sig, arg_types.len()) {
        return false;
    }
    let mut arg_iter = arg_types.iter();
    for param_sig in &sig.parameters {
        let (expr, args): (_, Vec<&KotlinType>) = match param_sig {
            ParameterSig::Single(type_expr) => (type_expr, arg_iter.next().into_iter().collect()),
            ParameterSig::Vararg(element_expr) => (element_expr, arg_iter.by_ref().collect()),
        };
        for arg_type in args {
            if unifier.unify(expr, arg_type).is_err() {
                if strict {
                    return false;
                }
                // Arguments of different types bound one type variable. The variable
                // takes their common supertype.
                if let TypeExpr::TypeVar(name) = expr {
                    let widened = unifier.substitutions.get(name).map_or_else(
                        || arg_type.clone(),
                        |existing| common_supertype(existing, arg_type),
                    );
                    unifier.substitutions.insert(name.clone(), widened);
                }
            }
        }
    }
    true
}

/// Infer the return type of a method call with support for user-defined generics
///
/// This function first tries the user registry for user-defined generic classes,
/// then falls back to the stdlib registry for standard library functions.
///
/// # Arguments
///
/// * `receiver_type` - The type of the receiver (e.g., Repository<User>)
/// * `method_name` - The method being called (e.g., "findById")
/// * `arg_types` - Types of the arguments passed to the method
/// * `user_registry` - Optional registry of user-defined generic classes
///
/// # Returns
///
/// The inferred return type with type variables substituted, or None if
/// the method cannot be resolved
///
/// # Example
///
/// ```ignore
/// // For: val repo = Repository<User>()
/// //      val user = repo.findById(1)
/// // With Repository<T> { fun findById(id: Int): T }
///
/// let repo_type = KotlinType::generic("com.example.Repository", vec![KotlinType::simple("User")]);
/// let result = infer_method_return_type_with_registry(
///     &repo_type,
///     "findById",
///     &[KotlinType::int()],
///     Some(&user_registry),
/// );
/// // Returns Some(KotlinType::simple("User"))
/// ```
pub fn infer_method_return_type_with_registry(
    receiver_type: &KotlinType,
    method_name: &str,
    arg_types: &[KotlinType],
    user_registry: Option<&UserGenericRegistry>,
) -> Option<KotlinType> {
    // First try user-defined generic registry
    if let Some(registry) = user_registry {
        if let Some(result) = registry.infer_method_return_type(receiver_type, method_name) {
            return Some(result);
        }
    }

    // Fall back to stdlib registry
    infer_method_return_type(receiver_type, method_name, arg_types)
}

// ============================================================================
// Lambda Parameter Type Inference
// ============================================================================

/// Infer the type of the implicit `it` parameter in a lambda expression
///
/// When a lambda has a single parameter and doesn't declare it explicitly,
/// Kotlin uses `it` as the implicit parameter name. This function determines
/// the type of `it` based on the context where the lambda is used.
///
/// # Arguments
///
/// * `receiver_type` - The type of the receiver (e.g., List<User>)
/// * `method_name` - The method being called (e.g., "filter", "map")
///
/// # Returns
///
/// The inferred type for `it`, or None if it cannot be determined
///
/// # Examples
///
/// ```ignore
/// // list.filter { it > 0 } where list: List<Int>
/// // -> it: Int
/// infer_lambda_it_type(&list_int_type, "filter") // returns Some(Int)
///
/// // users.map { it.name } where users: List<User>
/// // -> it: User
/// infer_lambda_it_type(&list_user_type, "map") // returns Some(User)
/// ```
pub fn infer_lambda_it_type(receiver_type: &KotlinType, method_name: &str) -> Option<KotlinType> {
    // Look up the method signature in stdlib
    let sigs = STDLIB.lookup(method_name)?;

    // Find a matching extension function
    for sig in sigs {
        if !sig.is_extension {
            continue;
        }

        if let Some(receiver_expr) = &sig.receiver_type {
            let mut unifier = TypeUnifier::new();

            // Unify receiver to bind type variables (e.g., T in List<T>)
            if unifier.unify(receiver_expr, receiver_type).is_ok() {
                // Look for the first lambda parameter in the signature
                for param_sig in &sig.parameters {
                    if let ParameterSig::Single(TypeExpr::Function { params, .. }) = param_sig {
                        // The lambda's first parameter type is what `it` should be
                        if let Some(first_param) = params.first() {
                            return Some(unifier.apply(first_param));
                        }
                    }
                }
            }
        }
    }

    // Fallback: If the receiver is a collection, extract element type
    extract_element_type(receiver_type)
}

/// Extract the element type from a collection type
///
/// For `List<T>`, `Set<T>`, etc., returns `T`.
/// For `Map<K, V>`, returns a `Pair<K, V>` (for iteration).
fn extract_element_type(collection_type: &KotlinType) -> Option<KotlinType> {
    match collection_type {
        KotlinType::Class {
            fqn,
            type_arguments,
            ..
        } => {
            let simple_name = fqn.rsplit('.').next().unwrap_or(fqn);
            match simple_name {
                // Single-element collections
                "List" | "MutableList" | "ArrayList" | "Set" | "MutableSet" | "HashSet"
                | "Array" | "Sequence" | "Iterable" | "Collection" => {
                    type_arguments.first().and_then(|arg| match arg {
                        crate::types::TypeArgument::Type(ty) => Some(ty.clone()),
                        _ => None,
                    })
                }
                // Maps iterate over entries (Pair<K, V>)
                "Map" | "MutableMap" | "HashMap" => {
                    if type_arguments.len() >= 2 {
                        let key = match &type_arguments[0] {
                            crate::types::TypeArgument::Type(ty) => ty.clone(),
                            _ => KotlinType::Unknown,
                        };
                        let value = match &type_arguments[1] {
                            crate::types::TypeArgument::Type(ty) => ty.clone(),
                            _ => KotlinType::Unknown,
                        };
                        Some(KotlinType::generic("kotlin.Pair", vec![key, value]))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Context information for lambda type inference
///
/// This struct captures the context needed to infer the type of `it`
/// in a lambda expression.
#[derive(Debug, Clone)]
pub struct LambdaContext {
    /// The method being called (e.g., "filter", "map", "forEach")
    pub method_name: String,
    /// The receiver type (e.g., List<User>)
    pub receiver_type: Option<KotlinType>,
    /// The inferred type for `it`
    pub it_type: Option<KotlinType>,
}

impl LambdaContext {
    /// Create a new lambda context
    pub fn new(method_name: String, receiver_type: Option<KotlinType>) -> Self {
        let it_type = receiver_type
            .as_ref()
            .and_then(|rt| infer_lambda_it_type(rt, &method_name));

        LambdaContext {
            method_name,
            receiver_type,
            it_type,
        }
    }

    /// Create a context from method call information
    pub fn from_method_call(method_name: &str, receiver_type: Option<KotlinType>) -> Self {
        Self::new(method_name.to_string(), receiver_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unify_type_var() {
        let mut unifier = TypeUnifier::new();

        let result = unifier.unify(&TypeExpr::type_var("T"), &KotlinType::string());
        assert!(result.is_ok());

        let subs = unifier.substitutions();
        assert_eq!(subs.get("T"), Some(&KotlinType::string()));
    }

    #[test]
    fn test_accepts_arity_counts_fixed_parameters_and_varargs() {
        let nullary = GenericFunctionSignature::new("f").returns(TypeExpr::type_var("T"));
        let unary = GenericFunctionSignature::new("f")
            .param(TypeExpr::type_var("T"))
            .returns(TypeExpr::type_var("T"));
        let variadic = GenericFunctionSignature::new("f")
            .vararg(TypeExpr::type_var("T"))
            .returns(TypeExpr::type_var("T"));

        assert!(accepts_arity(&nullary, 0));
        assert!(!accepts_arity(&nullary, 1));
        assert!(accepts_arity(&unary, 1));
        assert!(!accepts_arity(&unary, 2));
        assert!(accepts_arity(&variadic, 0));
        assert!(accepts_arity(&variadic, 3));
    }

    #[test]
    fn test_same_arity_overloads_are_told_apart_by_argument_type() {
        let takes_int = GenericFunctionSignature::new("f")
            .param(TypeExpr::concrete("Int"))
            .returns(TypeExpr::concrete("Int"));
        let takes_string = GenericFunctionSignature::new("f")
            .param(TypeExpr::concrete("String"))
            .returns(TypeExpr::concrete("String"));

        let fits = |sig: &GenericFunctionSignature, arg: KotlinType| {
            unify_parameters(&mut TypeUnifier::new(), sig, &[arg], true)
        };
        assert!(fits(&takes_int, KotlinType::int()));
        assert!(!fits(&takes_int, KotlinType::string()));
        assert!(fits(&takes_string, KotlinType::string()));
        assert!(!fits(&takes_string, KotlinType::int()));
    }

    #[test]
    fn test_infer_listof_string() {
        let result = infer_generic_call("listOf", &[KotlinType::string()]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "List<String>");
    }

    #[test]
    fn test_infer_listof_mixed_types_widens_to_any() {
        let result = infer_generic_call("listOf", &[KotlinType::int(), KotlinType::string()]);

        assert_eq!(result.unwrap().to_string(), "List<Any>");
    }

    #[test]
    fn test_infer_listof_null_then_int_is_nullable() {
        let null = KotlinType::from(crate::type_extractor::Type::Nullable(Box::new(
            crate::type_extractor::Type::Simple("Nothing".to_string()),
        )));
        let result = infer_generic_call("listOf", &[null, KotlinType::int()]);

        assert_eq!(result.unwrap().to_string(), "List<Int?>");
    }

    #[test]
    fn test_infer_listof_int_null_int_stays_nullable() {
        let null = KotlinType::from(crate::type_extractor::Type::Nullable(Box::new(
            crate::type_extractor::Type::Simple("Nothing".to_string()),
        )));
        let result = infer_generic_call("listOf", &[KotlinType::int(), null, KotlinType::int()]);

        assert_eq!(result.unwrap().to_string(), "List<Int?>");
    }

    #[test]
    fn test_infer_listof_int() {
        let result = infer_generic_call("listOf", &[KotlinType::int(), KotlinType::int()]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "List<Int>");
    }

    #[test]
    fn test_infer_listof_empty() {
        // Empty listOf should return List<?>
        let result = infer_generic_call("listOf", &[]);

        assert!(result.is_some());
        let ty = result.unwrap();
        // With no args, T is not bound, so returns Unknown
        assert_eq!(ty.to_string(), "List<?>");
    }

    /// A written type argument binds the function's type parameter,
    /// keeps it against a value argument that would widen it, and rules out an
    /// overload with another number of type parameters.
    #[test]
    fn explicit_type_arguments_bind_the_function_type_parameters() {
        let string = || KotlinType::string();
        let call = |name: &str, type_arguments: &[KotlinType], arguments: &[KotlinType]| {
            infer_generic_call_with_type_arguments(name, type_arguments, arguments)
                .map(|ty| ty.to_string())
        };
        assert_eq!(
            call("emptyList", &[string()], &[]).as_deref(),
            Some("List<String>")
        );
        assert_eq!(
            call("arrayOfNulls", &[string()], &[KotlinType::int()]).as_deref(),
            Some("Array<String?>")
        );
        let number = KotlinType::simple("kotlin.Number");
        assert_eq!(
            call("listOf", &[number], &[KotlinType::int(), string()]).as_deref(),
            Some("List<Number>")
        );
        assert_eq!(call("emptyMap", &[string()], &[]), None);
        assert_eq!(call("emptyList", &[], &[]).as_deref(), Some("List<?>"));

        let filtered = infer_method_return_type_with_type_arguments(
            &KotlinType::list(string()),
            "filterIsInstance",
            &[KotlinType::int()],
            &[],
        );
        assert_eq!(
            filtered.map(|ty| ty.to_string()).as_deref(),
            Some("List<Int>")
        );
        let unbound =
            infer_method_return_type(&KotlinType::list(string()), "filterIsInstance", &[]);
        assert_eq!(unbound.map(|ty| ty.to_string()).as_deref(), Some("List<?>"));
    }

    #[test]
    fn test_infer_mapof() {
        let pair_type =
            KotlinType::generic("kotlin.Pair", vec![KotlinType::string(), KotlinType::int()]);
        let result = infer_generic_call("mapOf", &[pair_type]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_infer_method_first() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "first", &[]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "String");
    }

    #[test]
    fn test_infer_method_filter() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "filter", &[]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "List<Int>");
    }

    #[test]
    fn test_infer_method_firstornull() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "firstOrNull", &[]);

        assert!(result.is_some());
        let ty = result.unwrap();
        assert_eq!(ty.to_string(), "String?");
    }

    // ===== Lambda 'it' Parameter Inference Tests =====

    #[test]
    fn test_lambda_it_type_filter() {
        // list.filter { it > 0 } where list: List<Int>
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_lambda_it_type(&list_int, "filter");

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Int");
    }

    #[test]
    fn test_lambda_it_type_map() {
        // users.map { it.name } where users: List<User>
        let list_user = KotlinType::list(KotlinType::simple("User"));
        let result = infer_lambda_it_type(&list_user, "map");

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");
    }

    #[test]
    fn test_lambda_it_type_foreach() {
        // items.forEach { println(it) } where items: List<String>
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_lambda_it_type(&list_string, "forEach");

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "String");
    }

    #[test]
    fn test_lambda_it_type_any() {
        // numbers.any { it > 10 } where numbers: List<Int>
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_lambda_it_type(&list_int, "any");

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Int");
    }

    #[test]
    fn test_lambda_it_type_find() {
        // users.find { it.active } where users: List<User>
        let list_user = KotlinType::list(KotlinType::simple("User"));
        let result = infer_lambda_it_type(&list_user, "find");

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");
    }

    #[test]
    fn test_lambda_context_creation() {
        let list_string = KotlinType::list(KotlinType::string());
        let ctx = LambdaContext::new("filter".to_string(), Some(list_string));

        assert_eq!(ctx.method_name, "filter");
        assert!(ctx.it_type.is_some());
        assert_eq!(ctx.it_type.unwrap().to_string(), "String");
    }

    #[test]
    fn test_extract_element_type_list() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = extract_element_type(&list_int);

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Int");
    }

    #[test]
    fn test_extract_element_type_set() {
        let set_string = KotlinType::set(KotlinType::string());
        let result = extract_element_type(&set_string);

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "String");
    }

    #[test]
    fn test_extract_element_type_map() {
        // Map iterates over Pair<K, V>
        let map_type = KotlinType::map(KotlinType::string(), KotlinType::int());
        let result = extract_element_type(&map_type);

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Pair<String, Int>");
    }

    // ===== User Registry Integration Tests =====

    #[test]
    fn test_infer_with_user_registry_finds_user_method() {
        use crate::types::{Nullability, TypeArgument};
        use crate::user_registry::{UserGenericClass, UserGenericRegistry, UserMethodSignature};

        // Create a user registry with a Repository<T> class
        let mut registry = UserGenericRegistry::new();
        registry.register_class(UserGenericClass {
            fqn: "com.example.Repository".to_string(),
            type_params: vec!["T".to_string()],
            methods: vec![UserMethodSignature {
                name: "findById".to_string(),
                return_type: Some("T".to_string()),
                parameter_types: vec!["Int".to_string()],
            }],
        });

        // Create Repository<User> receiver type
        let receiver = KotlinType::Class {
            fqn: "com.example.Repository".to_string(),
            type_arguments: vec![TypeArgument::Type(KotlinType::simple("User"))],
            nullability: Nullability::NonNull,
        };

        // Infer findById() return type
        let result = infer_method_return_type_with_registry(
            &receiver,
            "findById",
            &[KotlinType::int()],
            Some(&registry),
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");
    }

    #[test]
    fn test_infer_with_user_registry_falls_back_to_stdlib() {
        // Empty user registry
        let registry = UserGenericRegistry::new();

        // List<String> is a stdlib type
        let receiver = KotlinType::list(KotlinType::string());

        // first() is a stdlib method
        let result =
            infer_method_return_type_with_registry(&receiver, "first", &[], Some(&registry));

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "String");
    }

    #[test]
    fn test_infer_with_no_registry_uses_stdlib() {
        // List<Int> is a stdlib type
        let receiver = KotlinType::list(KotlinType::int());

        // filter() is a stdlib method
        let result = infer_method_return_type_with_registry(
            &receiver,
            "filter",
            &[],
            None, // No user registry
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<Int>");
    }
}
