// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! One entry point for every editor feature that offers a type's inherited
//! members.
//!
//! Two questions have to be answered before "implement interface methods",
//! "override methods", or their completion equivalents can produce anything,
//! and each feature used to answer both for itself:
//!
//! 1. *Which type is being edited?* Walking to the first `CLASS_DECL` picks
//!    the wrong declaration in a multi-type file and finds nothing at all in
//!    an interface, enum or record; using the written supertype spelling as an
//!    index key finds nothing for any packaged type.
//! 2. *What does that type inherit?* Reading the members of the directly named
//!    supertypes misses everything a grandparent or superinterface contributes.
//!
//! [`enclosing_type_fqn`] answers the first from the cursor's own ancestors,
//! and [`inherited_method_stubs`] answers the second by delegating to
//! `WorkspaceIndex::find_inherited_methods`, the canonical JLS
//! effective-method election. What lives here is only the *editor policy* that
//! the index deliberately has no opinion about: which inherited methods are
//! worth offering, and how to render one as compilable source.

use std::collections::HashSet;

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{
    index::WorkspaceIndex, java_parameter_type_from_declaration, InheritedMethod,
    ProjectedCallableParameter, ProjectedVariadicStyle,
};
use rowan::SyntaxToken;

use crate::navigation::import_resolver::{extract_class_name, extract_package_from_root};

/// `java.lang.Object`'s methods are inherited by every reference type, but
/// offering them here would bury the interesting candidates and generate
/// `super.clone()`/`super.finalize()` stubs in every class. The three that
/// users actually want have purpose-built generators already
/// (`generate_equals_hashcode_action`, `generate_tostring_action`).
const UNIVERSAL_SUPERTYPE: &str = "java.lang.Object";

/// Types that are in scope without an import, and whose package prefix a
/// generated stub can therefore drop (JLS 7.3).
const IMPLICITLY_IMPORTED_PACKAGE: &str = "java.lang";

/// One inherited method rendered as the source a generator can paste.
#[derive(Debug, Clone)]
pub struct InheritedMethodStub {
    pub name: String,
    /// FQN of the type that declares the method being redeclared.
    pub declaring_owner: String,
    /// `true` when the declaration is abstract, so the stub must supply a body
    /// rather than delegating to the inherited one.
    pub requires_implementation: bool,
    /// `true` when the declaring owner is an interface.
    pub declaring_owner_is_interface: bool,
    pub return_type: String,
    /// Parameter types only, already receiver-projected and spelled so that
    /// they resolve from the editing file without a new import.
    pub parameter_types: Vec<String>,
}

impl InheritedMethodStub {
    /// Render the parameter list of the redeclaration, naming the parameters
    /// positionally.
    #[must_use]
    pub fn parameter_list(&self) -> String {
        self.parameter_types
            .iter()
            .enumerate()
            .map(|(index, type_name)| format!("{type_name} arg{index}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Render the positional argument list for a delegation.
    #[must_use]
    pub fn argument_list(&self) -> String {
        (0..self.parameter_types.len())
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Render the delegation to the inherited body, given the argument list.
    ///
    /// Plain `super` denotes the direct superclass (JLS 15.12.1), so it cannot
    /// reach an inherited interface `default` method: that needs the qualified
    /// `Owner.super.member(...)` form.
    #[must_use]
    pub fn super_call(&self, arguments: &str) -> String {
        let receiver = if self.declaring_owner_is_interface {
            let owner = self
                .declaring_owner
                .rsplit('.')
                .next()
                .unwrap_or(&self.declaring_owner);
            format!("{owner}.super")
        } else {
            "super".to_owned()
        };
        format!("{receiver}.{}({arguments})", self.name)
    }
}

/// The type declaration the cursor sits in.
///
/// Ancestor search alone is not enough. Hyperion's CST is precede-style: a
/// declaration's `MODIFIER_LIST` (and its annotations) are *siblings* of the
/// declaration they decorate, not children of it. A cursor on the `public` of
/// `public class Impl implements Derived {` therefore has ancestors
/// `MODIFIER -> MODIFIER_LIST -> SOURCE_FILE` and no type declaration among
/// them at all, which is why the generation actions were silently unavailable
/// from the most obvious place to invoke them.
#[must_use]
pub fn type_declaration_at(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
) -> Option<SyntaxNode> {
    let mut current = token.parent()?;
    loop {
        if current.kind().is_type_declaration() {
            return Some(current);
        }
        if let Some(decorated) = current
            .next_sibling()
            .filter(|sibling| sibling.kind().is_type_declaration())
        {
            return Some(decorated);
        }
        current = current.parent()?;
    }
}

/// The fully qualified name of a type declaration node.
///
/// Nested declarations are joined outermost-first (`p.Outer.Inner`). Every
/// type declaration kind counts, so this works in interfaces, enums, records
/// and annotations rather than only in classes.
#[must_use]
pub fn type_declaration_fqn(type_node: &SyntaxNode) -> Option<String> {
    type_declaration_fqn_in(type_node, None)
}

/// [`type_declaration_fqn`] with a package to fall back on.
///
/// The file's own `PACKAGE_DECL` always wins; `fallback_package` covers the
/// callers that hold an indexed package for a file whose parsed range does not
/// contain one.
#[must_use]
pub fn type_declaration_fqn_in(
    type_node: &SyntaxNode,
    fallback_package: Option<&str>,
) -> Option<String> {
    let mut names = type_node
        .ancestors()
        .filter(|node| node.kind().is_type_declaration())
        .filter_map(|node| extract_class_name(&node))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return None;
    }
    names.reverse();
    let joined = names.join(".");
    let root = type_node.ancestors().last()?;
    let package = extract_package_from_root(&root)
        .or_else(|| fallback_package.map(str::to_owned))
        .filter(|package| !package.is_empty());
    Some(package.map_or_else(|| joined.clone(), |package| format!("{package}.{joined}")))
}

/// The fully qualified name of the type declaration the cursor sits in.
///
/// The *cursor's* position decides, so a multi-type file resolves the type the
/// user is actually editing rather than the file's first `CLASS_DECL`.
#[must_use]
pub fn enclosing_type_fqn(token: &SyntaxToken<hyperion_parser::JavaLanguage>) -> Option<String> {
    type_declaration_fqn(&type_declaration_at(token)?)
}

/// Every method name written directly in `type_node`, including the ones a
/// pending edit has added since the last index publication.
#[must_use]
pub fn method_names_declared_in(type_node: &SyntaxNode) -> HashSet<String> {
    type_node
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .filter_map(|node| {
            node.children_with_tokens()
                .find_map(|element| match element {
                    rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                        Some(token.text().to_owned())
                    }
                    _ => None,
                })
        })
        .collect()
}

/// Shorten a projected type to the spelling that resolves from `package`.
///
/// A generated stub has no import manager behind it, so the safe default is
/// the fully qualified name; only the two package prefixes that are in scope
/// unconditionally are dropped (JLS 6.3, 7.3).
fn spell_in_package(type_name: &str, package: Option<&str>) -> String {
    let Some((prefix, simple)) = type_name.rsplit_once('.') else {
        return type_name.to_owned();
    };
    if simple.is_empty() || simple.contains(['<', '>', '[', ' ']) {
        return type_name.to_owned();
    }
    if prefix == IMPLICITLY_IMPORTED_PACKAGE || Some(prefix) == package {
        return simple.to_owned();
    }
    type_name.to_owned()
}

fn projected_parameter_type(
    parameter: &ProjectedCallableParameter,
    package: Option<&str>,
) -> String {
    let spelled = spell_in_package(&parameter.type_name, package);
    match parameter.variadic_style {
        ProjectedVariadicStyle::JavaEllipsis => spelled.strip_suffix("[]").map_or_else(
            || format!("{spelled}..."),
            |element| format!("{element}..."),
        ),
        _ => spelled,
    }
}

fn stub_for(method: &InheritedMethod, package: Option<&str>) -> InheritedMethodStub {
    let symbol = &method.definition.symbol;
    // The projection is receiver-relative, so an inherited `Repo<T>.save(T)`
    // becomes `save(User)` on a `Repo<User>` implementor. Only when the
    // generic hierarchy cannot be projected does the written spelling remain
    // the best available evidence.
    let (return_type, parameter_types) = match method.projection.as_ref() {
        Some(projection) => (
            projection
                .return_type
                .as_deref()
                .map(|written| spell_in_package(written, package)),
            projection
                .parameters
                .iter()
                .map(|parameter| projected_parameter_type(parameter, package))
                .collect(),
        ),
        None => (
            symbol
                .return_type
                .as_deref()
                .map(|written| spell_in_package(written, package)),
            symbol
                .parameters
                .iter()
                // `Symbol::parameters` holds `"Type name"` declarations
                // Pasting the whole string where a type belongs
                // generates `void save(User user arg0)`.
                .map(|parameter| {
                    spell_in_package(java_parameter_type_from_declaration(parameter), package)
                })
                .collect(),
        ),
    };
    InheritedMethodStub {
        name: symbol.simple_name.to_string(),
        declaring_owner: method.declaring_owner.clone(),
        requires_implementation: method.requires_implementation,
        declaring_owner_is_interface: method.declaring_owner_is_interface,
        return_type: return_type.unwrap_or_else(|| "void".to_owned()),
        parameter_types,
    }
}

/// Every method `receiver_fqn` inherits that is worth offering to redeclare.
///
/// The hierarchy walk itself is `WorkspaceIndex::find_inherited_methods`, so
/// this is transitive through superclasses and superinterfaces alike and
/// uniform across classes, interfaces, enums and records. `already_declared`
/// carries the names written in the buffer being edited, which the index
/// cannot know about until the edit is published.
#[must_use]
pub fn inherited_method_stubs(
    workspace_index: &WorkspaceIndex,
    receiver_fqn: &str,
    already_declared: &HashSet<String>,
) -> Vec<InheritedMethodStub> {
    let package = receiver_fqn.rsplit_once('.').map(|(package, _)| package);
    let mut seen = HashSet::new();
    workspace_index
        .find_inherited_methods(receiver_fqn, &[])
        .iter()
        .filter(|method| method.declaring_owner != UNIVERSAL_SUPERTYPE)
        .filter(|method| !already_declared.contains(method.definition.symbol.simple_name.as_ref()))
        .map(|method| stub_for(method, package))
        .filter(|stub| seen.insert((stub.name.clone(), stub.parameter_types.clone())))
        .collect()
}
