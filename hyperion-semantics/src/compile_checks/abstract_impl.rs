// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! `Impl is not abstract and does not override abstract method m() in Iface`
//! (JLS 8.1.1.1): a concrete class must implement every inherited abstract
//! method.

use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::{SyntaxKind, SyntaxNode};

use super::constructors::simplify_qualified_names;
use super::facts::TypeFacts;
use super::lexical::{is_in_local_or_anonymous_type, lexical_type_fqns};
use crate::semantic_error::{SemanticError, SemanticErrorKind};

/// Check every concrete class, enum and record declared in `root`.
pub fn check_abstract_implementations(
    root: &SyntaxNode,
    facts: &TypeFacts<'_>,
    package: Option<&str>,
) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    for node in root.descendants() {
        if !matches!(
            node.kind(),
            SyntaxKind::CLASS_DECL | SyntaxKind::ENUM_DECL | SyntaxKind::RECORD_DECL
        ) {
            continue;
        }
        if let Some(error) = check_type(&node, facts, package) {
            errors.push(error);
        }
    }
    errors
}

fn check_type(
    node: &SyntaxNode,
    facts: &TypeFacts<'_>,
    package: Option<&str>,
) -> Option<SemanticError> {
    if declaration_modifiers(node).is_abstract() || is_in_local_or_anonymous_type(node) {
        return None;
    }
    // Constant-specific class bodies implement an enum's abstract methods.
    if node.kind() == SyntaxKind::ENUM_DECL
        && node.children().any(|child| {
            child.kind() == SyntaxKind::ENUM_CONSTANT
                && child
                    .children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::L_BRACE)
        })
    {
        return None;
    }
    let fqn = lexical_type_fqns(node, package).into_iter().next()?;
    let index = facts.index();
    let symbol = facts.type_symbol(&fqn)?;
    if matches!(
        symbol.kind,
        lsp_types::SymbolKind::INTERFACE | lsp_types::SymbolKind::OBJECT
    ) {
        return None;
    }
    // Inherited members are only knowable over a fully resolved hierarchy;
    // Lombok may add implementations no source declares.
    let supertypes = facts.supertypes(&fqn)?;
    if facts.is_foreign_language_type(&fqn)
        || supertypes
            .iter()
            .any(|supertype| facts.is_foreign_language_type(supertype))
    {
        return None;
    }
    if supertypes.iter().any(|supertype| {
        index.find_symbol(supertype).is_some_and(|location| {
            index.is_workspace_file(location.file_id)
                && index
                    .get_file_contents(location.file_id)
                    .is_none_or(|source| source.contains("lombok"))
        })
    }) {
        return None;
    }

    let inherited = index.find_inherited_methods(&fqn, &[]);
    // A signature the index could not project is unreliable: judge nothing.
    if inherited.iter().any(|method| method.projection.is_none()) {
        return None;
    }
    let missing = inherited
        .iter()
        .filter(|method| method.requires_implementation)
        .filter(|method| !redeclares_object_method(method))
        .filter(|method| !has_concrete_method_of_same_shape(facts, &fqn, method))
        .min_by(|a, b| {
            a.definition
                .symbol
                .range
                .start
                .cmp(&b.definition.symbol.range.start)
        })?;

    let name_range = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_type_identifier())
        .map(|token| token.text_range())?;
    let projection = missing.projection.as_ref()?;
    let parameters = projection
        .parameters
        .iter()
        .map(|parameter| simplify_qualified_names(&parameter.type_name))
        .collect::<Vec<_>>()
        .join(",");
    Some(SemanticError::new(
        name_range,
        SemanticErrorKind::AbstractMethodNotImplemented {
            type_name: simple_name(&fqn),
            method: format!("{}({parameters})", missing.definition.symbol.simple_name),
            owner: simple_name(&missing.declaring_owner),
        },
    ))
}

/// Simple (member) name of a type: drops the package, keeps nesting.
fn simple_name(fqn: &str) -> String {
    let segments: Vec<&str> = fqn.split('.').collect();
    let first_type = segments
        .iter()
        .position(|segment| segment.chars().next().is_some_and(char::is_uppercase))
        .unwrap_or(segments.len().saturating_sub(1));
    segments[first_type..].join(".")
}

/// Interfaces (and `java.lang.Record`) redeclare `equals`, `hashCode` and
/// `toString` as abstract; every class inherits the implementation from
/// `Object` (JLS 9.4.1.2), and records get generated ones.
fn redeclares_object_method(method: &hyperion_workspace::InheritedMethod) -> bool {
    let symbol = &method.definition.symbol;
    let parameters = method
        .projection
        .as_ref()
        .map_or(0, |projection| projection.parameters.len());
    match &*symbol.simple_name {
        "equals" => parameters == 1,
        "hashCode" | "toString" => parameters == 0,
        _ => false,
    }
}

/// Is there a non-abstract method of that name and arity visible on the type?
///
/// The index elects one dispatch slot per erased signature, and the erased
/// signature of a member can be spelled through a guessed package (a wildcard
/// import resolved before the imported class was known). Matching by name and
/// arity instead trades away "wrong parameter types" errors for never
/// reporting an override the index merely failed to pair up.
fn has_concrete_method_of_same_shape(
    facts: &TypeFacts<'_>,
    fqn: &str,
    method: &hyperion_workspace::InheritedMethod,
) -> bool {
    let arity = method
        .projection
        .as_ref()
        .map_or(0, |projection| projection.parameters.len());
    facts
        .index()
        .find_effective_methods_by_name(fqn, &method.definition.symbol.simple_name)
        .iter()
        .any(|candidate| {
            !candidate.modifiers.is_abstract
                && candidate.container.as_deref() != Some(method.declaring_owner.as_str())
                && candidate.parameters.len() == arity
        })
}
