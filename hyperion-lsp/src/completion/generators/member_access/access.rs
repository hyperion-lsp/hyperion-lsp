// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Caller-sensitive access checks for member completion.
//!
//! Effective-member election belongs to the workspace index. This module adds
//! the facts that are specific to a completion site: its lexical owner,
//! package, receiver type, and whether the expression uses `super`.

use hyperion_workspace::{index::WorkspaceIndex, Language, Symbol};
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::SymbolKind;

/// Access facts that are not properties of the receiver type itself.
///
/// Keeping these facts explicit matters for Java `protected`: outside the
/// declaring package, the lexical owner must be a subclass and an instance
/// qualifier must have that owner (or one of its subclasses) as its static
/// type.
#[derive(Debug, Default)]
pub(super) struct MemberAccessContext {
    pub(super) caller_package: Option<String>,
    pub(super) caller_owner: Option<String>,
    pub(super) is_super_access: bool,
}

#[derive(Clone, Debug)]
struct DeclaringOwnerFacts {
    is_interface: bool,
    language: Option<Language>,
    package: String,
    nest_host: Option<String>,
}

/// Receiver-specific facts shared by every member considered for one request.
pub(super) struct MemberAccessFacts<'a> {
    workspace_index: &'a WorkspaceIndex,
    context: &'a MemberAccessContext,
    receiver_fqn: &'a str,
    receiver_supertypes: HashSet<String>,
    caller_hierarchies: Vec<(String, HashSet<String>)>,
    caller_nest_host: Option<String>,
    owner_cache: HashMap<String, DeclaringOwnerFacts>,
}

impl MemberAccessContext {
    pub(super) fn for_package(caller_package: Option<&str>) -> Self {
        Self {
            caller_package: caller_package.map(str::to_string),
            ..Self::default()
        }
    }

    pub(super) fn at_completion_site(
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
        caller_package: Option<String>,
        receiver: &str,
    ) -> Self {
        Self {
            caller_owner: Self::caller_owner_at(root, offset, caller_package.as_deref()),
            caller_package,
            is_super_access: receiver.trim() == "super",
        }
    }

    pub(super) fn caller_owner_at(
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
        caller_package: Option<&str>,
    ) -> Option<String> {
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .left_biased()
            .or_else(|| {
                root.token_at_offset(rowan::TextSize::from(offset))
                    .right_biased()
            })?;
        let mut names = token
            .parent()?
            .ancestors()
            .filter(|node| node.kind().is_type_declaration())
            .filter_map(|node| crate::navigation::import_resolver::extract_class_name(&node))
            .collect::<Vec<_>>();
        if names.is_empty() {
            return None;
        }
        names.reverse();
        let owner = names.join(".");
        Some(caller_package.map_or(owner.clone(), |package| format!("{package}.{owner}")))
    }

    pub(super) fn for_receiver<'a>(
        &'a self,
        workspace_index: &'a WorkspaceIndex,
        receiver_fqn: &'a str,
    ) -> MemberAccessFacts<'a> {
        let caller_owners = lexical_owner_chain(workspace_index, self.caller_owner.as_deref());
        let caller_nest_host = caller_owners.last().cloned();
        let caller_hierarchies = caller_owners
            .into_iter()
            .map(|owner| {
                let hierarchy = type_and_supertypes(workspace_index, &owner);
                (owner, hierarchy)
            })
            .collect();

        MemberAccessFacts {
            workspace_index,
            context: self,
            receiver_fqn,
            receiver_supertypes: type_and_supertypes(workspace_index, receiver_fqn),
            caller_hierarchies,
            caller_nest_host,
            owner_cache: HashMap::new(),
        }
    }
}

impl MemberAccessFacts<'_> {
    /// Return whether `symbol` belongs in the instance or static completion
    /// surface and is accessible from this completion site.
    pub(super) fn includes(&mut self, symbol: &Symbol, is_static_access: bool) -> bool {
        let is_static = self.member_is_static(symbol);
        is_static == is_static_access && self.is_symbol_accessible(symbol, is_static)
    }

    fn member_is_static(&mut self, symbol: &Symbol) -> bool {
        if symbol.modifiers.is_static || symbol.kind == SymbolKind::ENUM_MEMBER {
            return true;
        }
        if !is_value_member(symbol) {
            return false;
        }
        let Some(owner) = symbol.container.as_deref() else {
            return false;
        };
        let owner = self.owner_facts(owner);

        // Java and Groovy interface fields are implicitly static. Kotlin and
        // Scala interface/trait properties are instance members. Unknown
        // source kinds (including classfiles) must carry the JVM static flag
        // explicitly, so a missing path cannot accidentally change semantics.
        owner.is_interface && matches!(owner.language, Some(Language::Java | Language::Groovy))
    }

    fn is_symbol_accessible(&mut self, symbol: &Symbol, is_static: bool) -> bool {
        // A Kotlin function/property/object with no visibility modifier is
        // public (the extractor records only what is written).
        if matches!(
            symbol.kind,
            SymbolKind::FUNCTION | SymbolKind::PROPERTY | SymbolKind::OBJECT
        ) && !symbol.modifiers.is_private
            && !symbol.modifiers.is_protected
        {
            return true;
        }
        let Some(declaring_owner) = symbol.container.as_deref() else {
            return symbol.modifiers.is_public || symbol.modifiers.is_internal;
        };
        let owner = self.owner_facts(declaring_owner).clone();

        // Interface members are public by default in all currently supported
        // source languages unless the extractor recorded an explicit private
        // modifier.
        if !symbol.modifiers.is_private
            && (symbol.modifiers.is_public || symbol.modifiers.is_internal || owner.is_interface)
        {
            return true;
        }

        if symbol.modifiers.is_private {
            return owner.nest_host.is_some() && owner.nest_host == self.caller_nest_host;
        }

        let caller_package = self.context.caller_package.as_deref().unwrap_or_default();
        if owner.package == caller_package {
            return true;
        }

        if !symbol.modifiers.is_protected {
            return false;
        }

        // JLS 6.6.2: in another package the access must occur in a subclass
        // body. The additional qualifying-type restriction applies only to an
        // instance member (JLS 6.6.2.1); protected static access through either
        // the declaring class or the subclass is legal. `super` is the explicit
        // instance-member exception.
        self.caller_hierarchies
            .iter()
            .any(|(caller_owner, hierarchy)| {
                hierarchy.contains(declaring_owner)
                    && (is_static
                        || self.context.is_super_access
                        || self.receiver_fqn == caller_owner
                        || self.receiver_supertypes.contains(caller_owner))
            })
    }

    fn owner_facts(&mut self, owner: &str) -> &DeclaringOwnerFacts {
        self.owner_cache
            .entry(owner.to_string())
            .or_insert_with(|| {
                let owner_definition = self.workspace_index.find_symbol_definition(
                    &hyperion_workspace::SymbolTarget::new(
                        owner,
                        hyperion_workspace::DeclarationNamespace::Type,
                    ),
                );
                let language = owner_definition
                    .as_ref()
                    .and_then(|definition| {
                        self.workspace_index
                            .get_file_path(definition.location.file_id)
                    })
                    .as_deref()
                    .and_then(Language::from_path);
                DeclaringOwnerFacts {
                    is_interface: owner_definition
                        .as_ref()
                        .is_some_and(|definition| definition.symbol.kind == SymbolKind::INTERFACE),
                    language,
                    package: type_package(self.workspace_index, owner),
                    nest_host: lexical_owner_chain(self.workspace_index, Some(owner))
                        .last()
                        .cloned(),
                }
            })
    }
}

const fn is_value_member(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::FIELD | SymbolKind::PROPERTY | SymbolKind::CONSTANT | SymbolKind::ENUM_MEMBER
    )
}

fn type_package(workspace_index: &WorkspaceIndex, type_fqn: &str) -> String {
    let mut top_level = type_fqn.to_string();
    let mut visited = HashSet::new();
    while visited.insert(top_level.clone()) {
        let Some(container) =
            nominal_type(workspace_index, &top_level).and_then(|symbol| symbol.container.clone())
        else {
            break;
        };
        let Some(container_symbol) = nominal_type(workspace_index, &container) else {
            break;
        };
        if !hyperion_workspace::is_type_declaration_kind(container_symbol.kind) {
            break;
        }
        top_level = container.to_string();
    }
    top_level
        .rsplit_once('.')
        .map_or_else(String::new, |(package, _)| package.to_string())
}

pub(super) fn lexical_owner_chain(
    workspace_index: &WorkspaceIndex,
    caller_owner: Option<&str>,
) -> Vec<String> {
    let mut owners = Vec::new();
    let mut current = caller_owner.map(str::to_string);
    let mut visited = HashSet::new();
    while let Some(owner) = current {
        if !visited.insert(owner.clone()) {
            break;
        }
        owners.push(owner.clone());
        current = nominal_type(workspace_index, &owner)
            .and_then(|symbol| symbol.container.clone())
            .filter(|container| {
                nominal_type(workspace_index, container)
                    .is_some_and(|symbol| hyperion_workspace::is_type_declaration_kind(symbol.kind))
            })
            .map(|container| container.to_string());
    }
    owners
}

fn nominal_type(workspace_index: &WorkspaceIndex, fqn: &str) -> Option<std::sync::Arc<Symbol>> {
    workspace_index.get_symbol_target(&hyperion_workspace::SymbolTarget::new(
        fqn,
        hyperion_workspace::DeclarationNamespace::Type,
    ))
}

fn type_and_supertypes(workspace_index: &WorkspaceIndex, type_fqn: &str) -> HashSet<String> {
    let mut result = HashSet::new();
    let mut pending = vec![type_fqn.to_string()];
    while let Some(candidate) = pending.pop() {
        if !result.insert(candidate.clone()) {
            continue;
        }
        pending.extend(workspace_index.direct_supertypes(&candidate));
    }
    result
}
