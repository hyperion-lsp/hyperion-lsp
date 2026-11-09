// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Checked-exception analysis (JLS 11.2): `unreported exception X; must be
//! caught or declared to be thrown`.
//!
//! Only the exception *types* are decided here; where they are thrown and which
//! handlers surround the site is tracked by the type checker's walk. Every
//! question answers `None` -- "cannot tell" -- unless the exception's whole
//! class hierarchy is resolved, so a missing dependency never turns into an
//! accusation.

use std::sync::Arc;

use hyperion_workspace::{Symbol, TypeNameResolution};

use super::facts::TypeFacts;

/// What surrounds a throwing site, innermost first while searching.
#[derive(Debug, Clone)]
pub enum Handler {
    /// A method or constructor with its declared `throws` types (FQNs).
    Method(Vec<String>),
    /// A `try` whose body encloses the site; catches these types.
    Caught(Vec<String>),
    /// A lambda body. `Some` when the target's functional method is known.
    Lambda(Option<Vec<String>>),
    /// Initializers, unresolvable declarations, `@SneakyThrows`: judge nothing.
    Opaque,
}

/// Is `exception` a *checked* exception class? `None` when unknown.
pub fn is_checked(facts: &TypeFacts<'_>, exception: &str) -> Option<bool> {
    let supertypes = facts.supertypes(exception)?;
    if !supertypes.contains("java.lang.Throwable") {
        // Not throwable at all: another check's problem.
        return None;
    }
    Some(
        !supertypes.contains("java.lang.RuntimeException")
            && !supertypes.contains("java.lang.Error"),
    )
}

/// Resolve a `throws` spelling of `method` to a class FQN.
///
/// Bytecode declarations already carry qualified names. Source declarations
/// keep the written spelling, which is resolved in the declaring file's
/// context (its imports, its enclosing types).
pub fn resolve_declared_exception(
    facts: &TypeFacts<'_>,
    method: &Symbol,
    written: &str,
) -> Option<String> {
    if written.contains('.') && facts.is_known_type(written) {
        return Some(written.to_owned());
    }
    let owner = method.container.as_deref()?;
    let index = facts.index();
    let location = index.find_symbol(owner)?;
    // Nearest first: the owner, then each enclosing type.
    let mut owners = vec![owner.to_owned()];
    let mut current = owner;
    while let Some((outer, _)) = current.rsplit_once('.') {
        if index.has_class(outer) {
            owners.push(outer.to_owned());
        }
        current = outer;
    }
    match index.resolve_type_name(location.file_id, &owners, written) {
        TypeNameResolution::Exact(fqn) if facts.is_known_type(&fqn) => Some(fqn),
        _ => None,
    }
}

/// The checked exceptions *every* possibly-applicable candidate declares.
///
/// When several overloads remain possible, only exceptions all of them throw
/// are certain to be thrown by the call. `None` when any declaration cannot be
/// read completely.
pub fn certainly_thrown(facts: &TypeFacts<'_>, candidates: &[Arc<Symbol>]) -> Option<Vec<String>> {
    let mut common: Option<Vec<String>> = None;
    for candidate in candidates {
        let type_variables: Vec<&str> = candidate
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.as_ref())
            .collect();
        // Bytecode reports a thrown type variable by its name (see the
        // extractor); source declarations spell it. Either way it is a name in
        // scope as a type variable of the member, its owner or an outer type.
        let variable_names = type_variable_names(facts, candidate);
        let mut thrown = Vec::new();
        for written in candidate.throws() {
            // `throws E` with a type variable: whatever the caller infers.
            if type_variables.contains(&&**written)
                || variable_names.iter().any(|name| name == &**written)
            {
                return None;
            }
            let resolved = resolve_declared_exception(facts, candidate, written)?;
            if is_checked(facts, &resolved)? {
                thrown.push(resolved);
            }
        }
        common = Some(match common {
            None => thrown,
            Some(previous) => previous
                .into_iter()
                .filter(|exception| thrown.contains(exception))
                .collect(),
        });
    }
    common
}

/// Search the handler stack from the innermost frame outward. `Some(true)`
/// when handled, `Some(false)` when provably unhandled, `None` when unknown.
pub fn is_handled(facts: &TypeFacts<'_>, handlers: &[Handler], exception: &str) -> Option<bool> {
    for handler in handlers.iter().rev() {
        match handler {
            Handler::Caught(types) => {
                for caught in types {
                    if facts.is_subtype(exception, caught)? {
                        return Some(true);
                    }
                }
            }
            Handler::Method(declared) => {
                for declared in declared {
                    if facts.is_subtype(exception, declared)? {
                        return Some(true);
                    }
                }
                return Some(false);
            }
            Handler::Lambda(Some(allowed)) => {
                for allowed in allowed {
                    if facts.is_subtype(exception, allowed)? {
                        return Some(true);
                    }
                }
                return Some(false);
            }
            Handler::Lambda(None) | Handler::Opaque => return None,
        }
    }
    // No enclosing body at all: nothing to judge.
    None
}

/// Names of every type variable a member's `throws` clause could name: its
/// own, its owner's and its enclosing types'.
fn type_variable_names(facts: &TypeFacts<'_>, member: &Symbol) -> Vec<String> {
    let mut names: Vec<String> = member
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.to_string())
        .collect();
    let mut owner = member.container.as_deref().map(str::to_owned);
    while let Some(current) = owner {
        if let Some(symbol) = facts.type_symbol(&current) {
            names.extend(symbol.type_parameters.iter().map(|p| p.name.to_string()));
        }
        owner = current.rsplit_once('.').map(|(outer, _)| outer.to_owned());
    }
    names
}
