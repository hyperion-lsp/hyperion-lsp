// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lombok awareness for "this member does not exist" conclusions.
//!
//! Lombok adds members at compile time that no source file declares. The
//! index synthesizes the common ones (getters, setters, builders, constructors,
//! loggers), but the annotation set is large and partly configurable, so a
//! class touched by Lombok can never be *proven* to lack a member. This module
//! answers the one question that keeps errors precise: could Lombok have
//! generated a member with this name on this type?

use super::facts::TypeFacts;

/// Lombok features whose generated members cannot be enumerated from the
/// annotation alone.
const OPAQUE_FEATURES: &[&str] = &[
    "@Delegate",
    "@ExtensionMethod",
    "@SuperBuilder",
    "@Builder",
    "@Jacksonized",
    "@Singular",
    "@With",
    "@Withers",
    "@FieldNameConstants",
    "@StandardException",
    "@UtilityClass",
    "@CustomLog",
    "@Accessors",
    "@Cleanup",
    "@SneakyThrows",
    "@Synchronized",
    "@Helper",
];

/// Names generated with fixed spelling.
const FIXED_NAMES: &[&str] = &[
    "builder",
    "toBuilder",
    "build",
    "canEqual",
    "equals",
    "hashCode",
    "toString",
];

/// True when the file mentions Lombok at all.
fn source_uses_lombok(source: &str) -> bool {
    source.contains("lombok")
}

/// Could Lombok have generated a member called `name` on `type_fqn` or on any
/// type it inherits from (or is nested in)?
///
/// `false` means "no": the receiver's whole workspace-source hierarchy is free
/// of Lombok, or Lombok is present but cannot produce this spelling.
pub fn lombok_may_generate(facts: &TypeFacts<'_>, type_fqn: &str, name: &str) -> bool {
    let index = facts.index();
    let Some(supertypes) = facts.supertypes(type_fqn) else {
        return true;
    };
    let mut owners: Vec<String> = supertypes.iter().cloned().collect();
    // Lexically enclosing types contribute nested builders and log fields.
    let mut outer = type_fqn;
    while let Some((prefix, _)) = outer.rsplit_once('.') {
        if index.has_class(prefix) {
            owners.push(prefix.to_owned());
        }
        outer = prefix;
    }

    for owner in owners {
        let Some(location) = index.find_symbol(&owner) else {
            continue;
        };
        if !index.is_workspace_file(location.file_id) {
            continue;
        }
        let Some(source) = index.get_file_contents(location.file_id) else {
            // Cannot inspect the declaring source: assume the worst.
            return true;
        };
        if !source_uses_lombok(&source) {
            continue;
        }
        if OPAQUE_FEATURES
            .iter()
            .any(|feature| source.contains(feature))
        {
            return true;
        }
        if FIXED_NAMES.contains(&name) {
            return true;
        }
        if accessor_matches_a_field(facts, &owner, name) {
            return true;
        }
    }
    false
}

/// `getX`/`isX`/`setX`/`x` for a field `x` of `owner`.
fn accessor_matches_a_field(facts: &TypeFacts<'_>, owner: &str, name: &str) -> bool {
    let fields: Vec<String> = facts
        .index()
        .get_class_members(owner)
        .iter()
        .filter(|member| member.kind == lsp_types::SymbolKind::FIELD)
        .map(|member| member.simple_name.to_string())
        .collect();
    if fields.iter().any(|field| field == name) {
        return true; // @Accessors(fluent = true)
    }
    for prefix in ["get", "is", "set", "with"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            let mut chars = rest.chars();
            let Some(first) = chars.next() else { continue };
            if !first.is_uppercase() {
                continue;
            }
            let decapitalized: String = first.to_lowercase().chain(chars).collect();
            if fields.iter().any(|field| {
                *field == decapitalized
                    // boolean fields named `isFoo` keep the prefix in their getter
                    || format!("is{}", capitalize(&decapitalized)) == *field
            }) {
                return true;
            }
        }
    }
    false
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
