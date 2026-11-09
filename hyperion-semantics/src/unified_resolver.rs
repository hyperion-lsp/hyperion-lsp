// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unified Name Resolution
//!
//! This module provides a unified resolution interface that ensures diagnostics,
//! navigation, hover, and all other LSP features use the exact same resolution logic.
//!
//! # `resolve_name()` — the single resolution function
//!
//! Every identifier in the AST resolves through `resolve_name()`. It returns a
//! `ResolvedName` that tells the caller *what* the symbol is (type, method, field,
//! local variable, etc.) so features can act on it without re-resolving.
//!
//! # Resolution Order
//!
//! 1. Type parameters (generics in scope)
//! 2. Known local variables and parameters
//! 3. Local type declarations (same-file classes)
//! 4. Explicit imports → workspace lookup
//! 5. Same-package classes (shadow java.lang, JLS §6.5.5)
//! 6. Static imports
//! 7. java.lang implicit imports
//! 8. Wildcard imports → workspace lookup
//! 9. Enum constants in switch context (heuristic)
//! 10. Same-file scope-aware resolution (inner classes)
//! 11. External dependency suppression

use crate::resolved_name::ResolvedName;
use crate::resolver::SymbolResolver;
use crate::symbol::{
    ClassType, PrimitiveType, ScopeKind, Symbol, SymbolId, SymbolKind, SymbolTable, Type,
    Visibility,
};
use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::{
    DeclarationNamespace, ImportMap, Language, Modifiers, TypeNameResolution, WorkspaceIndex,
};
use std::collections::HashSet;
use tracing::debug;

/// Context about the AST position where a name appears.
///
/// This tells `resolve_name()` what resolution strategies are applicable.
/// For example, a name inside a switch case label should resolve as an
/// enum constant, while a name in a type position should only resolve as a type.
///
/// Fields borrow from the semantic walker to avoid cloning HashSets on every
/// name resolution call.
#[derive(Debug)]
pub struct NameContext<'a> {
    /// True if the name appears inside a CASE_CLAUSE
    pub in_switch_case: bool,
    /// Type parameters currently in scope (T, E, K, V, etc.)
    pub type_parameters: &'a HashSet<String>,
    /// Local-variable scopes visible at the current position, outermost first.
    ///
    /// Keep these borrowed. Whole-file semantics resolves every identifier in
    /// a source file, so materializing the union for each lookup turns a method
    /// with many locals into quadratic allocation and hashing work.
    pub variable_scopes: &'a [HashSet<String>],
    /// Local type declarations in the current file
    pub local_types: &'a HashSet<String>,
    /// FQNs of the lexically enclosing types, nearest first. Member types of
    /// these (and of their supertypes) are in scope by simple name, which is
    /// how `Cart.Audit` names a type nested in a nested class.
    pub lexical_owners: &'a [String],
    /// Whether an unresolved name may denote a type from an unindexed dependency.
    /// This is valid in type positions and qualified-name receiver positions, but
    /// not for ordinary value expressions such as `missing = 1`.
    pub allow_external_type: bool,
    /// Restrict lookup to the namespace required by the syntax position.
    /// Java type positions must not become ambiguous merely because a field or
    /// method has the same spelling as a type (JLS 6.5).
    pub expected_namespace: Option<DeclarationNamespace>,
}

/// Result of unified type resolution
#[derive(Debug, Clone)]
pub enum ResolutionResult {
    /// Resolved via imports/workspace index (cross-file)
    Workspace {
        /// Fully qualified name of the resolved type
        fqn: String,
    },
    /// Resolved via same-file symbol table (inner classes, local types)
    SameFile {
        /// Symbol ID in the local symbol table
        symbol_id: SymbolId,
    },
    /// Assumed to be external (unindexed package, suppress diagnostic)
    External,
    /// Not found anywhere
    NotFound,
}

impl ResolutionResult {
    /// Returns true if the symbol was resolved (any way except NotFound)
    pub const fn is_resolved(&self) -> bool {
        !matches!(self, Self::NotFound)
    }

    /// Returns true if this is a workspace resolution
    pub const fn is_workspace(&self) -> bool {
        matches!(self, Self::Workspace { .. })
    }

    /// Returns true if this is a same-file resolution
    pub const fn is_same_file(&self) -> bool {
        matches!(self, Self::SameFile { .. })
    }
}

/// Unified type resolver that provides consistent resolution for both
/// diagnostics and navigation.
///
/// This resolver implements the full resolution chain that navigation uses,
/// ensuring diagnostics don't report errors for symbols that navigation can find.
pub struct UnifiedTypeResolver<'a> {
    workspace_index: &'a WorkspaceIndex,
    imports: &'a ImportMap,
    symbol_resolver: Option<&'a SymbolResolver>,
    language: Language,
}

impl<'a> UnifiedTypeResolver<'a> {
    /// Create a new unified resolver
    pub const fn new(
        workspace_index: &'a WorkspaceIndex,
        imports: &'a ImportMap,
        symbol_resolver: Option<&'a SymbolResolver>,
    ) -> Self {
        Self::for_language(workspace_index, imports, symbol_resolver, Language::Java)
    }

    /// Create a resolver using the source language's exact import and default
    /// import rules.
    pub const fn for_language(
        workspace_index: &'a WorkspaceIndex,
        imports: &'a ImportMap,
        symbol_resolver: Option<&'a SymbolResolver>,
        language: Language,
    ) -> Self {
        Self {
            workspace_index,
            imports,
            symbol_resolver,
            language,
        }
    }

    /// Resolve a name to what it refers to, considering all resolution strategies.
    ///
    /// This is the single entry point for name resolution. Both diagnostics and
    /// navigation call this function, so they can never diverge on whether a
    /// symbol is resolved.
    ///
    /// # Resolution Order
    /// 1. Type parameters (generics)
    /// 2. Known local variables/parameters
    /// 3. Local type declarations
    /// 4. Explicit imports → workspace lookup
    /// 5. Same-package classes (shadow java.lang, JLS §6.5.5)
    /// 6. Static imports
    /// 7. java.lang implicit imports
    /// 8. Wildcard imports → workspace lookup
    /// 9. Enum constants in switch context
    /// 10. Same-file scope-aware resolution
    /// 11. External dependency suppression
    pub fn resolve_name(
        &self,
        name: &str,
        offset: u32,
        context: &NameContext<'_>,
    ) -> Option<ResolvedName> {
        let type_namespace_allowed = matches!(
            context.expected_namespace,
            None | Some(DeclarationNamespace::Type)
        );
        let value_namespace_allowed = matches!(
            context.expected_namespace,
            None | Some(DeclarationNamespace::Value)
        );

        // 1. Type parameters
        if type_namespace_allowed && context.type_parameters.contains(name) {
            debug!("UNIFIED: Resolved '{}' as type parameter", name);
            return Some(ResolvedName::TypeParameter {
                name: name.to_string(),
                bounds: vec![],
            });
        }

        // 2. Known local variables/parameters
        if value_namespace_allowed
            && context
                .variable_scopes
                .iter()
                .rev()
                .any(|scope| scope.contains(name))
        {
            // Try scope-aware resolution first to get location + type info
            if let Some(resolver) = self.symbol_resolver {
                let simple = extract_simple_name(name);
                if let Some(scope_id) = resolver.find_scope_at_offset(offset) {
                    if let Some(symbol) =
                        resolver.resolve_in_scope_at_offset(simple, scope_id, offset)
                    {
                        debug!(
                            "UNIFIED: Resolved '{}' as known variable (scope-aware)",
                            name
                        );
                        return Some(self.resolved_symbol_to_name(&symbol));
                    }
                }
            }
            debug!(
                "UNIFIED: Resolved '{}' as known variable (placeholder)",
                name
            );
            return Some(ResolvedName::LocalVariable {
                name: name.to_string(),
                var_type: Type::Unknown,
                scope: crate::symbol::ScopeId(0),
                location: None,
                is_parameter: false,
            });
        }

        // 3. Local type declarations
        if type_namespace_allowed && context.local_types.contains(name) {
            if let Some(resolver) = self.symbol_resolver {
                let simple_name = extract_simple_name(name);
                if let Some(scope_id) = resolver.find_scope_at_offset(offset) {
                    if let Some(symbol) =
                        resolver.resolve_in_scope_at_offset(simple_name, scope_id, offset)
                    {
                        if resolver
                            .symbol_table()
                            .get_symbol(symbol.symbol_id)
                            .is_some_and(|resolved| {
                                matches!(resolved.kind, SymbolKind::Class { .. })
                            })
                        {
                            debug!("UNIFIED: Resolved '{}' as scoped local type", name);
                            return Some(self.resolved_symbol_to_name(&symbol));
                        }
                    }
                }

                // A same-named type exists elsewhere in the file but the
                // scope table does not list it here. It may still be a member
                // type inherited from a superclass (JLS 8.5), which only the
                // index's lexical/inheritance-aware lookup below can see, so
                // continue instead of concluding "not visible".
            } else {
                let fqn = self
                    .imports
                    .current_package
                    .as_ref()
                    .map_or_else(|| name.to_string(), |package| format!("{package}.{name}"));
                debug!("UNIFIED: Resolved '{}' as local type '{}'", name, fqn);
                return Some(ResolvedName::Type {
                    fqn,
                    location: None,
                });
            }
        }

        // 4-8: Import resolution + workspace lookup
        let import_resolver =
            ImportResolver::for_language(self.imports, self.workspace_index, self.language);
        // A qualified expression receiver may be a statically imported value.
        // Its binding takes precedence over an identically named type import.
        let receiver_values = if context.expected_namespace.is_none() {
            import_resolver.resolve_static_values(name)
        } else {
            Vec::new()
        };
        let contextual_identity =
            type_namespace_allowed.then(|| import_resolver.resolve_type_identity(name));

        if type_namespace_allowed && receiver_values.is_empty() {
            if let TypeNameResolution::Exact(candidate) =
                self.workspace_index.resolve_type_name_in_context(
                    self.imports,
                    context.lexical_owners,
                    name,
                    self.language,
                )
            {
                debug!("UNIFIED: Resolved '{}' via workspace: {}", name, candidate);
                return Some(ResolvedName::Type {
                    fqn: candidate,
                    location: None, // Location resolved by feature layer
                });
            }
        }

        // 6. Static imports (JLS §7.5.3 — checked before java.lang)
        let mut static_candidates = match context.expected_namespace {
            Some(DeclarationNamespace::Type) => import_resolver.resolve_static_types(name),
            Some(DeclarationNamespace::Value) => import_resolver.resolve_static_values(name),
            Some(DeclarationNamespace::Callable) => import_resolver.resolve_static_callables(name),
            Some(DeclarationNamespace::Constructor | DeclarationNamespace::Other) => Vec::new(),
            None if !receiver_values.is_empty() => receiver_values,
            None => {
                let mut candidates = import_resolver.resolve_static_callables(name);
                candidates.extend(import_resolver.resolve_static_types(name));
                candidates
            }
        };
        static_candidates.sort_unstable_by(|left, right| left.fqn.cmp(&right.fqn));
        static_candidates.dedup_by(|left, right| left.fqn == right.fqn && left.kind == right.kind);
        // Statically imported overloads (`assertEquals`, `Math.max`, ...) are
        // one callable name with several signatures. The name resolves; which
        // overload applies is decided at the call site.
        if static_candidates.len() > 1
            && matches!(
                context.expected_namespace,
                Some(DeclarationNamespace::Callable)
            )
            && static_candidates.iter().all(|candidate| {
                matches!(
                    candidate.kind,
                    lsp_types::SymbolKind::METHOD | lsp_types::SymbolKind::FUNCTION
                )
            })
        {
            // javac merges the overload sets of every static import that
            // names the method (JLS 15.12.1): `import static a.A.*; import
            // static b.B.*;` with an `any` in each is one callable name. The
            // owners may differ; only the call site decides which applies.
            let first = &static_candidates[0];
            // Several owners: the name is callable but no declaration is
            // elected (navigation must not pick one arbitrarily).
            let single_owner = static_candidates
                .iter()
                .all(|candidate| candidate.container == first.container);
            let owner_fqn = if single_owner {
                first.container.as_deref().unwrap_or_default()
            } else {
                ""
            };
            let return_types: HashSet<Option<&str>> = static_candidates
                .iter()
                .map(|candidate| candidate.return_type.as_deref())
                .collect();
            let return_type = if return_types.len() == 1 {
                first
                    .return_type
                    .as_ref()
                    .and_then(|rt| parse_simple_type(rt))
                    .unwrap_or(Type::Void)
            } else {
                Type::Unknown
            };
            debug!(
                "UNIFIED: Resolved '{}' via {} statically imported overloads",
                name,
                static_candidates.len()
            );
            return Some(ResolvedName::Method {
                owner_fqn: owner_fqn.to_string(),
                name: name.to_string(),
                return_type,
                location: None,
                modifier_prefix: build_modifier_prefix(&first.modifiers),
            });
        }
        if let [symbol] = static_candidates.as_slice() {
            let fqn = symbol.fqn.as_ref();
            debug!("UNIFIED: Resolved '{}' via static import: {}", name, fqn);

            let owner_fqn = symbol.container.as_ref().map_or_else(
                || {
                    fqn.rfind('.')
                        .map_or(String::new(), |i| fqn[..i].to_string())
                },
                std::string::ToString::to_string,
            );
            let modifier_prefix = build_modifier_prefix(&symbol.modifiers);

            if matches!(
                symbol.kind,
                lsp_types::SymbolKind::METHOD | lsp_types::SymbolKind::FUNCTION
            ) {
                let return_type = symbol
                    .return_type
                    .as_ref()
                    .and_then(|rt| parse_simple_type(rt))
                    .unwrap_or(Type::Void);
                return Some(ResolvedName::Method {
                    owner_fqn,
                    name: name.to_string(),
                    return_type,
                    location: None,
                    modifier_prefix,
                });
            } else if symbol.kind == lsp_types::SymbolKind::ENUM_MEMBER {
                return Some(ResolvedName::EnumConstant {
                    enum_fqn: owner_fqn,
                    name: name.to_string(),
                    location: None,
                });
            } else if matches!(
                symbol.kind,
                lsp_types::SymbolKind::CLASS
                    | lsp_types::SymbolKind::INTERFACE
                    | lsp_types::SymbolKind::ENUM
                    | lsp_types::SymbolKind::STRUCT
            ) {
                return Some(ResolvedName::Type {
                    fqn: fqn.to_owned(),
                    location: None,
                });
            }
            let field_type = symbol
                .return_type
                .as_ref()
                .and_then(|rt| parse_simple_type(rt))
                .unwrap_or(Type::Unknown);
            return Some(ResolvedName::Field {
                owner_fqn,
                name: name.to_string(),
                field_type,
                location: None,
                modifier_prefix,
            });
        }

        // 7. java.lang implicit imports (JLS §7.5.5 — lowest priority implicit import)
        if type_namespace_allowed
            && context.allow_external_type
            && matches!(
                contextual_identity,
                Some(TypeNameResolution::Exact(ref fqn)) if fqn.starts_with("java.lang.")
            )
            && !self.workspace_index.has_core_jdk_classes()
        {
            debug!(
                "UNIFIED: Resolved '{}' as java.lang (JDK not indexed)",
                name
            );
            return Some(ResolvedName::External {
                fqn: match contextual_identity.as_ref() {
                    Some(TypeNameResolution::Exact(fqn)) => fqn.clone(),
                    _ => unreachable!("java.lang guard requires exact identity"),
                },
            });
        }

        // 9. Enum constants need the selector's declared type. A bare spelling
        // is not a semantic identity; the caller may conservatively suppress a
        // diagnostic while the feature layer performs receiver-aware lookup.
        if context.in_switch_case {
            debug!("UNIFIED: Deferring switch-label resolution for '{}'", name);
            return None;
        }

        // 10. Same-file scope-aware resolution
        let simple_name = extract_simple_name(name);
        if let Some(resolver) = self.symbol_resolver {
            if let Some(scope_id) = resolver.find_scope_at_offset(offset) {
                if let Some(symbol) =
                    resolver.resolve_in_scope_at_offset(simple_name, scope_id, offset)
                {
                    if resolver
                        .symbol_table()
                        .get_symbol(symbol.symbol_id)
                        .is_some_and(|symbol| {
                            symbol_matches_namespace(symbol, context.expected_namespace)
                        })
                    {
                        debug!("UNIFIED: Resolved '{}' via scope-aware resolution", name);
                        return Some(self.resolved_symbol_to_name(&symbol));
                    }
                }
            } else if let Some(symbol_id) =
                resolver.symbol_table().find_symbol_anywhere(simple_name)
            {
                if let Some(symbol) = resolver.symbol_table().get_symbol(symbol_id) {
                    if symbol_matches_namespace(symbol, context.expected_namespace) {
                        debug!("UNIFIED: Resolved '{}' via exhaustive search", name);
                        return Some(symbol_to_resolved_name(
                            symbol,
                            symbol_id,
                            Some(resolver.symbol_table()),
                        ));
                    }
                }
            }
        }

        // 11. Suppress only an exact single-type binding whose dependency is
        // absent, or the one reading a spelling still has while its package is
        // wholly unindexed. Both questions are answered by the workspace
        // resolver; this layer must not re-derive candidates.
        if context.allow_external_type {
            let candidate = match contextual_identity {
                Some(TypeNameResolution::Exact(candidate)) => candidate,
                _ => {
                    let Some(cold) = import_resolver.sole_reading_while_dependency_is_cold(name)
                    else {
                        debug!("UNIFIED: External spelling '{name}' has no single cold reading");
                        return None;
                    };
                    cold
                }
            };
            debug!("UNIFIED: Suppressing '{}' (external dependency)", name);
            return Some(ResolvedName::External { fqn: candidate });
        }

        debug!("UNIFIED: Could not resolve '{}'", name);
        None
    }

    /// Convert a `ResolvedSymbol` from the SymbolResolver into a `ResolvedName`.
    fn resolved_symbol_to_name(&self, resolved: &crate::resolver::ResolvedSymbol) -> ResolvedName {
        self.symbol_resolver
            .and_then(|r| r.symbol_table().get_symbol(resolved.symbol_id))
            .map_or_else(
                || ResolvedName::Type {
                    fqn: resolved.name.clone(),
                    location: None,
                },
                |symbol| {
                    let st = self
                        .symbol_resolver
                        .map(super::resolver::SymbolResolver::symbol_table);
                    symbol_to_resolved_name(symbol, resolved.symbol_id, st)
                },
            )
    }

    /// Resolve a type name using the full resolution chain.
    ///
    /// This is a backward-compatible wrapper around `resolve_name()`.
    /// Prefer `resolve_name()` for new code.
    pub fn resolve_type(&self, name: &str, offset: u32) -> ResolutionResult {
        let simple_name = extract_simple_name(name);

        // 1-4: Use ImportResolver for imports/workspace resolution
        let import_resolver =
            ImportResolver::for_language(self.imports, self.workspace_index, self.language);
        let contextual_identity = import_resolver.resolve_type_identity(name);

        if let TypeNameResolution::Exact(candidate) = self
            .workspace_index
            .resolve_type_name_in_context(self.imports, &[], name, self.language)
        {
            debug!(
                "UNIFIED: Resolved '{}' via workspace index: {}",
                name, candidate
            );
            return ResolutionResult::Workspace { fqn: candidate };
        }

        // java.lang special case: if the exact contextual identity is a core
        // language type and the JDK is not indexed, treat it as external.
        if matches!(
            contextual_identity,
            TypeNameResolution::Exact(ref candidate) if candidate.starts_with("java.lang.")
        ) && !self.workspace_index.has_core_jdk_classes()
        {
            debug!(
                "UNIFIED: Resolved '{}' as java.lang type (JDK not indexed)",
                name
            );
            return ResolutionResult::External;
        }

        // 5: Same-file scope-aware resolution
        if let Some(resolver) = self.symbol_resolver {
            // Find the scope at the given offset for proper scope-aware resolution
            if let Some(scope_id) = resolver.find_scope_at_offset(offset) {
                debug!(
                    "UNIFIED: Attempting scope-aware resolution for '{}' in scope {:?}",
                    simple_name, scope_id
                );

                // Use the same resolution chain as navigation:
                // a) resolve_in_scope_for_navigation (scope-aware)
                // b) resolve (current scope chain)
                // c) resolve_anywhere (exhaustive fallback)
                if let Some(symbol) =
                    resolver.resolve_in_scope_at_offset(simple_name, scope_id, offset)
                {
                    debug!(
                        "UNIFIED: Resolved '{}' via scope-aware resolution",
                        simple_name
                    );
                    return ResolutionResult::SameFile {
                        symbol_id: symbol.symbol_id,
                    };
                }
            } else if let Some(symbol_id) =
                resolver.symbol_table().find_symbol_anywhere(simple_name)
            {
                // No source scope is available (notably resolve_type_no_position),
                // so exhaustive lookup is the only same-file option.
                debug!(
                    "UNIFIED: Resolved '{}' via exhaustive search (fallback)",
                    simple_name
                );
                return ResolutionResult::SameFile { symbol_id };
            }
        }

        // 6: An explicit or implicit single-type binding remains an identity
        // while its dependency is absent. On-demand/current-package guesses
        // remain `Missing` and must not suppress a real unresolved-type error.
        if matches!(contextual_identity, TypeNameResolution::Exact(_)) {
            debug!(
                "UNIFIED: Suppressing error for exact external binding '{}'",
                name
            );
            return ResolutionResult::External;
        }

        debug!("UNIFIED: Could not resolve '{}'", name);
        ResolutionResult::NotFound
    }

    /// Resolve a type name without position context.
    ///
    /// This is a simpler version that doesn't require an offset. It still uses
    /// the full resolution chain but falls back to exhaustive search for same-file
    /// symbols rather than scope-aware resolution.
    ///
    /// Use `resolve_type` with an offset when possible for better accuracy.
    pub fn resolve_type_no_position(&self, name: &str) -> ResolutionResult {
        // Use max u32 as offset - find_scope_at_offset will return None
        // and we'll fall through to the exhaustive search
        self.resolve_type(name, u32::MAX)
    }
}

/// Return whether a same-file symbol belongs to the namespace required by the
/// syntax position. Java permits types, methods, and values to share a name.
const fn symbol_matches_namespace(
    symbol: &Symbol,
    expected_namespace: Option<DeclarationNamespace>,
) -> bool {
    match expected_namespace {
        None => true,
        Some(DeclarationNamespace::Type) => matches!(
            symbol.kind,
            SymbolKind::Class { .. } | SymbolKind::TypeParameter { .. }
        ),
        Some(DeclarationNamespace::Value) => matches!(
            symbol.kind,
            SymbolKind::Field { .. }
                | SymbolKind::EnumConstant { .. }
                | SymbolKind::LocalVariable { .. }
                | SymbolKind::Parameter { .. }
        ),
        Some(DeclarationNamespace::Callable) => matches!(
            symbol.kind,
            SymbolKind::Method {
                is_constructor: false,
                ..
            }
        ),
        Some(DeclarationNamespace::Constructor) => matches!(
            symbol.kind,
            SymbolKind::Method {
                is_constructor: true,
                ..
            }
        ),
        Some(DeclarationNamespace::Other) => matches!(symbol.kind, SymbolKind::Package),
    }
}

/// Convert a `Symbol` from the symbol table into a `ResolvedName`.
///
/// When a `SymbolTable` is provided, walks the scope chain to find the enclosing
/// class name (for methods/fields/enum constants) and builds a modifier prefix
/// from the symbol's visibility and modifiers.
fn symbol_to_resolved_name(
    symbol: &Symbol,
    symbol_id: SymbolId,
    symbol_table: Option<&SymbolTable>,
) -> ResolvedName {
    // Find the enclosing class name by walking the scope chain. ResolverBuilder
    // defines a class symbol in its enclosing scope, and its ClassType is the
    // authoritative source for both package and nested-class qualification.
    let owner_fqn = symbol_table
        .and_then(|st| {
            let scope_id = st.find_scope_for_symbol(symbol_id)?;
            let class_scope_id = if st
                .get_scope(scope_id)
                .is_some_and(|s| s.kind == ScopeKind::Class)
            {
                scope_id
            } else {
                st.find_enclosing_class(scope_id)?
            };

            let class_scope = st.get_scope(class_scope_id)?;
            let class_range = class_scope.range;
            let declared_in_parent = class_scope.parent.and_then(|parent_id| {
                st.local_symbols(parent_id)
                    .into_iter()
                    .filter_map(|(_, id)| st.get_symbol(id))
                    .find(|candidate| {
                        matches!(candidate.kind, SymbolKind::Class { .. })
                            && class_range.is_some_and(|(start, end)| {
                                candidate.location.range.start >= start
                                    && candidate.location.range.start < end
                            })
                    })
            });

            declared_in_parent
                // Compatibility with symbol tables using the former layout,
                // where the type symbol lived inside its own class scope.
                .or_else(|| {
                    st.local_symbols(class_scope_id)
                        .into_iter()
                        .filter_map(|(_, id)| st.get_symbol(id))
                        .find(|candidate| matches!(candidate.kind, SymbolKind::Class { .. }))
                })
                .and_then(
                    |class_symbol| match (&class_symbol.kind, &class_symbol.type_info) {
                        (SymbolKind::Class { .. }, Type::Class(class_type)) => {
                            Some(class_type.qualified_name().into_owned())
                        }
                        _ => None,
                    },
                )
        })
        .unwrap_or_default();

    // Build modifier prefix from visibility + modifiers
    let modifier_prefix = build_symbol_modifier_prefix(symbol);

    match &symbol.kind {
        SymbolKind::Class { .. } => ResolvedName::Type {
            fqn: match &symbol.type_info {
                Type::Class(class_type) => class_type.qualified_name().into_owned(),
                _ => symbol.name.clone(),
            },
            location: Some(symbol.location.clone()),
        },
        SymbolKind::Method { .. } => ResolvedName::Method {
            owner_fqn,
            name: symbol.name.clone(),
            return_type: symbol.type_info.clone(),
            location: Some(symbol.location.clone()),
            modifier_prefix,
        },
        SymbolKind::Field { .. } => ResolvedName::Field {
            owner_fqn,
            name: symbol.name.clone(),
            field_type: symbol.type_info.clone(),
            location: Some(symbol.location.clone()),
            modifier_prefix,
        },
        SymbolKind::EnumConstant { .. } => ResolvedName::EnumConstant {
            enum_fqn: owner_fqn,
            name: symbol.name.clone(),
            location: Some(symbol.location.clone()),
        },
        SymbolKind::LocalVariable { .. } | SymbolKind::Parameter { .. } => {
            let is_param = matches!(&symbol.kind, SymbolKind::Parameter { .. });
            ResolvedName::LocalVariable {
                name: symbol.name.clone(),
                var_type: symbol.type_info.clone(),
                scope: crate::symbol::ScopeId(0),
                location: Some(symbol.location.clone()),
                is_parameter: is_param,
            }
        }
        SymbolKind::TypeParameter { bounds } => ResolvedName::TypeParameter {
            name: symbol.name.clone(),
            bounds: bounds.clone(),
        },
        SymbolKind::Package => ResolvedName::Package {
            fqn: symbol.name.clone(),
        },
    }
}

/// Build a Java-style modifier prefix string from a semantics symbol's visibility and modifiers.
fn build_symbol_modifier_prefix(symbol: &Symbol) -> String {
    let mut parts = Vec::new();

    match symbol.visibility {
        Visibility::Public => parts.push("public"),
        Visibility::Protected => parts.push("protected"),
        Visibility::Private => parts.push("private"),
        Visibility::PackagePrivate => {} // no keyword
    }

    if symbol.modifiers.is_static() {
        parts.push("static");
    }
    if symbol.modifiers.is_final() {
        parts.push("final");
    }
    if symbol.modifiers.is_abstract() {
        parts.push("abstract");
    }

    if parts.is_empty() {
        String::new()
    } else {
        let mut prefix = parts.join(" ");
        prefix.push(' ');
        prefix
    }
}

/// Extract the simple name from a potentially qualified name.
///
/// Examples:
/// - "User" -> "User"
/// - "com.example.User" -> "User"
/// - "Outer.Inner" -> "Inner"
fn extract_simple_name(name: &str) -> &str {
    name.split('.').next_back().unwrap_or(name)
}

/// Parse a simple type string (from workspace Symbol.return_type) into a Type.
fn parse_simple_type(type_str: &str) -> Option<Type> {
    let type_str = type_str.trim();
    if type_str.is_empty() {
        return None;
    }
    match type_str {
        "void" => Some(Type::Void),
        "int" => Some(Type::Primitive(PrimitiveType::Int)),
        "long" => Some(Type::Primitive(PrimitiveType::Long)),
        "short" => Some(Type::Primitive(PrimitiveType::Short)),
        "byte" => Some(Type::Primitive(PrimitiveType::Byte)),
        "char" => Some(Type::Primitive(PrimitiveType::Char)),
        "float" => Some(Type::Primitive(PrimitiveType::Float)),
        "double" => Some(Type::Primitive(PrimitiveType::Double)),
        "boolean" => Some(Type::Primitive(PrimitiveType::Boolean)),
        other => {
            let (name, package) = other.rfind('.').map_or_else(
                || (other.to_string(), None),
                |last_dot| {
                    (
                        other[last_dot + 1..].to_string(),
                        Some(other[..last_dot].to_string()),
                    )
                },
            );
            Some(Type::Class(ClassType::new(name, package)))
        }
    }
}

/// Build a modifier prefix string (e.g., "static ", "static final ") from workspace Modifiers.
fn build_modifier_prefix(modifiers: &Modifiers) -> String {
    let mut prefix = String::new();
    if modifiers.is_static {
        prefix.push_str("static ");
    }
    if modifiers.is_final {
        prefix.push_str("final ");
    }
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_workspace::{Symbol, TextRange};
    use std::path::PathBuf;

    fn make_class_symbol(fqn: &str, simple_name: &str) -> Symbol {
        Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            range: TextRange::new(0, 100),
            kind: lsp_types::SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces: vec![],
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    /// Workspace with an indexed JDK (java.lang.Object/String/Record) plus a
    /// same-package class com.foo.Record, referenced from package com.foo.
    fn make_shadowing_fixture() -> (WorkspaceIndex, ImportMap) {
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-unified-test"));
        index.add_external_symbol(make_class_symbol("java.lang.Object", "Object"));
        index.add_external_symbol(make_class_symbol("java.lang.String", "String"));
        index.add_external_symbol(make_class_symbol("java.lang.Record", "Record"));
        index.add_external_symbol(make_class_symbol("com.foo.Record", "Record"));

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.foo".into());
        (index, imports)
    }

    // Same-package classes shadow java.lang (JLS §6.5.5)
    #[test]
    fn test_resolve_name_same_package_shadows_java_lang() {
        let (index, imports) = make_shadowing_fixture();
        let resolver = UnifiedTypeResolver::new(&index, &imports, None);

        let type_parameters = HashSet::new();
        let local_types = HashSet::new();
        let context = NameContext {
            in_switch_case: false,
            type_parameters: &type_parameters,
            variable_scopes: &[],
            local_types: &local_types,
            lexical_owners: &[],
            allow_external_type: true,
            expected_namespace: Some(DeclarationNamespace::Type),
        };

        let resolved = resolver.resolve_name("Record", 0, &context);
        match resolved {
            Some(ResolvedName::Type { ref fqn, .. }) => assert_eq!(
                fqn, "com.foo.Record",
                "same-package class must shadow java.lang.Record"
            ),
            other => panic!("expected Type resolution to com.foo.Record, got {other:?}"),
        }
    }

    // Same-package classes shadow java.lang (JLS §6.5.5)
    #[test]
    fn test_resolve_type_same_package_shadows_java_lang() {
        let (index, imports) = make_shadowing_fixture();
        let resolver = UnifiedTypeResolver::new(&index, &imports, None);

        match resolver.resolve_type("Record", 0) {
            ResolutionResult::Workspace { ref fqn } => assert_eq!(
                fqn, "com.foo.Record",
                "same-package class must shadow java.lang.Record"
            ),
            other => panic!("expected Workspace resolution to com.foo.Record, got {other:?}"),
        }
    }

    #[test]
    fn statically_imported_overloads_resolve_as_one_callable() {
        let temp = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("Assertions.java"),
                concat!(
                    "package lib; public class Assertions { ",
                    "public static void assertEquals(int a, int b) {} ",
                    "public static void assertEquals(Object a, Object b) {} ",
                    "public static void assertEquals(double a, double b, double d) {} }",
                ),
            )
            .unwrap();
        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".to_owned());
        imports.add_static_import(
            "assertEquals".to_owned(),
            "lib.Assertions.assertEquals".to_owned(),
        );
        let resolver = UnifiedTypeResolver::new(&index, &imports, None);
        let type_parameters = HashSet::new();
        let local_types = HashSet::new();
        let context = NameContext {
            in_switch_case: false,
            type_parameters: &type_parameters,
            variable_scopes: &[],
            local_types: &local_types,
            lexical_owners: &[],
            allow_external_type: false,
            expected_namespace: Some(DeclarationNamespace::Callable),
        };

        match resolver.resolve_name("assertEquals", 0, &context) {
            Some(ResolvedName::Method {
                owner_fqn, name, ..
            }) => {
                assert_eq!(owner_fqn, "lib.Assertions");
                assert_eq!(name, "assertEquals");
            }
            other => panic!("overloaded static import must resolve, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_type_includes_static_member_type_imports() {
        let temp = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &temp.path().join("Outer.java"),
                concat!(
                    "package p; public class Outer { ",
                    "public static class Nested {} ",
                    "public static int Nested; ",
                    "public static void Nested() {} }",
                ),
            )
            .unwrap();
        let mut imports = ImportMap::new();
        imports.current_package = Some("consumer".to_owned());
        imports.add_static_import("Nested".to_owned(), "p.Outer.Nested".to_owned());
        let resolver = UnifiedTypeResolver::new(&index, &imports, None);

        match resolver.resolve_type("Nested", 0) {
            ResolutionResult::Workspace { fqn } => assert_eq!(fqn, "p.Outer.Nested"),
            other => panic!("expected static member type resolution, got {other:?}"),
        }
    }

    #[test]
    fn external_suppression_requires_an_exact_single_type_binding() {
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-external-binding-test"));
        let type_parameters = HashSet::new();
        let local_types = HashSet::new();
        let context = NameContext {
            in_switch_case: false,
            type_parameters: &type_parameters,
            variable_scopes: &[],
            local_types: &local_types,
            lexical_owners: &[],
            allow_external_type: true,
            expected_namespace: Some(DeclarationNamespace::Type),
        };

        let mut wildcard = ImportMap::new();
        wildcard.current_package = Some("app".to_owned());
        wildcard.add_wildcard("missing.dep".to_owned());
        let wildcard_resolver = UnifiedTypeResolver::new(&index, &wildcard, None);
        assert_eq!(wildcard_resolver.resolve_name("Ghost", 0, &context), None);
        assert!(matches!(
            wildcard_resolver.resolve_type("Ghost", 0),
            ResolutionResult::NotFound
        ));

        let mut explicit = wildcard;
        explicit.add_import("Ghost".to_owned(), "missing.dep.Ghost".to_owned());
        let explicit_resolver = UnifiedTypeResolver::new(&index, &explicit, None);
        assert_eq!(
            explicit_resolver.resolve_name("Ghost", 0, &context),
            Some(ResolvedName::External {
                fqn: "missing.dep.Ghost".to_owned()
            })
        );
        assert!(matches!(
            explicit_resolver.resolve_type("Ghost", 0),
            ResolutionResult::External
        ));
    }

    /// A current-package spelling has exactly one possible reading, so when
    /// nothing from that package is indexed there is no evidence the name is
    /// wrong and reporting it would cry wolf while the dependency is cold.
    /// Adding a wildcard makes the spelling ambiguous again, which must keep
    /// the suppression off.
    #[test]
    fn unambiguous_current_package_spelling_suppresses_while_its_package_is_cold() {
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-current-package-test"));
        let type_parameters = HashSet::new();
        let local_types = HashSet::new();
        let context = NameContext {
            in_switch_case: false,
            type_parameters: &type_parameters,
            variable_scopes: &[],
            local_types: &local_types,
            lexical_owners: &[],
            allow_external_type: true,
            expected_namespace: Some(DeclarationNamespace::Type),
        };

        let mut current_package_only = ImportMap::new();
        current_package_only.current_package = Some("app".to_owned());
        let resolver = UnifiedTypeResolver::new(&index, &current_package_only, None);
        assert_eq!(
            resolver.resolve_name("Ghost", 0, &context),
            Some(ResolvedName::External {
                fqn: "app.Ghost".to_owned()
            }),
            "the only reading of an unindexed same-package name is an identity"
        );

        let mut with_wildcard = current_package_only;
        with_wildcard.add_wildcard("missing.dep".to_owned());
        let ambiguous = UnifiedTypeResolver::new(&index, &with_wildcard, None);
        assert_eq!(
            ambiguous.resolve_name("Ghost", 0, &context),
            None,
            "two candidate readings must not suppress an unresolved-type error"
        );
    }

    #[test]
    fn test_extract_simple_name() {
        assert_eq!(extract_simple_name("User"), "User");
        assert_eq!(extract_simple_name("com.example.User"), "User");
        assert_eq!(extract_simple_name("Outer.Inner"), "Inner");
        assert_eq!(extract_simple_name("a.b.c.d.E"), "E");
    }

    #[test]
    fn test_resolution_result_is_resolved() {
        assert!(ResolutionResult::Workspace { fqn: "test".into() }.is_resolved());
        assert!(ResolutionResult::SameFile {
            symbol_id: crate::symbol::SymbolId(0)
        }
        .is_resolved());
        assert!(ResolutionResult::External.is_resolved());
        assert!(!ResolutionResult::NotFound.is_resolved());
    }

    #[test]
    fn test_parse_simple_type() {
        use crate::symbol::{ClassType, PrimitiveType, Type};

        assert_eq!(parse_simple_type("void"), Some(Type::Void));
        assert_eq!(
            parse_simple_type("int"),
            Some(Type::Primitive(PrimitiveType::Int))
        );
        assert_eq!(
            parse_simple_type("boolean"),
            Some(Type::Primitive(PrimitiveType::Boolean))
        );
        assert_eq!(
            parse_simple_type("String"),
            Some(Type::Class(ClassType::new("String".into(), None)))
        );
        assert_eq!(
            parse_simple_type("java.util.List"),
            Some(Type::Class(ClassType::new(
                "List".into(),
                Some("java.util".into())
            )))
        );
        assert_eq!(parse_simple_type(""), None);
    }

    #[test]
    fn test_build_modifier_prefix() {
        use hyperion_workspace::Modifiers;

        let default = Modifiers::default();
        assert_eq!(build_modifier_prefix(&default), "");

        let static_mod = Modifiers {
            is_static: true,
            ..Default::default()
        };
        assert_eq!(build_modifier_prefix(&static_mod), "static ");

        let static_final = Modifiers {
            is_static: true,
            is_final: true,
            ..Default::default()
        };
        assert_eq!(build_modifier_prefix(&static_final), "static final ");
    }
}
