// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Parent Class and Interface Extraction
//!
//! Handles extraction of extends and implements clauses from Java type declarations.

use crate::ImportMap;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;
use tracing::warn;

/// Preserve explicit type arguments from every direct `extends`/`implements`
/// edge while leaving nominal name resolution contextual.
pub(crate) fn extract_generic_supertypes_in_context(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    lexical_types: &HashMap<String, String>,
) -> Vec<crate::GenericSupertype> {
    let mut edges = Vec::new();
    let mut in_supertype_clause = false;

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::EXTENDS_KW | SyntaxKind::IMPLEMENTS_KW => {
                    in_supertype_clause = true;
                }
                SyntaxKind::PERMITS_KW | SyntaxKind::L_BRACE => {
                    in_supertype_clause = false;
                }
                _ => {}
            },
            NodeOrToken::Node(class_type)
                if in_supertype_clause && class_type.kind() == SyntaxKind::CLASS_TYPE =>
            {
                let Ok(structure) = hyperion_types::extract_type_structure(&class_type) else {
                    continue;
                };
                if structure.type_args.is_empty() {
                    continue;
                }
                let name_parts = class_type_name_parts(&class_type);
                if name_parts.is_empty() {
                    continue;
                }
                edges.push(crate::GenericSupertype {
                    written_target: qualify_type_name_in_context(
                        &name_parts,
                        package,
                        imports,
                        lexical_types,
                    ),
                    arguments: structure
                        .type_args
                        .iter()
                        .map(hyperion_types::TypeStructure::to_fqn_string)
                        .collect(),
                });
            }
            NodeOrToken::Node(_) => {}
        }
    }
    edges
}

/// Extract parent class from extends clause
pub(crate) fn extract_parent_class(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    current_class_name: &str,
) -> Option<String> {
    extract_parent_class_in_context(node, package, imports, current_class_name, &HashMap::new())
}

pub(crate) fn extract_parent_class_in_context(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    current_class_name: &str,
    lexical_types: &HashMap<String, String>,
) -> Option<String> {
    let mut types = header_types_after_keyword(node, SyntaxKind::EXTENDS_KW);
    if types.is_empty() {
        return None;
    }
    // A class `extends` clause has exactly one supertype.
    let parent_name_parts = types.swap_remove(0);

    // Check for self-reference
    if parent_name_parts.len() == 1 && parent_name_parts[0] == current_class_name {
        warn!(
            "Detected self-referencing parent class in '{}' - ignoring invalid extends clause",
            current_class_name
        );
        return None;
    }

    // Qualify name using imports
    let parent_class =
        qualify_type_name_in_context(&parent_name_parts, package, imports, lexical_types);

    Some(parent_class)
}

/// Extract implemented interfaces from implements clause
pub(crate) fn extract_interfaces(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
) -> Vec<String> {
    extract_interfaces_in_context(node, package, imports, &HashMap::new())
}

pub(crate) fn extract_interfaces_in_context(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    lexical_types: &HashMap<String, String>,
) -> Vec<String> {
    extract_type_list_after_keyword(
        node,
        SyntaxKind::IMPLEMENTS_KW,
        package,
        imports,
        lexical_types,
    )
}

/// Extract the parent interfaces from an interface's `extends` clause.
///
/// Java interfaces support multi-parent inheritance:
///
/// ```java
/// interface C extends A, B { ... }
/// ```
///
/// This is structurally different from a class's `extends` clause (which is
/// single-parent and stored in `Symbol::parent_class`). For interfaces we
/// route the `extends` list into `Symbol::interfaces` so downstream
/// inheritance walks see all parents — same shape as `class D implements
/// A, B`.
pub(crate) fn extract_interface_extends(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
) -> Vec<String> {
    extract_interface_extends_in_context(node, package, imports, &HashMap::new())
}

pub(crate) fn extract_interface_extends_in_context(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    lexical_types: &HashMap<String, String>,
) -> Vec<String> {
    extract_type_list_after_keyword(
        node,
        SyntaxKind::EXTENDS_KW,
        package,
        imports,
        lexical_types,
    )
}

/// Generic helper: extract the comma-separated list of (potentially generic)
/// qualified type names anchored on `anchor_kind` in the declaration header of
/// `node`. Used by `extract_interfaces` (anchor = `IMPLEMENTS_KW`),
/// `extract_interface_extends` (anchor = `EXTENDS_KW`), and
/// `extract_parent_class` (anchor = `EXTENDS_KW`, single entry).
fn extract_type_list_after_keyword(
    node: &SyntaxNode,
    anchor_kind: SyntaxKind,
    package: &Option<String>,
    imports: &ImportMap,
    lexical_types: &HashMap<String, String>,
) -> Vec<String> {
    header_types_after_keyword(node, anchor_kind)
        .iter()
        .map(|parts| qualify_type_name_in_context(parts, package, imports, lexical_types))
        .collect()
}

/// Collect the name parts of each supertype listed after `anchor_kind` in the
/// header of the type declaration `node`.
///
/// Only the *direct* children of the declaration node are inspected: the
/// parser attaches the header clause keywords (`extends` / `implements` /
/// `permits`) and their `CLASS_TYPE` operands directly to the declaration
/// node, while
///
/// - type-parameter bounds (`class Cache<K extends Comparable<K>>`) are nested
///   inside `TYPE_PARAM_LIST > TYPE_PARAM > TYPE_BOUND`,
/// - record components live inside `PARAM_LIST`,
/// - and body members (nested classes, fields with wildcard bounds like
///   `? extends X`, …) appear after the body's `L_BRACE` token.
///
/// A descendant scan would latch onto `extends`/`implements` keywords in any
/// of those subtrees; restricting the walk to header level and
/// stopping at the first header-level `L_BRACE` (start of the body) or the
/// next clause keyword keeps only the declaration's own clause.
fn header_types_after_keyword(node: &SyntaxNode, anchor_kind: SyntaxKind) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    let mut started = false;

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == SyntaxKind::L_BRACE {
                    // Start of the type body — end of the header.
                    break;
                }
                if !started {
                    if kind == anchor_kind {
                        started = true;
                    }
                } else if matches!(
                    kind,
                    SyntaxKind::EXTENDS_KW | SyntaxKind::IMPLEMENTS_KW | SyntaxKind::PERMITS_KW
                ) {
                    // Next clause of the header — end of this clause's list.
                    break;
                }
            }
            NodeOrToken::Node(child_node) => {
                // Header nodes we deliberately do not descend into:
                // TYPE_PARAM_LIST (generic bounds), PARAM_LIST (record
                // components), and anything else that is not a supertype.
                if started && child_node.kind() == SyntaxKind::CLASS_TYPE {
                    let parts = class_type_name_parts(&child_node);
                    if !parts.is_empty() {
                        result.push(parts);
                    }
                }
            }
        }
    }

    result
}

/// Extract the qualified-name parts of a `CLASS_TYPE` node, ignoring generic
/// type arguments (`Base<String>` → `["Base"]`) and type-use annotations.
fn class_type_name_parts(class_type: &SyntaxNode) -> Vec<String> {
    let Some(qualified_name) = class_type
        .children()
        .find(|child| child.kind() == SyntaxKind::QUALIFIED_NAME)
    else {
        return Vec::new();
    };

    let mut parts = qualified_name
        .children_with_tokens()
        .filter_map(|child| match child {
            NodeOrToken::Token(token) if token.kind().is_identifier() => {
                Some(token.text().to_string())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    // The parser represents `Outer<T>.Inner<U>` with only `Outer` in the
    // leading QUALIFIED_NAME and the member segment as direct tokens after the
    // first TYPE_ARGS node. Retain those identifiers while still ignoring the
    // identifiers nested inside argument lists.
    let mut after_type_arguments = false;
    for child in class_type.children_with_tokens() {
        match child {
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::TYPE_ARGS => {
                after_type_arguments = true;
            }
            NodeOrToken::Token(token) if after_type_arguments && token.kind().is_identifier() => {
                parts.push(token.text().to_string());
            }
            _ => {}
        }
    }
    parts
}

/// Preserve a type name for later index-backed contextual resolution.
#[cfg(test)]
pub(crate) fn qualify_type_name(
    name_parts: &[String],
    package: &Option<String>,
    imports: &ImportMap,
) -> String {
    qualify_type_name_in_context(name_parts, package, imports, &HashMap::new())
}

/// Qualify a source type name using bindings known without a workspace index.
///
/// Lexical member types precede single-type imports. For unresolved simple
/// names we deliberately retain the written spelling: the canonical indexed
/// resolver later distinguishes same-package, `java.lang`, and all
/// explicit on-demand candidates as Exact/Missing/Ambiguous. A finite
/// hard-coded `java.lang` list cannot model late JDK indexing or ambiguity.
pub(crate) fn qualify_type_name_in_context(
    name_parts: &[String],
    package: &Option<String>,
    imports: &ImportMap,
    lexical_types: &HashMap<String, String>,
) -> String {
    if name_parts.is_empty() {
        return String::new();
    }

    let first = &name_parts[0];
    let suffix = &name_parts[1..];

    if let Some(lexical) = lexical_types.get(first) {
        return std::iter::once(lexical.as_str())
            .chain(suffix.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(".");
    }

    // Keep every non-lexical spelling exactly as written. Imports, packages,
    // and `java.lang.*` may not be indexed yet, and fabricating a package FQN
    // loses the distinction between `p.Outer.Base` and source `Outer.Base` in
    // package `p`. The indexed resolver owns contextual resolution.
    let _ = (package, imports);
    name_parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_extractor::{ExtractionResult, SymbolExtractor};
    use crate::Symbol;

    fn extract(source: &str) -> ExtractionResult {
        SymbolExtractor::extract(source).expect("extraction should succeed")
    }

    fn symbol<'a>(result: &'a ExtractionResult, simple_name: &str) -> &'a Symbol {
        result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == simple_name)
            .unwrap_or_else(|| panic!("symbol '{}' not extracted", simple_name))
    }

    fn interfaces_of(sym: &Symbol) -> Vec<&str> {
        sym.interfaces.iter().map(|i| i.as_ref()).collect()
    }

    // Type-parameter bounds must not be mistaken for the extends clause

    #[test]
    fn test_bounded_type_param_with_extends_uses_real_superclass() {
        let result = extract(
            "package com.example;\n\
             class Cache<K extends Comparable<K>> extends Base {}",
        );
        let sym = symbol(&result, "Cache");
        assert_eq!(sym.parent_class.as_deref(), Some("Base"));
        assert!(interfaces_of(sym).is_empty());
    }

    #[test]
    fn test_bounded_type_param_with_extends_and_implements() {
        let result = extract(
            "package com.example;\n\
             class Foo<T extends A> extends B implements I, J {}",
        );
        let sym = symbol(&result, "Foo");
        assert_eq!(sym.parent_class.as_deref(), Some("B"));
        assert_eq!(interfaces_of(sym), vec!["I", "J"]);
    }

    #[test]
    fn test_bounded_type_param_without_extends_has_no_parent() {
        let result = extract(
            "package com.example;\n\
             class Foo<T extends A> {}",
        );
        let sym = symbol(&result, "Foo");
        assert_eq!(sym.parent_class, None);
        assert!(interfaces_of(sym).is_empty());
    }

    #[test]
    fn test_crtp_bound_does_not_mask_real_parent() {
        // The self-reference guard previously swallowed the misparsed CRTP
        // bound; with header-level extraction the real parent must survive.
        let result = extract(
            "package com.example;\n\
             class Foo<T extends Foo<T>> extends Base {}",
        );
        let sym = symbol(&result, "Foo");
        assert_eq!(sym.parent_class.as_deref(), Some("Base"));
    }

    #[test]
    fn test_crtp_without_extends_has_no_parent() {
        let result = extract(
            "package com.example;\n\
             class Foo<T extends Foo<T>> {}",
        );
        let sym = symbol(&result, "Foo");
        assert_eq!(sym.parent_class, None);
    }

    #[test]
    fn test_genuine_self_extends_is_still_ignored() {
        let result = extract(
            "package com.example;\n\
             class Foo extends Foo {}",
        );
        let sym = symbol(&result, "Foo");
        assert_eq!(sym.parent_class, None);
    }

    #[test]
    fn test_interface_bounded_param_with_extends_list() {
        let result = extract(
            "package com.example;\n\
             interface Multi<T extends Number> extends A, B {}",
        );
        let sym = symbol(&result, "Multi");
        assert_eq!(sym.parent_class, None);
        assert_eq!(interfaces_of(sym), vec!["A", "B"]);
    }

    #[test]
    fn test_nested_class_extends_does_not_leak_to_outer() {
        let result = extract(
            "package com.example;\n\
             class Outer {\n\
                 class Inner extends Base {}\n\
             }",
        );
        let outer = symbol(&result, "Outer");
        assert_eq!(outer.parent_class, None);
        let inner = symbol(&result, "Inner");
        assert_eq!(inner.parent_class.as_deref(), Some("Base"));
    }

    #[test]
    fn test_nested_class_implements_does_not_leak_to_outer() {
        let result = extract(
            "package com.example;\n\
             class Outer {\n\
                 static class Inner implements Iface {}\n\
             }",
        );
        let outer = symbol(&result, "Outer");
        assert!(interfaces_of(outer).is_empty());
        let inner = symbol(&result, "Inner");
        assert_eq!(interfaces_of(inner), vec!["Iface"]);
    }

    #[test]
    fn test_wildcard_bound_in_body_not_mistaken_for_parent() {
        let result = extract(
            "package com.example;\n\
             class Holder {\n\
                 java.util.List<? extends Number> xs;\n\
             }",
        );
        let sym = symbol(&result, "Holder");
        assert_eq!(sym.parent_class, None);
    }

    #[test]
    fn test_enum_implements_extracted() {
        let result = extract(
            "package com.example;\n\
             enum Status implements Describable { OPEN, CLOSED }",
        );
        let sym = symbol(&result, "Status");
        assert_eq!(sym.parent_class, None);
        assert_eq!(interfaces_of(sym), vec!["Describable"]);
    }

    #[test]
    fn test_record_with_bounded_param_implements() {
        let result = extract(
            "package com.example;\n\
             record Pair<T extends Comparable<T>>(T first, T second) implements Ordered {}",
        );
        let sym = symbol(&result, "Pair");
        assert_eq!(sym.parent_class, None);
        assert_eq!(interfaces_of(sym), vec!["Ordered"]);
    }

    #[test]
    fn test_generic_supertypes_strip_type_arguments() {
        let result = extract(
            "package com.example;\n\
             class Impl extends Base<String> implements Func<String, Integer> {}",
        );
        let sym = symbol(&result, "Impl");
        assert_eq!(sym.parent_class.as_deref(), Some("Base"));
        assert_eq!(interfaces_of(sym), vec!["Func"]);
        let metadata = result
            .generic_supertypes
            .iter()
            .find(|metadata| metadata.type_fqn == "com.example.Impl")
            .expect("parameterized edges should be retained separately");
        assert_eq!(
            metadata.edges,
            [
                crate::GenericSupertype {
                    written_target: "Base".to_string(),
                    arguments: vec!["String".to_string()],
                },
                crate::GenericSupertype {
                    written_target: "Func".to_string(),
                    arguments: vec!["String".to_string(), "Integer".to_string()],
                },
            ]
        );
    }

    #[test]
    fn test_interface_extends_stops_before_permits_clause() {
        let result = extract(
            "package com.example;\n\
             sealed interface Shape extends Drawable permits Circle {}",
        );
        let sym = symbol(&result, "Shape");
        assert_eq!(interfaces_of(sym), vec!["Drawable"]);
    }

    #[test]
    fn generic_nested_supertype_preserves_outer_and_inner_arguments() {
        let result = extract(
            "package com.example;\n\
             class Outer<T> { class Inner<U> {} }\n\
             class Child extends Outer<String>.Inner<Integer> {}",
        );
        let metadata = result
            .generic_supertypes
            .iter()
            .find(|metadata| metadata.type_fqn == "com.example.Child")
            .expect("nested parameterized edge metadata");
        assert_eq!(
            symbol(&result, "Child").parent_class.as_deref(),
            Some("Outer.Inner")
        );
        assert_eq!(
            metadata.edges,
            [crate::GenericSupertype {
                written_target: "Outer.Inner".to_string(),
                arguments: vec!["String".to_string(), "Integer".to_string()],
            }]
        );
    }

    #[test]
    fn test_qualify_type_name_explicit_import() {
        let mut imports = ImportMap::new();
        imports.add_import("List".to_string(), "java.util.List".to_string());
        imports.add_import("Map".to_string(), "java.util.Map".to_string());

        let package = Some("com.example".into());

        // Source extraction preserves contextual spelling; indexed resolution
        // applies the explicit import once the file is published.
        assert_eq!(
            qualify_type_name(&["List".to_string()], &package, &imports),
            "List"
        );
        assert_eq!(
            qualify_type_name(&["Map".to_string()], &package, &imports),
            "Map"
        );
    }

    #[test]
    fn test_qualify_type_name_java_lang() {
        let imports = ImportMap::new();
        let package = Some("com.example".into());

        // `java.lang.*` is resolved with all other on-demand imports later.
        assert_eq!(
            qualify_type_name(&["Object".to_string()], &package, &imports),
            "Object"
        );
        assert_eq!(
            qualify_type_name(&["String".to_string()], &package, &imports),
            "String"
        );
        assert_eq!(
            qualify_type_name(&["Comparable".to_string()], &package, &imports),
            "Comparable"
        );
        assert_eq!(
            qualify_type_name(&["Runnable".to_string()], &package, &imports),
            "Runnable"
        );
    }

    #[test]
    fn test_qualify_type_name_fallback_to_package() {
        let imports = ImportMap::new();
        let package = Some("com.example".into());

        // Unknown types retain their original source spelling.
        assert_eq!(
            qualify_type_name(&["CustomInterface".to_string()], &package, &imports),
            "CustomInterface"
        );
        assert_eq!(
            qualify_type_name(&["MyService".to_string()], &package, &imports),
            "MyService"
        );
    }

    #[test]
    fn test_qualify_type_name_already_qualified() {
        let imports = ImportMap::new();
        let package = Some("com.example".into());

        // Already qualified names should be preserved
        assert_eq!(
            qualify_type_name(
                &["java".to_string(), "util".to_string(), "List".to_string()],
                &package,
                &imports
            ),
            "java.util.List"
        );
    }

    #[test]
    fn test_qualify_type_name_no_package() {
        let imports = ImportMap::new();
        let package = None;

        // Without package, unknown types should be returned as-is
        assert_eq!(
            qualify_type_name(&["CustomType".to_string()], &package, &imports),
            "CustomType"
        );

        // Contextual names remain raw in the default package too.
        assert_eq!(
            qualify_type_name(&["Object".to_string()], &package, &imports),
            "Object"
        );
    }

    #[test]
    fn test_qualify_type_name_import_overrides_java_lang() {
        let mut imports = ImportMap::new();
        // Custom String class imported (unusual but valid)
        imports.add_import("String".to_string(), "com.custom.String".to_string());

        let package = Some("com.example".into());

        // Precedence is applied by the indexed resolver, not extraction.
        assert_eq!(
            qualify_type_name(&["String".to_string()], &package, &imports),
            "String"
        );
    }
}
