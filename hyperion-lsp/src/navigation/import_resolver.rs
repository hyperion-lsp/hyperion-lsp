// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import and class FQN resolution utilities

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::type_parameter_mapping::parse_type_in_declaration_exact;
use hyperion_semantics::symbol::Type;
use hyperion_workspace::{DeclarationNamespace, SymbolLocation, SymbolTarget, WorkspaceIndex};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

fn invocation_argument_types(
    argument_types: &[Type],
) -> Vec<hyperion_workspace::InvocationArgumentType> {
    argument_types
        .iter()
        .map(|argument| match argument {
            Type::Null => hyperion_workspace::InvocationArgumentType::Null,
            Type::Unknown => hyperion_workspace::InvocationArgumentType::Unknown,
            argument => hyperion_workspace::InvocationArgumentType::Known(argument.to_string()),
        })
        .collect()
}

/// A static value binding shadows a same-spelled type in expression position.
/// Keep an unresolved value distinct from an absent binding so callers never
/// promote the type after a failed value type projection.
pub(crate) enum StaticValueTypeResolution {
    Missing,
    Resolved(Type),
    Unresolved,
}

pub(crate) fn resolve_static_value_type_via_imports(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    name: &str,
) -> StaticValueTypeResolution {
    let Some(imports) = workspace_index.get_imports(file_id) else {
        return StaticValueTypeResolution::Missing;
    };
    let resolver =
        hyperion_workspace::import_resolver::ImportResolver::new(&imports, workspace_index);
    let values = resolver.resolve_static_values(name);
    let [value] = values.as_slice() else {
        return if values.is_empty() {
            StaticValueTypeResolution::Missing
        } else {
            StaticValueTypeResolution::Unresolved
        };
    };
    let Some(written_type) = value.return_type.as_deref() else {
        return StaticValueTypeResolution::Unresolved;
    };
    let Some(owner) = value.container.as_deref() else {
        return StaticValueTypeResolution::Unresolved;
    };
    parse_type_in_declaration_exact(workspace_index, owner, written_type, &HashSet::new()).map_or(
        StaticValueTypeResolution::Unresolved,
        StaticValueTypeResolution::Resolved,
    )
}

/// Helper function to check if a simple name could be an inner class
///
/// FIX: Extracted to eliminate duplicate logic between lines 29-51 and 62-78
fn try_resolve_as_inner_class(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    package: &str,
    simple_name: &str,
) -> Option<String> {
    let file_path = workspace_index.get_file_path(file_id)?;
    let file_name = file_path.file_stem()?;
    let potential_outer_class = format!("{}.{}", package, file_name.to_string_lossy());
    let potential_inner = format!("{potential_outer_class}.{simple_name}");

    debug!(
        "IMPORT_RESOLVE: Checking inner class '{}' of '{}'",
        simple_name, potential_outer_class
    );

    if workspace_index
        .get_symbol_target(&SymbolTarget::new(
            potential_inner.as_str(),
            DeclarationNamespace::Type,
        ))
        .is_some()
    {
        debug!(
            "IMPORT_RESOLVE: OK Resolved via inner class: {}",
            potential_inner
        );
        Some(potential_inner)
    } else {
        None
    }
}

/// Resolve a simple class name to its fully qualified name using imports
pub fn resolve_class_fqn_via_imports(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    simple_name: &str,
) -> Result<String, String> {
    let imports = workspace_index
        .get_imports(file_id)
        .ok_or_else(|| format!("No imports found for file {file_id:?}"))?;

    // FIX: Check if this could be an inner class of the current class before imports
    // Uses extracted helper function to avoid code duplication
    if let Some(package) = &imports.current_package {
        if let Some(inner_class_fqn) =
            try_resolve_as_inner_class(workspace_index, file_id, package, simple_name)
        {
            return Ok(inner_class_fqn);
        }
    }

    match workspace_index.resolve_type_name(file_id, &[], simple_name) {
        hyperion_workspace::TypeNameResolution::Exact(fqn) => Ok(fqn),
        hyperion_workspace::TypeNameResolution::Missing => {
            Err(format!("Could not resolve class name: {simple_name}"))
        }
        hyperion_workspace::TypeNameResolution::Ambiguous(candidates) => Err(format!(
            "Ambiguous class name '{simple_name}': {}",
            candidates.join(", ")
        )),
    }
}

/// Extract the name (first IDENT token) from a type declaration node.
///
/// Works for `CLASS_DECL`, `INTERFACE_DECL`, `ENUM_DECL`, `ANNOTATION_DECL`, and `RECORD_DECL`.
/// Returns the simple name (not FQN) — e.g., `"User"` from `class User`.
pub fn extract_class_name(node: &SyntaxNode) -> Option<String> {
    for child in node.children_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = child {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

/// Extract package name from the root syntax node
pub fn extract_package_from_root(root: &SyntaxNode) -> Option<String> {
    for child in root.children() {
        if child.kind() == SyntaxKind::PACKAGE_DECL {
            // Extract all IDENTs separated by dots
            let mut parts = Vec::new();
            for token in child.descendants_with_tokens() {
                if let rowan::NodeOrToken::Token(t) = token {
                    if t.kind() == SyntaxKind::IDENT {
                        parts.push(t.text().to_string());
                    }
                }
            }
            if !parts.is_empty() {
                return Some(parts.join("."));
            }
        }
    }
    None
}

/// Resolve one statically imported declaration in an exact namespace.
///
/// Callable overloads and unrelated wildcard owners remain ambiguous until a
/// caller has invocation arguments. This helper has no such context, so it
/// fails closed instead of electing an index-order winner.
pub fn resolve_static_member_via_imports(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    member_name: &str,
    namespace: DeclarationNamespace,
) -> Option<SymbolLocation> {
    debug!(
        "STATIC_IMPORT: Resolving static member '{}' for file {:?}",
        member_name, file_id
    );

    let imports = workspace_index.get_imports(file_id)?;
    let resolver =
        hyperion_workspace::import_resolver::ImportResolver::new(&imports, workspace_index);
    let candidates = match namespace {
        DeclarationNamespace::Callable => resolver.resolve_static_callables(member_name),
        DeclarationNamespace::Value => resolver.resolve_static_values(member_name),
        DeclarationNamespace::Type => resolver.resolve_static_types(member_name),
        DeclarationNamespace::Constructor | DeclarationNamespace::Other => Vec::new(),
    };

    debug!(
        "STATIC_IMPORT: {} candidate(s) for '{}'",
        candidates.len(),
        member_name
    );

    let [symbol] = candidates.as_slice() else {
        debug!(
            "STATIC_IMPORT: unresolved or ambiguous {:?} binding for '{}'",
            namespace, member_name
        );
        return None;
    };
    workspace_index
        .find_symbol_definition(&SymbolTarget::for_symbol(symbol))
        .map(|definition| definition.location)
}

/// Resolve an overloaded static invocation using semantic argument facts.
///
/// Candidate collection, Java conversions, and ambiguity handling remain in
/// the workspace resolver; this adapter only translates semantic types.
pub fn resolve_static_invocation_via_imports(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    method_name: &str,
    argument_types: &[Type],
) -> Option<SymbolLocation> {
    let imports = workspace_index.get_imports(file_id)?;
    let resolver =
        hyperion_workspace::import_resolver::ImportResolver::new(&imports, workspace_index);
    let arguments = invocation_argument_types(argument_types);
    let symbol = resolver
        .resolve_static_callable_invocation(method_name, &arguments)
        .or_else(|| {
            // Navigation may encounter an expression whose type inference is
            // incomplete (most notably a lambda before target typing). If the
            // import binding denotes exactly one declaration, its return type
            // is still unambiguous and is safe to use for a read-only chain.
            // Known-but-incompatible calls and overloaded families continue to
            // fail closed.
            argument_types
                .iter()
                .any(|argument| matches!(argument, Type::Unknown))
                .then(|| resolver.resolve_static_callables(method_name))
                .and_then(|candidates| {
                    let [candidate] = candidates.as_slice() else {
                        return None;
                    };
                    Some(candidate.clone())
                })
        })?;
    workspace_index
        .find_symbol_definition(&SymbolTarget::for_symbol(&symbol))
        .map(|definition| definition.location)
}

/// Resolve a lambda call against the complete visible static-import overload
/// set. Import binding and hiding are applied before semantic applicability.
pub(crate) fn resolve_static_lambda_invocation_via_imports(
    workspace_index: &Arc<WorkspaceIndex>,
    file_id: FileId,
    method_name: &str,
    argument_types: &[Type],
    arguments: &SyntaxNode,
    syntax_root: &SyntaxNode,
) -> Option<SymbolLocation> {
    let imports = workspace_index.get_imports(file_id)?;
    let resolver =
        hyperion_workspace::import_resolver::ImportResolver::new(&imports, workspace_index);
    let allowed = resolver.resolve_static_callables(method_name);
    let mut seen = HashSet::new();
    let mut owners = HashSet::new();
    let mut candidates = Vec::new();
    for symbol in &allowed {
        let Some(owner) = symbol.container.as_deref() else {
            continue;
        };
        owners.insert(owner.to_owned());
    }
    for owner in owners {
        for candidate in
            workspace_index.find_projected_effective_methods_by_name(&owner, &[], method_name)
        {
            if !allowed
                .iter()
                .any(|symbol| Arc::ptr_eq(symbol, &candidate.definition.symbol))
            {
                continue;
            }
            let location = &candidate.definition.location;
            if seen.insert((
                candidate.definition.target.clone(),
                location.file_id,
                location.range,
            )) {
                candidates.push(candidate);
            }
        }
    }
    crate::navigation::overload_resolver::resolve_imported_overload_with_arguments(
        workspace_index,
        candidates,
        argument_types,
        arguments,
        file_id,
        syntax_root,
    )
    .map(|matched| matched.definition.location)
}

/// Whether enclosing-type method search still owns an unqualified invocation.
/// Static imports are considered only after this tier has no potentially
/// applicable member (JLS 15.12.1).
pub fn enclosing_callable_shadows_static_import(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    enclosing_owner: &str,
    method_name: &str,
    argument_types: &[Type],
) -> bool {
    let Some(imports) = workspace_index.get_imports(file_id) else {
        return true;
    };
    hyperion_workspace::import_resolver::ImportResolver::new(&imports, workspace_index)
        .has_potentially_applicable_callable_member(
            enclosing_owner,
            method_name,
            &invocation_argument_types(argument_types),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::symbol::{ClassType, PrimitiveType};

    #[tokio::test]
    async fn class_resolution_includes_static_member_type_imports() {
        let temp = tempfile::tempdir().unwrap();
        let outer = temp.path().join("Outer.java");
        let usage = temp.path().join("Use.java");
        std::fs::write(
            &outer,
            concat!(
                "package p; public class Outer { ",
                "public static class Nested {} ",
                "public static int Nested; ",
                "public static void Nested() {} }",
            ),
        )
        .unwrap();
        std::fs::write(
            &usage,
            "package consumer; import static p.Outer.Nested; class Use { Nested value; }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&outer).await.unwrap();
        index.index_file(&usage).await.unwrap();
        let file_id = index.get_file_id(&usage).unwrap();

        assert_eq!(
            resolve_class_fqn_via_imports(&index, file_id, "Nested"),
            Ok("p.Outer.Nested".to_owned())
        );
    }

    #[tokio::test]
    async fn class_resolution_preserves_qualified_member_type_spellings() {
        let temp = tempfile::tempdir().unwrap();
        let outer = temp.path().join("Outer.java");
        let usage = temp.path().join("Use.java");
        std::fs::write(
            &outer,
            "package p; public class Outer { public static class Nested {} }",
        )
        .unwrap();
        std::fs::write(&usage, "package p; class Use { Outer.Nested value; }").unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&outer).await.unwrap();
        index.index_file(&usage).await.unwrap();
        let file_id = index.get_file_id(&usage).unwrap();

        for (written, expected) in [
            ("Outer", "p.Outer"),
            ("Outer.Nested", "p.Outer.Nested"),
            ("p.Outer.Nested", "p.Outer.Nested"),
        ] {
            assert_eq!(
                resolve_class_fqn_via_imports(&index, file_id, written),
                Ok(expected.to_owned()),
                "failed to resolve {written}",
            );
        }
    }

    #[tokio::test]
    async fn constructor_prefix_does_not_fabricate_a_same_named_inner_type() {
        let temp = tempfile::tempdir().unwrap();
        let color = temp.path().join("Color.java");
        std::fs::write(
            &color,
            "package p; enum Color { RED(1); Color(int value) {} }",
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&color).await.unwrap();
        let file_id = index.get_file_id(&color).unwrap();

        assert_eq!(
            resolve_class_fqn_via_imports(&index, file_id, "Color"),
            Ok("p.Color".to_owned())
        );
        assert!(index
            .get_symbol_target(&SymbolTarget::new(
                "p.Color.Color",
                DeclarationNamespace::Type,
            ))
            .is_none());
    }

    #[tokio::test]
    async fn lsp_static_invocation_bridge_selects_one_applicable_overload() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("First.java");
        let second = temp.path().join("Second.java");
        let usage = temp.path().join("Use.java");
        std::fs::write(
            &first,
            "package p; public class First { public static void pick(int value) {} }",
        )
        .unwrap();
        std::fs::write(
            &second,
            "package p; public class Second { public static void pick(Integer value) {} }",
        )
        .unwrap();
        std::fs::write(
            &usage,
            concat!(
                "package consumer; ",
                "import static p.First.pick; ",
                "import static p.Second.*; ",
                "class Use { void run() { pick(1); } }",
            ),
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&first).await.unwrap();
        index.index_file(&second).await.unwrap();
        index.index_file(&usage).await.unwrap();
        let file_id = index.get_file_id(&usage).unwrap();

        let primitive = resolve_static_invocation_via_imports(
            &index,
            file_id,
            "pick",
            &[Type::Primitive(PrimitiveType::Int)],
        )
        .unwrap();
        assert_eq!(primitive.fqn.as_ref(), "p.First.pick(int)");

        let boxed = resolve_static_invocation_via_imports(
            &index,
            file_id,
            "pick",
            &[Type::Class(ClassType::new(
                "Integer".to_string(),
                Some("java.lang".to_string()),
            ))],
        )
        .unwrap();
        assert_eq!(boxed.fqn.as_ref(), "p.Second.pick(java.lang.Integer)");
        assert!(
            resolve_static_invocation_via_imports(&index, file_id, "pick", &[Type::Unknown],)
                .is_none()
        );
    }

    #[tokio::test]
    async fn inapplicable_enclosing_method_does_not_hide_static_import() {
        let temp = tempfile::tempdir().unwrap();
        let imported = temp.path().join("Imported.java");
        let usage = temp.path().join("Use.java");
        std::fs::write(
            &imported,
            "package p; public class Imported { public static void pick(int value) {} }",
        )
        .unwrap();
        std::fs::write(
            &usage,
            concat!(
                "package consumer; import static p.Imported.pick; ",
                "class Use { void pick(String value) {} void run() { pick(1); } }",
            ),
        )
        .unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file(&imported).await.unwrap();
        index.index_file(&usage).await.unwrap();
        let file_id = index.get_file_id(&usage).unwrap();

        assert!(!enclosing_callable_shadows_static_import(
            &index,
            file_id,
            "consumer.Use",
            "pick",
            &[Type::Primitive(PrimitiveType::Int)],
        ));
        assert!(enclosing_callable_shadows_static_import(
            &index,
            file_id,
            "consumer.Use",
            "pick",
            &[Type::Class(ClassType::new(
                "String".to_string(),
                Some("java.lang".to_string()),
            ))],
        ));
    }
}
