// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Declaration Extraction
//!
//! Handles extraction of type declarations (classes, interfaces, enums, records)
//! from parsed Java AST nodes.

use crate::{ImportMap, Modifiers, Symbol, TextRange, TypeParameterInfo};
use hyperion_parser::syntax::{AstNode, SyntaxKind, SyntaxNode, TypeParamList};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::sync::Arc;

use crate::parent_interface_extractor::{
    extract_interface_extends, extract_interfaces, extract_parent_class,
};

/// Retain the one Java implicit edge whose declaration kind is otherwise lost
/// when the syntax tree becomes a `Symbol`.
///
/// Ordinary class/enum/record implicit parents are derived from `SymbolKind`
/// by the index and deliberately remain absent here. That keeps them exact:
/// storing `java.lang.Object` in the same field as source spelling would let a
/// lexical type named `java` incorrectly rebind the compiler-generated edge.
/// Annotation declarations share `SymbolKind::INTERFACE` with normal
/// interfaces, so the otherwise-unused parent slot is their compact marker;
/// the index exposes it as an interface edge.
pub(crate) fn complete_implicit_java_supertypes(
    syntax_kind: SyntaxKind,
    parent_class: Option<String>,
    interfaces: Vec<String>,
) -> (Option<String>, Vec<String>) {
    let parent_class = (syntax_kind == SyntaxKind::ANNOTATION_DECL)
        .then(|| "java.lang.annotation.Annotation".to_owned())
        .or(parent_class);
    (parent_class, interfaces)
}

/// Extract type parameters from a type, method, or constructor declaration.
///
/// For `class Map<K, V extends Comparable>` or a generic method declaration,
/// returns:
/// [TypeParameterInfo{name:"K", position:0}, TypeParameterInfo{name:"V", position:1, bounds:["Comparable"]}]
pub fn extract_type_parameters(node: &SyntaxNode) -> Vec<TypeParameterInfo> {
    // Try to get TypeParamList from the node
    let type_param_list = get_type_param_list(node);

    match type_param_list {
        Some(list) => extract_from_param_list(&list),
        None => Vec::new(),
    }
}

/// Get TypeParamList from a type declaration node
fn get_type_param_list(node: &SyntaxNode) -> Option<TypeParamList> {
    node.children().find_map(TypeParamList::cast)
}

/// Extract TypeParameterInfo from a TypeParamList
fn extract_from_param_list(list: &TypeParamList) -> Vec<TypeParameterInfo> {
    list.params()
        .enumerate()
        .map(|(position, param)| {
            let name = param
                .name()
                .map(|t| t.text().to_string())
                .unwrap_or_else(|| "T".to_string());

            // Extract bounds from TYPE_BOUND if present
            let bounds = param
                .bounds()
                .map(|bound| {
                    bound
                        .types()
                        .map(|ty| Arc::from(ty.syntax().text().to_string().trim()))
                        .collect()
                })
                .unwrap_or_default();

            TypeParameterInfo {
                name: name.into(),
                position,
                bounds,
            }
        })
        .collect()
}

/// Apply a modifier token to the Modifiers struct
fn apply_modifier_token(kind: SyntaxKind, modifiers: &mut Modifiers) {
    match kind {
        SyntaxKind::PUBLIC_KW => modifiers.is_public = true,
        SyntaxKind::PRIVATE_KW => modifiers.is_private = true,
        SyntaxKind::PROTECTED_KW => modifiers.is_protected = true,
        SyntaxKind::STATIC_KW => modifiers.is_static = true,
        SyntaxKind::FINAL_KW => modifiers.is_final = true,
        SyntaxKind::ABSTRACT_KW => modifiers.is_abstract = true,
        SyntaxKind::SEALED_KW => modifiers.is_sealed = true,
        _ => {}
    }
}

/// Extract modifiers from a MODIFIER_LIST node
fn extract_modifiers_from_list(modifier_list: &SyntaxNode, modifiers: &mut Modifiers) {
    // `non-sealed` is represented by IDENT, MINUS, SEALED_KW inside one
    // MODIFIER node so the same spelling can remain subtraction elsewhere.
    // Do not mistake its final token for the positive `sealed` modifier.
    for modifier in modifier_list
        .children()
        .filter(|child| child.kind() == SyntaxKind::MODIFIER)
    {
        if modifier.text() == "non-sealed" {
            continue;
        }
        for child in modifier.descendants_with_tokens() {
            if let NodeOrToken::Token(token) = child {
                apply_modifier_token(token.kind(), modifiers);
            }
        }
    }
}

/// Modifier lists are adjacent node siblings, separated only by trivia tokens.
pub(crate) fn preceding_modifier_list(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.prev_sibling()
        .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
}

/// Extract the modifier list immediately preceding a declaration.
///
/// Modifier lists are siblings of declarations in Hyperion's syntax tree. Only
/// the adjacent list belongs to this declaration; collecting every earlier list
/// leaks modifiers between declarations in the same compilation unit or type.
pub(crate) fn extract_declaration_modifiers(node: &SyntaxNode) -> Modifiers {
    let mut modifiers = Modifiers::default();
    if let Some(modifier_list) = preceding_modifier_list(node) {
        extract_modifiers_from_list(&modifier_list, &mut modifiers);
    }
    modifiers
}

/// Extract and normalize the JavaDoc immediately preceding a declaration.
pub(crate) fn extract_declaration_documentation(node: &SyntaxNode) -> Option<Arc<str>> {
    let mut comments = Vec::new();
    let mut previous = node.prev_sibling_or_token();

    while let Some(sibling) = previous {
        previous = sibling.prev_sibling_or_token();
        match sibling {
            NodeOrToken::Node(sibling_node)
                if matches!(
                    sibling_node.kind(),
                    SyntaxKind::METHOD_DECL
                        | SyntaxKind::FIELD_DECL
                        | SyntaxKind::CONSTRUCTOR_DECL
                        | SyntaxKind::CLASS_DECL
                        | SyntaxKind::INTERFACE_DECL
                        | SyntaxKind::ENUM_DECL
                        | SyntaxKind::RECORD_DECL
                ) =>
            {
                break;
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::DOC_COMMENT => {
                comments.push(token.text().to_string());
            }
            _ => {}
        }
    }

    // Walking backward avoids repeatedly scanning all preceding members.
    comments.reverse();
    let normalized = comments
        .iter()
        .flat_map(|comment| comment.lines())
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("/**").unwrap_or(line).trim();
            let line = line.strip_suffix("*/").unwrap_or(line).trim();
            let line = line.strip_prefix('*').unwrap_or(line).trim();
            (!line.is_empty()).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n");

    (!normalized.is_empty()).then(|| Arc::from(normalized))
}

pub(crate) fn is_deprecated_declaration(node: &SyntaxNode) -> bool {
    crate::symbol_extractor::has_deprecated_annotation(node)
        || extract_declaration_documentation(node)
            .is_some_and(|documentation| documentation.contains("@deprecated"))
}

/// Extract a type declaration (class, interface, enum, record)
pub(crate) fn extract_type_decl(
    node: &SyntaxNode,
    package: &Option<String>,
    imports: &ImportMap,
    kind: SymbolKind,
) -> Option<Symbol> {
    let mut name = None;
    let mut name_range: Option<rowan::TextRange> = None;
    let modifiers = extract_declaration_modifiers(node);

    // Extract name from the node itself
    for token in node.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = token {
            if token.kind().is_type_identifier() && name.is_none() {
                name = Some(token.text().to_string());
                name_range = Some(token.text_range());
            }
        }
    }

    let simple_name = name?;
    let name_range = name_range?;

    // Build FQN
    let fqn = if let Some(pkg) = package {
        format!("{}.{}", pkg, simple_name)
    } else {
        simple_name.clone()
    };

    // Extract parent class and interfaces.
    //
    // Java's syntax overloads `extends`: for classes it means single-parent
    // class extension; for interfaces it means multi-parent interface
    // extension (the same role `implements` plays for classes). Route the
    // `extends` clause accordingly so downstream inheritance walks see the
    // full set of parents.
    let (parent_class, interfaces) = if matches!(kind, SymbolKind::INTERFACE) {
        // Interface: `extends A, B` is multi-parent interface inheritance.
        // No `implements` clause is allowed on interfaces.
        let parents = extract_interface_extends(node, package, imports);
        (None, parents)
    } else {
        let parent = extract_parent_class(node, package, imports, &simple_name);
        let ifaces = extract_interfaces(node, package, imports);
        (parent, ifaces)
    };
    let (parent_class, interfaces) =
        complete_implicit_java_supertypes(node.kind(), parent_class, interfaces);

    // Extract type parameters (e.g., <T, K, V>)
    let type_parameters = extract_type_parameters(node);

    let range = TextRange::new(name_range.start().into(), name_range.end().into());

    Some(Symbol {
        fqn: fqn.into(),
        simple_name: simple_name.into(),
        range,
        kind,
        modifiers,
        container: None,
        return_type: None,
        parent_class: parent_class.map(Arc::from),
        interfaces: interfaces.into_iter().map(Arc::from).collect(),
        documentation: extract_declaration_documentation(node),
        parameters: vec![],
        type_parameters,
        is_deprecated: is_deprecated_declaration(node),
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    fn parse_and_extract_type_params(source: &str) -> Vec<TypeParameterInfo> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        // Find the first type declaration
        for child in root.children() {
            if matches!(
                child.kind(),
                SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL | SyntaxKind::RECORD_DECL
            ) {
                return extract_type_parameters(&child);
            }
        }
        Vec::new()
    }

    #[test]
    fn test_simple_type_param() {
        let params = parse_and_extract_type_params("class Supplier<T> {}");
        assert_eq!(params.len(), 1);
        assert_eq!(&*params[0].name, "T");
        assert_eq!(params[0].position, 0);
        assert!(params[0].bounds.is_empty());
    }

    #[test]
    fn test_multiple_type_params() {
        let params = parse_and_extract_type_params("class Map<K, V> {}");
        assert_eq!(params.len(), 2);
        assert_eq!(&*params[0].name, "K");
        assert_eq!(params[0].position, 0);
        assert_eq!(&*params[1].name, "V");
        assert_eq!(params[1].position, 1);
    }

    #[test]
    fn test_bounded_type_param() {
        let params = parse_and_extract_type_params("class NumberBox<T extends Number> {}");
        assert_eq!(params.len(), 1);
        assert_eq!(&*params[0].name, "T");
        assert_eq!(params[0].position, 0);
        assert_eq!(params[0].bounds.len(), 1);
        assert!(params[0].bounds[0].contains("Number"));
    }

    #[test]
    fn test_interface_type_params() {
        let params = parse_and_extract_type_params("interface Function<T, R> {}");
        assert_eq!(params.len(), 2);
        assert_eq!(&*params[0].name, "T");
        assert_eq!(&*params[1].name, "R");
    }

    #[test]
    fn test_method_type_params() {
        let tokens =
            Lexer::new("class Factory { <Result> Result make() { return null; } }").collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
            .expect("method declaration");

        let params = extract_type_parameters(&method);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.as_ref(), "Result");
        assert_eq!(params[0].position, 0);
    }

    #[test]
    fn test_constructor_type_params() {
        let tokens = Lexer::new("class Factory { <Input> Factory(Input value) {} }").collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let constructor = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CONSTRUCTOR_DECL)
            .expect("constructor declaration");

        let params = extract_type_parameters(&constructor);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name.as_ref(), "Input");
        assert_eq!(params[0].position, 0);
    }

    #[test]
    fn test_no_type_params() {
        let params = parse_and_extract_type_params("class SimpleClass {}");
        assert!(params.is_empty());
    }

    #[test]
    fn test_enum_no_type_params() {
        let params = parse_and_extract_type_params("enum Status { ACTIVE, INACTIVE }");
        assert!(params.is_empty());
    }

    #[test]
    fn non_sealed_modifier_does_not_set_the_sealed_flag() {
        let extraction = crate::symbol_extractor::SymbolExtractor::extract(
            "sealed class Base permits Child {} non-sealed class Child extends Base {}",
        )
        .unwrap();
        let base = extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "Base")
            .unwrap();
        let child = extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "Child")
            .unwrap();
        assert!(base.modifiers.is_sealed);
        assert!(!child.modifiers.is_sealed);
    }
    #[test]
    fn large_generated_class_preserves_member_metadata() {
        use std::fmt::Write;
        let mut source = String::from("package example; public class GeneratedConstants {\n");
        for member in 0..12_000 {
            writeln!(
                source,
                "/** Constant {member}. */ public static final int FIELD_{member} = {member};"
            )
            .unwrap();
        }
        source.push_str("/** First line. */ /** Second line. @deprecated Use another value. */ @Deprecated public static final int LEGACY = 1;\n");
        source
            .push_str("int ordinary;\n/** Final constant. */ public static final int VALUE = 2; }");
        let extraction = crate::symbol_extractor::SymbolExtractor::extract(&source).unwrap();
        for member in [0, 6_000, 11_999] {
            let name = format!("example.GeneratedConstants.FIELD_{member}");
            let symbol = extraction
                .symbols
                .iter()
                .find(|symbol| symbol.fqn.as_ref() == name)
                .unwrap();
            assert_eq!(
                symbol.documentation.as_deref(),
                Some(format!("Constant {member}.").as_str())
            );
            assert!(
                symbol.modifiers.is_public
                    && symbol.modifiers.is_static
                    && symbol.modifiers.is_final
            );
            assert!(!symbol.is_deprecated);
        }
        let legacy = extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "LEGACY")
            .unwrap();
        assert!(legacy.is_deprecated);
        assert_eq!(
            legacy.documentation.as_deref(),
            Some("First line.\nSecond line. @deprecated Use another value.")
        );
        let ordinary = extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "ordinary")
            .unwrap();
        assert!(!ordinary.is_deprecated && !ordinary.modifiers.is_static);
        assert!(ordinary.documentation.is_none());
        let last = extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "VALUE")
            .unwrap();
        assert_eq!(last.documentation.as_deref(), Some("Final constant."));
        assert!(last.modifiers.is_static);
        assert!(!last.is_deprecated);
    }
}
