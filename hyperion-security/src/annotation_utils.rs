// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Utilities for parsing and analyzing Java annotations
//!
//! This module provides helper functions to extract information from Spring
//! and other framework annotations.

use hyperion_parser::syntax::{Annotation, AstNode, SyntaxKind, TypeDecl};
use tracing::debug;

/// Check if a class has a specific annotation
///
/// # Arguments
/// * `type_decl` - The class/interface declaration to check
/// * `annotation_name` - The simple name of the annotation (e.g., "RestController")
///
/// # Returns
/// `true` if the annotation is present, `false` otherwise
pub fn has_annotation(type_decl: &TypeDecl, annotation_name: &str) -> bool {
    // First check descendants of the TypeDecl (for nested annotations)
    for child in type_decl.syntax().descendants() {
        if child.kind() == SyntaxKind::ANNOTATION {
            if let Some(annotation) = Annotation::cast(child) {
                if let Some(name) = annotation.name() {
                    let name_text = name.as_string();
                    // Match simple name or fully qualified name
                    if name_text == annotation_name
                        || name_text.ends_with(&format!(".{annotation_name}"))
                    {
                        let class_name = type_decl.name().map(|n| n.text().to_string());
                        debug!(class = ?class_name, annotation = %annotation_name, "Found annotation in descendants");
                        return true;
                    }
                }
            }
        }
    }

    // Also check preceding siblings - in Java AST, annotations may be in a MODIFIER_LIST
    // that is a sibling of the CLASS_DECL, not a child
    if let Some(mut prev) = type_decl.syntax().prev_sibling() {
        loop {
            // Look for MODIFIER_LIST which contains annotations
            if prev.kind() == SyntaxKind::MODIFIER_LIST {
                for child in prev.descendants() {
                    if child.kind() == SyntaxKind::ANNOTATION {
                        if let Some(annotation) = Annotation::cast(child) {
                            if let Some(name) = annotation.name() {
                                let name_text = name.as_string();
                                if name_text == annotation_name
                                    || name_text.ends_with(&format!(".{annotation_name}"))
                                {
                                    let class_name = type_decl.name().map(|n| n.text().to_string());
                                    debug!(class = ?class_name, annotation = %annotation_name, "Found annotation in preceding MODIFIER_LIST");
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            // Move to the previous sibling
            match prev.prev_sibling() {
                Some(p) => prev = p,
                None => break,
            }
        }
    }

    false
}

/// Extract the value of an annotation attribute
///
/// # Arguments
/// * `annotation` - The annotation to extract from
/// * `attribute_name` - The name of the attribute (e.g., "produces", "value")
///
/// # Returns
/// The attribute value as a string, if found
///
/// # Example
/// ```java
/// @RequestMapping(value = "/api", produces = "application/json")
/// ```
/// Calling `get_annotation_attribute(annotation, "produces")` would return `Some("application/json")`
pub fn get_annotation_attribute(annotation: &Annotation, attribute_name: &str) -> Option<String> {
    // The annotation structure in the AST looks like:
    // ANNOTATION
    //   QUALIFIED_NAME (annotation name)
    //   ANNOTATION_ARG_LIST
    //     ANNOTATION_ARG: 'produces = "application/json"'

    for child in annotation.syntax().descendants() {
        if child.kind() == SyntaxKind::ANNOTATION_ARG {
            // The ANNOTATION_ARG text is like: 'produces = "application/json"'
            let text = child.text().to_string();

            // Parse the argument: look for "name = value" pattern
            if let Some(eq_pos) = text.find('=') {
                let name = text[..eq_pos].trim();
                let value = text[eq_pos + 1..].trim();

                if name == attribute_name {
                    // Strip quotes from the value
                    let value = value.trim_matches('"');
                    debug!(attribute = %attribute_name, value = %value, "Extracted annotation attribute");
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

/// Check if a class has a REST controller annotation
///
/// This checks for both `@RestController` and `@Controller` with JSON produces
pub fn is_rest_controller(type_decl: &TypeDecl) -> bool {
    // Direct @RestController annotation
    if has_annotation(type_decl, "RestController") {
        let class_name = type_decl.name().map(|n| n.text().to_string());
        debug!(class = ?class_name, "Class is @RestController");
        return true;
    }

    // @Controller with produces = "application/json"
    if has_annotation(type_decl, "Controller") {
        // Check if any method has produces = "application/json"
        for child in type_decl.syntax().descendants() {
            if child.kind() == SyntaxKind::ANNOTATION {
                if let Some(annotation) = Annotation::cast(child) {
                    if let Some(name) = annotation.name() {
                        let name_text = name.as_string();
                        // Check request mapping annotations
                        if name_text.contains("Mapping") {
                            if let Some(produces) =
                                get_annotation_attribute(&annotation, "produces")
                            {
                                if produces.contains("application/json") {
                                    let class_name = type_decl.name().map(|n| n.text().to_string());
                                    debug!(class = ?class_name, "Class is @Controller with JSON mapping");
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Check if a method or class has a specific produces media type
///
/// # Arguments
/// * `node` - The syntax node to check (method or class)
/// * `media_type` - The media type to check for (e.g., "application/json")
///
/// # Returns
/// `true` if the produces attribute contains the media type, `false` otherwise
pub fn has_produces_media_type(
    node: &hyperion_parser::syntax::SyntaxNode,
    media_type: &str,
) -> bool {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::ANNOTATION {
            if let Some(annotation) = Annotation::cast(child) {
                if let Some(name) = annotation.name() {
                    let name_text = name.as_string();
                    // Check request mapping annotations
                    if name_text.contains("Mapping") || name_text.contains("RequestMapping") {
                        if let Some(produces) = get_annotation_attribute(&annotation, "produces") {
                            if produces.contains(media_type) {
                                debug!(media_type = %media_type, "Found produces annotation");
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::syntax::{AstNode, SourceFile, SyntaxNode};
    use hyperion_parser::{Lexer, Parser};

    fn parse_java(source: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    #[test]
    fn test_has_rest_controller_annotation() {
        let source = r#"
            package com.example;
            import org.springframework.web.bind.annotation.RestController;

            @RestController
            public class UserController {
                public String getUser() {
                    return "user";
                }
            }
        "#;
        let root = parse_java(source);

        let source_file = SourceFile::cast(root).unwrap();
        let td = source_file.types().next().unwrap();
        assert!(
            has_annotation(&td, "RestController"),
            "Expected @RestController annotation to be found"
        );
    }

    #[test]
    fn test_is_rest_controller() {
        let source = r#"
            package com.example;
            @RestController
            public class UserController { }
        "#;
        let root = parse_java(source);
        let source_file = SourceFile::cast(root).unwrap();
        let type_decl = source_file.types().next().unwrap();

        assert!(
            is_rest_controller(&type_decl),
            "Expected class to be recognized as REST controller"
        );
    }

    #[test]
    fn test_controller_with_json_produces() {
        let source = r#"
            package com.example;
            @Controller
            public class UserController {
                @RequestMapping(produces = "application/json")
                public String getUser() { return ""; }
            }
        "#;
        let root = parse_java(source);

        // Debug: print annotations in the class
        for child in root.descendants() {
            if child.kind() == SyntaxKind::ANNOTATION {
                eprintln!("Found ANNOTATION: '{:.80}'", child.text().to_string());
                if let Some(annotation) = Annotation::cast(child.clone()) {
                    if let Some(name) = annotation.name() {
                        eprintln!("  Name: '{}'", name.as_string());
                    }

                    // Debug: print all descendants of this annotation
                    eprintln!("  Annotation descendants:");
                    for desc in annotation.syntax().descendants() {
                        eprintln!("    {:?}: '{:.50}'", desc.kind(), desc.text().to_string());
                    }

                    if let Some(produces) = get_annotation_attribute(&annotation, "produces") {
                        eprintln!("  produces = '{}'", produces);
                    }
                }
            }
        }

        let source_file = SourceFile::cast(root).unwrap();
        let type_decl = source_file.types().next().unwrap();

        // Check if @Controller is found
        eprintln!(
            "has_annotation(Controller): {}",
            has_annotation(&type_decl, "Controller")
        );

        // Check descendants for RequestMapping with produces
        for child in type_decl.syntax().descendants() {
            if child.kind() == SyntaxKind::ANNOTATION {
                eprintln!(
                    "Type decl descendant ANNOTATION: '{:.80}'",
                    child.text().to_string()
                );
            }
        }

        assert!(
            is_rest_controller(&type_decl),
            "Expected @Controller with JSON produces to be recognized as REST controller"
        );
    }

    #[test]
    fn test_not_rest_controller() {
        let source = r#"
            package com.example;
            public class UserService { }
        "#;
        let root = parse_java(source);
        let source_file = SourceFile::cast(root).unwrap();
        let type_decl = source_file.types().next().unwrap();

        assert!(
            !is_rest_controller(&type_decl),
            "Expected plain class NOT to be recognized as REST controller"
        );
    }
}
