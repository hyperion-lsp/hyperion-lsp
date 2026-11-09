// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared file, symbol, and semantic declaration model types.
//!
//! These data shapes are used across indexing, language analysis, and query
//! code. Content snapshots and revision-scoped aggregate results remain with
//! the workspace API.

use hyperion_types::{DeclarationModifiers as Modifiers, TextRange, TypeParameterInfo};
use lsp_types::SymbolKind;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use std::sync::Arc;

/// File identifier in the workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

impl FileId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

/// Source of a file in the workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileSource {
    /// Workspace file (user code)
    Workspace,
    /// JDK/standard library class
    Jdk,
    /// External dependency (JAR, etc.)
    External,
}

/// Information about a symbol's location
///
/// PERF: Uses Arc<str> instead of String to enable cheap cloning (refcount increment)
/// This reduces allocations during indexing from 118 clones/file to near-zero
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolLocation {
    pub file_id: FileId,
    /// Exact source publication that owns `range`.
    ///
    /// Stored index entries leave this unset. Query APIs stamp it while cloning
    /// the location under their read lock, so consumers can carry provenance
    /// without a second structural lookup. The low bit distinguishes workspace
    /// publication IDs from external archive generations; the remaining bits
    /// retain the monotonic ID. If a process ever exceeds the 63-bit compact
    /// range, query stamping fails closed rather than growing this
    /// high-cardinality structure.
    #[serde(skip)]
    pub(crate) source_revision_id: Option<NonZeroU64>,
    pub range: TextRange,
    pub kind: SymbolKind,
    #[serde(with = "arc_str_serde")]
    pub name: Arc<str>,
    #[serde(with = "arc_str_serde")]
    pub fqn: Arc<str>, // Fully qualified name (e.g., "java.util.ArrayList")
    #[serde(with = "option_arc_str_serde")]
    pub container_name: Option<Arc<str>>, // e.g., class name for methods
    #[serde(with = "option_arc_str_serde")]
    pub documentation: Option<Arc<str>>, // JavaDoc or documentation for external libraries
}

impl SymbolLocation {
    pub(crate) fn set_source_revision(&mut self, revision: NonZeroU64, external: bool) {
        self.source_revision_id = revision
            .get()
            .checked_mul(2)
            .and_then(|encoded| NonZeroU64::new(encoded | u64::from(external)));
    }

    pub(crate) fn workspace_source_revision(&self) -> Option<NonZeroU64> {
        self.decoded_source_revision(false)
    }

    pub(crate) fn external_source_generation(&self) -> Option<NonZeroU64> {
        self.decoded_source_revision(true)
    }

    fn decoded_source_revision(&self, external: bool) -> Option<NonZeroU64> {
        let encoded = self.source_revision_id?.get();
        if encoded & 1 != u64::from(external) {
            return None;
        }
        NonZeroU64::new(encoded >> 1)
    }
}

/// Return whether a symbol kind declares a concrete nominal type.
///
/// This is the canonical filter for simple-name type lookup across Java and
/// the JVM languages supported by the workspace.
pub const fn is_type_declaration_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::CLASS
            | SymbolKind::INTERFACE
            | SymbolKind::ENUM
            | SymbolKind::OBJECT
            | SymbolKind::STRUCT
    )
}

/// Semantic declaration namespace used to distinguish equal textual FQNs.
///
/// Java permits declarations such as a nested type and a field, or a
/// constructor and an ordinary same-named method, to share the exact string
/// used by the legacy index. Keeping this separate from LSP's presentation
/// `SymbolKind` makes semantic identity explicit without widening every index
/// entry.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeclarationNamespace {
    Type,
    Constructor,
    Callable,
    Value,
    Other,
}

impl DeclarationNamespace {
    /// Semantic namespace occupied by a declaration kind.
    pub const fn for_kind(kind: SymbolKind) -> Self {
        if is_type_declaration_kind(kind) {
            Self::Type
        } else {
            match kind {
                SymbolKind::CONSTRUCTOR => Self::Constructor,
                SymbolKind::METHOD | SymbolKind::FUNCTION => Self::Callable,
                SymbolKind::FIELD
                | SymbolKind::PROPERTY
                | SymbolKind::CONSTANT
                | SymbolKind::ENUM_MEMBER => Self::Value,
                _ => Self::Other,
            }
        }
    }

    /// Whether `kind` belongs to this semantic namespace.
    pub fn accepts(self, kind: SymbolKind) -> bool {
        match self {
            Self::Type => is_type_declaration_kind(kind),
            Self::Constructor => kind == SymbolKind::CONSTRUCTOR,
            Self::Callable => matches!(kind, SymbolKind::METHOD | SymbolKind::FUNCTION),
            Self::Value => matches!(
                kind,
                SymbolKind::FIELD
                    | SymbolKind::PROPERTY
                    | SymbolKind::CONSTANT
                    | SymbolKind::ENUM_MEMBER
            ),
            Self::Other => {
                !is_type_declaration_kind(kind)
                    && !matches!(
                        kind,
                        SymbolKind::CONSTRUCTOR
                            | SymbolKind::METHOD
                            | SymbolKind::FUNCTION
                            | SymbolKind::FIELD
                            | SymbolKind::PROPERTY
                            | SymbolKind::CONSTANT
                            | SymbolKind::ENUM_MEMBER
                    )
            }
        }
    }

    pub(crate) const ALL: [Self; 5] = [
        Self::Type,
        Self::Constructor,
        Self::Callable,
        Self::Value,
        Self::Other,
    ];
}

/// Exact semantic declaration identity used by workspace queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolTarget {
    pub fqn: Arc<str>,
    pub namespace: DeclarationNamespace,
}

impl SymbolTarget {
    pub fn new(fqn: impl Into<Arc<str>>, namespace: DeclarationNamespace) -> Self {
        Self {
            fqn: fqn.into(),
            namespace,
        }
    }

    pub fn for_symbol(symbol: &Symbol) -> Self {
        Self::new(
            Arc::clone(&symbol.fqn),
            DeclarationNamespace::for_kind(symbol.kind),
        )
    }
}

/// Result of resolving one source-level type name in a concrete file context.
///
/// Ambiguity is data, not ordering: language-specific on-demand imports do not
/// gain precedence from declaration order.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TypeNameResolution {
    Exact(String),
    Missing,
    Ambiguous(Vec<String>),
}

/// Explicit type arguments written on one direct supertype edge.
///
/// `written_target` deliberately has the same contextual spelling as
/// [`Symbol::parent_class`] or an entry in [`Symbol::interfaces`]. The index
/// resolves that spelling against the exact declaring publication before it
/// composes arguments across multiple hierarchy edges.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GenericSupertype {
    pub written_target: String,
    pub arguments: Vec<String>,
}

/// Generic hierarchy metadata owned by one nominal declaration.
///
/// This is stored separately from [`Symbol`] so the overwhelmingly more common
/// method and field symbols do not pay for type-only metadata.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypeGenericSupertypes {
    pub type_fqn: String,
    pub edges: Vec<GenericSupertype>,
}

/// Canonical hierarchy facts for one proven indexed source revision.
///
/// The fingerprint hashes every edge's Exact/Missing/Ambiguous state, not just
/// the returned exact targets. It therefore changes when an unchanged source
/// file is rebound by a newly arrived same-package or on-demand declaration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedDirectSupertypes {
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub fingerprint: u64,
}

/// Canonical hierarchy facts for one nominal declaration in a file revision.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedTypeSupertypes {
    pub type_fqn: String,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub fingerprint: u64,
}

/// A symbol definition extracted from source code
///
/// PERF: Uses Arc<str> instead of String to enable cheap cloning (refcount increment)
/// This reduces allocations during indexing from 118 clones/file to near-zero
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Fully qualified name (e.g., "com.acme.utils.ImageTools")
    #[serde(with = "arc_str_serde")]
    pub fqn: Arc<str>,
    /// Simple name (e.g., "ImageTools")
    #[serde(with = "arc_str_serde")]
    pub simple_name: Arc<str>,
    /// Location in source
    pub range: TextRange,
    /// Kind of symbol (class, interface, method, etc.)
    pub kind: SymbolKind,
    /// Modifiers (public, static, final, etc.)
    pub modifiers: Modifiers,
    /// Container FQN (e.g., class for a method)
    #[serde(with = "option_arc_str_serde")]
    pub container: Option<Arc<str>>,
    /// Return type for methods. Java source and bytecode declarations retain a
    /// canonical qualified identity when it is available; other frontends may
    /// preserve source-language spelling. Used for method chaining resolution.
    #[serde(with = "option_arc_str_serde")]
    pub return_type: Option<Arc<str>>,
    /// Parameter types for methods
    #[serde(with = "vec_arc_str_serde")]
    pub parameters: Vec<Arc<str>>,
    /// Written parent type for source declarations, or an exact FQN for
    /// external bytecode declarations. Java annotation declarations use this
    /// otherwise-unused slot as a compact exact
    /// `java.lang.annotation.Annotation` marker; canonical queries expose it
    /// with interface role.
    ///
    /// Source spelling is intentionally preserved: an indexed name that is
    /// unique today can become shadowed or ambiguous when more workspace or
    /// classpath declarations arrive. Use `WorkspaceIndex::direct_supertypes`
    /// when a canonical, currently valid edge is required.
    #[serde(with = "option_arc_str_serde")]
    pub parent_class: Option<Arc<str>>,
    /// Written interface types for source declarations, or exact FQNs for
    /// external bytecode declarations. Bare Kotlin supertypes whose role is
    /// not syntactically known also live here and are classified by resolved
    /// declaration kind before member precedence is applied. See
    /// `parent_class` for the open-world resolution contract.
    #[serde(with = "vec_arc_str_serde")]
    pub interfaces: Vec<Arc<str>>,
    /// JavaDoc or documentation for this symbol
    #[serde(with = "option_arc_str_serde")]
    pub documentation: Option<Arc<str>>,
    /// Type parameters for generic classes/interfaces/methods (e.g., [T, K, V])
    /// Used for generic type substitution
    pub type_parameters: Vec<TypeParameterInfo>,
    /// Whether this symbol is marked with @Deprecated annotation
    #[serde(default)]
    pub is_deprecated: bool,
    /// Receiver type for extension functions (Kotlin)
    /// e.g., "String" for `fun String.hello() = ...`
    /// None for regular functions/methods
    #[serde(default)]
    #[serde(with = "option_arc_str_serde")]
    pub receiver_type: Option<Arc<str>>,
    /// Kotlin-specific declaration facts recovered from `@kotlin.Metadata`.
    ///
    /// This is `None` for Java symbols and Kotlin source symbols that have not
    /// gone through metadata-aware indexing.
    #[serde(default)]
    pub kotlin: Option<Box<KotlinSymbolMetadata>>,
    /// Exception types named in a method's or constructor's `throws` clause,
    /// as qualified names (source declarations resolve the written names as
    /// best extraction can; bytecode declarations are exact). Empty for
    /// everything else.
    #[serde(default, with = "option_arc_vec_serde")]
    pub throws: Throws,
}

/// The exception types of a `throws` clause. Thin and nullable on purpose:
/// nearly every symbol declares none, and `Symbol` is held per declaration.
pub type Throws = Option<Arc<Vec<Arc<str>>>>;

impl Symbol {
    /// The declared `throws` types (empty when there are none).
    pub fn throws(&self) -> &[Arc<str>] {
        self.throws.as_deref().map_or(&[], Vec::as_slice)
    }

    /// Whether this symbol is a method (Java) or function (Kotlin)
    pub fn is_method(&self) -> bool {
        matches!(self.kind, SymbolKind::METHOD | SymbolKind::FUNCTION)
    }

    /// Check if this symbol is an enum member
    pub fn is_enum_member(&self) -> bool {
        self.kind == SymbolKind::ENUM_MEMBER
    }

    /// Check if this symbol is a class
    pub fn is_class(&self) -> bool {
        self.kind == SymbolKind::CLASS
    }

    /// Check if this symbol is an interface
    pub fn is_interface(&self) -> bool {
        self.kind == SymbolKind::INTERFACE
    }

    /// Check if this symbol is a class or interface
    pub fn is_class_or_interface(&self) -> bool {
        self.is_class() || self.is_interface()
    }
}

/// Kotlin-specific metadata attached to a workspace symbol.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KotlinSymbolMetadata {
    /// Nullability of the callable/property return type.
    pub return_type_nullable: bool,
    /// Nullability of an extension receiver, when present.
    pub receiver_type_nullable: bool,
    /// Per-parameter Kotlin facts, in the same order as `Symbol::parameters`.
    pub parameters: Vec<KotlinParameterMetadata>,
    pub is_suspend: bool,
    pub is_operator: bool,
    pub is_inline: bool,
    /// Property facts are present only when this symbol is a Kotlin property.
    pub property: Option<KotlinPropertyMetadata>,
    /// Set on symbols that exist only in the JVM view of a Kotlin declaration
    /// (file-facade members, property accessors, `@JvmStatic` forwarders): the
    /// FQN of the Kotlin declaration they derive from.
    #[serde(default, with = "option_arc_str_serde")]
    pub jvm_origin: Option<Arc<str>>,
    /// Package-level typealiases declared by one Kotlin file, set only on that
    /// file's facade class symbol (`kotlin.TypeAliasesKt`): the public ones of
    /// a library file, and the non-private ones of a workspace source file
    /// whose expansion that file's own text qualifies. The index maps each
    /// alias to its expansion for Kotlin type-name resolution. The index is
    /// bincode, which is positional and ignores `serde(default)`, so the cache
    /// version is bumped with this field.
    #[serde(default)]
    pub type_aliases: Vec<KotlinTypeAlias>,
}

/// A Kotlin `typealias`, read from library metadata or from workspace source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KotlinTypeAlias {
    /// Fully qualified alias name, for example `kotlin.Comparator`.
    pub fqn: String,
    /// Fully qualified class the alias expands to, for example `java.util.Comparator`.
    pub expanded_class: String,
    /// The alias's own type parameters, in declaration order: `["K", "V"]`
    /// for `typealias Inverse<K, V> = java.util.HashMap<V, K>`. Empty for an
    /// alias that declares none.
    pub type_parameters: Vec<String>,
    /// The type arguments the expansion gives `expanded_class`, as Kotlin type
    /// spellings: `["String"]` for `typealias Names = java.util.ArrayList<String>`.
    /// A library alias's are qualified (`kotlin.String`). A source alias's are
    /// qualified the way its expanded class is, from that file's text alone,
    /// and are otherwise as written. A name in `type_parameters` stands for
    /// the argument a use gives that parameter (`["V", "K"]` above). Empty
    /// when the expansion has none.
    pub expanded_type_arguments: Vec<String>,
}

/// What a Kotlin type written as a name with type arguments gives the class
/// the name resolves to, when the name is a typealias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KotlinTypeAliasArguments {
    /// The name is not a typealias, or not in every binding it has: the
    /// written arguments apply to the class as they are.
    NotAnAlias,
    /// The type arguments the alias's expansion gives its class at this use,
    /// as Kotlin spellings, with the use's arguments in place of the alias's
    /// type parameters: `["String", "Int"]` for `Inverse<Int, String>` under
    /// `typealias Inverse<K, V> = java.util.HashMap<V, K>`.
    Expanded(Vec<String>),
    /// The name is a typealias whose type parameters the use's arguments do
    /// not match: there are more or fewer of them, one is missing, or a
    /// projection would stand where the expansion cannot take one. The use
    /// is left unresolved rather than guessed.
    Unmatched,
}

/// Kotlin facts attached to one callable parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KotlinParameterMetadata {
    pub nullable: bool,
    pub has_default: bool,
}

/// Identity and mutability/accessor facts for a Kotlin property.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KotlinPropertyMetadata {
    pub is_val: bool,
    pub has_getter: bool,
    pub has_setter: bool,
}

/// A reference to a symbol in source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    /// Symbol being referenced (simple name or FQN)
    pub symbol: String,
    /// Location of the reference
    pub range: TextRange,
    /// Kind of reference
    pub kind: ReferenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceKind {
    /// Type usage (variable declaration, cast, etc.)
    Type,
    /// Method call
    MethodCall,
    /// Field access
    FieldAccess,
    /// Constructor call
    ConstructorCall,
}

// Serde helpers for Arc<str> serialization
// These modules enable transparent serialization/deserialization of Arc<str> as String
mod arc_str_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(|s| s.into())
    }
}

mod option_arc_str_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(value: &Option<Arc<str>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_ref().map(|s| s.as_ref()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Arc<str>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(|opt| opt.map(|s| s.into()))
    }
}

mod option_arc_vec_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(
        value: &Option<Arc<Vec<Arc<str>>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .as_ref()
            .map(|names| names.iter().map(|s| s.as_ref()).collect::<Vec<&str>>())
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<super::Throws, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<Vec<String>>::deserialize(deserializer).map(|names| {
            names.map(|names| Arc::new(names.into_iter().map(Arc::<str>::from).collect()))
        })
    }
}

mod vec_arc_str_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(value: &[Arc<str>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<&str>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Arc<str>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<String>::deserialize(deserializer)
            .map(|vec| vec.into_iter().map(|s| s.into()).collect())
    }
}
