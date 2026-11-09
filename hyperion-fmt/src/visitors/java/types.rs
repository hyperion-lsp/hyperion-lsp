// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type visitors: wildcard, intersection, union, array, primitive, class, generics

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::SyntaxElement;

use super::JavaFormatter;

impl JavaFormatter<'_> {
    /// Visit a wildcard type (e.g., `?`, `? extends T`, `? super T`)
    pub(super) fn visit_wildcard_type(&mut self, node: &SyntaxNode) {
        // Wildcard types should not break - use non-breaking spaces
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::QUESTION => {
                        self.builder.text("?");
                    }
                    SyntaxKind::EXTENDS_KW => {
                        self.builder.text(" extends ");
                    }
                    SyntaxKind::SUPER_KW => {
                        self.builder.text(" super ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an intersection type (e.g., `T & Comparable & Serializable`)
    pub(super) fn visit_intersection_type(&mut self, node: &SyntaxNode) {
        // Intersection types should not break - use non-breaking spaces
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::AMP => {
                        self.builder.text(" & ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a union type in multi-catch (e.g., `IOException | SQLException`)
    pub(super) fn visit_union_type(&mut self, node: &SyntaxNode) {
        // Union types should not break - use non-breaking spaces
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::PIPE => {
                        self.builder.text(" | ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit an array type (e.g., `int[]`, `String[][]`)
    pub(super) fn visit_array_type(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::L_BRACKET | SyntaxKind::R_BRACKET => {
                                // No space around brackets in type
                                formatter.builder.text(t.text());
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        formatter.visit_node(&n);
                    }
                }
            }
        });
    }

    /// Visit a primitive type (e.g., `int`, `boolean`, `void`)
    pub(super) fn visit_primitive_type(&mut self, node: &SyntaxNode) {
        for child in node.children_with_tokens() {
            if let SyntaxElement::Token(t) = child {
                if t.kind() != SyntaxKind::WHITESPACE {
                    self.builder.text(t.text());
                }
            }
        }
    }

    /// Visit a class/interface type (e.g., `String`, `List<T>`, `Map.Entry<K, V>`)
    pub(super) fn visit_class_type(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::DOT => {
                                // No space around dot in qualified types
                                formatter.builder.text(".");
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        formatter.visit_node(&n);
                    }
                }
            }
        });
    }

    /// Visit type arguments (e.g., `<T>`, `<K, V>`, `<? extends Number>`)
    pub(super) fn visit_type_args(&mut self, node: &SyntaxNode) {
        self.builder.group(|b| {
            let mut formatter = JavaFormatter::new(b);

            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Token(t) => {
                        match t.kind() {
                            SyntaxKind::LT => formatter.builder.text("<"),
                            SyntaxKind::GT => formatter.builder.text(">"),
                            SyntaxKind::COMMA => {
                                formatter.builder.text(",");
                                formatter.builder.soft_break_with(" ");
                            }
                            SyntaxKind::WHITESPACE => {
                                // Skip whitespace
                            }
                            _ => formatter.visit_token(&t),
                        }
                    }
                    SyntaxElement::Node(n) => {
                        formatter.visit_node(&n);
                    }
                }
            }
        });
    }

    /// Visit a type parameter list (e.g., `<T, U extends Number>`)
    pub(super) fn visit_type_param_list(&mut self, node: &SyntaxNode) {
        // Type parameter lists should not break
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::LT => self.builder.text("<"),
                    SyntaxKind::GT => self.builder.text(">"),
                    SyntaxKind::COMMA => self.builder.text(", "),
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a type parameter (e.g., `T` or `T extends Comparable<T>`)
    pub(super) fn visit_type_param(&mut self, node: &SyntaxNode) {
        // Type parameters should not break
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    if n.kind() == SyntaxKind::TYPE_BOUND {
                        self.builder.text(" ");
                    }
                    self.visit_node(&n);
                }
            }
        }
    }

    /// Visit a type bound (e.g., `extends Comparable<T> & Serializable`)
    pub(super) fn visit_type_bound(&mut self, node: &SyntaxNode) {
        // Type bounds should not break - use non-breaking spaces
        for child in node.children_with_tokens() {
            match child {
                SyntaxElement::Token(t) => match t.kind() {
                    SyntaxKind::EXTENDS_KW => {
                        self.builder.text("extends ");
                    }
                    SyntaxKind::AMP => {
                        self.builder.text(" & ");
                    }
                    SyntaxKind::WHITESPACE => {
                        // Skip whitespace
                    }
                    _ => self.visit_token(&t),
                },
                SyntaxElement::Node(n) => {
                    self.visit_node(&n);
                }
            }
        }
    }
}
