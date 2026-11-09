// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type structure extraction from parser AST
//!
//! # Parser Structure and JLS Alignment
//!
//! **IMPORTANT**: The hyperion-parser AST structure does NOT exactly match JLS grammar.
//! The deviations relevant to type extraction are documented below.
//!
//! Key quirks handled in this module:
//! - **Array types**: Parser represents arrays as siblings (CLASS_TYPE, ARRAY_TYPE)
//!   instead of parent-child. Each `ARRAY_TYPE` sibling node holds its own
//!   `L_BRACKET`/`R_BRACKET` tokens and represents one `[]` dimension pair.
//!   This module works around this by expecting the element type to be passed
//!   separately (workspace pattern). (Older parser versions emitted a spurious
//!   top-level `L_BRACKET` token alongside `ARRAY_TYPE`; that double-token bug
//!   was fixed in the parser — don't count top-level
//!   `L_BRACKET` tokens, count `ARRAY_TYPE` sibling nodes instead.)
//! - **Type arguments**: Nested under TYPE_ARGS child node of CLASS_TYPE.
//! - **Qualified names**: Represented as QUALIFIED_NAME child with IDENT tokens.
//!
//! See also:
//! - JLS §4 (Types, Values, and Variables)
//! - JLS §19.3 (Productions from §4: Types)

use crate::TypeStructure;
use hyperion_parser::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Collect type parameters visible at a type node from enclosing declarations.
///
/// Parameters declared by a method or nested type shadow outer declarations. A
/// static method or static nested type forms a boundary and cannot see type
/// parameters from its enclosing class.
pub fn enclosing_type_parameter_names(node: &SyntaxNode) -> Vec<String> {
    enclosing_type_parameter_declarations(node)
        .into_iter()
        .filter_map(|parameter| {
            parameter.children_with_tokens().find_map(|element| {
                element.as_token().and_then(|token| {
                    (token.kind() == SyntaxKind::IDENT).then(|| token.text().to_string())
                })
            })
        })
        .collect()
}

/// Type parameter declarations in lexical lookup order, respecting static
/// boundaries. Consumers needing bounds use the same scope rules as name lookup.
pub fn enclosing_type_parameter_declarations(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut declarations = Vec::new();

    for declaration in node.ancestors().filter(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::FIELD_DECL
                | SyntaxKind::STATIC_INIT
                | SyntaxKind::METHOD_DECL
                | SyntaxKind::CONSTRUCTOR_DECL
                | SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL
        )
    }) {
        // A type parameter list belongs to the declaration header, so it always
        // precedes the members. Stopping at the first member keeps this linear
        // in the header rather than the whole declaration: this runs for every
        // declaration in a file, so scanning every member made whole-file
        // resolution quadratic, and a large generated class spent seconds here.
        if let Some(list) = declaration
            .children()
            .take_while(|child| !is_declaration_member(child.kind()))
            .find(|child| child.kind() == SyntaxKind::TYPE_PARAM_LIST)
        {
            declarations.extend(
                list.children()
                    .filter(|parameter| parameter.kind() == SyntaxKind::TYPE_PARAM),
            );
        }

        if is_static_scope_boundary(&declaration) {
            break;
        }
    }

    declarations
}

/// True for node kinds that can only appear after a declaration's header, and
/// therefore after its type parameter list.
const fn is_declaration_member(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::FIELD_DECL
            | SyntaxKind::METHOD_DECL
            | SyntaxKind::CONSTRUCTOR_DECL
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL
            | SyntaxKind::ENUM_CONSTANT
            | SyntaxKind::STATIC_INIT
            | SyntaxKind::BLOCK_STMT
    )
}

fn is_static_scope_boundary(declaration: &SyntaxNode) -> bool {
    if declaration.kind() == SyntaxKind::STATIC_INIT {
        return true;
    }

    let explicitly_static = declaration.prev_sibling().is_some_and(|sibling| {
        sibling.kind() == SyntaxKind::MODIFIER_LIST
            && sibling.descendants_with_tokens().any(|element| {
                element
                    .as_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::STATIC_KW)
            })
    });
    if explicitly_static {
        return true;
    }

    let enclosing_type = declaration.ancestors().skip(1).find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL
        )
    });

    let implicitly_static_interface_field = declaration.kind() == SyntaxKind::FIELD_DECL
        && enclosing_type.as_ref().is_some_and(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
            )
        });
    if implicitly_static_interface_field {
        return true;
    }

    let is_member_type = matches!(
        declaration.kind(),
        SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL
    );
    is_member_type
        && enclosing_type.is_some_and(|ancestor| {
            matches!(
                declaration.kind(),
                SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
                    | SyntaxKind::ANNOTATION_DECL
            ) || matches!(
                ancestor.kind(),
                SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
            )
        })
}

/// Error during type extraction
#[derive(Debug, Clone)]
pub enum TypeExtractionError {
    /// Node kind is not a type node
    InvalidNodeKind(SyntaxKind),

    /// Malformed type structure
    MalformedType(String),
}

impl std::fmt::Display for TypeExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeExtractionError::InvalidNodeKind(kind) => {
                write!(f, "Invalid node kind for type extraction: {:?}", kind)
            }
            TypeExtractionError::MalformedType(msg) => {
                write!(f, "Malformed type structure: {}", msg)
            }
        }
    }
}

impl std::error::Error for TypeExtractionError {}

/// Extract type structure from an AST node
///
/// Handles:
/// - `PRIMITIVE_TYPE`: int, boolean, etc.
/// - `CLASS_TYPE`: User, List\<T>, java.util.Map\<K,V>
/// - `ARRAY_TYPE`: User[], int[][]
///
/// Returns unresolved structure (identifiers as written in source).
/// Use [`crate::resolve_type_structure_without_lookup`] to resolve simple names to FQNs.
///
/// # Non-JLS Parser Structure
///
/// The current parser emits arrays as siblings:
/// ```text
/// METHOD_DECL
/// ├── CLASS_TYPE = "User"
/// └── ARRAY_TYPE = "[]"   (holds its own L_BRACKET/R_BRACKET tokens)
/// ```
///
/// This function handles this non-compliant structure by detecting
/// ARRAY_TYPE nodes and counting dimensions.
pub fn extract_type_structure(node: &SyntaxNode) -> Result<TypeStructure, TypeExtractionError> {
    match node.kind() {
        SyntaxKind::PRIMITIVE_TYPE => extract_primitive_type(node),
        SyntaxKind::CLASS_TYPE => extract_class_type(node),
        SyntaxKind::ARRAY_TYPE => extract_array_type(node),
        kind => Err(TypeExtractionError::InvalidNodeKind(kind)),
    }
}

/// Extract primitive type (int, boolean, etc.)
fn extract_primitive_type(node: &SyntaxNode) -> Result<TypeStructure, TypeExtractionError> {
    for token in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = token {
            if matches!(
                t.kind(),
                SyntaxKind::BOOLEAN_KW
                    | SyntaxKind::BYTE_KW
                    | SyntaxKind::SHORT_KW
                    | SyntaxKind::INT_KW
                    | SyntaxKind::LONG_KW
                    | SyntaxKind::CHAR_KW
                    | SyntaxKind::FLOAT_KW
                    | SyntaxKind::DOUBLE_KW
                    | SyntaxKind::VOID_KW
            ) {
                return Ok(TypeStructure::primitive(t.text()));
            }
        }
    }

    Err(TypeExtractionError::MalformedType(
        "No primitive keyword found".to_string(),
    ))
}

/// Extract class type (User, List\<T>, java.util.Map\<K,V>)
fn extract_class_type(node: &SyntaxNode) -> Result<TypeStructure, TypeExtractionError> {
    // Extract base type name from QUALIFIED_NAME or direct IDENTs
    let type_name = extract_type_name(node);

    if type_name.is_empty() {
        return Err(TypeExtractionError::MalformedType(
            "Empty type name".to_string(),
        ));
    }

    // Extract generic type arguments if present
    let (type_args, has_type_args_node) = extract_type_arguments(node)?;

    let mut ts = TypeStructure::class(type_name).with_type_args(type_args);
    // Diamond: TYPE_ARGS node exists but contained no actual type children
    if has_type_args_node && ts.type_args.is_empty() {
        ts.is_diamond = true;
    }
    Ok(ts)
}

/// Extract type name from CLASS_TYPE node
///
/// Handles:
/// - Simple names: `User`
/// - Qualified names: `java.util.List`
/// - Nested classes: `Outer.Inner`
fn extract_type_name(node: &SyntaxNode) -> String {
    // CLASS_TYPE structure:
    // - QUALIFIED_NAME (contains the type name with dots)
    // - TYPE_ARGS (optional, contains generic type parameters)

    // First, look for QUALIFIED_NAME child
    for child in node.children() {
        if child.kind() == SyntaxKind::QUALIFIED_NAME {
            let mut parts = Vec::new();
            for token in child.descendants_with_tokens() {
                if let NodeOrToken::Token(t) = token {
                    if t.kind() == SyntaxKind::IDENT {
                        parts.push(t.text().to_string());
                    }
                }
            }
            if !parts.is_empty() {
                let mut base = parts.join(".");
                // Nested generic types like `Outer<String>.Inner` carry the inner
                // name(s) as trailing IDENT tokens after the TYPE_ARGS node. Append
                // them so the base name isn't truncated to the enclosing type.
                let mut seen_type_args = false;
                for element in node.children_with_tokens() {
                    match element {
                        NodeOrToken::Node(n) if n.kind() == SyntaxKind::TYPE_ARGS => {
                            seen_type_args = true;
                        }
                        NodeOrToken::Token(t)
                            if seen_type_args && t.kind() == SyntaxKind::IDENT =>
                        {
                            base.push('.');
                            base.push_str(t.text());
                        }
                        _ => {}
                    }
                }
                return base;
            }
        }
    }

    // Fallback: look for IDENT tokens directly (simple names)
    let mut parts = Vec::new();
    for token in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = token {
            if t.kind() == SyntaxKind::IDENT {
                parts.push(t.text().to_string());
                // For simple names, stop after first IDENT
                break;
            }
        }
    }

    parts.join(".")
}

/// Extract generic type arguments from CLASS_TYPE node
///
/// Returns `(type_args, has_type_args_node)` so the caller can distinguish
/// diamond `<>` (node exists, vec empty) from no generics (no node).
fn extract_type_arguments(
    node: &SyntaxNode,
) -> Result<(Vec<TypeStructure>, bool), TypeExtractionError> {
    let mut type_args = Vec::new();
    let mut has_type_args_node = false;

    for child in node.children() {
        if child.kind() == SyntaxKind::TYPE_ARGS {
            has_type_args_node = true;
            for type_arg_child in child.children() {
                match type_arg_child.kind() {
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE => {
                        let arg = extract_type_structure(&type_arg_child)?;
                        type_args.push(arg);
                    }
                    SyntaxKind::ARRAY_TYPE => {
                        // An array-valued type argument (e.g. `List<String[]>`) is
                        // emitted by the parser as the element type followed by a
                        // bare `ARRAY_TYPE` ("[]") sibling with no element child.
                        // Fold those bracket dimensions into the preceding argument.
                        // A self-contained ARRAY_TYPE (with its own element child)
                        // still extracts normally.
                        if let Ok(arr) = extract_array_type(&type_arg_child) {
                            type_args.push(arr);
                        } else {
                            let dims = count_array_dimensions(&type_arg_child);
                            if let Some(last) = type_args.last_mut() {
                                last.dimensions += dims;
                            }
                        }
                    }
                    SyntaxKind::WILDCARD_TYPE => {
                        // Never drop a wildcard: it must occupy its positional slot
                        // (so `Map<?, String>` keeps two args) and keep the arg list
                        // non-empty (so a wildcard generic is not mistaken for `<>`).
                        type_args.push(extract_wildcard_type(&type_arg_child));
                    }
                    _ => {}
                }
            }
        }
    }

    Ok((type_args, has_type_args_node))
}

/// Extract a wildcard type argument (`?`, `? extends T`, `? super T`).
///
/// A bounded wildcard nests its bound as a `CLASS_TYPE` / `ARRAY_TYPE` /
/// `PRIMITIVE_TYPE` child. The existing `TypeStructure` fields encode it as a
/// compact marker base (`? extends` / `? super`) with exactly one type argument,
/// avoiding another retained discriminant in this frequently-created value.
/// An unbounded `?` has no child and remains a bare placeholder.
fn extract_wildcard_type(node: &SyntaxNode) -> TypeStructure {
    let variance = node
        .children_with_tokens()
        .find_map(|element| match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EXTENDS_KW => {
                Some("? extends")
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::SUPER_KW => Some("? super"),
            _ => None,
        });
    let mut bound = None;
    let mut trailing_dimensions = 0;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE if bound.is_none() => {
                bound = extract_type_structure(&child).ok();
            }
            SyntaxKind::ARRAY_TYPE => match extract_array_type(&child) {
                Ok(array) => bound = Some(array),
                Err(_) => trailing_dimensions += count_array_dimensions(&child),
            },
            _ => {}
        }
    }
    if let Some(mut bound) = bound {
        bound.dimensions += trailing_dimensions;
        return match variance {
            Some(variance) => TypeStructure::class(variance.to_string()).with_type_arg(bound),
            None => bound,
        };
    }
    TypeStructure::class("?".to_string())
}

/// Count array dimensions by counting L_BRACKET tokens in a node
///
/// This is a utility function to consolidate the dimension counting logic
/// that appears in multiple places across the codebase.
///
/// **Parser Structure Note**: Due to non-JLS compliant parser structure,
/// arrays are represented with L_BRACKET tokens as siblings rather than
/// in a hierarchical structure. This function counts those tokens.
pub fn count_array_dimensions(node: &SyntaxNode) -> usize {
    let mut dimensions = 0;
    for token in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = token {
            if t.kind() == SyntaxKind::L_BRACKET {
                dimensions += 1;
            }
        }
    }
    dimensions
}

/// Extract array type
///
/// Handles the non-JLS compliant parser structure where array dimensions
/// are counted via L_BRACKET tokens and the element type is extracted
/// from child nodes.
fn extract_array_type(node: &SyntaxNode) -> Result<TypeStructure, TypeExtractionError> {
    let mut element_type = None;

    // Extract element type (CLASS_TYPE or PRIMITIVE_TYPE)
    for child in node.children() {
        match child.kind() {
            SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE => {
                element_type = Some(extract_type_structure(&child)?);
            }
            _ => {}
        }
    }

    // Count dimensions via L_BRACKET tokens
    let dimensions = count_array_dimensions(node);

    match element_type {
        Some(mut ty) => {
            ty.dimensions += dimensions;
            Ok(ty)
        }
        None => Err(TypeExtractionError::MalformedType(
            "Array type missing element type".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    // Note: These are unit tests for the extraction logic.
    // Integration tests with actual parser output are in tests/ directory.

    #[test]
    fn test_extraction_error_display() {
        let err = TypeExtractionError::InvalidNodeKind(SyntaxKind::IDENT);
        assert!(err.to_string().contains("Invalid node kind"));

        let err = TypeExtractionError::MalformedType("test".to_string());
        assert!(err.to_string().contains("Malformed"));
    }

    fn type_node(source: &str, text: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        SyntaxNode::new_root(green)
            .descendants()
            .find(|node| {
                node.kind() == SyntaxKind::CLASS_TYPE && node.text().to_string().trim() == text
            })
            .unwrap_or_else(|| panic!("missing type node {text}"))
    }

    #[test]
    fn enclosing_parameters_include_method_and_class_scope() {
        let node = type_node(
            "class Box<Element> { <Result> Result map(Result value) { return value; } }",
            "Result",
        );

        let names = enclosing_type_parameter_names(&node);

        assert_eq!(names, ["Result", "Element"]);
    }

    #[test]
    fn static_method_does_not_inherit_class_type_parameters() {
        let node = type_node(
            "class Box<Element> { static <Result> Result map(Result value) { return value; } }",
            "Result",
        );

        let names = enclosing_type_parameter_names(&node);

        assert_eq!(names, ["Result"]);
    }

    #[test]
    fn static_field_does_not_inherit_class_type_parameters() {
        let node = type_node("class Box<Element> { static Element value; }", "Element");

        let names = enclosing_type_parameter_names(&node);

        assert!(names.is_empty());
    }

    #[test]
    fn wildcard_type_arguments_preserve_variance_and_bounds() {
        for (source_type, expected) in [
            ("List<? extends Number>", "List<? extends Number>"),
            ("List<? extends Number[]>", "List<? extends Number[]>"),
            ("List<? super Number>", "List<? super Number>"),
            ("List<?>", "List<?>"),
        ] {
            let source = format!("class Box {{ {source_type} value; }}");
            let node = type_node(&source, source_type);
            let extracted = extract_type_structure(&node).unwrap();
            assert_eq!(extracted.to_fqn_string(), expected);
        }
    }
}
