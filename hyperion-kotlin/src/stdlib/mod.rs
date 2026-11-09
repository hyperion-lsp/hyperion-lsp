// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Standard Library Registry
//!
//! This module provides type signatures for Kotlin standard library functions,
//! enabling type inference for generic function calls like `listOf()`, `mapOf()`, etc.

pub mod coroutines;

use crate::types::{KotlinType, TypeArgument, TypeParameter};
use std::collections::HashMap;
use std::sync::LazyLock;

/// Type expression for describing types in function signatures
///
/// This represents types that may contain type variables that need
/// to be resolved through type inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// Concrete type: "String", "Int"
    Concrete(String),
    /// Type variable: "T", "K", "V"
    TypeVar(String),
    /// Generic type: List<T>, Map<K, V>
    Generic(String, Vec<TypeExpr>),
    /// Nullable type
    Nullable(Box<TypeExpr>),
    /// Function type: (T) -> R
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

impl TypeExpr {
    /// Create a concrete type expression
    pub fn concrete(name: impl Into<String>) -> Self {
        TypeExpr::Concrete(name.into())
    }

    /// Create a type variable expression
    pub fn type_var(name: impl Into<String>) -> Self {
        TypeExpr::TypeVar(name.into())
    }

    /// Create a generic type expression
    pub fn generic(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        TypeExpr::Generic(name.into(), args)
    }

    /// Make this type expression nullable
    pub fn nullable(self) -> Self {
        TypeExpr::Nullable(Box::new(self))
    }

    /// Convert to KotlinType, substituting type variables with concrete types
    pub fn to_kotlin_type(&self, substitutions: &HashMap<String, KotlinType>) -> KotlinType {
        match self {
            TypeExpr::Concrete(name) => {
                let fqn = resolve_stdlib_name(name);
                KotlinType::simple(fqn)
            }
            TypeExpr::TypeVar(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or(KotlinType::Unknown),
            TypeExpr::Generic(name, args) => {
                let fqn = resolve_stdlib_name(name);
                let type_args: Vec<TypeArgument> = args
                    .iter()
                    .map(|arg| TypeArgument::Type(arg.to_kotlin_type(substitutions)))
                    .collect();
                KotlinType::generic_with_args(fqn, type_args)
            }
            TypeExpr::Nullable(inner) => inner.to_kotlin_type(substitutions).nullable(),
            TypeExpr::Function { params, ret } => {
                let param_types: Vec<KotlinType> = params
                    .iter()
                    .map(|p| p.to_kotlin_type(substitutions))
                    .collect();
                let return_type = ret.to_kotlin_type(substitutions);
                KotlinType::function(param_types, return_type)
            }
        }
    }
}

/// Parameter signature in a function
#[derive(Debug, Clone)]
pub enum ParameterSig {
    /// Single parameter
    Single(TypeExpr),
    /// Vararg parameter (vararg items: T)
    Vararg(TypeExpr),
}

/// Generic function signature
#[derive(Debug, Clone)]
pub struct GenericFunctionSignature {
    /// Function name
    pub name: String,
    /// Type parameters (T, K, V, etc.)
    pub type_parameters: Vec<TypeParameter>,
    /// Parameter signatures
    pub parameters: Vec<ParameterSig>,
    /// Return type expression
    pub return_type: TypeExpr,
    /// Whether this is an extension function
    pub is_extension: bool,
    /// Receiver type for extension functions
    pub receiver_type: Option<TypeExpr>,
}

impl GenericFunctionSignature {
    /// Create a new generic function signature
    pub fn new(name: impl Into<String>) -> Self {
        GenericFunctionSignature {
            name: name.into(),
            type_parameters: Vec::new(),
            parameters: Vec::new(),
            return_type: TypeExpr::concrete("Unit"),
            is_extension: false,
            receiver_type: None,
        }
    }

    /// Add a type parameter
    pub fn type_param(mut self, param: TypeParameter) -> Self {
        self.type_parameters.push(param);
        self
    }

    /// Add type parameters (shorthand for simple type vars)
    pub fn type_params(mut self, names: &[&str]) -> Self {
        for name in names {
            self.type_parameters.push(TypeParameter::simple(*name));
        }
        self
    }

    /// Add a vararg parameter
    pub fn vararg(mut self, ty: TypeExpr) -> Self {
        self.parameters.push(ParameterSig::Vararg(ty));
        self
    }

    /// Add a single parameter
    pub fn param(mut self, ty: TypeExpr) -> Self {
        self.parameters.push(ParameterSig::Single(ty));
        self
    }

    /// Set the return type
    pub fn returns(mut self, ty: TypeExpr) -> Self {
        self.return_type = ty;
        self
    }

    /// Make this an extension function
    pub fn extension(mut self, receiver: TypeExpr) -> Self {
        self.is_extension = true;
        self.receiver_type = Some(receiver);
        self
    }
}

/// Registry of stdlib function signatures
pub struct StdlibRegistry {
    functions: HashMap<String, Vec<GenericFunctionSignature>>,
}

impl StdlibRegistry {
    /// Create a new registry with standard library signatures
    pub fn new() -> Self {
        let mut registry = StdlibRegistry {
            functions: HashMap::new(),
        };
        registry.register_collection_constructors();
        registry.register_collection_methods();
        registry.register_utility_functions();
        registry.register_coroutine_stubs();
        registry
    }

    /// Register a function signature
    pub fn register(&mut self, sig: GenericFunctionSignature) {
        self.functions
            .entry(sig.name.clone())
            .or_default()
            .push(sig);
    }

    /// Look up function signatures by name
    pub fn lookup(&self, name: &str) -> Option<&Vec<GenericFunctionSignature>> {
        self.functions.get(name)
    }

    /// Register collection constructor functions
    fn register_collection_constructors(&mut self) {
        // listOf<T>(vararg elements: T): List<T>
        self.register(
            GenericFunctionSignature::new("listOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // emptyList<T>(): List<T>
        self.register(
            GenericFunctionSignature::new("emptyList")
                .type_params(&["T"])
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // mutableListOf<T>(vararg elements: T): MutableList<T>
        self.register(
            GenericFunctionSignature::new("mutableListOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic(
                    "MutableList",
                    vec![TypeExpr::type_var("T")],
                )),
        );

        // arrayListOf<T>(vararg elements: T): ArrayList<T>
        self.register(
            GenericFunctionSignature::new("arrayListOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic(
                    "ArrayList",
                    vec![TypeExpr::type_var("T")],
                )),
        );

        // setOf<T>(vararg elements: T): Set<T>
        self.register(
            GenericFunctionSignature::new("setOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("Set", vec![TypeExpr::type_var("T")])),
        );

        // emptySet<T>(): Set<T>
        self.register(
            GenericFunctionSignature::new("emptySet")
                .type_params(&["T"])
                .returns(TypeExpr::generic("Set", vec![TypeExpr::type_var("T")])),
        );

        // mutableSetOf<T>(vararg elements: T): MutableSet<T>
        self.register(
            GenericFunctionSignature::new("mutableSetOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic(
                    "MutableSet",
                    vec![TypeExpr::type_var("T")],
                )),
        );

        // hashSetOf<T>(vararg elements: T): HashSet<T>
        self.register(
            GenericFunctionSignature::new("hashSetOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("HashSet", vec![TypeExpr::type_var("T")])),
        );

        // mapOf<K, V>(vararg pairs: Pair<K, V>): Map<K, V>
        self.register(
            GenericFunctionSignature::new("mapOf")
                .type_params(&["K", "V"])
                .vararg(TypeExpr::generic(
                    "Pair",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .returns(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                )),
        );

        // emptyMap<K, V>(): Map<K, V>
        self.register(
            GenericFunctionSignature::new("emptyMap")
                .type_params(&["K", "V"])
                .returns(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                )),
        );

        // mutableMapOf<K, V>(vararg pairs: Pair<K, V>): MutableMap<K, V>
        self.register(
            GenericFunctionSignature::new("mutableMapOf")
                .type_params(&["K", "V"])
                .vararg(TypeExpr::generic(
                    "Pair",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .returns(TypeExpr::generic(
                    "MutableMap",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                )),
        );

        // hashMapOf<K, V>(vararg pairs: Pair<K, V>): HashMap<K, V>
        self.register(
            GenericFunctionSignature::new("hashMapOf")
                .type_params(&["K", "V"])
                .vararg(TypeExpr::generic(
                    "Pair",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .returns(TypeExpr::generic(
                    "HashMap",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                )),
        );

        // arrayOf<T>(vararg elements: T): Array<T>
        self.register(
            GenericFunctionSignature::new("arrayOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("Array", vec![TypeExpr::type_var("T")])),
        );

        // arrayOfNulls<T>(size: Int): Array<T?>, a built-in that no class file of
        // the stdlib JAR declares
        self.register(
            GenericFunctionSignature::new("arrayOfNulls")
                .type_params(&["T"])
                .param(TypeExpr::concrete("Int"))
                .returns(TypeExpr::generic(
                    "Array",
                    vec![TypeExpr::type_var("T").nullable()],
                )),
        );

        // sequenceOf<T>(vararg elements: T): Sequence<T>
        self.register(
            GenericFunctionSignature::new("sequenceOf")
                .type_params(&["T"])
                .vararg(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("Sequence", vec![TypeExpr::type_var("T")])),
        );
    }

    /// Register collection extension methods
    fn register_collection_methods(&mut self) {
        // List<T>.first(): T
        self.register(
            GenericFunctionSignature::new("first")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T")),
        );

        // List<T>.firstOrNull(): T?
        self.register(
            GenericFunctionSignature::new("firstOrNull")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // List<T>.last(): T
        self.register(
            GenericFunctionSignature::new("last")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T")),
        );

        // List<T>.lastOrNull(): T?
        self.register(
            GenericFunctionSignature::new("lastOrNull")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // List<T>.filter(predicate: (T) -> Boolean): List<T>
        self.register(
            GenericFunctionSignature::new("filter")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.filterNot(predicate: (T) -> Boolean): List<T>
        self.register(
            GenericFunctionSignature::new("filterNot")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<*>.filterIsInstance<R>(): List<R>, declared on Iterable<*>; its R
        // is bound only by a type argument the call writes
        self.register(
            GenericFunctionSignature::new("filterIsInstance")
                .type_params(&["R"])
                .extension(TypeExpr::concrete("List"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("R")])),
        );

        // List<T>.map<R>(transform: (T) -> R): List<R>
        self.register(
            GenericFunctionSignature::new("map")
                .type_params(&["T", "R"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("R")])),
        );

        // List<T>.flatMap<R>(transform: (T) -> Iterable<R>): List<R>
        self.register(
            GenericFunctionSignature::new("flatMap")
                .type_params(&["T", "R"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::generic("Iterable", vec![TypeExpr::type_var("R")])),
                })
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("R")])),
        );

        // List<T>.forEach(action: (T) -> Unit): Unit
        self.register(
            GenericFunctionSignature::new("forEach")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::concrete("Unit")),
        );

        // List<T>.any(predicate: (T) -> Boolean): Boolean
        self.register(
            GenericFunctionSignature::new("any")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.all(predicate: (T) -> Boolean): Boolean
        self.register(
            GenericFunctionSignature::new("all")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.none(predicate: (T) -> Boolean): Boolean
        self.register(
            GenericFunctionSignature::new("none")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.find(predicate: (T) -> Boolean): T?
        self.register(
            GenericFunctionSignature::new("find")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // List<T>.sorted(): List<T>
        self.register(
            GenericFunctionSignature::new("sorted")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.sortedDescending(): List<T>
        self.register(
            GenericFunctionSignature::new("sortedDescending")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.reversed(): List<T>
        self.register(
            GenericFunctionSignature::new("reversed")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.distinct(): List<T>
        self.register(
            GenericFunctionSignature::new("distinct")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.take(n: Int): List<T>
        self.register(
            GenericFunctionSignature::new("take")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::concrete("Int"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.drop(n: Int): List<T>
        self.register(
            GenericFunctionSignature::new("drop")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::concrete("Int"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // Map<K, V>.get(key: K): V?
        self.register(
            GenericFunctionSignature::new("get")
                .type_params(&["K", "V"])
                .extension(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .param(TypeExpr::type_var("K"))
                .returns(TypeExpr::type_var("V").nullable()),
        );

        // Map<K, V>.keys: Set<K>
        self.register(
            GenericFunctionSignature::new("keys")
                .type_params(&["K", "V"])
                .extension(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .returns(TypeExpr::generic("Set", vec![TypeExpr::type_var("K")])),
        );

        // Map<K, V>.values: Collection<V>
        self.register(
            GenericFunctionSignature::new("values")
                .type_params(&["K", "V"])
                .extension(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
                ))
                .returns(TypeExpr::generic(
                    "Collection",
                    vec![TypeExpr::type_var("V")],
                )),
        );
    }

    /// Register utility functions
    fn register_utility_functions(&mut self) {
        // to<A, B>(A, B): Pair<A, B> (infix function)
        self.register(
            GenericFunctionSignature::new("to")
                .type_params(&["A", "B"])
                .extension(TypeExpr::type_var("A"))
                .param(TypeExpr::type_var("B"))
                .returns(TypeExpr::generic(
                    "Pair",
                    vec![TypeExpr::type_var("A"), TypeExpr::type_var("B")],
                )),
        );

        // println(message: Any?): Unit
        self.register(
            GenericFunctionSignature::new("println")
                .param(TypeExpr::concrete("Any").nullable())
                .returns(TypeExpr::concrete("Unit")),
        );

        // print(message: Any?): Unit
        self.register(
            GenericFunctionSignature::new("print")
                .param(TypeExpr::concrete("Any").nullable())
                .returns(TypeExpr::concrete("Unit")),
        );

        // TODO<T>(): T (throws NotImplementedError)
        self.register(
            GenericFunctionSignature::new("TODO")
                .type_params(&["T"])
                .returns(TypeExpr::type_var("T")),
        );

        // repeat(times: Int, action: (Int) -> Unit): Unit
        self.register(
            GenericFunctionSignature::new("repeat")
                .param(TypeExpr::concrete("Int"))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::concrete("Int")],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::concrete("Unit")),
        );

        // require(value: Boolean): Unit
        self.register(
            GenericFunctionSignature::new("require")
                .param(TypeExpr::concrete("Boolean"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // check(value: Boolean): Unit
        self.register(
            GenericFunctionSignature::new("check")
                .param(TypeExpr::concrete("Boolean"))
                .returns(TypeExpr::concrete("Unit")),
        );

        // error(message: Any): Nothing
        self.register(
            GenericFunctionSignature::new("error")
                .param(TypeExpr::concrete("Any"))
                .returns(TypeExpr::concrete("Nothing")),
        );

        // ========== Additional Collection Methods ==========

        // List<T>.count(): Int
        self.register(
            GenericFunctionSignature::new("count")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Int")),
        );

        // List<T>.size: Int (accessed as method for simplicity)
        self.register(
            GenericFunctionSignature::new("size")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Int")),
        );

        // List<T>.isEmpty(): Boolean
        self.register(
            GenericFunctionSignature::new("isEmpty")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.isNotEmpty(): Boolean
        self.register(
            GenericFunctionSignature::new("isNotEmpty")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.contains(element: T): Boolean
        self.register(
            GenericFunctionSignature::new("contains")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::type_var("T"))
                .returns(TypeExpr::concrete("Boolean")),
        );

        // List<T>.indexOf(element: T): Int
        self.register(
            GenericFunctionSignature::new("indexOf")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::type_var("T"))
                .returns(TypeExpr::concrete("Int")),
        );

        // List<T>.reduce(operation: (T, T) -> T): T
        self.register(
            GenericFunctionSignature::new("reduce")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T"), TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("T")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // List<T>.fold<R>(initial: R, operation: (R, T) -> R): R
        self.register(
            GenericFunctionSignature::new("fold")
                .type_params(&["T", "R"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::type_var("R"))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("R"), TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // List<T>.groupBy<K>(keySelector: (T) -> K): Map<K, List<T>>
        self.register(
            GenericFunctionSignature::new("groupBy")
                .type_params(&["T", "K"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("K")),
                })
                .returns(TypeExpr::generic(
                    "Map",
                    vec![
                        TypeExpr::type_var("K"),
                        TypeExpr::generic("List", vec![TypeExpr::type_var("T")]),
                    ],
                )),
        );

        // List<T>.associateBy<K>(keySelector: (T) -> K): Map<K, T>
        self.register(
            GenericFunctionSignature::new("associateBy")
                .type_params(&["T", "K"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("K")),
                })
                .returns(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("K"), TypeExpr::type_var("T")],
                )),
        );

        // List<T>.associateWith<V>(valueSelector: (T) -> V): Map<T, V>
        self.register(
            GenericFunctionSignature::new("associateWith")
                .type_params(&["T", "V"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("V")),
                })
                .returns(TypeExpr::generic(
                    "Map",
                    vec![TypeExpr::type_var("T"), TypeExpr::type_var("V")],
                )),
        );

        // List<T>.toSet(): Set<T>
        self.register(
            GenericFunctionSignature::new("toSet")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("Set", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.toMutableList(): MutableList<T>
        self.register(
            GenericFunctionSignature::new("toMutableList")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic(
                    "MutableList",
                    vec![TypeExpr::type_var("T")],
                )),
        );

        // List<T>.toList(): List<T>
        self.register(
            GenericFunctionSignature::new("toList")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.plus(element: T): List<T>
        self.register(
            GenericFunctionSignature::new("plus")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.minus(element: T): List<T>
        self.register(
            GenericFunctionSignature::new("minus")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .param(TypeExpr::type_var("T"))
                .returns(TypeExpr::generic("List", vec![TypeExpr::type_var("T")])),
        );

        // List<T>.joinToString(separator: String = ", "): String
        self.register(
            GenericFunctionSignature::new("joinToString")
                .type_params(&["T"])
                .extension(TypeExpr::generic("List", vec![TypeExpr::type_var("T")]))
                .returns(TypeExpr::concrete("String")),
        );

        // ========== String Methods ==========

        // String.length: Int
        self.register(
            GenericFunctionSignature::new("length")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("Int")),
        );

        // String.substring(startIndex: Int): String
        self.register(
            GenericFunctionSignature::new("substring")
                .extension(TypeExpr::concrete("String"))
                .param(TypeExpr::concrete("Int"))
                .returns(TypeExpr::concrete("String")),
        );

        // String.split(delimiter: String): List<String>
        self.register(
            GenericFunctionSignature::new("split")
                .extension(TypeExpr::concrete("String"))
                .param(TypeExpr::concrete("String"))
                .returns(TypeExpr::generic(
                    "List",
                    vec![TypeExpr::concrete("String")],
                )),
        );

        // String.trim(): String
        self.register(
            GenericFunctionSignature::new("trim")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("String")),
        );

        // String.uppercase(): String
        self.register(
            GenericFunctionSignature::new("uppercase")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("String")),
        );

        // String.lowercase(): String
        self.register(
            GenericFunctionSignature::new("lowercase")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("String")),
        );

        // String.toInt(): Int
        self.register(
            GenericFunctionSignature::new("toInt")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("Int")),
        );

        // String.toIntOrNull(): Int?
        self.register(
            GenericFunctionSignature::new("toIntOrNull")
                .extension(TypeExpr::concrete("String"))
                .returns(TypeExpr::concrete("Int").nullable()),
        );

        // ========== Scope Functions ==========
        //
        // Kotlin's 5 core scope functions for inline context manipulation:
        // - let: T.let { it -> R } returns R (transforms receiver)
        // - also: T.also { it -> Unit } returns T (side-effects, returns receiver)
        // - apply: T.apply { this: T -> Unit } returns T (configure receiver)
        // - run: T.run { this: T -> R } returns R (execute block with receiver)
        // - with: with(T) { this: T -> R } returns R (top-level, execute block)

        // T.let<R>(block: (T) -> R): R
        // Returns the result of the lambda, passing receiver as 'it'
        self.register(
            GenericFunctionSignature::new("let")
                .type_params(&["T", "R"])
                .extension(TypeExpr::type_var("T"))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // T?.let<R>(block: (T) -> R): R? (nullable overload)
        self.register(
            GenericFunctionSignature::new("let")
                .type_params(&["T", "R"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R").nullable()),
        );

        // T.also(block: (T) -> Unit): T
        // Returns the receiver after executing the lambda (for side-effects)
        self.register(
            GenericFunctionSignature::new("also")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T"))
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // T?.also(block: (T) -> Unit): T? (nullable overload)
        self.register(
            GenericFunctionSignature::new("also")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // T.apply(block: T.() -> Unit): T
        // Returns the receiver after executing the lambda with receiver as 'this'
        self.register(
            GenericFunctionSignature::new("apply")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::type_var("T")),
        );

        // T?.apply(block: T.() -> Unit): T? (nullable overload)
        self.register(
            GenericFunctionSignature::new("apply")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::concrete("Unit")),
                })
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // T.run<R>(block: T.() -> R): R
        // Returns the result of the lambda, with receiver as 'this'
        self.register(
            GenericFunctionSignature::new("run")
                .type_params(&["T", "R"])
                .extension(TypeExpr::type_var("T"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // T?.run<R>(block: T.() -> R): R? (nullable overload)
        self.register(
            GenericFunctionSignature::new("run")
                .type_params(&["T", "R"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R").nullable()),
        );

        // with<T, R>(receiver: T, block: T.() -> R): R
        // Top-level function, returns the result of the lambda
        self.register(
            GenericFunctionSignature::new("with")
                .type_params(&["T", "R"])
                .param(TypeExpr::type_var("T"))
                .param(TypeExpr::Function {
                    params: vec![],
                    ret: Box::new(TypeExpr::type_var("R")),
                })
                .returns(TypeExpr::type_var("R")),
        );

        // T?.takeIf(predicate: (T) -> Boolean): T?
        self.register(
            GenericFunctionSignature::new("takeIf")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::type_var("T").nullable()),
        );

        // T?.takeUnless(predicate: (T) -> Boolean): T?
        self.register(
            GenericFunctionSignature::new("takeUnless")
                .type_params(&["T"])
                .extension(TypeExpr::type_var("T").nullable())
                .param(TypeExpr::Function {
                    params: vec![TypeExpr::type_var("T")],
                    ret: Box::new(TypeExpr::concrete("Boolean")),
                })
                .returns(TypeExpr::type_var("T").nullable()),
        );
    }
}

impl Default for StdlibRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global stdlib registry (lazily initialized)
pub static STDLIB: LazyLock<StdlibRegistry> = LazyLock::new(StdlibRegistry::new);

/// Resolve a simple type name to its fully qualified name
fn resolve_stdlib_name(name: &str) -> String {
    match name {
        // Primitives
        "Int" => "kotlin.Int".to_string(),
        "Long" => "kotlin.Long".to_string(),
        "Short" => "kotlin.Short".to_string(),
        "Byte" => "kotlin.Byte".to_string(),
        "Float" => "kotlin.Float".to_string(),
        "Double" => "kotlin.Double".to_string(),
        "Boolean" => "kotlin.Boolean".to_string(),
        "Char" => "kotlin.Char".to_string(),
        "String" => "kotlin.String".to_string(),
        // Special types
        "Unit" => "kotlin.Unit".to_string(),
        "Nothing" => "kotlin.Nothing".to_string(),
        "Any" => "kotlin.Any".to_string(),
        // Collections
        "List" => "kotlin.collections.List".to_string(),
        "MutableList" => "kotlin.collections.MutableList".to_string(),
        "ArrayList" => "kotlin.collections.ArrayList".to_string(),
        "Set" => "kotlin.collections.Set".to_string(),
        "MutableSet" => "kotlin.collections.MutableSet".to_string(),
        "HashSet" => "kotlin.collections.HashSet".to_string(),
        "Map" => "kotlin.collections.Map".to_string(),
        "MutableMap" => "kotlin.collections.MutableMap".to_string(),
        "HashMap" => "kotlin.collections.HashMap".to_string(),
        "Array" => "kotlin.Array".to_string(),
        "Sequence" => "kotlin.sequences.Sequence".to_string(),
        "Iterable" => "kotlin.collections.Iterable".to_string(),
        "Collection" => "kotlin.collections.Collection".to_string(),
        "Pair" => "kotlin.Pair".to_string(),
        "Triple" => "kotlin.Triple".to_string(),
        // Default: keep as-is
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_listof() {
        let sigs = STDLIB.lookup("listOf");
        assert!(sigs.is_some());
        let sigs = sigs.unwrap();
        assert_eq!(sigs.len(), 1);
        assert_eq!(&*sigs[0].name, "listOf");
    }

    #[test]
    fn test_registry_contains_map_functions() {
        assert!(STDLIB.lookup("mapOf").is_some());
        assert!(STDLIB.lookup("mutableMapOf").is_some());
        assert!(STDLIB.lookup("hashMapOf").is_some());
        assert!(STDLIB.lookup("emptyMap").is_some());
    }

    #[test]
    fn test_registry_contains_collection_methods() {
        assert!(STDLIB.lookup("first").is_some());
        assert!(STDLIB.lookup("filter").is_some());
        assert!(STDLIB.lookup("map").is_some());
        assert!(STDLIB.lookup("forEach").is_some());
    }

    #[test]
    fn test_type_expr_to_kotlin_type() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());

        let expr = TypeExpr::generic("List", vec![TypeExpr::type_var("T")]);
        let kt = expr.to_kotlin_type(&subs);

        assert_eq!(kt.to_string(), "List<String>");
    }

    #[test]
    fn test_type_expr_nested_generic() {
        let mut subs = HashMap::new();
        subs.insert("K".to_string(), KotlinType::string());
        subs.insert("V".to_string(), KotlinType::int());

        let expr = TypeExpr::generic(
            "Map",
            vec![TypeExpr::type_var("K"), TypeExpr::type_var("V")],
        );
        let kt = expr.to_kotlin_type(&subs);

        assert_eq!(kt.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_listof_signature() {
        let sigs = STDLIB.lookup("listOf").unwrap();
        let sig = &sigs[0];

        // Check type parameters
        assert_eq!(sig.type_parameters.len(), 1);
        assert_eq!(sig.type_parameters[0].name, "T");

        // Check return type with substitution
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());

        let return_type = sig.return_type.to_kotlin_type(&subs);
        assert_eq!(return_type.to_string(), "List<String>");
    }

    #[test]
    fn test_mapof_signature() {
        let sigs = STDLIB.lookup("mapOf").unwrap();
        let sig = &sigs[0];

        // Check type parameters
        assert_eq!(sig.type_parameters.len(), 2);
        assert_eq!(sig.type_parameters[0].name, "K");
        assert_eq!(sig.type_parameters[1].name, "V");

        // Check return type with substitution
        let mut subs = HashMap::new();
        subs.insert("K".to_string(), KotlinType::string());
        subs.insert("V".to_string(), KotlinType::int());

        let return_type = sig.return_type.to_kotlin_type(&subs);
        assert_eq!(return_type.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_scope_functions_registered() {
        // All 5 scope functions should be registered
        assert!(STDLIB.lookup("let").is_some());
        assert!(STDLIB.lookup("also").is_some());
        assert!(STDLIB.lookup("apply").is_some());
        assert!(STDLIB.lookup("run").is_some());
        assert!(STDLIB.lookup("with").is_some());
    }

    #[test]
    fn test_let_has_both_overloads() {
        let sigs = STDLIB.lookup("let").unwrap();
        // Should have 2 overloads: T.let and T?.let
        assert_eq!(sigs.len(), 2);

        // Non-nullable version: T.let<R>(block: (T) -> R): R
        let non_null = &sigs[0];
        assert_eq!(non_null.type_parameters.len(), 2);
        assert!(non_null.is_extension);

        // Nullable version: T?.let<R>(block: (T) -> R): R?
        let nullable = &sigs[1];
        assert_eq!(nullable.type_parameters.len(), 2);
        assert!(nullable.is_extension);
    }

    #[test]
    fn test_also_returns_receiver_type() {
        let sigs = STDLIB.lookup("also").unwrap();
        let sig = &sigs[0]; // Non-nullable version

        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());

        // also returns T (the receiver type)
        let return_type = sig.return_type.to_kotlin_type(&subs);
        assert_eq!(return_type.to_string(), "String");
    }

    #[test]
    fn test_with_is_top_level_function() {
        let sigs = STDLIB.lookup("with").unwrap();
        let sig = &sigs[0];

        // with is NOT an extension function
        assert!(!sig.is_extension);
        assert!(sig.receiver_type.is_none());

        // Takes 2 params: receiver and block
        assert_eq!(sig.parameters.len(), 2);
    }

    #[test]
    fn test_run_returns_lambda_result() {
        let sigs = STDLIB.lookup("run").unwrap();
        let sig = &sigs[0]; // Non-nullable version

        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());
        subs.insert("R".to_string(), KotlinType::int());

        // run returns R (the lambda's return type)
        let return_type = sig.return_type.to_kotlin_type(&subs);
        assert_eq!(return_type.to_string(), "Int");
    }
}
