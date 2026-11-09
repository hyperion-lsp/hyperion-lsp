// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol Table and Scope Management for Hyperion Semantics
//!
//! This module provides the core data structures for managing symbols, types, and scopes
//! in the Hyperion semantic analysis engine. It implements a hierarchical scope structure
//! with efficient symbol resolution and querying capabilities.
//!
//! # Design Overview
//!
//! The symbol table design follows these principles:
//!
//! 1. **Hierarchical Scoping**: Scopes are organized in a tree structure matching the
//!    lexical structure of Java code (package -> class -> method -> block).
//!
//! 2. **Efficient Lookups**: Symbol resolution walks the scope chain from innermost to
//!    outermost, with caching for performance.
//!
//! 3. **Type-Safe Representations**: All types are represented with full fidelity,
//!    including generics, wildcards, and type bounds.
//!
//! 4. **Incremental Updates**: Symbols and scopes can be updated incrementally as code
//!    changes, supporting the Salsa-based query system.
//!
//! # Architecture
//!
//! ```text
//! SymbolTable
//! ├── symbols: HashMap<SymbolId, Symbol>
//! ├── scopes: HashMap<ScopeId, Scope>
//! └── root_scope: ScopeId
//!
//! Symbol
//! ├── name: String
//! ├── kind: SymbolKind (Class, Method, Field, etc.)
//! ├── type_info: Type
//! ├── visibility: Visibility (Public, Private, etc.)
//! ├── modifiers: Modifiers (Static, Final, etc.)
//! └── location: SourceLocation
//!
//! Scope
//! ├── kind: ScopeKind (Package, Class, Method, Block)
//! ├── parent: Option<ScopeId>
//! ├── children: Vec<ScopeId>
//! └── symbols: HashMap<String, SymbolId>
//! ```
//!
//! # Usage Example
//!
//! ```rust,ignore
//! let mut symbol_table = SymbolTable::new();
//! let class_scope = symbol_table.create_scope(ScopeKind::Class, Some(root_scope));
//!
//! let field_symbol = Symbol {
//!     name: "count".into(),
//!     kind: SymbolKind::Field,
//!     type_info: Type::Primitive(PrimitiveType::Int),
//!     visibility: Visibility::Private,
//!     modifiers: Modifiers::empty(),
//!     location: SourceLocation::new(file_id, 10, 15),
//!     metadata: SymbolMetadata::default(),
//! };
//!
//! let symbol_id = symbol_table.add_symbol(field_symbol, class_scope);
//!
//! // Later, resolve the symbol
//! if let Some(symbol) = symbol_table.lookup_symbol("count", some_scope) {
//!     println!("Found symbol: {:?}", symbol);
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use tracing::debug;

// Re-export FileId from db module to avoid duplication
pub use crate::db::FileId;

/// Unique identifier for a symbol in the symbol table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u64);

/// Unique identifier for a scope in the symbol table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u64);

/// The kind of symbol (class, method, field, variable, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    /// A class declaration (including interfaces, enums, records)
    Class {
        /// Whether this is an interface
        is_interface: bool,
        /// Whether this is an enum
        is_enum: bool,
        /// Whether this is a record (Java 14+)
        is_record: bool,
        /// Whether this is an annotation type
        is_annotation: bool,
    },

    /// A method or constructor
    Method {
        /// Whether this is a constructor
        is_constructor: bool,
        /// Method parameters
        parameters: Vec<Parameter>,
        /// Return type (None for constructors)
        return_type: Option<Type>,
        /// Type parameters (generics)
        type_parameters: Vec<TypeParameter>,
    },

    /// A field (instance or static variable)
    Field {
        /// The field's type
        field_type: Type,
    },

    /// A local variable within a method or block
    LocalVariable {
        /// The variable's type
        var_type: Type,
        /// Whether this is a final variable
        is_final: bool,
    },

    /// A method or constructor parameter
    Parameter {
        /// The parameter's type
        param_type: Type,
        /// Whether this is a varargs parameter
        is_varargs: bool,
    },

    /// A package declaration
    Package,

    /// A type parameter (generic type variable)
    TypeParameter {
        /// Upper bounds (extends constraints)
        bounds: Vec<Type>,
    },

    /// An enum constant
    EnumConstant {
        /// The enum type this constant belongs to
        enum_type: Type,
    },
}

impl From<&SymbolKind> for lsp_types::SymbolKind {
    fn from(kind: &SymbolKind) -> Self {
        match kind {
            SymbolKind::Class {
                is_interface: true, ..
            } => Self::INTERFACE,
            SymbolKind::Class { is_enum: true, .. } => Self::ENUM,
            SymbolKind::Class { .. } => Self::CLASS,
            SymbolKind::Method {
                is_constructor: true,
                ..
            } => Self::CONSTRUCTOR,
            SymbolKind::Method { .. } => Self::METHOD,
            SymbolKind::Field { .. } => Self::FIELD,
            SymbolKind::LocalVariable { .. } | SymbolKind::Parameter { .. } => Self::VARIABLE,
            SymbolKind::Package => Self::PACKAGE,
            SymbolKind::TypeParameter { .. } => Self::TYPE_PARAMETER,
            SymbolKind::EnumConstant { .. } => Self::ENUM_MEMBER,
        }
    }
}

impl From<&SymbolKind> for crate::db::SymbolKind {
    fn from(kind: &SymbolKind) -> Self {
        match kind {
            SymbolKind::Class {
                is_interface: true, ..
            } => Self::Interface,
            SymbolKind::Class { is_enum: true, .. } => Self::Enum,
            SymbolKind::Class { .. } | SymbolKind::TypeParameter { .. } => Self::Class, // no TypeParameter in db::SymbolKind
            SymbolKind::Method { .. } => Self::Method,
            SymbolKind::Field { .. } | SymbolKind::EnumConstant { .. } => Self::Field, // enum members stored as fields in DB
            SymbolKind::LocalVariable { .. } => Self::Variable,
            SymbolKind::Parameter { .. } => Self::Parameter,
            SymbolKind::Package => Self::Package,
        }
    }
}

/// Visibility/access modifier for a symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// public access
    Public,
    /// protected access
    Protected,
    /// package-private (default) access
    PackagePrivate,
    /// private access
    Private,
}

/// Additional modifiers for symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modifiers {
    bits: u32,
}

impl Modifiers {
    pub const STATIC: u32 = 1 << 0;
    pub const FINAL: u32 = 1 << 1;
    pub const ABSTRACT: u32 = 1 << 2;
    pub const SYNCHRONIZED: u32 = 1 << 3;
    pub const NATIVE: u32 = 1 << 4;
    pub const STRICTFP: u32 = 1 << 5;
    pub const TRANSIENT: u32 = 1 << 6;
    pub const VOLATILE: u32 = 1 << 7;
    pub const DEFAULT: u32 = 1 << 8; // Java 8 default methods
    pub const SEALED: u32 = 1 << 9; // Java 17 sealed classes
    pub const NON_SEALED: u32 = 1 << 10;

    /// Create an empty set of modifiers
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Create modifiers from raw bits
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Check if a specific modifier is set
    pub const fn has(&self, modifier: u32) -> bool {
        (self.bits & modifier) != 0
    }

    /// Set a modifier
    pub const fn set(&mut self, modifier: u32) {
        self.bits |= modifier;
    }

    /// Check if static modifier is set
    pub const fn is_static(&self) -> bool {
        self.has(Self::STATIC)
    }

    /// Check if final modifier is set
    pub const fn is_final(&self) -> bool {
        self.has(Self::FINAL)
    }

    /// Check if abstract modifier is set
    pub const fn is_abstract(&self) -> bool {
        self.has(Self::ABSTRACT)
    }
}

/// Source location information for a symbol
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// The file containing this symbol
    pub file_id: FileId,
    /// Canonical half-open byte range within the file.
    pub range: hyperion_types::TextRange,
    /// Starting line number (1-based)
    pub start_line: u32,
    /// Starting column number (1-based)
    pub start_column: u32,
}

impl SourceLocation {
    /// Create a new source location
    pub const fn new(file_id: FileId, start_offset: u32, end_offset: u32) -> Self {
        Self {
            file_id,
            range: hyperion_types::TextRange::new(start_offset, end_offset),
            start_line: 0,
            start_column: 0,
        }
    }

    /// Create a source location with line and column information
    pub const fn with_position(
        file_id: FileId,
        start_offset: u32,
        end_offset: u32,
        start_line: u32,
        start_column: u32,
    ) -> Self {
        Self {
            file_id,
            range: hyperion_types::TextRange::new(start_offset, end_offset),
            start_line,
            start_column,
        }
    }
}

/// Additional metadata associated with a symbol
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolMetadata {
    /// Documentation comments associated with this symbol
    pub documentation: Option<String>,
    /// Annotations/attributes on this symbol
    pub annotations: Vec<Annotation>,
    /// Whether this symbol is deprecated
    pub is_deprecated: bool,
    /// Deprecation message if deprecated
    pub deprecation_message: Option<String>,
}

/// Represents an annotation on a symbol
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The annotation type (e.g., "Override", "Deprecated")
    pub name: String,
    /// Annotation arguments
    pub arguments: HashMap<String, AnnotationValue>,
}

/// Value of an annotation argument
#[derive(Debug, Clone)]
pub enum AnnotationValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Float(f64),
    Class(Type),
    Enum { enum_type: Type, constant: String },
    Array(Vec<Self>),
    Annotation(Box<Annotation>),
}

// Manual PartialEq and Eq implementation because f64 doesn't implement Eq.
// We compare floats by bit pattern, which is correct for caching purposes.
impl PartialEq for AnnotationValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => {
                // Canonicalize NaN to ensure all NaN values compare equal.
                // This is required for Eq correctness - all NaNs are semantically
                // equivalent for annotation purposes.
                let a_bits = if a.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    a.to_bits()
                };
                let b_bits = if b.is_nan() {
                    f64::NAN.to_bits()
                } else {
                    b.to_bits()
                };
                a_bits == b_bits
            }
            (Self::Class(a), Self::Class(b)) => a == b,
            (
                Self::Enum {
                    enum_type: at,
                    constant: ac,
                },
                Self::Enum {
                    enum_type: bt,
                    constant: bc,
                },
            ) => at == bt && ac == bc,
            (Self::Array(a), Self::Array(b)) => a == b,
            (Self::Annotation(a), Self::Annotation(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for AnnotationValue {}

/// A symbol in the program (class, method, field, variable, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name of the symbol
    pub name: String,
    /// The kind of symbol
    pub kind: SymbolKind,
    /// The type of this symbol
    pub type_info: Type,
    /// Visibility modifier
    pub visibility: Visibility,
    /// Additional modifiers (static, final, etc.)
    pub modifiers: Modifiers,
    /// Source location where this symbol is defined
    pub location: SourceLocation,
    /// Additional metadata
    pub metadata: SymbolMetadata,
}

impl Symbol {
    /// Create a new symbol with the given properties
    pub fn new(
        name: String,
        kind: SymbolKind,
        type_info: Type,
        visibility: Visibility,
        location: SourceLocation,
    ) -> Self {
        Self {
            name,
            kind,
            type_info,
            visibility,
            modifiers: Modifiers::empty(),
            location,
            metadata: SymbolMetadata::default(),
        }
    }
}

/// Method or constructor parameter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub param_type: Type,
    /// Whether this is a varargs parameter
    pub is_varargs: bool,
}

/// Generic type parameter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParameter {
    /// Type parameter name (e.g., "T", "E")
    pub name: String,
    /// Upper bounds (extends constraints)
    pub bounds: Vec<Type>,
}

/// Represents a type in the Java type system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Primitive types (int, boolean, etc.)
    Primitive(PrimitiveType),

    /// Class or interface type
    Class(ClassType),

    /// Array type
    Array {
        /// Element type
        element_type: Box<Self>,
        /// Number of dimensions
        dimensions: u8,
    },

    /// Type variable (generic type parameter)
    TypeVariable {
        /// Name of the type variable
        name: String,
        /// Upper bounds
        bounds: Vec<Self>,
    },

    /// Wildcard type (? extends T, ? super T, ?)
    Wildcard {
        /// Upper bound (extends)
        upper_bound: Option<Box<Self>>,
        /// Lower bound (super)
        lower_bound: Option<Box<Self>>,
    },

    /// Intersection type (T & U & V)
    Intersection {
        /// Types in the intersection
        types: Vec<Self>,
    },

    /// Union type (for multi-catch)
    Union {
        /// Types in the union
        types: Vec<Self>,
    },

    /// The null type
    Null,

    /// The void type
    Void,

    /// Unknown type (for error recovery)
    Unknown,
}

/// Structural assignability accepts only comparisons it has actually proven.
///
/// `Type` is an owned, acyclic tree today. The active-pair guards defend
/// against future recursive representations and accidental algorithmic
/// re-entry, while the budgets bound adversarially deep/wide syntax. Hitting
/// any guard is conservatively incompatible: resource exhaustion must never
/// manufacture assignability.
const MAX_ASSIGNABILITY_RECURSION_DEPTH: usize = 512;
const MAX_ASSIGNABILITY_NODES: usize = 16_384;

struct AssignabilityState {
    remaining_nodes: usize,
    active_types: HashSet<(usize, usize)>,
    active_type_arguments: HashSet<(usize, usize)>,
}

impl Default for AssignabilityState {
    fn default() -> Self {
        Self {
            remaining_nodes: MAX_ASSIGNABILITY_NODES,
            active_types: HashSet::new(),
            active_type_arguments: HashSet::new(),
        }
    }
}

impl AssignabilityState {
    fn pair(target: &Type, source: &Type) -> (usize, usize) {
        (
            std::ptr::from_ref(target) as usize,
            std::ptr::from_ref(source) as usize,
        )
    }

    const fn consume_node(&mut self, depth: usize) -> bool {
        if depth > MAX_ASSIGNABILITY_RECURSION_DEPTH || self.remaining_nodes == 0 {
            return false;
        }
        self.remaining_nodes -= 1;
        true
    }
}

impl Type {
    /// Check if this type is a primitive type
    pub const fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    /// Check if this type is a reference type
    pub const fn is_reference(&self) -> bool {
        matches!(
            self,
            Self::Class(_)
                | Self::Array { .. }
                | Self::TypeVariable { .. }
                | Self::Intersection { .. }
                | Self::Null
        )
    }

    /// Get the erasure of this type (remove generic type arguments)
    pub fn erasure(&self) -> Self {
        match self {
            Self::Class(class_type) => Self::Class(ClassType {
                name: class_type.name.clone(),
                package: class_type.package.clone(),
                type_arguments: vec![],
                is_diamond: false,
            }),
            Self::Array {
                element_type,
                dimensions,
            } => Self::Array {
                element_type: Box::new(element_type.erasure()),
                dimensions: *dimensions,
            },
            // L11: JLS §4.6 - Type erasure of type variables
            //
            // Per the Java Language Specification §4.6:
            //   "The erasure of a type variable is the erasure of its leftmost bound."
            //   "For a type variable without an explicit bound, the erasure is Object."
            //
            // Examples:
            //   <T>              -> Object  (no bound)
            //   <T extends Foo>  -> Foo     (explicit bound)
            //   <T extends Foo & Bar> -> Foo  (first bound)
            Self::TypeVariable { bounds, .. } => bounds
                .first()
                .map_or_else(|| Self::Class(ClassType::object()), Self::erasure),
            Self::Wildcard { upper_bound, .. } => upper_bound
                .as_ref()
                .map_or_else(|| Self::Class(ClassType::object()), |bound| bound.erasure()),
            Self::Intersection { types } => types
                .first()
                .map_or_else(|| Self::Class(ClassType::object()), Self::erasure),
            _ => self.clone(),
        }
    }

    /// Check if this type is assignable from another type
    ///
    /// This is the public entry point that initializes cycle detection.
    /// Note: This version does NOT use workspace for type hierarchy lookups.
    /// For workspace-aware checking, use `is_assignable_from_with_workspace`.
    pub fn is_assignable_from(&self, other: &Self) -> bool {
        self.is_assignable_from_impl(other, 0, &mut AssignabilityState::default(), None)
    }

    /// Check if this type is assignable from another type, using workspace for hierarchy.
    ///
    /// This is the preferred method when a workspace index is available, as it
    /// uses actual inheritance information from source files and JDK bytecode
    /// instead of hardcoded relationships.
    pub fn is_assignable_from_with_workspace(
        &self,
        other: &Self,
        workspace: &hyperion_workspace::WorkspaceIndex,
    ) -> bool {
        self.is_assignable_from_impl(
            other,
            0,
            &mut AssignabilityState::default(),
            Some(workspace),
        )
    }

    /// Internal implementation with pair-based cycle detection.
    ///
    /// If `workspace` is provided, uses it for subtype checks via inheritance
    /// hierarchy. Otherwise falls back to structural comparison only.
    fn is_assignable_from_impl(
        &self,
        other: &Self,
        depth: usize,
        state: &mut AssignabilityState,
        workspace: Option<&hyperion_workspace::WorkspaceIndex>,
    ) -> bool {
        if !state.consume_node(depth) {
            return false;
        }
        let pair = AssignabilityState::pair(self, other);
        if !state.active_types.insert(pair) {
            return false;
        }

        let is_wrapper_for = |class: &ClassType, prim: PrimitiveType| -> bool {
            let wrapper = prim.wrapper_class();
            if class.raw_name() != wrapper {
                return false;
            }
            class.package.as_ref().is_none_or(|pkg| pkg == "java.lang")
        };

        // Simplified assignability check - a full implementation would need
        // access to the type hierarchy
        let assignable = match (self, other) {
            // Unknown types are compatible with everything (for error recovery)
            (Self::Unknown, _) | (_, Self::Unknown) => true,

            // Primitive types must match exactly or allow widening
            (Self::Primitive(p1), Self::Primitive(p2)) => p1 == p2 || p1.is_widening_from(*p2),

            // Autoboxing/unboxing between primitives and java.lang wrappers
            (Self::Class(c1), Self::Primitive(p2)) => is_wrapper_for(c1, *p2),
            (Self::Primitive(p1), Self::Class(c2)) => is_wrapper_for(c2, *p1),

            // Class types: exact match or subtype
            (Self::Class(c1), Self::Class(c2)) => {
                // Use raw names (without generics) for comparison
                let raw1 = c1.raw_qualified_name();
                let raw2 = c2.raw_qualified_name();

                // Exact raw type match
                if raw1 == raw2 {
                    // For diamond operator <>, assume type inference works
                    // and the types are compatible
                    if c2.has_diamond() {
                        true
                    } else {
                        // Compare type arguments for generic types
                        Self::compare_type_arguments(
                            &c1.type_arguments,
                            &c2.type_arguments,
                            depth + 1,
                            state,
                            workspace,
                        )
                    }
                } else if raw1 == "java.lang.Object" {
                    // java.lang.Object is assignable from any class type
                    true
                } else if let Some(ws) = workspace {
                    if c1.type_arguments.is_empty() {
                        crate::types::is_subtype_in_workspace(&raw2, &raw1, ws)
                    } else {
                        // Compare the source as the target's declaration, after
                        // composing generic inheritance edges. Erasure alone
                        // would accept Box<Other> from an implementation of Box<Item>.
                        let arguments = c2
                            .type_arguments
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>();
                        ws.project_owner_type_arguments(&raw2, &arguments, &raw1)
                            .is_some_and(|projected| {
                                let Some(offset) =
                                    projected.len().checked_sub(c1.type_arguments.len())
                                else {
                                    return false;
                                };
                                let projected = &projected[offset..];
                                // A raw supertype permits Java's unchecked conversion.
                                if projected.iter().all(|(_, argument)| argument.is_none()) {
                                    return true;
                                }
                                let variables = crate::resolver::type_parameter_mapping::type_variable_bindings(&c2.type_arguments);
                                let arguments = projected
                                    .iter()
                                    .map(|(_, argument)| {
                                        argument
                                            .as_deref()
                                            .and_then(crate::type_parser::parse_type_string)
                                            .map(|ty| crate::resolver::type_parameter_mapping::apply_type_parameter_substitutions(ty, &variables))
                                    })
                                    .collect::<Option<Vec<_>>>();
                                arguments.is_some_and(|arguments| {
                                    Self::compare_type_arguments(
                                        &c1.type_arguments,
                                        &arguments,
                                        depth + 1,
                                        state,
                                        workspace,
                                    )
                                })
                            })
                    }
                } else {
                    // Fall back to JLS constants when workspace unavailable.
                    crate::types::is_known_jdk_subtype(&raw2, &raw1)
                }
            }

            // Array types
            (
                Self::Array {
                    element_type: e1,
                    dimensions: d1,
                },
                Self::Array {
                    element_type: e2,
                    dimensions: d2,
                },
            ) => match d1.cmp(d2) {
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => match (e1.as_ref(), e2.as_ref()) {
                    (Self::Primitive(p1), Self::Primitive(p2)) => p1 == p2,
                    (Self::Primitive(_), _) | (_, Self::Primitive(_)) => false,
                    _ => e1.is_assignable_from_impl(e2, depth + 1, state, workspace),
                },
                std::cmp::Ordering::Less => {
                    let component = Self::Array {
                        element_type: e2.clone(),
                        dimensions: d2 - d1,
                    };
                    e1.is_assignable_from_impl(&component, depth + 1, state, workspace)
                }
            },

            // Every array implements these two interfaces, including primitive arrays.
            (Self::Class(c), Self::Array { .. }) => matches!(
                c.raw_qualified_name().as_str(),
                "java.lang.Object" | "java.lang.Cloneable" | "java.io.Serializable"
            ),

            // Null can be assigned to any reference type
            (_, Self::Null) if self.is_reference() => true,

            // Intersection type: A & B is assignable to any component (A or B)
            // because intersection is a subtype of all its components
            (target, Self::Intersection { types }) => types
                .iter()
                .any(|t| target.is_assignable_from_impl(t, depth + 1, state, workspace)),

            // Type variables - check both name and bounds
            (
                Self::TypeVariable {
                    name: n1,
                    bounds: b1,
                },
                Self::TypeVariable {
                    name: n2,
                    bounds: b2,
                },
            ) => {
                // Distinct variables only widen when the source explicitly
                // extends the target, e.g. <T, U extends T>.
                (n1 == n2 && b1 == b2)
                    || b2.iter().any(|bound| {
                        self.is_assignable_from_impl(bound, depth + 1, state, workspace)
                    })
            }

            // A type variable is a subtype of each of its upper bounds. The
            // reverse conversion to an arbitrary T is deliberately not allowed.
            (target, Self::TypeVariable { bounds, .. }) => {
                if bounds.is_empty() {
                    target.is_assignable_from_impl(
                        &Self::Class(ClassType::object()),
                        depth + 1,
                        state,
                        workspace,
                    )
                } else {
                    bounds.iter().any(|bound| {
                        target.is_assignable_from_impl(bound, depth + 1, state, workspace)
                    })
                }
            }

            // Wildcard compatibility
            (
                Self::Wildcard {
                    upper_bound: Some(upper),
                    ..
                },
                other,
            ) => {
                // For ? extends T, we can assign any type that is assignable to T
                upper.is_assignable_from_impl(other, depth + 1, state, workspace)
            }

            _ => false,
        };

        state.active_types.remove(&pair);
        assignable
    }

    /// Compare type arguments for generic type compatibility.
    ///
    /// Implements Java's generic type argument checking:
    /// - Raw types (no type args) match any parameterized type (Java backward compat)
    /// - Invariant: `List<String>` only matches `List<String>`
    /// - Covariant: `List<? extends Number>` accepts `List<Integer>`
    /// - Contravariant: `List<? super Integer>` accepts `List<Number>`
    /// - Unbounded wildcard: `List<?>` accepts any `List<T>`
    fn compare_type_arguments(
        target_args: &[Self],
        source_args: &[Self],
        depth: usize,
        state: &mut AssignabilityState,
        workspace: Option<&hyperion_workspace::WorkspaceIndex>,
    ) -> bool {
        // Raw types are compatible with any parameterized type (Java backward compat)
        // If either side has no type arguments, accept it
        if target_args.is_empty() || source_args.is_empty() {
            return true;
        }

        // Type argument counts must match for parameterized types
        if target_args.len() != source_args.len() {
            return false;
        }

        // Compare each type argument
        for (target_arg, source_arg) in target_args.iter().zip(source_args.iter()) {
            if !Self::is_type_argument_compatible(
                target_arg,
                source_arg,
                depth + 1,
                state,
                workspace,
            ) {
                return false;
            }
        }

        true
    }

    /// Check if a source type argument is compatible with a target type argument.
    ///
    /// Handles wildcards, covariance, and contravariance:
    /// - Unbounded wildcard `?` accepts any type
    /// - `? extends T` (covariant) accepts subtypes of T
    /// - `? super T` (contravariant) accepts supertypes of T
    /// - Concrete types must match exactly (invariance)
    fn is_type_argument_compatible(
        target: &Self,
        source: &Self,
        depth: usize,
        state: &mut AssignabilityState,
        workspace: Option<&hyperion_workspace::WorkspaceIndex>,
    ) -> bool {
        if !state.consume_node(depth) {
            return false;
        }
        let pair = AssignabilityState::pair(target, source);
        if !state.active_type_arguments.insert(pair) {
            return false;
        }

        let compatible = match (target, source) {
            // Unbounded target wildcard accepts any type
            (
                Self::Wildcard {
                    upper_bound: None,
                    lower_bound: None,
                },
                _,
            ) => true,

            // SOURCE IS A WILDCARD - these patterns must come BEFORE target-only patterns
            // to ensure wildcard-to-wildcard comparisons use the correct logic

            // Source is unbounded wildcard - compatible with unbounded target or ? extends Object
            // because unbounded wildcard is semantically equivalent to ? extends Object
            (
                target_type,
                Self::Wildcard {
                    upper_bound: None,
                    lower_bound: None,
                },
            ) => {
                match target_type {
                    // Unbounded matches unbounded
                    Self::Wildcard {
                        upper_bound: None,
                        lower_bound: None,
                    } => true,
                    // ? is equivalent to ? extends Object
                    Self::Wildcard {
                        upper_bound: Some(upper),
                        lower_bound: None,
                    } => {
                        // Check if upper bound is Object
                        if let Self::Class(c) = upper.as_ref() {
                            let qualified = c.qualified_name();
                            qualified == "java.lang.Object"
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            }

            // Source is ? extends T - check wildcard compatibility
            (
                target_type,
                Self::Wildcard {
                    upper_bound: Some(source_upper),
                    lower_bound: None,
                },
            ) => {
                // For ? extends T on source side, target must accept subtypes
                // This is only valid if target is also a covariant wildcard
                match target_type {
                    Self::Wildcard {
                        upper_bound: Some(target_upper),
                        lower_bound: None,
                    } => {
                        // ? extends SubType is compatible with ? extends SuperType
                        // e.g., List<? extends Number> assignable to List<? extends Object>
                        target_upper.is_assignable_from_impl(
                            source_upper,
                            depth + 1,
                            state,
                            workspace,
                        )
                    }
                    Self::Wildcard {
                        upper_bound: None,
                        lower_bound: None,
                    } => true, // Unbounded accepts anything
                    _ => false,
                }
            }

            // Source is ? super T - check wildcard compatibility
            (
                target_type,
                Self::Wildcard {
                    upper_bound: None,
                    lower_bound: Some(source_lower),
                },
            ) => {
                // For ? super T on source side
                match target_type {
                    Self::Wildcard {
                        upper_bound: None,
                        lower_bound: Some(target_lower),
                    } => {
                        // ? super SubType is compatible with ? super SuperType
                        // e.g., List<? super Integer> assignable to List<? super Number>
                        // because Integer is a subtype of Number (contravariance)
                        source_lower.is_assignable_from_impl(
                            target_lower,
                            depth + 1,
                            state,
                            workspace,
                        )
                    }
                    Self::Wildcard {
                        upper_bound: None,
                        lower_bound: None,
                    } => true, // Unbounded accepts anything
                    _ => false,
                }
            }

            // TARGET IS A WILDCARD (source is not a wildcard at this point)

            // Covariant wildcard: ? extends T accepts subtypes of T
            (
                Self::Wildcard {
                    upper_bound: Some(upper),
                    lower_bound: None,
                },
                source_type,
            ) => {
                // Source (concrete type) must be assignable to the upper bound
                upper.is_assignable_from_impl(source_type, depth + 1, state, workspace)
            }

            // Contravariant wildcard: ? super T accepts supertypes of T
            (
                Self::Wildcard {
                    upper_bound: None,
                    lower_bound: Some(lower),
                },
                source_type,
            ) => {
                // Source (concrete type) must be a supertype of the lower bound
                // i.e., the lower bound must be assignable to source
                source_type.is_assignable_from_impl(lower, depth + 1, state, workspace)
            }

            // Both are concrete types - must be equal (invariance for generics)
            (Self::Class(c1), Self::Class(c2)) => {
                // For nested generics, compare raw names and recurse on type arguments
                let raw1 = c1.raw_qualified_name();
                let raw2 = c2.raw_qualified_name();

                if raw1 == raw2 {
                    // Recursively compare nested type arguments
                    Self::compare_type_arguments(
                        &c1.type_arguments,
                        &c2.type_arguments,
                        depth + 1,
                        state,
                        workspace,
                    )
                } else {
                    false
                }
            }

            // Type variables lack declaration IDs in this representation, so
            // matching a spelling alone is not enough: preserve their bound
            // shape as the strongest identity evidence available here.
            (
                Self::TypeVariable {
                    name: n1,
                    bounds: b1,
                },
                Self::TypeVariable {
                    name: n2,
                    bounds: b2,
                },
            ) => n1 == n2 && b1 == b2,

            // Primitives, arrays, etc. - use standard assignability
            _ => {
                target == source
                    || target.is_assignable_from_impl(source, depth + 1, state, workspace)
            }
        };

        state.active_type_arguments.remove(&pair);
        compatible
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(p) => write!(f, "{p}"),
            Self::Class(c) => write!(f, "{c}"),
            Self::Array {
                element_type,
                dimensions,
            } => {
                write!(f, "{element_type}")?;
                for _ in 0..*dimensions {
                    write!(f, "[]")?;
                }
                Ok(())
            }
            Self::TypeVariable { name, .. } => write!(f, "{name}"),
            Self::Wildcard {
                upper_bound: Some(bound),
                lower_bound: None,
            } => {
                write!(f, "? extends {bound}")
            }
            Self::Wildcard {
                upper_bound: None,
                lower_bound: Some(bound),
            } => {
                write!(f, "? super {bound}")
            }
            Self::Wildcard { .. } => write!(f, "?"),
            Self::Intersection { types } => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, " & ")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
            Self::Union { types } => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
            Self::Null => write!(f, "null"),
            Self::Void => write!(f, "void"),
            Self::Unknown => write!(f, "<unknown>"),
        }
    }
}

/// Java primitive types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Char,
    Float,
    Double,
}

impl PrimitiveType {
    /// Check if this primitive type can be widened to another
    pub const fn is_widening_from(self, other: Self) -> bool {
        use PrimitiveType::{Byte, Char, Double, Float, Int, Long, Short};
        matches!(
            (self, other),
            (Short, Byte)
                | (Int, Byte | Short | Char)
                | (Long, Byte | Short | Int | Char)
                | (Float, Byte | Short | Int | Long | Char)
                | (Double, Byte | Short | Int | Long | Char | Float)
        )
    }

    /// Get the wrapper class name for this primitive type
    pub const fn wrapper_class(&self) -> &'static str {
        match self {
            Self::Boolean => "Boolean",
            Self::Byte => "Byte",
            Self::Short => "Short",
            Self::Int => "Integer",
            Self::Long => "Long",
            Self::Char => "Character",
            Self::Float => "Float",
            Self::Double => "Double",
        }
    }
}

impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean => write!(f, "boolean"),
            Self::Byte => write!(f, "byte"),
            Self::Short => write!(f, "short"),
            Self::Int => write!(f, "int"),
            Self::Long => write!(f, "long"),
            Self::Char => write!(f, "char"),
            Self::Float => write!(f, "float"),
            Self::Double => write!(f, "double"),
        }
    }
}

/// A class or interface type
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassType {
    /// Simple name of the class
    pub name: String,
    /// Package name (None for default package)
    pub package: Option<String>,
    /// Type arguments for generic types
    pub type_arguments: Vec<Type>,
    /// True when diamond operator `<>` was used (type args should be inferred)
    pub is_diamond: bool,
}

impl ClassType {
    /// Create a new class type
    pub const fn new(name: String, package: Option<String>) -> Self {
        Self {
            name,
            package,
            type_arguments: vec![],
            is_diamond: false,
        }
    }

    /// Create a class type with type arguments
    pub const fn with_type_args(
        name: String,
        package: Option<String>,
        type_arguments: Vec<Type>,
    ) -> Self {
        Self {
            name,
            package,
            type_arguments,
            is_diamond: false,
        }
    }

    /// Create a class type with diamond operator (type args inferred from context)
    pub const fn with_diamond(name: String, package: Option<String>) -> Self {
        Self {
            name,
            package,
            type_arguments: vec![],
            is_diamond: true,
        }
    }

    /// Get the fully qualified name
    ///
    /// Returns `Cow::Borrowed` when there's no package (avoids allocation),
    /// `Cow::Owned` when package + name need concatenation.
    pub fn qualified_name(&self) -> std::borrow::Cow<'_, str> {
        self.package.as_ref().map_or_else(
            || std::borrow::Cow::Borrowed(self.name.as_str()),
            |pkg| std::borrow::Cow::Owned(format!("{pkg}.{}", self.name)),
        )
    }

    /// Get the raw name without generic type parameters
    ///
    /// Strips generics from the name. For example:
    /// - "List<String>" -> "List"
    /// - "ArrayList<>" -> "ArrayList"
    /// - "HashMap<K,V>" -> "HashMap"
    pub fn raw_name(&self) -> &str {
        // Find the '<' and return everything before it
        self.name.split('<').next().unwrap_or(&self.name)
    }

    /// Get the raw fully qualified name (without generic type parameters)
    pub fn raw_qualified_name(&self) -> String {
        let raw = self.raw_name();
        self.package
            .as_ref()
            .map_or_else(|| raw.to_string(), |pkg| format!("{pkg}.{raw}"))
    }

    /// Check if this type has a diamond operator <>
    ///
    /// The diamond operator means type arguments should be inferred from context
    pub const fn has_diamond(&self) -> bool {
        self.is_diamond
    }

    /// Create the java.lang.Object type
    pub fn object() -> Self {
        Self::new("Object".to_string(), Some("java.lang".into()))
    }

    /// Create the java.lang.String type
    pub fn string() -> Self {
        Self::new("String".to_string(), Some("java.lang".into()))
    }
}

impl fmt::Display for ClassType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.qualified_name())?;
        if !self.type_arguments.is_empty() {
            write!(f, "<")?;
            for (i, arg) in self.type_arguments.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{arg}")?;
            }
            write!(f, ">")?;
        }
        Ok(())
    }
}

/// The kind of scope (determines symbol resolution rules)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// Package-level scope
    Package,
    /// Class or interface scope
    Class,
    /// Method or constructor scope
    Method,
    /// Block scope (for loops, if statements, etc.)
    Block,
    /// Lambda expression scope
    Lambda,
}

/// A lexical scope that contains symbols
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The kind of scope
    pub kind: ScopeKind,
    /// Parent scope (None for root/package scope)
    pub parent: Option<ScopeId>,
    /// Child scopes
    pub children: Vec<ScopeId>,
    /// Symbols defined directly in this scope (name -> symbol ID)
    pub symbols: HashMap<String, SymbolId>,
    /// Imported symbols (for package/class scopes)
    pub imports: Vec<Import>,
    /// Text range of this scope (start and end offsets)
    /// None for scopes that don't correspond to a specific syntax node
    pub range: Option<(u32, u32)>,
    /// BUGFIX: File context for this scope
    /// Prevents cross-file scope collisions when multiple files have overlapping byte offsets
    /// None for scopes that span multiple files (e.g., package scopes)
    pub file_id: Option<u32>,
    /// Narrowed types from instanceof checks (smart cast)
    /// Maps variable name to its narrowed type within this scope.
    /// Example: after `if (obj instanceof String)`, obj is narrowed to String in the then-block.
    pub narrowed_types: HashMap<String, Type>,
}

impl Scope {
    /// Create a new scope
    pub fn new(kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            kind,
            parent,
            children: Vec::new(),
            symbols: HashMap::new(),
            imports: Vec::new(),
            range: None,
            file_id: None,
            narrowed_types: HashMap::new(),
        }
    }

    /// Create a new scope with a specific text range
    pub fn new_with_range(kind: ScopeKind, parent: Option<ScopeId>, start: u32, end: u32) -> Self {
        Self {
            kind,
            parent,
            children: Vec::new(),
            symbols: HashMap::new(),
            imports: Vec::new(),
            range: Some((start, end)),
            file_id: None,
            narrowed_types: HashMap::new(),
        }
    }

    /// Add a narrowed type for a variable in this scope
    ///
    /// This is used for smart cast after instanceof checks.
    pub fn add_narrowed_type(&mut self, name: String, ty: Type) {
        debug!(
            "Scope::add_narrowed_type: adding '{}' -> {:?} (scope kind={:?})",
            name, ty, self.kind
        );
        self.narrowed_types.insert(name, ty);
    }

    /// Get the narrowed type for a variable in this scope
    #[must_use]
    pub fn get_narrowed_type(&self, name: &str) -> Option<&Type> {
        self.narrowed_types.get(name)
    }

    /// Add a symbol to this scope
    pub fn add_symbol(&mut self, name: String, symbol_id: SymbolId) {
        self.symbols.insert(name, symbol_id);
    }

    /// Look up a symbol by name in this scope (does not search parent scopes)
    pub fn lookup_local(&self, name: &str) -> Option<SymbolId> {
        self.symbols.get(name).copied()
    }
}

/// Represents an import statement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The imported path (e.g., "java.util.List")
    pub path: String,
    /// Whether this is a static import
    pub is_static: bool,
    /// Whether this is a wildcard import (e.g., "java.util.*")
    pub is_wildcard: bool,
    /// Alias for the import (for future use, if Java adds import aliases)
    pub alias: Option<String>,
}

/// The main symbol table that stores all symbols and scopes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolTable {
    /// All symbols, indexed by their ID
    symbols: HashMap<SymbolId, Symbol>,
    /// All scopes, indexed by their ID
    scopes: HashMap<ScopeId, Scope>,
    /// Root (package-level) scope
    root_scope: ScopeId,
    /// Next symbol ID to allocate
    next_symbol_id: u64,
    /// Next scope ID to allocate
    next_scope_id: u64,
    // Note: resolution_cache was removed from SymbolTable to fix Salsa caching correctness.
    // The cache is now in SymbolResolver, which is per-request and not cached by Salsa.
}

/// Deeper than any real Java nesting; only a guard against a corrupted scope tree.
const MAX_SCOPE_DEPTH: usize = 512;

impl SymbolTable {
    /// Create a new symbol table with a root scope
    pub fn new() -> Self {
        let mut table = Self {
            symbols: HashMap::new(),
            scopes: HashMap::new(),
            root_scope: ScopeId(0),
            next_symbol_id: 0,
            next_scope_id: 1, // 0 is reserved for root
        };

        // Create root scope
        let root = Scope::new(ScopeKind::Package, None);
        table.scopes.insert(ScopeId(0), root);

        table
    }

    /// Get the root scope ID
    pub const fn root_scope(&self) -> ScopeId {
        self.root_scope
    }

    /// Create a new scope with the given parent
    pub fn create_scope(&mut self, kind: ScopeKind, parent: Option<ScopeId>) -> ScopeId {
        let scope_id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;

        let scope = Scope::new(kind, parent);
        self.scopes.insert(scope_id, scope);

        // A scope with no range sorts before every ranged sibling: it cannot be
        // selected by offset, so its position only needs to keep the ordering
        // invariant intact.
        if let Some(parent_id) = parent {
            if let Some(parent_scope) = self.scopes.get_mut(&parent_id) {
                parent_scope.children.insert(0, scope_id);
            }
        }

        scope_id
    }

    /// Create a new scope with a specific text range
    pub fn create_scope_with_range(
        &mut self,
        kind: ScopeKind,
        parent: Option<ScopeId>,
        start: u32,
        end: u32,
    ) -> ScopeId {
        // L10: Validate that scope range is well-formed
        debug_assert!(
            start <= end,
            "Invalid scope range: start ({start}) > end ({end}). Kind: {kind:?}"
        );

        // M25 FIX + L10: Validate that child scope range is within parent scope range
        if let Some(parent_id) = parent {
            if let Some(parent_scope) = self.scopes.get(&parent_id) {
                if let Some((p_start, p_end)) = parent_scope.range {
                    debug_assert!(
                        start >= p_start && end <= p_end,
                        "Child scope range ({}-{}) not within parent scope range ({}-{}). \
                         Kind: {:?}, Parent: {:?}",
                        start,
                        end,
                        p_start,
                        p_end,
                        kind,
                        parent_scope.kind
                    );
                }
            }
        }

        let scope_id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;

        let scope = Scope::new_with_range(kind, parent, start, end);
        self.scopes.insert(scope_id, scope);

        // Keep children ordered by start offset. Sibling scopes are disjoint, so
        // this lets `find_scope_at_offset` binary search instead of scanning
        // every sibling. Declarations are walked in source order, so this is a
        // push onto the end in the overwhelming majority of cases.
        if let Some(parent_id) = parent {
            let insert_at = self.scopes.get(&parent_id).map(|parent_scope| {
                parent_scope.children.partition_point(|child| {
                    self.scopes
                        .get(child)
                        .and_then(|child_scope| child_scope.range)
                        .is_some_and(|(child_start, _)| child_start <= start)
                })
            });
            if let (Some(insert_at), Some(parent_scope)) =
                (insert_at, self.scopes.get_mut(&parent_id))
            {
                parent_scope.children.insert(insert_at, scope_id);
            }
        }

        scope_id
    }

    /// Set the file_id for all scopes in this table.
    ///
    /// This should be called after building the symbol table to ensure
    /// cross-file scope disambiguation. All scopes in a single file share
    /// the same file_id.
    pub fn set_all_scopes_file_id(&mut self, file_id: FileId) {
        let file_id_val = file_id.0;
        for scope in self.scopes.values_mut() {
            scope.file_id = Some(file_id_val);
        }
    }

    /// Find the deepest (most specific) scope that contains the given offset
    ///
    /// Descends the scope tree, binary searching each level. This runs for
    /// essentially every identifier during resolution and type inference, so a
    /// linear scan of each level's children made whole-file work quadratic in
    /// the number of declarations: a generated file with a few thousand methods
    /// spent seconds here alone.
    #[must_use]
    pub fn find_scope_at_offset(&self, offset: u32) -> Option<ScopeId> {
        let mut scope_id = self.root_scope;
        if !self.scope_contains(scope_id, offset) {
            return None;
        }

        // Sibling scopes nest, so the chain of containing scopes is at most as
        // deep as the source nesting. The bound only guards against a scope tree
        // corrupted into a cycle by a construction bug.
        for _ in 0..MAX_SCOPE_DEPTH {
            match self.child_scope_at_offset(scope_id, offset) {
                Some(child) => scope_id = child,
                None => return Some(scope_id),
            }
        }

        tracing::error!(
            "Scope tree deeper than {} at {:?}; treating it as a construction cycle",
            MAX_SCOPE_DEPTH,
            scope_id
        );
        Some(scope_id)
    }

    fn scope_contains(&self, scope_id: ScopeId, offset: u32) -> bool {
        self.get_scope(scope_id).is_some_and(|scope| {
            // Scopes without ranges (like root) contain everything.
            scope
                .range
                .is_none_or(|(start, end)| offset >= start && offset <= end)
        })
    }

    /// The child of `scope_id` containing `offset`, if any.
    ///
    /// Children are kept sorted by start offset and are disjoint, so the only
    /// candidate is the last child starting at or before `offset`.
    fn child_scope_at_offset(&self, scope_id: ScopeId, offset: u32) -> Option<ScopeId> {
        let scope = self.get_scope(scope_id)?;
        let candidate = scope.children.partition_point(|child| {
            self.get_scope(*child)
                .and_then(|child_scope| child_scope.range)
                .is_some_and(|(start, _)| start <= offset)
        });
        let child = *scope.children.get(candidate.checked_sub(1)?)?;
        self.scope_contains(child, offset).then_some(child)
    }

    /// Look up a narrowed type for a variable at a specific offset
    ///
    /// Searches the scope at the given offset and walks up the scope chain
    /// to find any narrowed type for the variable. This is used for smart cast
    /// after instanceof checks.
    ///
    /// Returns the narrowed type if found, None otherwise.
    #[must_use]
    pub fn lookup_narrowed_type(&self, name: &str, offset: u32) -> Option<&Type> {
        debug!("lookup_narrowed_type: name='{}' offset={}", name, offset);
        let scope_id = self.find_scope_at_offset(offset)?;
        debug!("lookup_narrowed_type: found scope_id={:?}", scope_id);
        let result = self.lookup_narrowed_type_in_scope(name, scope_id);
        debug!("lookup_narrowed_type: result={:?}", result);
        result
    }

    /// Look up a narrowed type in a specific scope and its parent scopes
    pub fn lookup_narrowed_type_in_scope(
        &self,
        name: &str,
        mut scope_id: ScopeId,
    ) -> Option<&Type> {
        debug!(
            "lookup_narrowed_type_in_scope: name='{}' starting_scope={:?}",
            name, scope_id
        );
        loop {
            let scope = self.get_scope(scope_id)?;
            debug!("lookup_narrowed_type_in_scope: checking scope {:?}, kind={:?}, narrowed_types={:?}",
                   scope_id, scope.kind, scope.narrowed_types);

            // Check if this scope has a narrowing for the variable
            if let Some(ty) = scope.narrowed_types.get(name) {
                debug!(
                    "lookup_narrowed_type_in_scope: found narrowed type in scope {:?}: {:?}",
                    scope_id, ty
                );
                return Some(ty);
            }

            // Move to parent scope
            scope_id = scope.parent?;
        }
    }

    /// Add a symbol to the symbol table and associate it with a scope
    pub fn add_symbol(&mut self, symbol: Symbol, scope: ScopeId) -> SymbolId {
        let symbol_id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;

        let name = symbol.name.clone();
        self.symbols.insert(symbol_id, symbol);

        if let Some(scope_data) = self.scopes.get_mut(&scope) {
            scope_data.add_symbol(name, symbol_id);
        }

        symbol_id
    }

    /// Get a symbol by its ID
    pub fn get_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(&id)
    }

    /// Get a mutable reference to a symbol by its ID
    pub fn get_symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(&id)
    }

    /// Get all symbols in the symbol table
    ///
    /// Returns a vector of all symbols, useful for operations that need to
    /// process all symbols (e.g., type checking, analysis passes)
    pub fn all_symbols(&self) -> Vec<Symbol> {
        self.symbols.values().cloned().collect()
    }

    /// Get a scope by its ID
    pub fn get_scope(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.get(&id)
    }

    /// Get a mutable reference to a scope by its ID
    pub fn get_scope_mut(&mut self, id: ScopeId) -> Option<&mut Scope> {
        self.scopes.get_mut(&id)
    }

    /// Look up a symbol by name, starting from the given scope and walking up the scope chain
    ///
    /// Note: This method is intentionally not cached. Caching is handled by SymbolResolver
    /// which is created per-request and not stored in Salsa's cache.
    pub fn lookup_symbol(&self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        self.lookup_symbol_impl(name, scope)
    }

    /// Internal implementation of symbol lookup (without caching)
    fn lookup_symbol_impl(&self, name: &str, mut scope: ScopeId) -> Option<SymbolId> {
        loop {
            let current_scope = self.scopes.get(&scope)?;

            // Check local symbols first
            if let Some(symbol_id) = current_scope.lookup_local(name) {
                return Some(symbol_id);
            }

            // Check imports (for package/class scopes)
            if matches!(current_scope.kind, ScopeKind::Package | ScopeKind::Class) {
                for import in &current_scope.imports {
                    if import.is_wildcard {
                        // Would need to search all symbols in the imported package
                        // This requires additional infrastructure
                        continue;
                    }

                    // Check if the import matches this name
                    let import_name = import.path.split('.').next_back().unwrap_or("");
                    if import_name == name {
                        // Would need to resolve the import path to a symbol
                        // This requires additional infrastructure
                        continue;
                    }
                }
            }

            // Move to parent scope
            scope = current_scope.parent?;
        }
    }

    /// Look up a symbol for goto-definition, preferring fields over parameters.
    ///
    /// Resolve a symbol for navigation, respecting Java scoping semantics.
    ///
    /// Inner scopes shadow outer scopes: a method parameter shadows a class field
    /// with the same name. This is the correct behavior per JLS.
    pub fn lookup_symbol_for_navigation(&self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        // Just use standard lookup - innermost scope wins (correct shadowing)
        self.lookup_symbol(name, scope)
    }

    /// Look up a symbol at a concrete source offset using Java's
    /// point-of-declaration rules.
    ///
    /// Fields, methods, parameters, and member types are visible throughout
    /// their lexical scope. Local variables and local types are visible only
    /// from their declarator onward. If a not-yet-visible local shadows an
    /// outer symbol, lookup continues in the parent scope.
    pub fn lookup_symbol_at_offset(
        &self,
        name: &str,
        mut scope: ScopeId,
        offset: u32,
    ) -> Option<SymbolId> {
        loop {
            let current_scope = self.scopes.get(&scope)?;
            if let Some(symbol_id) = current_scope.lookup_local(name) {
                let symbol = self.symbols.get(&symbol_id)?;
                let starts_at_declaration = matches!(symbol.kind, SymbolKind::LocalVariable { .. })
                    || (matches!(symbol.kind, SymbolKind::Class { .. })
                        && matches!(current_scope.kind, ScopeKind::Block | ScopeKind::Method));

                if !starts_at_declaration || symbol.location.range.start <= offset {
                    return Some(symbol_id);
                }
            }

            scope = current_scope.parent?;
        }
    }

    /// Get all symbols defined in a scope (including parent scopes)
    pub fn symbols_in_scope(&self, scope: ScopeId) -> Vec<(String, SymbolId)> {
        let mut result = Vec::new();
        let mut current = Some(scope);

        while let Some(scope_id) = current {
            if let Some(scope_data) = self.scopes.get(&scope_id) {
                for (name, symbol_id) in &scope_data.symbols {
                    result.push((name.clone(), *symbol_id));
                }
                current = scope_data.parent;
            } else {
                break;
            }
        }

        result
    }

    /// Get all symbols defined directly in a scope (not including parent scopes)
    pub fn local_symbols(&self, scope: ScopeId) -> Vec<(String, SymbolId)> {
        self.scopes
            .get(&scope)
            .map(|s| s.symbols.iter().map(|(k, v)| (k.clone(), *v)).collect())
            .unwrap_or_default()
    }

    /// Find all references to a symbol across all scopes
    pub fn find_references(&self, symbol_id: SymbolId) -> Vec<ScopeId> {
        if !self.symbols.contains_key(&symbol_id) {
            return vec![];
        }

        let mut references = Vec::new();

        for (scope_id, scope) in &self.scopes {
            if scope.symbols.values().any(|id| *id == symbol_id) {
                references.push(*scope_id);
            }
        }

        references
    }

    /// Find the enclosing class scope for a given scope
    pub fn find_enclosing_class(&self, mut scope: ScopeId) -> Option<ScopeId> {
        while let Some(scope_data) = self.scopes.get(&scope) {
            if scope_data.kind == ScopeKind::Class {
                return Some(scope);
            }
            scope = scope_data.parent?;
        }
        None
    }

    /// Find the enclosing package scope for a given scope
    pub fn find_enclosing_package(&self, mut scope: ScopeId) -> Option<ScopeId> {
        while let Some(scope_data) = self.scopes.get(&scope) {
            if scope_data.kind == ScopeKind::Package {
                return Some(scope);
            }
            scope = scope_data.parent?;
        }
        None
    }

    /// Find the enclosing method scope for a given scope
    pub fn find_enclosing_method(&self, mut scope: ScopeId) -> Option<ScopeId> {
        while let Some(scope_data) = self.scopes.get(&scope) {
            if scope_data.kind == ScopeKind::Method {
                return Some(scope);
            }
            scope = scope_data.parent?;
        }
        None
    }

    /// Find a symbol by name across all scopes (exhaustive search)
    ///
    /// This searches all scopes in the symbol table, not just following the scope chain.
    /// This is useful as a fallback for goto-definition when normal scope-based resolution fails.
    pub fn find_symbol_anywhere(&self, name: &str) -> Option<SymbolId> {
        for scope in self.scopes.values() {
            if let Some(symbol_id) = scope.symbols.get(name) {
                return Some(*symbol_id);
            }
        }
        None
    }

    /// Find the scope that directly defines a given symbol.
    pub fn find_scope_for_symbol(&self, target: SymbolId) -> Option<ScopeId> {
        for (scope_id, scope) in &self.scopes {
            if scope.symbols.values().any(|id| *id == target) {
                return Some(*scope_id);
            }
        }
        None
    }

    /// Get statistics about the symbol table
    pub fn statistics(&self) -> SymbolTableStatistics {
        SymbolTableStatistics {
            symbol_count: self.symbols.len(),
            scope_count: self.scopes.len(),
        }
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about a symbol table
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolTableStatistics {
    pub symbol_count: usize,
    pub scope_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_creation() {
        let table = SymbolTable::new();
        assert_eq!(table.statistics().symbol_count, 0);
        assert_eq!(table.statistics().scope_count, 1); // Root scope
    }

    #[test]
    fn test_scope_creation() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let class_scope = table.create_scope(ScopeKind::Class, Some(root));

        assert_eq!(table.statistics().scope_count, 2);
        assert_eq!(table.get_scope(class_scope).unwrap().kind, ScopeKind::Class);
    }

    #[test]
    fn test_symbol_addition() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();

        let symbol = Symbol::new(
            "myVariable".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Int),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 10),
        );

        let symbol_id = table.add_symbol(symbol, root);
        assert!(table.get_symbol(symbol_id).is_some());
    }

    #[test]
    fn test_symbol_lookup() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();

        let symbol = Symbol::new(
            "count".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 0, 5),
        );

        table.add_symbol(symbol, root);

        let found = table.lookup_symbol("count", root);
        assert!(found.is_some());

        let not_found = table.lookup_symbol("missing", root);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_hierarchical_lookup() {
        let mut table = SymbolTable::new();
        let root = table.root_scope();
        let class_scope = table.create_scope(ScopeKind::Class, Some(root));
        let method_scope = table.create_scope(ScopeKind::Method, Some(class_scope));

        // Add symbol to class scope
        let field_symbol = Symbol::new(
            "field".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 0, 5),
        );
        table.add_symbol(field_symbol, class_scope);

        // Should be able to find it from method scope
        let found = table.lookup_symbol("field", method_scope);
        assert!(found.is_some());
    }

    #[test]
    fn test_type_display() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        assert_eq!(format!("{}", int_type), "int");

        let string_type = Type::Class(ClassType::string());
        assert_eq!(format!("{}", string_type), "java.lang.String");

        let array_type = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 2,
        };
        assert_eq!(format!("{}", array_type), "int[][]");
    }

    #[test]
    fn test_primitive_widening() {
        use PrimitiveType::*;

        assert!(Int.is_widening_from(Byte));
        assert!(Long.is_widening_from(Int));
        assert!(Double.is_widening_from(Float));
        assert!(!Int.is_widening_from(Long));
    }

    #[test]
    fn test_type_assignability() {
        let int_type = Type::Primitive(PrimitiveType::Int);
        let byte_type = Type::Primitive(PrimitiveType::Byte);

        // int can be assigned from byte (widening)
        assert!(int_type.is_assignable_from(&byte_type));

        // byte cannot be assigned from int (narrowing)
        assert!(!byte_type.is_assignable_from(&int_type));

        // Reference types can be assigned null
        let string_type = Type::Class(ClassType::string());
        assert!(string_type.is_assignable_from(&Type::Null));
    }

    #[test]
    fn test_modifiers() {
        let mut mods = Modifiers::empty();
        assert!(!mods.is_static());

        mods.set(Modifiers::STATIC);
        assert!(mods.is_static());
        assert!(!mods.is_final());

        mods.set(Modifiers::FINAL);
        assert!(mods.is_static());
        assert!(mods.is_final());
    }

    #[test]
    fn test_generic_type_arguments_invariance() {
        // List<String> should NOT be assignable from List<Integer>
        let list_string = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));
        let list_integer = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::new(
                "Integer".into(),
                Some("java.lang".into()),
            ))],
        ));

        // Different type arguments should NOT be compatible (invariance)
        assert!(!list_string.is_assignable_from(&list_integer));
        assert!(!list_integer.is_assignable_from(&list_string));

        // Same type arguments SHOULD be compatible
        let list_string2 = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));
        assert!(list_string.is_assignable_from(&list_string2));
    }

    #[test]
    fn test_generic_type_arguments_raw_types() {
        // Raw types (no type args) should be compatible with parameterized types
        let list_raw = Type::Class(ClassType::new("List".into(), Some("java.util".into())));
        let list_string = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));

        // Raw type can be assigned from parameterized type (Java backward compat)
        assert!(list_raw.is_assignable_from(&list_string));
        // Parameterized type can be assigned from raw type (Java backward compat)
        assert!(list_string.is_assignable_from(&list_raw));
    }

    #[test]
    fn test_generic_type_arguments_unbounded_wildcard() {
        // List<?> should accept any List<T>
        let list_wildcard = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
        ));
        let list_string = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));

        assert!(list_wildcard.is_assignable_from(&list_string));
    }

    #[test]
    fn test_generic_type_arguments_covariant() {
        // List<? extends Collection> should accept List<List>
        let collection_type = Type::Class(ClassType::new(
            "Collection".into(),
            Some("java.util".into()),
        ));
        let list_type = Type::Class(ClassType::new("List".into(), Some("java.util".into())));

        let set_extends_collection = Type::Class(ClassType::with_type_args(
            "Set".into(),
            Some("java.util".into()),
            vec![Type::Wildcard {
                upper_bound: Some(Box::new(collection_type)),
                lower_bound: None,
            }],
        ));
        let set_list = Type::Class(ClassType::with_type_args(
            "Set".into(),
            Some("java.util".into()),
            vec![list_type],
        ));

        // Set<? extends Collection> should accept Set<List>
        // (because List is a subtype of Collection)
        assert!(set_extends_collection.is_assignable_from(&set_list));
    }

    #[test]
    fn test_generic_type_arguments_contravariant() {
        // List<? super List> should accept List<Collection>
        let collection_type = Type::Class(ClassType::new(
            "Collection".into(),
            Some("java.util".into()),
        ));
        let list_type = Type::Class(ClassType::new("List".into(), Some("java.util".into())));

        let set_super_list = Type::Class(ClassType::with_type_args(
            "Set".into(),
            Some("java.util".into()),
            vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(list_type)),
            }],
        ));
        let set_collection = Type::Class(ClassType::with_type_args(
            "Set".into(),
            Some("java.util".into()),
            vec![collection_type],
        ));

        // Set<? super List> should accept Set<Collection>
        // (because Collection is a supertype of List)
        assert!(set_super_list.is_assignable_from(&set_collection));
    }

    #[test]
    fn test_wildcard_lower_bounds_are_compared_contravariantly() {
        let number = Type::Class(ClassType::new("Number".into(), Some("java.lang".into())));
        let integer = Type::Class(ClassType::new("Integer".into(), Some("java.lang".into())));
        let list_super = |lower| {
            Type::Class(ClassType::with_type_args(
                "List".into(),
                Some("java.util".into()),
                vec![Type::Wildcard {
                    upper_bound: None,
                    lower_bound: Some(Box::new(lower)),
                }],
            ))
        };

        let target_super_integer = list_super(integer.clone());
        let source_super_number = list_super(number.clone());
        assert!(target_super_integer.is_assignable_from(&source_super_number));

        let target_super_number = list_super(number);
        let source_super_integer = list_super(integer);
        assert!(!target_super_number.is_assignable_from(&source_super_integer));
    }

    #[test]
    fn test_generic_type_arguments_nested() {
        // Map<String, List<Integer>> should NOT match Map<String, List<String>>
        let list_integer = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::new(
                "Integer".into(),
                Some("java.lang".into()),
            ))],
        ));
        let list_string = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string())],
        ));

        let map_string_list_integer = Type::Class(ClassType::with_type_args(
            "Map".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string()), list_integer],
        ));
        let map_string_list_string = Type::Class(ClassType::with_type_args(
            "Map".into(),
            Some("java.util".into()),
            vec![Type::Class(ClassType::string()), list_string],
        ));

        // Nested generics with different inner type args should NOT match
        assert!(!map_string_list_integer.is_assignable_from(&map_string_list_string));
        assert!(!map_string_list_string.is_assignable_from(&map_string_list_integer));
    }

    #[test]
    fn test_recursive_generic_bounds_no_stack_overflow() {
        // Regression test for Type assignability infinite recursion
        //
        // This test verifies that recursive generic types like Enum<E extends Enum<E>>
        // don't cause stack overflow when checking type compatibility.
        //
        // Pair-based recursion tracking must terminate recursive-bound comparisons
        // without truncating unrelated, deeply nested generic trees.

        // Simulate Enum<E extends Enum<E>> pattern
        // This is a self-referential generic bound common in Java stdlib
        let enum_type = Type::Class(ClassType::with_type_args(
            "Enum".into(),
            Some("java.lang".into()),
            vec![Type::TypeVariable {
                name: "E".into(),
                bounds: vec![Type::Class(ClassType::with_type_args(
                    "Enum".into(),
                    Some("java.lang".into()),
                    vec![Type::TypeVariable {
                        name: "E".into(),
                        bounds: vec![],
                    }],
                ))],
            }],
        ));

        // Should not stack overflow when comparing with itself
        assert!(enum_type.is_assignable_from(&enum_type));

        // Test deeply nested generics (not recursive, just deep).
        let mut deep_type: Type = Type::Primitive(PrimitiveType::Int);
        for _ in 0..96 {
            deep_type = Type::Class(ClassType::with_type_args(
                "Map".into(),
                Some("java.util".into()),
                vec![Type::Class(ClassType::string()), deep_type],
            ));
        }

        // Structural identity remains compatible beyond the old depth limit.
        assert!(deep_type.is_assignable_from(&deep_type));
    }

    #[test]
    fn test_deep_generic_mismatch_is_not_assumed_compatible() {
        fn nested(leaf: Type) -> Type {
            (0..96).fold(leaf, |inner, _| {
                Type::Class(ClassType::with_type_args(
                    "Box".into(),
                    Some("example".into()),
                    vec![inner],
                ))
            })
        }

        let target = nested(Type::Class(ClassType::string()));
        let matching_source = nested(Type::Class(ClassType::string()));
        let source = nested(Type::Class(ClassType::new(
            "Integer".into(),
            Some("java.lang".into()),
        )));

        assert!(target.is_assignable_from(&matching_source));
        assert!(!target.is_assignable_from(&source));
        assert!(!source.is_assignable_from(&target));
    }

    #[test]
    fn recursive_bound_shapes_do_not_hide_a_leaf_mismatch() {
        fn enum_shape(leaf: Type) -> Type {
            Type::Class(ClassType::with_type_args(
                "Enum".into(),
                Some("java.lang".into()),
                vec![Type::TypeVariable {
                    name: "E".into(),
                    bounds: vec![Type::Class(ClassType::with_type_args(
                        "Comparable".into(),
                        Some("java.lang".into()),
                        vec![leaf],
                    ))],
                }],
            ))
        }

        let target = enum_shape(Type::Class(ClassType::string()));
        let matching_source = enum_shape(Type::Class(ClassType::string()));
        let mismatching_source = enum_shape(Type::Class(ClassType::new(
            "Integer".into(),
            Some("java.lang".into()),
        )));

        assert!(target.is_assignable_from(&matching_source));
        assert!(!target.is_assignable_from(&mismatching_source));
    }

    #[test]
    fn test_comparable_recursive_bound() {
        // Another common recursive bound: T extends Comparable<T>
        //
        // This pattern is used throughout Java stdlib for natural ordering.
        // Example: public class String implements Comparable<String>

        let comparable_t = Type::Class(ClassType::with_type_args(
            "Comparable".into(),
            Some("java.lang".into()),
            vec![Type::TypeVariable {
                name: "T".into(),
                bounds: vec![Type::Class(ClassType::with_type_args(
                    "Comparable".into(),
                    Some("java.lang".into()),
                    vec![Type::TypeVariable {
                        name: "T".into(),
                        bounds: vec![],
                    }],
                ))],
            }],
        ));

        // Should handle recursive bound without stack overflow
        assert!(comparable_t.is_assignable_from(&comparable_t));

        // Test with a concrete type implementing Comparable
        let string_comparable = Type::Class(ClassType::with_type_args(
            "Comparable".into(),
            Some("java.lang".into()),
            vec![Type::Class(ClassType::string())],
        ));

        // Comparable<String> to Comparable<String> should match
        assert!(string_comparable.is_assignable_from(&string_comparable));
    }

    #[test]
    fn test_scope_range_validation() {
        // M25 TEST: Validate that child scope ranges must be within parent scope ranges
        let mut table = SymbolTable::new();

        // Create parent scope with range 0-100
        let parent = table.create_scope_with_range(ScopeKind::Class, None, 0, 100);

        // Valid child scope within parent range
        let child1 = table.create_scope_with_range(ScopeKind::Method, Some(parent), 10, 50);
        assert_eq!(table.get_scope(child1).unwrap().range, Some((10, 50)));

        // Valid child scope at parent boundaries
        let child2 = table.create_scope_with_range(ScopeKind::Method, Some(parent), 0, 100);
        assert_eq!(table.get_scope(child2).unwrap().range, Some((0, 100)));

        // Verify parent has children
        let parent_scope = table.get_scope(parent).unwrap();
        assert_eq!(parent_scope.children.len(), 2);
        assert!(parent_scope.children.contains(&child1));
        assert!(parent_scope.children.contains(&child2));
    }

    #[test]
    #[should_panic(expected = "Child scope range")]
    #[cfg(debug_assertions)]
    fn test_scope_range_validation_fails_on_invalid_start() {
        // M25 TEST: Ensure debug_assert! fires when child scope starts before parent
        let mut table = SymbolTable::new();

        // Create parent scope with range 10-100
        let parent = table.create_scope_with_range(ScopeKind::Class, None, 10, 100);

        // Invalid: child starts before parent (should panic in debug builds)
        table.create_scope_with_range(ScopeKind::Method, Some(parent), 5, 50);
    }

    #[test]
    #[should_panic(expected = "Child scope range")]
    #[cfg(debug_assertions)]
    fn test_scope_range_validation_fails_on_invalid_end() {
        // M25 TEST: Ensure debug_assert! fires when child scope ends after parent
        let mut table = SymbolTable::new();

        // Create parent scope with range 0-50
        let parent = table.create_scope_with_range(ScopeKind::Class, None, 0, 50);

        // Invalid: child ends after parent (should panic in debug builds)
        table.create_scope_with_range(ScopeKind::Method, Some(parent), 10, 100);
    }
}
